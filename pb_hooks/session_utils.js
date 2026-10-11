/// <reference path="../pb_data/types.d.ts" />

// Shared helpers for the auto-clockout cron (auto_clockout.pb.js) and the
// event-driven guards (session_guards.pb.js). Plain .js (not .pb.js) so
// PocketBase doesn't execute it directly — it's loaded via require().

const NEPAL_OFFSET_MS = (5 * 60 + 45) * 60 * 1000; // company runs on Nepal time (UTC+5:45)

// Allowance for break types with no configured duration (e.g. "Other"),
// after which the usual idle/offline checks resume.
const UNTIMED_BREAK_MINUTES = 60;

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

// Earliest open break record on a session, or null. A session with an open
// break is on break no matter what its status field says (the client sets
// status in a separate, fallible PATCH).
function findOpenBreak(sessionId) {
    try {
        const openBreaks = $app.findRecordsByFilter(
            "breaks",
            `session_id = '${sessionId}' && end_time = ''`,
            "start_time",
            1,
            0
        );
        return openBreaks.length > 0 ? openBreaks[0] : null;
    } catch (e) {
        return null;
    }
}

function hasOpenBreak(sessionId) {
    return findOpenBreak(sessionId) !== null;
}

// A break's allowed length comes from its break_configs duration (breaks.type
// holds the config's type_key). Inactive configs count too, so disabling a
// break type never strands a break that is already running.
function allowedBreakSeconds(typeKey) {
    let minutes = 0;
    try {
        const configs = $app.findRecordsByFilter(
            "break_configs",
            "type_key = {:typeKey}",
            "",
            50,
            0,
            { typeKey: typeKey || "" }
        );
        for (const config of configs) {
            minutes = Math.max(minutes, config.getInt("duration_minutes") || 0);
        }
    } catch (e) {
        // No matching config — fall through to the untimed allowance.
    }
    return (minutes > 0 ? minutes : UNTIMED_BREAK_MINUTES) * 60;
}

// When a break's allowed length runs out, or null if it has no start time.
function breakAllowedEnd(breakRecord) {
    const start = parseDateTime(breakRecord.getString("start_time"));
    if (!start) {
        return null;
    }
    return new Date(start.getTime() + allowedBreakSeconds(breakRecord.getString("type")) * 1000);
}

function latestSnapshot(sessionId) {
    try {
        const snapshots = $app.findRecordsByFilter(
            "activity_snapshots",
            `session_id = '${sessionId}'`,
            "-timestamp",
            1,
            0
        );
        return snapshots.length > 0 ? snapshots[0] : null;
    } catch (e) {
        return null;
    }
}

// Where to end a session that is being closed while a break is still open.
// The break gets at most its allowed length; the session ends at the later
// of that and the person's last real input (latest snapshot minus its idle
// time), so work done after coming back still counts. Never past `now` —
// closing at `now` is what used to bill an overnight gap as break time.
function breakCloseTimes(session, openBreak, now) {
    const start = parseDateTime(openBreak.getString("start_time")) || now;
    const allowedEnd = breakAllowedEnd(openBreak) || start;
    const breakEnd = allowedEnd < now ? allowedEnd : now;

    let end = breakEnd;
    const snapshot = latestSnapshot(session.id);
    const lastSeen = snapshot ? parseDateTime(snapshot.getString("timestamp")) : null;
    if (lastSeen) {
        const lastInput = new Date(lastSeen.getTime() - (snapshot.getInt("idle_seconds") || 0) * 1000);
        if (lastInput > end) {
            end = lastInput < now ? lastInput : now;
        }
    }
    return { end, breakEnd };
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
        const end = endTime > start ? endTime : start;
        closedSeconds += Math.floor((end - start) / 1000);
        breakRecord.set("end_time", end.toISOString());
        try {
            $app.save(breakRecord);
        } catch (e) {
            console.error(`[session_utils] failed to close break ${breakRecord.id}:`, e);
        }
    }

    return closedSeconds;
}

// Closes a work_sessions record: ends open breaks (at `breakEndTime`, default
// `endTime`), sets clock_out, and marks it completed. `logPrefix` identifies
// the caller in the server log.
function closeSession(session, endTime, logPrefix, breakEndTime) {
    const breakSecs =
        (session.getInt("total_break_seconds") || 0) +
        closeOpenBreaks(session.id, breakEndTime || endTime);
    session.set("clock_out", endTime.toISOString());
    session.set("status", "completed");
    session.set("total_break_seconds", breakSecs);
    $app.save(session);
    console.log(`${logPrefix} — closed session ${session.id}`);
}

// Net loss = idle time outside breaks, capped at 30s per 30s snapshot.
// Mirrors the calculation in the desktop client (pocketbase.rs) so the
// stamped value and the client-computed fallback always agree. Snapshots
// after `clockOut` are ignored: a client that hasn't noticed its session was
// closed server-side keeps pushing them for up to a minute.
function computeNetLossSeconds(sessionId, clockOut) {
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
                clockOut
                    ? `session_id = '${sessionId}' && timestamp <= {:clockOut}`
                    : `session_id = '${sessionId}'`,
                "timestamp",
                PER_PAGE,
                page * PER_PAGE,
                { clockOut: clockOut || "" }
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

// Request-hook helpers (session_guards.pb.js). Admin = superuser or a user
// with is_admin; their edits to closed sessions are always allowed.
function isAdminRequest(e) {
    return e.hasSuperuserAuth() || (e.auth != null && e.auth.getBool("is_admin"));
}

// A completed session's breaks are final for regular users: a stale client
// (still showing "On Break" after a server-side close) must not rewrite the
// break the server closed, or start a new one.
function rejectBreakOnClosedSession(e) {
    if (isAdminRequest(e)) {
        return;
    }
    let session = null;
    try {
        session = $app.findRecordById("work_sessions", e.record.getString("session_id"));
    } catch (err) {
        return; // unknown session — leave it to the collection's API rules
    }
    if (session.getString("status") === "completed") {
        throw new BadRequestError("This session was already closed.");
    }
}

module.exports = {
    parseDateTime,
    isPastScheduledClockOut,
    isExternalStaff,
    findOpenBreak,
    hasOpenBreak,
    breakAllowedEnd,
    latestSnapshot,
    breakCloseTimes,
    closeOpenBreaks,
    closeSession,
    computeNetLossSeconds,
    isAdminRequest,
    rejectBreakOnClosedSession,
};
