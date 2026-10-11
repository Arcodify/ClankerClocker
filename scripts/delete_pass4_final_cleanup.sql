-- Final cleanup pass: 15 confirmed duplicate-instance sessions (no break on
-- either side, so earlier passes never touched them). Verified with real
-- numbers for every pair on 2026-09-29: in every case the later-clock-in
-- session's time range sits inside the earlier one's, with less activity —
-- same confirmed pattern as the previous pass. The earlier/larger session
-- in each pair is kept untouched.
--
-- SAIF excluded entirely per instruction — not touched in this pass.
--
-- Run ON THE SERVER against the live db:
--   sudo sqlite3 /opt/pb_apps/clanker_clocker/pb_data/data.db < delete_pass4_final_cleanup.sql

DROP TABLE IF EXISTS _to_delete;
CREATE TEMP TABLE _to_delete (id TEXT);
INSERT INTO _to_delete VALUES
  ('b9m3y2k3jsv3bz2'),   -- Ashma 06-14, kept uufsjh3iqvwjqb9
  ('u6q9q8z7a0y47t3'),   -- Ashma 07-06, kept ql56zc7vpbac6ex
  ('x1ywn88d4u0gt19'),   -- Ashma 08-02, kept nfh8xp5l8usmolg
  ('rpsh4rtkgc2w0sl'),   -- Ashma 08-14, kept 9gjtkletl3hvu3n
  ('em0uv1uxzclzpil'),   -- Avi 09-10,   kept lt9vofms6ustcj3
  ('dc94fc2uewi8az1'),   -- BEN 06-12,   kept tmz7becrlkt1al2
  ('bz74ymx9xtjzsqh'),   -- BEN 06-14,   kept twn6cwtwl2m6no2
  ('pt18nv7i15i7kau'),   -- BEN 07-24,   kept fa34zhocrv1qk9o
  ('d5fj6k6b0n2fl85'),   -- BEN 07-31,   kept vmeavv7wcsnii01
  ('thu8xjve1h7onfa'),   -- Bijay 06-09, kept uar4xlatckfv187
  ('9eg0vtbdcmcqmeg'),   -- Mandoj 06-18, kept n84nraxvmyvaab6
  ('5pxu49jqwr3nt5i'),   -- Mandoj 09-11, kept 16gbnm7f1oyhy1t
  ('1tyr8g8owmq3guw'),   -- Namarata 08-10, kept ol7u0kj0ngjgrk0
  ('lg02l5swmzgriwb'),   -- Namarata 08-17, kept ev4r10jp4r3b337
  ('fdunm6tuhpd2kt4');   -- Upashana 06-26, kept dchcz3wyeqx37hp

SELECT 'sessions to delete:', COUNT(*) FROM _to_delete;

DELETE FROM breaks WHERE session_id IN (SELECT id FROM _to_delete);
DELETE FROM activity_snapshots WHERE session_id IN (SELECT id FROM _to_delete);
DELETE FROM network_connections WHERE session_id IN (SELECT id FROM _to_delete);
DELETE FROM tasks WHERE session_id IN (SELECT id FROM _to_delete);
DELETE FROM work_sessions WHERE id IN (SELECT id FROM _to_delete);

DROP TABLE _to_delete;

SELECT 'done';
