/// <reference path="../pb_data/types.d.ts" />

// Runs every minute. Closes any active/on_break session when:
//   - the app has stopped reporting (offline) for 5+ minutes, or
//   - the latest snapshot shows 5+ minutes of idle time, or
//   - the company's scheduled clock-out time has passed (auto clock-out enabled).
// This is the server-side safety net for when the desktop app isn't running
// to perform its own client-side auto clock-out.
//
// Shared helpers live in pb_hooks/session_utils.js (require()'d below) so the
// event-driven hooks in session_guards.pb.js apply identical close logic.
cronAdd("auto_clockout_offline", "* * * * *", () => {
    const utils = require(`${__hooks}/session_utils.js`);
    const OFFLINE_THRESHOLD_SECONDS = 5 * 60; // 5 minutes

    const now = new Date();
    const pastClockOutTime = utils.isPastScheduledClockOut(now);

    let activeSessions;
    try {
        activeSessions = $app.findRecordsByFilter(
            "work_sessions",
            "status = 'active' || status = 'on_break'",
            "",
            500,
            0
        );
    } catch (e) {
        console.error("[auto_clockout] failed to fetch active sessions:", e);
        return;
    }

    console.log(`[auto_clockout] checking ${activeSessions.length} active/on_break session(s)`);

    for (const session of activeSessions) {
        let sessionId = "?";
        try {
            sessionId = session.id;

            // Trust an open break record over the session's status field: the
            // client PATCHes status separately from creating the break, and a
            // failed PATCH used to get people clocked out mid-break.
            const onBreak =
                session.getString("status") === "on_break" ||
                utils.hasOpenBreak(sessionId);

            // While on a break, idle/offline is expected — only the scheduled
            // clock-out time can end the session early.
            let offlineTooLong = false;
            let idleTooLong = false;
            let secondsSinceLastPing = 0;
            let idleSeconds = 0;

            if (!onBreak) {
                // Find the most recent activity snapshot for this session
                let latestSnapshot = null;
                try {
                    const snapshots = $app.findRecordsByFilter(
                        "activity_snapshots",
                        `session_id = '${sessionId}'`,
                        "-timestamp",
                        1,
                        0
                    );
                    if (snapshots.length > 0) {
                        latestSnapshot = snapshots[0];
                    }
                } catch (e) {
                    // No snapshots found — treat as never active
                }

                if (!latestSnapshot) {
                    // Fall back to clock_in time if no snapshots at all
                    const clockIn = utils.parseDateTime(session.getString("clock_in"));
                    if (clockIn) {
                        secondsSinceLastPing = (now - clockIn) / 1000;
                    }
                } else {
                    const lastSeen = utils.parseDateTime(latestSnapshot.getString("timestamp"));
                    if (lastSeen) {
                        secondsSinceLastPing = (now - lastSeen) / 1000;
                    }
                    idleSeconds = latestSnapshot.getInt("idle_seconds") || 0;
                }

                offlineTooLong = secondsSinceLastPing >= OFFLINE_THRESHOLD_SECONDS;
                idleTooLong = idleSeconds >= OFFLINE_THRESHOLD_SECONDS;
            }

            // Scheduled clock-out does not apply to external staff (they work
            // outside company hours) or to sessions the user explicitly chose
            // to extend past the schedule to make up lost time.
            let scheduledClose = pastClockOutTime;
            if (scheduledClose && session.getBool("extended_past_schedule")) {
                scheduledClose = false;
            }
            if (scheduledClose && utils.isExternalStaff(session.getString("user_id"))) {
                scheduledClose = false;
            }

            console.log(
                `[auto_clockout] eval ${sessionId} status=${session.getString("status")} onBreak=${onBreak} ` +
                `offlineTooLong=${offlineTooLong}(${Math.round(secondsSinceLastPing)}s) ` +
                `idleTooLong=${idleTooLong}(${idleSeconds}s) scheduledClose=${scheduledClose}`
            );

            if (!offlineTooLong && !idleTooLong && !scheduledClose) {
                continue; // still on the clock
            }

            const reason = offlineTooLong
                ? `offline for ${Math.round(secondsSinceLastPing)}s`
                : idleTooLong
                    ? `idle for ${Math.round(idleSeconds)}s`
                    : "past scheduled clock-out time";
            utils.closeSession(session, now, `[auto_clockout] ${reason}`);
        } catch (e) {
            console.error(`[auto_clockout] error processing session ${sessionId}:`, e);
        }
    }

    // Sweep: an open break whose session is already completed is an orphan
    // (crash or failed PATCH). Left alone it poisons reports — the client
    // would count it as "still running". Close it at the session's clock_out.
    try {
        const openBreaks = $app.findRecordsByFilter("breaks", "end_time = ''", "", 200, 0);
        for (const breakRecord of openBreaks) {
            try {
                const sessionId = breakRecord.getString("session_id");
                let session = null;
                try {
                    session = $app.findRecordById("work_sessions", sessionId);
                } catch (e) {
                    // Session was deleted — neutralize as a zero-length break.
                }
                if (session && session.getString("status") !== "completed") {
                    continue; // legitimately on break right now
                }
                const start = utils.parseDateTime(breakRecord.getString("start_time"));
                let end = session ? utils.parseDateTime(session.getString("clock_out")) : null;
                if (!end || (start && end < start)) {
                    end = start || now;
                }
                breakRecord.set("end_time", end.toISOString());
                $app.save(breakRecord);
                console.log(`[auto_clockout] closed orphan break ${breakRecord.id} at ${end.toISOString()}`);
            } catch (e) {
                console.error(`[auto_clockout] orphan break sweep failed for ${breakRecord.id}:`, e);
            }
        }
    } catch (e) {
        console.error("[auto_clockout] orphan break sweep failed:", e);
    }

    // Stamp net_loss_seconds on completed sessions so report queries can use
    // the stored value instead of re-downloading every activity snapshot.
    // Stamps are floored at 1s so "0" reliably means "not stamped yet" —
    // that lets the backfill below find unstamped sessions, at the cost of a
    // 1-second rounding error nobody will ever see.
    function stampNetLoss(session, logPrefix) {
        const loss = Math.max(1, utils.computeNetLossSeconds(session.id));
        if (session.getInt("net_loss_seconds") !== loss) {
            session.set("net_loss_seconds", loss);
            $app.save(session);
            console.log(`${logPrefix} stamped net_loss=${loss}s on session ${session.id}`);
        }
    }

    // Recently completed sessions: recompute for ~15 minutes after close.
    // Idempotent, and catches every completion path (client, cron, guards).
    try {
        const cutoff = new Date(now.getTime() - 15 * 60 * 1000)
            .toISOString()
            .replace("T", " ");
        const recent = $app.findRecordsByFilter(
            "work_sessions",
            `status = 'completed' && clock_out >= '${cutoff}'`,
            "",
            100,
            0
        );
        for (const session of recent) {
            try {
                stampNetLoss(session, "[auto_clockout]");
            } catch (e) {
                console.error(`[auto_clockout] net_loss stamp failed for ${session.id}:`, e);
            }
        }
    } catch (e) {
        console.error("[auto_clockout] net_loss stamping failed:", e);
    }

    // Backfill: stamp historical sessions a few at a time. The unstamped
    // pool only shrinks, so this block goes quiet once the backlog is done.
    try {
        const backlog = $app.findRecordsByFilter(
            "work_sessions",
            "status = 'completed' && clock_out != '' && net_loss_seconds = 0",
            "-clock_out",
            25,
            0
        );
        for (const session of backlog) {
            try {
                stampNetLoss(session, "[auto_clockout backfill]");
            } catch (e) {
                console.error(`[auto_clockout] backfill failed for ${session.id}:`, e);
            }
        }
    } catch (e) {
        console.error("[auto_clockout] net_loss backfill failed:", e);
    }
});
