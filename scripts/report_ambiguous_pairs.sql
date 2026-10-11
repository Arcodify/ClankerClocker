-- Read-only report for the "ambiguous overlapping pairs" flagged by
-- cleanup_duplicate_break_sessions.sql (both sides of the pair had a
-- break recorded, so that script could not tell which side to remove).
--
-- Paste the pairs it printed into the VALUES list below (session_a,
-- session_b — the two id columns from its output, ignore user_id/clock_in),
-- then run this against the SAME data.db. It's read-only: no writes, safe
-- to run with PocketBase online or offline.
--
--   sqlite3 /opt/pb_apps/clanker_clocker/pb_data/data.db < report_ambiguous_pairs.sql
--
-- For each session on either side of a pair, shows clock_in/out, duration,
-- how many breaks it has, how many activity_snapshots it logged, and total
-- keystrokes+clicks across those snapshots (a proxy for "real work
-- happened here"). The side with near-zero activity and a short duration
-- relative to its sibling is almost always the ghost second-instance
-- session; the side with real activity volume is the one to keep.

DROP TABLE IF EXISTS _pairs;
CREATE TEMP TABLE _pairs (session_a TEXT, session_b TEXT);
INSERT INTO _pairs (session_a, session_b) VALUES
  ('1lp7mblfaj3c96i', 'kveok496lzyasei'),
  ('hr5bq1ulb0k619w', 'jelw8y13mbs4xjw'),
  ('8iplopaps55czcb', 'stn82ihrh5zc7dx'),
  ('1qe04l4prmnlmpq', '6sxxgyql5sl0yrz'),
  ('2pum77v0sxc56in', 's6ckvqnx37bzzce'),
  ('jfyy5mcopbn6v4j', 'ol4rmx9u5awyf20'),
  ('8ubax1cr4qv40cx', 'i8p3pak2gtd2729'),
  ('1sbqiv1eo6kehhx', '2z9yn7307uy874m'),
  ('1sbqiv1eo6kehhx', '3qdvm0sfv4irx11'),
  ('1sbqiv1eo6kehhx', 'vn62nczt0ub5glw'),
  ('fkp2b1ck4jangc2', 'n79k49gxlx61m9d'),
  ('rqwfb0t7l055sdo', 'xxnd0ydj2ghjsjk'),
  ('3fbyx43e5y35uxv', 'm1ypjxley2qcuqu'),
  ('6ajbkkt1l6xx1ds', 'ozj0k84vlnbcd9t'),
  ('a7t5zsax93b4ey6', 'e6b620ezchg1d6e'),
  ('4ca1dd4ed9o5p7f', 'kgt9ugl3dmscoef'),
  ('med4mfrshhll99x', 'vwq4i47m60fdj6z'),
  ('9gjtkletl3hvu3n', 'rpsh4rtkgc2w0sl'),
  ('68icqmnlhxn9mhm', 'nc16vfgubb04p8o'),
  ('bz74ymx9xtjzsqh', 'twn6cwtwl2m6no2'),
  ('4c4herjm4kja2gv', 'ol7u0kj0ngjgrk0');

DROP TABLE IF EXISTS _ids;
CREATE TEMP TABLE _ids AS
  SELECT session_a AS id, rowid AS pair_no FROM _pairs
  UNION ALL
  SELECT session_b AS id, rowid AS pair_no FROM _pairs;

SELECT
  i.pair_no,
  ws.user_name,
  ws.id AS session_id,
  ws.status,
  ws.clock_in,
  ws.clock_out,
  CAST((julianday(COALESCE(NULLIF(ws.clock_out,''), datetime('now'))) - julianday(ws.clock_in)) * 1440 AS INT) AS duration_minutes,
  ws.total_break_seconds,
  (SELECT COUNT(*) FROM breaks b WHERE b.session_id = ws.id) AS break_count,
  (SELECT COUNT(*) FROM activity_snapshots a WHERE a.session_id = ws.id) AS snapshot_count,
  (SELECT COALESCE(SUM(a.keystrokes),0) + COALESCE(SUM(a.mouse_clicks),0)
     FROM activity_snapshots a WHERE a.session_id = ws.id) AS input_events
FROM _ids i
JOIN work_sessions ws ON ws.id = i.id
ORDER BY i.pair_no, ws.clock_in;

DROP TABLE _ids;
DROP TABLE _pairs;
