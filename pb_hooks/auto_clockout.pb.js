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
});
