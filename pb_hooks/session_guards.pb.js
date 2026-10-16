/// <reference path="../pb_data/types.d.ts" />

// Event-driven complements to the auto_clockout cron. The cron only fires
// once a minute; these hooks react instantly to the records that matter, so
// idle sessions close within seconds and duplicate active sessions can never
// exist in the first place. The cron remains the safety net for sessions
// that go fully offline (no snapshots arrive, so Hook A never fires).

// Hook A — close an active session the moment a snapshot reports 5+ minutes
// of idle time. Sessions on break (by status or by open break record) are
// left alone; breaks are only ended by the user or the scheduled clock-out.
onRecordCreateRequest((e) => {
    e.next();

    try {
        const IDLE_THRESHOLD_SECONDS = 5 * 60;
        const idleSeconds = e.record.getInt("idle_seconds") || 0;
        if (idleSeconds < IDLE_THRESHOLD_SECONDS) {
            return;
        }

        // Same suppression as the client and the cron: no mouse/keyboard
        // input is expected while on a call.
        if (e.record.getBool("in_call")) {
            return;
        }

        const utils = require(`${__hooks}/session_utils.js`);
        const sessionId = e.record.getString("session_id");
        if (!sessionId) {
            return;
        }
        if (utils.effectiveIdleSeconds(sessionId, e.record, new Date()) < IDLE_THRESHOLD_SECONDS) {
            return; // idle predates the last break end
        }

        const session = $app.findRecordById("work_sessions", sessionId);
        if (!session || session.getString("status") !== "active") {
            return;
        }
        if (utils.hasOpenBreak(sessionId)) {
            return;
        }

        utils.closeSession(
            session,
            new Date(),
            `[session_guards] idle ${idleSeconds}s reported by snapshot`
        );
    } catch (err) {
        console.error("[session_guards] snapshot idle check failed:", err);
    }
}, "activity_snapshots");

// Hook B — when a new session is created for a user, close any session of
// theirs that is still active/on_break. The client calls
// close_stale_sessions() before creating, but if that PATCH fails a ghost
// session would keep accruing time forever.
onRecordCreateRequest((e) => {
    try {
        const utils = require(`${__hooks}/session_utils.js`);
        const userId = e.record.getString("user_id");
        if (userId) {
            const stale = $app.findRecordsByFilter(
                "work_sessions",
                `user_id = '${userId}' && (status = 'active' || status = 'on_break')`,
                "",
                50,
                0
            );
            const now = new Date();
            for (const session of stale) {
                const openBreak = utils.findOpenBreak(session.id);
                if (openBreak) {
                    const { end, breakEnd } = utils.breakCloseTimes(session, openBreak, now);
                    utils.closeSession(
                        session,
                        end,
                        "[session_guards] superseded by new clock-in (on break)",
                        breakEnd
                    );
                } else {
                    utils.closeSession(session, now, "[session_guards] superseded by new clock-in");
                }
            }
        }
    } catch (err) {
        console.error("[session_guards] stale session cleanup failed:", err);
    }

    e.next();
}, "work_sessions");

// Hook C — a completed session is final for regular users. A client that
// hasn't noticed the server closed its session (it still shows "On Break")
// would otherwise reopen it on "End Break" (status -> active) or overwrite
// the server's clock_out on clock-out, bringing the phantom hours back.
// Admins can still edit. Server-side $app.save() calls don't go through
// request hooks, so the cron and the guards above are unaffected.
//
// Also: when a client closes a session that still has an open break (the
// clock-in path's close_stale_sessions does this, at "now", without ending
// the break), re-close it with the same break-aware times the cron uses.
onRecordUpdateRequest((e) => {
    const utils = require(`${__hooks}/session_utils.js`);
    const admin = utils.isAdminRequest(e);
    const wasCompleted = e.record.original().getString("status") === "completed";
    if (wasCompleted && !admin) {
        throw new BadRequestError("This session was already closed.");
    }
    const closing = !wasCompleted && e.record.getString("status") === "completed";

    e.next();

    if (!closing || admin) {
        return;
    }
    try {
        const openBreak = utils.findOpenBreak(e.record.id);
        if (!openBreak) {
            return;
        }
        const session = $app.findRecordById("work_sessions", e.record.id);
        const { end, breakEnd } = utils.breakCloseTimes(session, openBreak, new Date());
        utils.closeSession(
            session,
            end,
            "[session_guards] client closed session mid-break",
            breakEnd
        );
    } catch (err) {
        console.error("[session_guards] mid-break close fixup failed:", err);
    }
}, "work_sessions");

// Hook D — breaks on a completed session are final for regular users too
// (same stale-client reasoning as Hook C), and no new break can start on one.
onRecordCreateRequest((e) => {
    require(`${__hooks}/session_utils.js`).rejectBreakOnClosedSession(e);
    e.next();
}, "breaks");

onRecordUpdateRequest((e) => {
    require(`${__hooks}/session_utils.js`).rejectBreakOnClosedSession(e);
    e.next();
}, "breaks");
