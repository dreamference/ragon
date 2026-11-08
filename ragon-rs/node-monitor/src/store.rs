//! The monitor's history (specs/DREAMFERENCE_MONITORING.md §5.2): one sqlite file in WAL mode with
//! the raw samples (48 h), the 1-minute rollups (14 days) and the 1-hour rollups (400 days), capped
//! at 256 MiB. When the file passes the cap the oldest raw samples go first, then the oldest minute
//! rollups. Time is always a parameter (Unix seconds), so the tests need no clock.

use crate::Item;
use rusqlite::{Connection, params};
use std::io;
use std::path::Path;

/// Raw samples are kept this long (§5.2).
pub const RAW_KEEP_SECS: i64 = 48 * 3600;
/// 1-minute rollups are kept this long.
pub const MINUTE_KEEP_SECS: i64 = 14 * 86400;
/// 1-hour rollups are kept this long.
pub const HOUR_KEEP_SECS: i64 = 400 * 86400;
/// The database is held under this size (§5.2, acceptance 14).
pub const CAP_BYTES: i64 = 256 * 1024 * 1024;

pub struct Store {
    conn: Connection,
    cap_bytes: i64,
}

fn err(e: rusqlite::Error) -> io::Error {
    io::Error::other(e.to_string())
}

impl Store {
    /// Opens (creating) the database at `path`.
    pub fn open(path: &Path) -> io::Result<Store> {
        let conn = Connection::open(path).map_err(err)?;
        Store::init(conn)
    }

    /// An in-memory database, for tests.
    pub fn in_memory() -> io::Result<Store> {
        Store::init(Connection::open_in_memory().map_err(err)?)
    }

    fn init(conn: Connection) -> io::Result<Store> {
        conn.execute_batch(
            "PRAGMA auto_vacuum=INCREMENTAL; PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;
             CREATE TABLE IF NOT EXISTS samples(ts INTEGER NOT NULL, item TEXT NOT NULL,
                 state TEXT NOT NULL, rank INTEGER NOT NULL, detail TEXT NOT NULL, vals TEXT NOT NULL);
             CREATE INDEX IF NOT EXISTS samples_item_ts ON samples(item, ts);
             CREATE INDEX IF NOT EXISTS samples_ts ON samples(ts);
             CREATE TABLE IF NOT EXISTS rollup_1m(ts INTEGER NOT NULL, item TEXT NOT NULL,
                 n INTEGER NOT NULL, worst TEXT NOT NULL, rank INTEGER NOT NULL,
                 PRIMARY KEY(item, ts)) WITHOUT ROWID;
             CREATE TABLE IF NOT EXISTS rollup_1h(ts INTEGER NOT NULL, item TEXT NOT NULL,
                 n INTEGER NOT NULL, worst TEXT NOT NULL, rank INTEGER NOT NULL,
                 PRIMARY KEY(item, ts)) WITHOUT ROWID;",
        )
        .map_err(err)?;
        Ok(Store {
            conn,
            cap_bytes: CAP_BYTES,
        })
    }

    /// Replaces the size cap (tests).
    pub fn with_cap(mut self, bytes: i64) -> Store {
        self.cap_bytes = bytes;
        self
    }

    /// Writes one sample per item, all stamped `ts`.
    pub fn record(&mut self, ts: i64, items: &[Item]) -> io::Result<()> {
        let tx = self.conn.transaction().map_err(err)?;
        {
            let mut stmt = tx
                .prepare_cached("INSERT INTO samples VALUES(?1,?2,?3,?4,?5,?6)")
                .map_err(err)?;
            for item in items {
                stmt.execute(params![
                    ts,
                    item.name,
                    item.state.as_str(),
                    item.state.rank() as i64,
                    item.detail,
                    item.values.to_string()
                ])
                .map_err(err)?;
            }
        }
        tx.commit().map_err(err)
    }

    pub fn sample_count(&self) -> i64 {
        self.count("samples")
    }

    pub fn count(&self, table: &str) -> i64 {
        self.conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .unwrap_or(0)
    }

    /// Rolls the finished minutes (and then hours) up; the minute holding `now` is left for later.
    pub fn rollup(&mut self, now: i64) -> io::Result<()> {
        let minute = now - now.rem_euclid(60);
        let hour = now - now.rem_euclid(3600);
        self.conn
            .execute(
                "INSERT OR REPLACE INTO rollup_1m
                 SELECT ts - ts % 60, item, COUNT(*), (SELECT s2.state FROM samples s2
                     WHERE s2.item = s.item AND s2.ts - s2.ts % 60 = s.ts - s.ts % 60
                     ORDER BY s2.rank DESC LIMIT 1), MAX(rank)
                 FROM samples s WHERE ts < ?1 AND ts >= COALESCE(
                     (SELECT MAX(ts) FROM rollup_1m), 0) GROUP BY item, ts - ts % 60",
                params![minute],
            )
            .map_err(err)?;
        self.conn
            .execute(
                "INSERT OR REPLACE INTO rollup_1h
                 SELECT ts - ts % 3600, item, SUM(n), (SELECT m2.worst FROM rollup_1m m2
                     WHERE m2.item = m.item AND m2.ts - m2.ts % 3600 = m.ts - m.ts % 3600
                     ORDER BY m2.rank DESC LIMIT 1), MAX(rank)
                 FROM rollup_1m m WHERE ts < ?1 AND ts >= COALESCE(
                     (SELECT MAX(ts) FROM rollup_1h), 0) GROUP BY item, ts - ts % 3600",
                params![hour],
            )
            .map_err(err)?;
        Ok(())
    }

    fn size_bytes(&self) -> i64 {
        let q = |pragma: &str| -> i64 {
            self.conn
                .query_row(&format!("PRAGMA {pragma}"), [], |r| r.get(0))
                .unwrap_or(0)
        };
        (q("page_count") - q("freelist_count")) * q("page_size")
    }

    /// The hourly pass: rollups first (so nothing is lost to the pruning), then retention, then the
    /// size cap.
    pub fn maintain(&mut self, now: i64) -> io::Result<()> {
        self.rollup(now)?;
        let cut = |table: &str, keep: i64, this: &Connection| {
            this.execute(
                &format!("DELETE FROM {table} WHERE ts < ?1"),
                params![now - keep],
            )
        };
        cut("samples", RAW_KEEP_SECS, &self.conn).map_err(err)?;
        cut("rollup_1m", MINUTE_KEEP_SECS, &self.conn).map_err(err)?;
        cut("rollup_1h", HOUR_KEEP_SECS, &self.conn).map_err(err)?;
        // Over the cap: the oldest raw samples first, then the oldest minute rollups, a tenth of
        // the rows at a time.
        for table in ["samples", "rollup_1m"] {
            while self.size_bytes() > self.cap_bytes && self.count(table) > 0 {
                let n = (self.count(table) / 10).max(1);
                self.conn
                    .execute(
                        &format!(
                            "DELETE FROM {table} WHERE rowid IN (SELECT rowid FROM {table} ORDER BY ts LIMIT ?1)"
                        ),
                        params![n],
                    )
                    .map_err(err)?;
                // Freed pages count only after they are returned to the file.
                let _ = self.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
                if self.conn.execute_batch("PRAGMA incremental_vacuum;").is_err() {
                    break;
                }
            }
        }
        Ok(())
    }

    /// `VACUUM`, run weekly when idle.
    pub fn vacuum(&mut self) -> io::Result<()> {
        self.conn.execute_batch("VACUUM;").map_err(err)
    }

    /// The worst rank recorded for `item` in `[from, to)`, by raw samples (tests, `history`).
    pub fn samples_between(&self, item: &str, from: i64, to: i64) -> i64 {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM samples WHERE item=?1 AND ts>=?2 AND ts<?3",
                params![item, from, to],
                |r| r.get(0),
            )
            .unwrap_or(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{State, Weight};

    fn item(name: &str, state: State) -> Item {
        Item::new(name, state, Weight::Item, "d")
    }

    #[test]
    fn raw_samples_older_than_48_hours_are_gone_after_maintain() {
        let mut s = Store::in_memory().unwrap();
        let now = 10 * 86400;
        s.record(now - RAW_KEEP_SECS - 10, &[item("a", State::Ok)])
            .unwrap();
        s.record(now - 60, &[item("a", State::Ok)]).unwrap();
        s.maintain(now).unwrap();
        assert_eq!(s.sample_count(), 1);
        // The old sample was rolled up before it was pruned.
        assert!(s.count("rollup_1m") >= 1);
    }

    #[test]
    fn rollup_keeps_the_worst_state_of_the_minute_and_hour() {
        let mut s = Store::in_memory().unwrap();
        let t = 7200;
        s.record(t + 1, &[item("a", State::Ok)]).unwrap();
        s.record(t + 20, &[item("a", State::Down)]).unwrap();
        s.record(t + 61, &[item("a", State::Degraded)]).unwrap();
        s.rollup(t + 7200).unwrap();
        let (n, worst): (i64, String) = s
            .conn
            .query_row("SELECT n, worst FROM rollup_1m WHERE ts=?1", [t], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((n, worst.as_str()), (2, "down"));
        let (n, worst): (i64, String) = s
            .conn
            .query_row("SELECT n, worst FROM rollup_1h WHERE ts=?1", [t], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!((n, worst.as_str()), (3, "down"));
    }

    #[test]
    fn the_current_minute_is_not_rolled_up_yet() {
        let mut s = Store::in_memory().unwrap();
        s.record(125, &[item("a", State::Ok)]).unwrap();
        s.rollup(130).unwrap();
        assert_eq!(s.count("rollup_1m"), 0);
        s.rollup(190).unwrap();
        assert_eq!(s.count("rollup_1m"), 1);
    }

    #[test]
    fn the_cap_drops_oldest_raw_samples_first() {
        let dir = std::env::temp_dir().join(format!("rnm-cap-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut s = Store::open(&dir.join("m.sqlite"))
            .unwrap()
            .with_cap(200 * 1024);
        let now = 100_000;
        for i in 0..3000 {
            s.record(now - 3000 + i, &[item("a", State::Ok), item("b", State::Ok)])
                .unwrap();
        }
        s.maintain(now).unwrap();
        assert!(s.size_bytes() <= 200 * 1024, "{}", s.size_bytes());
        assert!(s.sample_count() < 6000);
        // The newest survive.
        assert_eq!(s.samples_between("a", now - 10, now), 10);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn thirty_days_at_the_section_3_intervals_stay_far_below_the_cap() {
        // 6 h of raw samples (a quarter of the 48 h kept, so the size is checked times four) at
        // 10/15/60/300 s for ~25 items, then 30 days of minute rollups for
        // the same items, generated in SQL (a real month of inserts would take hours).
        let dir = std::env::temp_dir().join(format!("rnm-size-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("m.sqlite");
        let mut s = Store::open(&path).unwrap();
        let now: i64 = 40 * 86400;
        let groups: [(i64, usize); 4] = [(15, 3), (10, 4), (60, 12), (300, 6)];
        for (every, count) in groups {
            let items: Vec<Item> = (0..count)
                .map(|i| item(&format!("g{every}_{i}"), State::Ok))
                .collect();
            let mut t = now - 6 * 3600;
            while t < now {
                s.record(t, &items).unwrap();
                t += every;
            }
        }
        s.conn
            .execute(
                "WITH RECURSIVE m(ts) AS (SELECT ?1 UNION ALL SELECT ts+60 FROM m WHERE ts < ?2),
                 k(k) AS (SELECT 1 UNION ALL SELECT k+1 FROM k WHERE k < 25)
                 INSERT OR REPLACE INTO rollup_1m SELECT ts, 'item' || k, 4, 'ok', 0 FROM m, k",
                params![now - 30 * 86400, now],
            )
            .unwrap();
        s.maintain(now).unwrap();
        s.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").unwrap();
        let size = std::fs::metadata(&path).unwrap().len() as i64;
        let rollups = s.count("rollup_1m");
        assert!(rollups > 0);
        assert!(size * 4 < CAP_BYTES, "{size}");
        assert_eq!(s.samples_between("g15_0", 0, now - 6 * 3600), 0);
        std::fs::remove_dir_all(dir).ok();
    }
}
