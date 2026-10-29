//! Time the user says they were away from work, by day. Fitted days keep entries out of it and
//! do not count it as presence.

use std::collections::BTreeMap;

use anyhow::{anyhow, bail, Result};
use chrono::{Local, NaiveDate, NaiveDateTime, NaiveTime, TimeZone};

use crate::store::Store;

const KEY: &str = "away";

/// Day → `(start, end)` local times as `HH:MM`.
pub type AwayDays = BTreeMap<String, Vec<(String, String)>>;

pub fn load(store: &Store) -> Result<AwayDays> {
    Ok(store.setting(KEY)?.map(|v| serde_json::from_str(&v)).transpose()?.unwrap_or_default())
}

/// Records that the user was away on `day` from `start` to `end` (`HH:MM`, local).
pub fn add(store: &Store, day: &str, start: &str, end: &str) -> Result<()> {
    let (s, e) = (local_ms(day, start)?, local_ms(day, end)?);
    if e <= s {
        bail!("away time must end after it starts");
    }
    let mut days = load(store)?;
    let spans = days.entry(day.to_string()).or_default();
    spans.push((start.to_string(), end.to_string()));
    spans.sort();
    store.set_setting(KEY, &serde_json::to_string(&days)?)
}

/// Forgets the away time recorded on `day`.
pub fn clear(store: &Store, day: &str) -> Result<()> {
    let mut days = load(store)?;
    days.remove(day);
    store.set_setting(KEY, &serde_json::to_string(&days)?)
}

/// Away spans on the days from `from` to `to`, inclusive.
/// @returns day → sorted `(start_ms, end_ms)` spans.
pub fn spans_ms(store: &Store, from: &str, to: &str) -> Result<BTreeMap<String, Vec<(i64, i64)>>> {
    let mut out = BTreeMap::new();
    for (day, spans) in load(store)?.into_iter().filter(|(d, _)| d.as_str() >= from && d.as_str() <= to) {
        let ms = spans.iter().map(|(s, e)| Ok((local_ms(&day, s)?, local_ms(&day, e)?))).collect::<Result<Vec<_>>>()?;
        out.insert(day, ms);
    }
    Ok(out)
}

fn local_ms(day: &str, time: &str) -> Result<i64> {
    let at = NaiveDateTime::new(NaiveDate::parse_from_str(day, "%Y-%m-%d")?, NaiveTime::parse_from_str(time, "%H:%M")?);
    Ok(Local.from_local_datetime(&at).earliest().ok_or_else(|| anyhow!("{day} {time} does not exist locally"))?.timestamp_millis())
}
