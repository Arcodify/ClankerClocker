/// <reference path="../pb_data/types.d.ts" />

// Shared helpers for the auto-clockout cron (auto_clockout.pb.js) and the
// event-driven guards (session_guards.pb.js). Plain .js (not .pb.js) so
// PocketBase doesn't execute it directly — it's loaded via require().

const NEPAL_OFFSET_MS = (5 * 60 + 45) * 60 * 1000; // company runs on Nepal time (UTC+5:45)

function parseDateTime(value) {
    if (!value) {
        return null;
    }
    const d = new Date(value);
    return Number.isNaN(d.getTime()) ? null : d;
}

// True when the company's scheduled clock-out time (auto clock-out enabled)
// has passed for `now` in Nepal time. `graceMinutes` delays the answer past
// the scheduled time — the desktop client needs a window to show its
// "keep working to complete your hours?" prompt and mark the session
// extended_past_schedule before the server closes it.
function isPastScheduledClockOut(now, graceMinutes) {
    try {
        const companyConfig = $app.findFirstRecordByFilter("company_config", "");
        if (!companyConfig || !companyConfig.getBool("auto_clock_out_enabled")) {
            return false;
        }
        const match = /^(\d{1,2}):(\d{2})/.exec(companyConfig.getString("clock_out_time") || "");
        if (!match) {
            return false;
        }
        const clockOutMinutes = parseInt(match[1], 10) * 60 + parseInt(match[2], 10);
        const nepalNow = new Date(now.getTime() + NEPAL_OFFSET_MS);
        const nowMinutes = nepalNow.getUTCHours() * 60 + nepalNow.getUTCMinutes();
        return nowMinutes >= clockOutMinutes + (graceMinutes || 0);
    } catch (e) {
        console.error("[session_utils] failed to load company_config:", e);
        return false;
    }
}

// External staff work outside company hours — scheduled clock-out never
// applies to them. Unknown/missing users are treated as regular staff.
function isExternalStaff(userId) {
    if (!userId) {
        return false;
    }
    try {
        const user = $app.findRecordById("users", userId);
        return user ? user.getBool("is_external_staff") : false;
    } catch (e) {
        return false;
    }
}

// A session with an open break record is on break no matter what its status
// field says (the client sets status in a separate, fallible PATCH).
function hasOpenBreak(sessionId) {
    try {
        const openBreaks = $app.findRecordsByFilter(
            "breaks",
            `session_id = '${sessionId}' && end_time = ''`,
            "",
            1,
            0
        );
        return openBreaks.length > 0;
    } catch (e) {
        return false;
    }
}

function closeOpenBreaks(sessionId, endTime) {
    let closedSeconds = 0;
    let openBreaks = [];
    try {
        openBreaks = $app.findRecordsByFilter(
            "breaks",
            `session_id = '${sessionId}' && end_time = ''`,
            "",
            200,
            0
        );
    } catch (e) {
        console.error(`[session_utils] failed to fetch open breaks for ${sessionId}:`, e);
        return 0;
    }

    for (const breakRecord of openBreaks) {
        const start = parseDateTime(breakRecord.getString("start_time"));
        if (!start) {
            continue;
        }
        closedSeconds += Math.max(0, Math.floor((endTime - start) / 1000));
        breakRecord.set("end_time", endTime.toISOString());
        try {
            $app.save(breakRecord);
        } catch (e) {
            console.error(`[session_utils] failed to close break ${breakRecord.id}:`, e);
        }
    }

    return closedSeconds;
}

// Closes a work_sessions record: ends open breaks, sets clock_out, and marks
// it completed. `logPrefix` identifies the caller in the server log.
function closeSession(session, endTime, logPrefix) {
    const breakSecs =
        (session.getInt("total_break_seconds") || 0) +
        closeOpenBreaks(session.id, endTime);
    session.set("clock_out", endTime.toISOString());
    session.set("status", "completed");
    session.set("total_break_seconds", breakSecs);
    $app.save(session);
    console.log(`${logPrefix} — closed session ${session.id}`);
}

// Net loss = idle time outside breaks, capped at 30s per 30s snapshot.
// Mirrors the calculation in the desktop client (pocketbase.rs) so the
// stamped value and the client-computed fallback always agree.
function computeNetLossSeconds(sessionId) {
    let intervals = [];
    try {
        const breaks = $app.findRecordsByFilter(
            "breaks",
            `session_id = '${sessionId}' && end_time != ''`,
            "",
            200,
            0
        );
        for (const b of breaks) {
            const s = parseDateTime(b.getString("start_time"));
            const e = parseDateTime(b.getString("end_time"));
            if (s && e && e > s) {
                intervals.push([s, e]);
            }
        }
    } catch (err) {
        // No breaks — nothing to exclude.
    }

    let loss = 0;
    let page = 0;
    const PER_PAGE = 1000;
    for (;;) {
        let snaps;
        try {
            snaps = $app.findRecordsByFilter(
                "activity_snapshots",
                `session_id = '${sessionId}'`,
                "timestamp",
                PER_PAGE,
                page * PER_PAGE
            );
        } catch (err) {
            break;
        }
        for (const snap of snaps) {
            const idle = snap.getInt("idle_seconds") || 0;
            if (idle < 1) {
                continue;
            }
            const ts = parseDateTime(snap.getString("timestamp"));
            if (ts && intervals.some(([s, e]) => ts >= s && ts < e)) {
                continue;
            }
            loss += Math.min(idle, 30);
        }
        if (snaps.length < PER_PAGE) {
            break;
        }
        page++;
    }
    return loss;
}

module.exports = {
    parseDateTime,
    isPastScheduledClockOut,
    isExternalStaff,
    hasOpenBreak,
    closeOpenBreaks,
    closeSession,
    computeNetLossSeconds,
};
