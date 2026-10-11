-- Fixes sessions where someone started a break and never came back (laptop
-- shut / app quit mid-break). Before the break-allowance change in
-- pb_hooks/session_utils.js, nothing ended such a break except the
-- scheduled clock-out (skipped for extended_past_schedule sessions and
-- external staff), so it ran until the user's NEXT clock-in closed the
-- session at "now" — e.g. Upashana 2026-10-02: 2.55h work + 45.47h break.
--
-- Applies the same rule the server now uses going forward:
--   allowed      = the break type's break_configs.duration_minutes
--                  (largest across configs with that type_key; 60 min if
--                  none or 0 — same as UNTIMED_BREAK_MINUTES)
--   break end    = break start + allowed
--   clock_out    = later of (break end) and (last real input = latest
--                  snapshot before the old clock_out minus its idle_seconds),
--                  never later than the old clock_out
--
-- Only touches breaks on completed sessions that ran right up to clock_out
-- (end within 2 minutes of it) AND overran their allowance by 5+ minutes
-- (same grace as the cron). Recomputes total_break_seconds from the
-- session's breaks and zeroes net_loss_seconds so the auto_clockout
-- backfill cron re-stamps it (it now ignores snapshots after clock_out).
--
-- Run ON THE SERVER against the live db — BACK UP data.db first:
--   sudo sqlite3 /opt/pb_apps/clanker_clocker/pb_data/data.db < fix_abandoned_breaks.sql
--
-- Prints what it will change BEFORE changing it. Safe to re-run: fixed
-- breaks end at their allowance and no longer match.

.headers on
.mode column

DROP TABLE IF EXISTS _allow;
CREATE TEMP TABLE _allow AS
  SELECT type_key, MAX(duration_minutes) AS minutes
  FROM break_configs
  GROUP BY type_key;

DROP TABLE IF EXISTS _abandoned;
CREATE TEMP TABLE _abandoned AS
SELECT x.*,
       -- clock_out: later of the allowed break end and the last real input,
       -- capped at the old clock_out
       MIN(julianday(x.old_clock_out),
           MAX(x.new_break_end_jd, COALESCE(x.last_input_jd, 0))) AS new_clock_out_jd
FROM (
  SELECT ws.id AS session_id, ws.user_name, ws.clock_in,
         ws.clock_out AS old_clock_out,
         br.id AS break_id, br.type AS break_type, br.start_time AS break_start,
         br.end_time AS old_break_end,
         COALESCE(NULLIF(a.minutes, 0), 60) AS allowed_minutes,
         julianday(br.start_time) + COALESCE(NULLIF(a.minutes, 0), 60) / 1440.0 AS new_break_end_jd,
         (SELECT julianday(s.timestamp) - s.idle_seconds / 86400.0
            FROM activity_snapshots s
           WHERE s.session_id = ws.id AND s.timestamp <= ws.clock_out
           ORDER BY s.timestamp DESC LIMIT 1) AS last_input_jd
  FROM work_sessions ws
  JOIN breaks br ON br.session_id = ws.id
  LEFT JOIN _allow a ON a.type_key = br.type
  WHERE ws.status = 'completed'
    AND ws.clock_out != ''
    AND br.end_time != ''
    AND (julianday(ws.clock_out) - julianday(br.end_time)) * 86400 <= 120
) x
WHERE (julianday(x.old_break_end) - x.new_break_end_jd) * 86400 >= 5 * 60;

SELECT 'abandoned breaks to fix:', COUNT(*) FROM _abandoned;
SELECT user_name, session_id, break_type, allowed_minutes AS allowed_min,
       clock_in, old_clock_out,
       strftime('%Y-%m-%d %H:%M:%fZ', new_clock_out_jd) AS new_clock_out,
       ROUND((julianday(old_break_end) - julianday(break_start)) * 24, 2) AS old_break_h
FROM _abandoned ORDER BY clock_in;

UPDATE breaks
   SET end_time = (SELECT strftime('%Y-%m-%d %H:%M:%fZ', a.new_break_end_jd)
                     FROM _abandoned a WHERE a.break_id = breaks.id)
 WHERE id IN (SELECT break_id FROM _abandoned);

-- If a session somehow has two such breaks, end it at the earliest result.
UPDATE work_sessions
   SET clock_out = (SELECT strftime('%Y-%m-%d %H:%M:%fZ', MIN(a.new_clock_out_jd))
                      FROM _abandoned a WHERE a.session_id = work_sessions.id),
       net_loss_seconds = 0
 WHERE id IN (SELECT session_id FROM _abandoned);

UPDATE work_sessions
   SET total_break_seconds = COALESCE((
         SELECT CAST(ROUND(SUM((julianday(b.end_time) - julianday(b.start_time)) * 86400)) AS INTEGER)
           FROM breaks b
          WHERE b.session_id = work_sessions.id AND b.end_time != ''
       ), 0)
 WHERE id IN (SELECT session_id FROM _abandoned);

SELECT 'after fix:' AS '';
SELECT ws.user_name, ws.id, ws.clock_in, ws.clock_out,
       ROUND((julianday(ws.clock_out) - julianday(ws.clock_in)) * 24, 2) AS gross_h,
       ROUND(ws.total_break_seconds / 3600.0, 2) AS break_h
FROM work_sessions ws
WHERE ws.id IN (SELECT session_id FROM _abandoned)
ORDER BY ws.clock_in;

-- Report only: sessions STILL open on break right now. The new cron closes
-- these on its own once deployed; listed so you can see them go.
SELECT 'still open with an open break (cron will handle after deploy):' AS '';
SELECT ws.user_name, ws.id, ws.clock_in, br.type, br.start_time
FROM work_sessions ws
JOIN breaks br ON br.session_id = ws.id AND br.end_time = ''
WHERE ws.status IN ('active', 'on_break');

DROP TABLE _abandoned;
DROP TABLE _allow;
