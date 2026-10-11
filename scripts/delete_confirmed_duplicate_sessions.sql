-- Resolves the 13 remaining overlapping pairs (12 flagged "ambiguous" by
-- cleanup_duplicate_break_sessions.sql, plus the Avi/Aug-7 case surfaced
-- after fix_break_handoff_overlaps.sql ran) where BOTH sides had a break,
-- so no earlier script touched them automatically.
--
-- Manually reviewed with real numbers (clock_in/out, duration, break count,
-- activity_snapshots count, keystrokes+clicks) for every pair — see the
-- report pulled 2026-09-29. In all 13 cases the later-clock-in session's
-- time range sits entirely (or almost entirely) inside the earlier one's,
-- and the earlier one has equal-or-greater activity. That's the
-- second-instance ghost pattern, not two genuine separate work periods —
-- so the later session in each pair is deleted outright (with its
-- breaks / activity_snapshots / network_connections / tasks), the earlier
-- one is left untouched.
--
-- Run ON THE SERVER against the live db:
--   sudo sqlite3 /opt/pb_apps/clanker_clocker/pb_data/data.db < delete_confirmed_duplicate_sessions.sql

DROP TABLE IF EXISTS _to_delete;
CREATE TEMP TABLE _to_delete (id TEXT);
INSERT INTO _to_delete VALUES
  ('1lp7mblfaj3c96i'),   -- Mandoj 2026-08-05, kept kveok496lzyasei
  ('jelw8y13mbs4xjw'),   -- SAIF   2026-07-09, kept hr5bq1ulb0k619w
  ('1qe04l4prmnlmpq'),   -- Upashana 2026-08-16 (ran 24h, stuck), kept 6sxxgyql5sl0yrz
  ('s6ckvqnx37bzzce'),   -- Upashana 2026-08-18, kept 2pum77v0sxc56in
  ('i8p3pak2gtd2729'),   -- Rashi  2026-07-13, kept 8ubax1cr4qv40cx
  ('fkp2b1ck4jangc2'),   -- Rashi  2026-07-21, kept n79k49gxlx61m9d
  ('xxnd0ydj2ghjsjk'),   -- Rashi  2026-09-09, kept rqwfb0t7l055sdo
  ('m1ypjxley2qcuqu'),   -- Rashi  2026-09-17, kept 3fbyx43e5y35uxv
  ('e6b620ezchg1d6e'),   -- Ashma  2026-06-09, kept a7t5zsax93b4ey6
  ('4ca1dd4ed9o5p7f'),   -- Ashma  2026-06-16, kept kgt9ugl3dmscoef
  ('med4mfrshhll99x'),   -- Ashma  2026-07-30, kept vwq4i47m60fdj6z
  ('4c4herjm4kja2gv'),   -- Namarata 2026-08-10, kept ol7u0kj0ngjgrk0
  ('8iplopaps55czcb');   -- Avi    2026-08-07, kept stn82ihrh5zc7dx

SELECT 'sessions to delete:', COUNT(*) FROM _to_delete;

DELETE FROM breaks WHERE session_id IN (SELECT id FROM _to_delete);
DELETE FROM activity_snapshots WHERE session_id IN (SELECT id FROM _to_delete);
DELETE FROM network_connections WHERE session_id IN (SELECT id FROM _to_delete);
DELETE FROM tasks WHERE session_id IN (SELECT id FROM _to_delete);
DELETE FROM work_sessions WHERE id IN (SELECT id FROM _to_delete);

DROP TABLE _to_delete;

SELECT 'done';
