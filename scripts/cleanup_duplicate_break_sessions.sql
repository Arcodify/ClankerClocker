-- One-time cleanup of duplicate work_sessions created before the app
-- enforced a single running instance (fixed 2026-09-24 in commit 7e059b9,
-- see src-tauri/src/lib.rs, tauri_plugin_single_instance).
--
-- Before that fix, a user could run a second app window/instance for the
-- same account. If they hit "break" in one window, that window's session
-- picked up a breaks row (and usually ended up status='on_break' or later
-- 'completed'), while the OTHER window kept polling and clocked its own,
-- separate work_sessions row for an overlapping time range — so the person
-- shows as simultaneously on break AND actively working, inflating the
-- work/break hours reported in AdminView.
--
-- This finds pairs of work_sessions for the same user whose time ranges
-- overlap, where exactly one side of the pair ever had a break recorded
-- against it and the other side never did. The side WITH the break is
-- deleted (along with its breaks / activity_snapshots / network_connections
-- / tasks child rows); the side that just kept working is left as the
-- record of truth. Pairs where both sides (or neither side) have a break
-- are ambiguous and are left untouched — they're reported separately so you
-- can eyeball them by hand.
--
-- Run ON THE SERVER with PocketBase STOPPED (same procedure as
-- cleanup_orphans.sql):
--
--   sudo systemctl stop <pocketbase-service>       # or however PB is run
--   sqlite3 /opt/pb_apps/clanker_clocker/pb_data/data.db < cleanup_duplicate_break_sessions.sql
--   sudo systemctl start <pocketbase-service>
--
-- BACK UP data.db first — this permanently deletes work_sessions rows and
-- everything hanging off them. Safe to re-run afterward: once the duplicate
-- rows are gone, nothing else matches.

DROP TABLE IF EXISTS _break_sessions;
CREATE TEMP TABLE _break_sessions AS
  SELECT id, user_id, user_name, clock_in,
         COALESCE(NULLIF(clock_out, ''), '9999-12-31T00:00:00Z') AS effective_clock_out
  FROM work_sessions
  WHERE status = 'on_break'
     OR id IN (SELECT DISTINCT session_id FROM breaks);

DROP TABLE IF EXISTS _plain_sessions;
CREATE TEMP TABLE _plain_sessions AS
  SELECT id, user_id, clock_in,
         COALESCE(NULLIF(clock_out, ''), '9999-12-31T00:00:00Z') AS effective_clock_out
  FROM work_sessions
  WHERE id NOT IN (SELECT id FROM _break_sessions);

-- Overlapping pairs: a "break" session and a same-user sibling session that
-- never had a break, whose time ranges intersect.
DROP TABLE IF EXISTS _dupes;
CREATE TEMP TABLE _dupes AS
  SELECT DISTINCT b.id AS break_session_id, b.user_id, b.user_name,
         b.clock_in AS break_clock_in,
         p.id AS sibling_session_id, p.clock_in AS sibling_clock_in
  FROM _break_sessions b
  JOIN _plain_sessions p
    ON p.user_id = b.user_id
   AND b.clock_in < p.effective_clock_out
   AND p.clock_in < b.effective_clock_out;

-- Ambiguous pairs left untouched: both sides had a break, and their time
-- ranges overlap. Reported for manual review, nothing deleted here.
DROP TABLE IF EXISTS _ambiguous;
CREATE TEMP TABLE _ambiguous AS
  SELECT DISTINCT b1.id AS session_a, b2.id AS session_b, b1.user_id,
         b1.clock_in AS clock_in_a, b2.clock_in AS clock_in_b
  FROM _break_sessions b1
  JOIN _break_sessions b2
    ON b2.user_id = b1.user_id
   AND b2.id > b1.id
   AND b1.clock_in < b2.effective_clock_out
   AND b2.clock_in < b1.effective_clock_out;

-- ---- Report (informational) ----
SELECT 'duplicate on-break sessions to remove:', COUNT(DISTINCT break_session_id) FROM _dupes;
SELECT user_name, user_id, break_session_id, break_clock_in, sibling_session_id, sibling_clock_in
  FROM _dupes ORDER BY user_id, break_clock_in;

SELECT 'ambiguous overlapping pairs (NOT touched, review manually):', COUNT(*) FROM _ambiguous;
SELECT user_id, session_a, clock_in_a, session_b, clock_in_b
  FROM _ambiguous ORDER BY user_id, clock_in_a;

-- ---- Delete the duplicate "on break" sessions and their children ----
DELETE FROM breaks WHERE session_id IN (SELECT break_session_id FROM _dupes);
DELETE FROM activity_snapshots WHERE session_id IN (SELECT break_session_id FROM _dupes);
DELETE FROM network_connections WHERE session_id IN (SELECT break_session_id FROM _dupes);
DELETE FROM tasks WHERE session_id IN (SELECT break_session_id FROM _dupes);
DELETE FROM work_sessions WHERE id IN (SELECT break_session_id FROM _dupes);

DROP TABLE _dupes;
DROP TABLE _ambiguous;
DROP TABLE _break_sessions;
DROP TABLE _plain_sessions;

-- Reclaim disk space (may take a minute on a large db)
VACUUM;
