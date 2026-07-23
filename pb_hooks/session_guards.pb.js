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

        const utils = require(`${__hooks}/session_utils.js`);
        const sessionId = e.record.getString("session_id");
        if (!sessionId) {
            return;
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
                utils.closeSession(
                    session,
                    now,
                    "[session_guards] superseded by new clock-in"
                );
            }
        }
    } catch (err) {
        console.error("[session_guards] stale session cleanup failed:", err);
    }

    e.next();
}, "work_sessions");
