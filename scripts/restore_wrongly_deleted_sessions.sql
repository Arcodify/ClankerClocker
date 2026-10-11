-- Restores the 12 work_sessions rows (and their breaks / activity_snapshots /
-- network_connections / tasks children) that cleanup_duplicate_break_sessions.sql
-- incorrectly deleted on 2026-09-29.
--
-- ROOT CAUSE: that script's heuristic ("of two overlapping sessions for the
-- same user, delete whichever one has a break") is wrong. A normal single-
-- instance session very often has a break in it too — the heuristic doesn't
-- actually distinguish a real session from a duplicate-instance ghost, it
-- just deletes whichever side happens to have a breaks row. In all 12 cases
-- here, the deleted session was the substantial, real one (40 min to 9+
-- hours, hundreds of activity_snapshots, thousands of input events) and the
-- side left behind was often the thinner one. See report_ambiguous_pairs.sql
-- output from today for the audit trail.
--
-- Run ON THE SERVER against the live db, using today's pre-cleanup backup:
--
--   sudo sqlite3 /opt/pb_apps/clanker_clocker/pb_data/data.db < restore_wrongly_deleted_sessions.sql
--
-- Safe to re-run: every INSERT is guarded with "id NOT IN (already there)".

ATTACH DATABASE '/opt/pb_apps/clanker_clocker/pb_data/data.db.bak-2026-09-29' AS bak;

INSERT INTO work_sessions
  SELECT * FROM bak.work_sessions
  WHERE id IN (
    '16gbnm7f1oyhy1t','a7t5zsax93b4ey6','dchcz3wyeqx37hp','n84nraxvmyvaab6',
    'nfh8xp5l8usmolg','ol7u0kj0ngjgrk0','orz2qc11tdlkszr','ql56zc7vpbac6ex',
    'rpsh4rtkgc2w0sl','stn82ihrh5zc7dx','twn6cwtwl2m6no2','uufsjh3iqvwjqb9'
  )
  AND id NOT IN (SELECT id FROM work_sessions);

INSERT INTO breaks
  SELECT * FROM bak.breaks
  WHERE session_id IN (
    '16gbnm7f1oyhy1t','a7t5zsax93b4ey6','dchcz3wyeqx37hp','n84nraxvmyvaab6',
    'nfh8xp5l8usmolg','ol7u0kj0ngjgrk0','orz2qc11tdlkszr','ql56zc7vpbac6ex',
    'rpsh4rtkgc2w0sl','stn82ihrh5zc7dx','twn6cwtwl2m6no2','uufsjh3iqvwjqb9'
  )
  AND id NOT IN (SELECT id FROM breaks);

INSERT INTO activity_snapshots
  SELECT * FROM bak.activity_snapshots
  WHERE session_id IN (
    '16gbnm7f1oyhy1t','a7t5zsax93b4ey6','dchcz3wyeqx37hp','n84nraxvmyvaab6',
    'nfh8xp5l8usmolg','ol7u0kj0ngjgrk0','orz2qc11tdlkszr','ql56zc7vpbac6ex',
    'rpsh4rtkgc2w0sl','stn82ihrh5zc7dx','twn6cwtwl2m6no2','uufsjh3iqvwjqb9'
  )
  AND id NOT IN (SELECT id FROM activity_snapshots);

INSERT INTO network_connections
  SELECT * FROM bak.network_connections
  WHERE session_id IN (
    '16gbnm7f1oyhy1t','a7t5zsax93b4ey6','dchcz3wyeqx37hp','n84nraxvmyvaab6',
    'nfh8xp5l8usmolg','ol7u0kj0ngjgrk0','orz2qc11tdlkszr','ql56zc7vpbac6ex',
    'rpsh4rtkgc2w0sl','stn82ihrh5zc7dx','twn6cwtwl2m6no2','uufsjh3iqvwjqb9'
  )
  AND id NOT IN (SELECT id FROM network_connections);

INSERT INTO tasks
  SELECT * FROM bak.tasks
  WHERE session_id IN (
    '16gbnm7f1oyhy1t','a7t5zsax93b4ey6','dchcz3wyeqx37hp','n84nraxvmyvaab6',
    'nfh8xp5l8usmolg','ol7u0kj0ngjgrk0','orz2qc11tdlkszr','ql56zc7vpbac6ex',
    'rpsh4rtkgc2w0sl','stn82ihrh5zc7dx','twn6cwtwl2m6no2','uufsjh3iqvwjqb9'
  )
  AND id NOT IN (SELECT id FROM tasks);

DETACH DATABASE bak;

SELECT 'work_sessions now present (should be 12):', COUNT(*) FROM work_sessions WHERE id IN (
    '16gbnm7f1oyhy1t','a7t5zsax93b4ey6','dchcz3wyeqx37hp','n84nraxvmyvaab6',
    'nfh8xp5l8usmolg','ol7u0kj0ngjgrk0','orz2qc11tdlkszr','ql56zc7vpbac6ex',
    'rpsh4rtkgc2w0sl','stn82ihrh5zc7dx','twn6cwtwl2m6no2','uufsjh3iqvwjqb9'
);
