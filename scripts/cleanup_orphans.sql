-- One-time cleanup of rows whose parent work_session was deleted.
-- As of 2026-07-23 this removes ~45k activity_snapshots, ~149k
-- network_connections, and ~130 breaks (all pre-June-8 orphans).
--
-- Run ON THE SERVER with PocketBase STOPPED:
--
--   sudo systemctl stop <pocketbase-service>       # or however PB is run
--   sqlite3 /opt/pb_apps/clanker_clocker/pb_data/data.db < cleanup_orphans.sql
--   sudo systemctl start <pocketbase-service>
--
-- Safe to re-run; it only ever touches rows with no parent session.

-- Count before (informational)
SELECT 'orphan snapshots:', COUNT(*) FROM activity_snapshots
  WHERE session_id NOT IN (SELECT id FROM work_sessions);
SELECT 'orphan network:', COUNT(*) FROM network_connections
  WHERE session_id NOT IN (SELECT id FROM work_sessions);
SELECT 'orphan breaks:', COUNT(*) FROM breaks
  WHERE session_id NOT IN (SELECT id FROM work_sessions);

DELETE FROM activity_snapshots
  WHERE session_id NOT IN (SELECT id FROM work_sessions);
DELETE FROM network_connections
  WHERE session_id NOT IN (SELECT id FROM work_sessions);
DELETE FROM breaks
  WHERE session_id NOT IN (SELECT id FROM work_sessions);

-- Reclaim disk space (may take a minute on a large db)
VACUUM;
