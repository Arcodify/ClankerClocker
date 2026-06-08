/// <reference path="../pb_data/types.d.ts" />

// Runs every minute. Closes any active/on_break session when:
//   - the app has stopped reporting (offline) for 5+ minutes, or
//   - the latest snapshot shows 5+ minutes of idle time, or
//   - the company's scheduled clock-out time has passed (auto clock-out enabled).
// This is the server-side safety net for when the desktop app isn't running
// to perform its own client-side auto clock-out.
cronAdd("auto_clockout_offline", "* * * * *", () => {
    const OFFLINE_THRESHOLD_SECONDS = 5 * 60; // 5 minutes
    const NEPAL_OFFSET_MS = (5 * 60 + 45) * 60 * 1000; // company runs on Nepal time (UTC+5:45)

    const now = new Date();
    const nepalNow = new Date(now.getTime() + NEPAL_OFFSET_MS);
    const nowMinutes = nepalNow.getUTCHours() * 60 + nepalNow.getUTCMinutes();

    // Look up the single company_config record for the clock-out policy.
    let pastClockOutTime = false;
    try {
        const companyConfig = $app.findFirstRecordByFilter("company_config", "");
        const rawClockOut = companyConfig ? companyConfig.getString("clock_out_time") : null;
        const enabled = companyConfig ? companyConfig.getBool("auto_clock_out_enabled") : false;
        let clockOutMinutes = null;
        if (companyConfig && enabled) {
            const match = /^(\d{1,2}):(\d{2})/.exec(rawClockOut || "");
            if (match) {
                clockOutMinutes = parseInt(match[1], 10) * 60 + parseInt(match[2], 10);
                pastClockOutTime = nowMinutes >= clockOutMinutes;
            }
        }
        console.log(
            `[auto_clockout] company_config: enabled=${enabled} clock_out_time=${JSON.stringify(rawClockOut)} ` +
            `nowMinutes=${nowMinutes} clockOutMinutes=${clockOutMinutes} pastClockOutTime=${pastClockOutTime}`
        );
    } catch (e) {
        console.error("[auto_clockout] failed to load company_config:", e);
    }

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
            const onBreak = session.getString("status") === "on_break";

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
                    const clockIn = session.getString("clock_in");
                    if (clockIn) {
                        const clockInDate = new Date(clockIn);
                        secondsSinceLastPing = (now - clockInDate) / 1000;
                    }
                } else {
                    const ts = latestSnapshot.getString("timestamp");
                    const lastSeen = new Date(ts);
                    secondsSinceLastPing = (now - lastSeen) / 1000;
                    idleSeconds = latestSnapshot.getInt("idle_seconds") || 0;
                }

                offlineTooLong = secondsSinceLastPing >= OFFLINE_THRESHOLD_SECONDS;
                idleTooLong = idleSeconds >= OFFLINE_THRESHOLD_SECONDS;
            }

            console.log(
                `[auto_clockout] eval ${sessionId} status=${session.getString("status")} onBreak=${onBreak} ` +
                `offlineTooLong=${offlineTooLong}(${Math.round(secondsSinceLastPing)}s) ` +
                `idleTooLong=${idleTooLong}(${idleSeconds}s) pastClockOutTime=${pastClockOutTime}`
            );

            // Clock out if any of:
            // - the app stopped sending snapshots (offline 5+ minutes) — not while on break, or
            // - the latest snapshot shows 5+ minutes of idle time — not while on break, or
            // - the scheduled clock-out time has passed (auto clock-out enabled).
            if (!offlineTooLong && !idleTooLong && !pastClockOutTime) {
                continue; // still on the clock
            }

            // Auto clock-out: set clock_out to now and mark completed
            const breakSecs = session.getInt("total_break_seconds") || 0;
            session.set("clock_out", now.toISOString());
            session.set("status", "completed");
            session.set("total_break_seconds", breakSecs);
            $app.save(session);
            const reason = offlineTooLong
                ? `offline for ${Math.round(secondsSinceLastPing)}s`
                : idleTooLong
                    ? `idle for ${Math.round(idleSeconds)}s`
                    : "past scheduled clock-out time";
            console.log(
                `[auto_clockout] clocked out session ${sessionId} — ${reason}`
            );
        } catch (e) {
            console.error(`[auto_clockout] error processing session ${sessionId}:`, e);
        }
    }
});
