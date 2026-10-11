-- Fixes the actual duplicate-instance bug pattern: session A goes on break
-- (an open breaks row), and before that break is ever closed, a second app
-- instance for the same user clocks in fresh as session B. A never
-- properly resumes — it just sits there with a dangling break while B
-- carries on as the real, continuing session.
--
-- Detection: for each user, session A has a break whose start_time is at or
-- before session B's clock_in, and whose end_time is either still empty or
-- lands after B's clock_in (i.e. the break was still "in progress" — from
-- A's point of view — at the moment B started). B is the nearest such
-- following session. Both sides must already be status='completed' — this
-- intentionally does not touch anyone currently clocked in.
--
-- Fix, exactly as specified: delete the dangling break, then make it look
-- like A cleanly ended the moment B began (A.clock_out := B.clock_in).
-- A's total_break_seconds is reduced by the deleted break's duration (that
-- duration was baked in when A got force-closed). A's net_loss_seconds is
-- reset to 0 so the existing auto_clockout backfill cron re-stamps it
-- correctly on its own — same logic PocketBase already uses everywhere
-- else, not reimplemented here. B is untouched.
--
-- KNOWN LIMITATION: any activity_snapshots already recorded under A's
-- session_id between the break start and the new clock_out stay attached
-- to A (not moved to B). They still count correctly toward A's own
-- idle/activity totals once net_loss_seconds is re-stamped; they just mean
-- A can have snapshot timestamps past its (new) clock_out, which is a
-- cosmetic-only quirk in any raw per-snapshot listing, not a hours/pay
-- calculation.
--
-- Run ON THE SERVER against the live db:
--   sudo sqlite3 /opt/pb_apps/clanker_clocker/pb_data/data.db < fix_break_handoff_overlaps.sql
--
-- Prints exactly what it's about to change BEFORE changing it.

DROP TABLE IF EXISTS _handoffs;
CREATE TEMP TABLE _handoffs AS
SELECT h.old_session_id, h.user_id, h.user_name, h.old_clock_in, h.old_clock_out,
       h.break_id, h.break_start, h.break_end, h.new_session_id, h.new_clock_in
FROM (
  SELECT
    a.id AS old_session_id, a.user_id, a.user_name,
    a.clock_in AS old_clock_in, a.clock_out AS old_clock_out,
    br.id AS break_id, br.start_time AS break_start, br.end_time AS break_end,
    b.id AS new_session_id, b.clock_in AS new_clock_in,
    ROW_NUMBER() OVER (
      PARTITION BY a.id, br.id ORDER BY b.clock_in ASC
    ) AS rn
  FROM work_sessions a
  JOIN breaks br ON br.session_id = a.id
  JOIN work_sessions b
    ON b.user_id = a.user_id
   AND b.id != a.id
   AND b.clock_in > a.clock_in
  WHERE a.status = 'completed'
    AND b.status = 'completed'
    AND br.start_time <= b.clock_in
    AND (br.end_time = '' OR br.end_time > b.clock_in)
    AND (a.clock_out = '' OR a.clock_out > b.clock_in)
) h
WHERE h.rn = 1;

-- ---- Report (informational, before any change) ----
SELECT 'break-handoff pairs to fix:', COUNT(*) FROM _handoffs;
SELECT user_name, old_session_id, old_clock_in, old_clock_out,
       break_id, break_start, break_end, new_session_id, new_clock_in
FROM _handoffs ORDER BY user_id, old_clock_in;

-- ---- Apply ----
UPDATE work_sessions
SET total_break_seconds = MAX(
      0,
      total_break_seconds - CAST(
        (julianday((SELECT break_end FROM _handoffs WHERE old_session_id = work_sessions.id))
         - julianday((SELECT break_start FROM _handoffs WHERE old_session_id = work_sessions.id))) * 86400
        AS INT
      )
    ),
    clock_out = (SELECT new_clock_in FROM _handoffs WHERE old_session_id = work_sessions.id),
    net_loss_seconds = 0
WHERE id IN (SELECT old_session_id FROM _handoffs);

DELETE FROM breaks WHERE id IN (SELECT break_id FROM _handoffs);

DROP TABLE _handoffs;

SELECT 'done';
