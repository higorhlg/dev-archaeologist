//! What the user's own Toggl history says about how they file tickets, read from a Toggl
//! "Detailed report" CSV export.

use std::collections::{BTreeMap, HashMap};

use anyhow::{bail, Result};

use crate::store::Store;

const KEY: &str = "logged_epics";

/// Ticket → the epic the user files it under in Toggl.
pub type LoggedEpics = BTreeMap<String, String>;

pub fn load(store: &Store) -> Result<LoggedEpics> {
    Ok(store.setting(KEY)?.map(|v| serde_json::from_str(&v)).transpose()?.unwrap_or_default())
}

/// Reads a Toggl detailed-report CSV and stores, for each ticket its descriptions name, the epic
/// it was logged under for the most time. Replaces what an earlier import stored.
/// @returns the stored ticket → epic map.
pub fn import(store: &Store, csv: &str) -> Result<LoggedEpics> {
    let epics = logged_epics(csv)?;
    store.set_setting(KEY, &serde_json::to_string(&epics)?)?;
    Ok(epics)
}

/// Ticket → epic from the "EPIC TICKET" descriptions of a Toggl detailed-report CSV, weighed by
/// duration. A description may list several tickets after the epic ("SHOP-1 SHOP-2/ SHOP-3"); each
/// takes that epic. Keys are matched without regard to case. Meetings and other descriptions
/// that do not start with two keys teach nothing.
pub fn logged_epics(csv: &str) -> Result<LoggedEpics> {
    let mut rows = csv.trim_start_matches('\u{feff}').lines().map(csv_fields);
    let header = rows.next().unwrap_or_default();
    let col = |name: &str| header.iter().position(|h| h == name);
    let (Some(desc), Some(duration)) = (col("Description"), col("Duration")) else {
        bail!("not a Toggl detailed report: no Description and Duration columns");
    };
    let mut minutes: HashMap<(String, String), i64> = HashMap::new();
    for row in rows {
        let (Some(text), Some(min)) = (row.get(desc), row.get(duration).and_then(|d| duration_min(d))) else { continue };
        let Some((epic, tickets)) = leading_keys(text).split_first().map(|(e, t)| (e.clone(), t.to_vec())) else { continue };
        for t in &tickets {
            *minutes.entry((t.clone(), epic.clone())).or_default() += min;
        }
    }
    let mut best: BTreeMap<String, (i64, String)> = BTreeMap::new();
    for ((ticket, epic), min) in minutes {
        let slot = best.entry(ticket).or_insert((0, String::new()));
        if (min, &epic) > (slot.0, &slot.1) {
            *slot = (min, epic);
        }
    }
    Ok(best.into_iter().map(|(ticket, (_, epic))| (ticket, epic)).collect())
}

/// The Jira keys a description opens with, upper-cased, up to its first word that is not a key.
fn leading_keys(text: &str) -> Vec<String> {
    text.to_uppercase()
        .split(|c: char| c.is_whitespace() || c == '/')
        .filter(|w| !w.is_empty())
        .map_while(|w| (crate::tickets::jira_keys_in(w) == [w]).then(|| w.to_string()))
        .collect()
}

/// `H:MM:SS` → whole minutes.
fn duration_min(d: &str) -> Option<i64> {
    let mut parts = d.split(':').map(|p| p.trim().parse::<i64>());
    match (parts.next()?, parts.next()?, parts.next()) {
        (Ok(h), Ok(m), _) => Some(h * 60 + m),
        _ => None,
    }
}

/// The fields of one CSV line, with quotes removed and doubled quotes unescaped.
fn csv_fields(line: &str) -> Vec<String> {
    let mut fields = vec![String::new()];
    let mut quoted = false;
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' if quoted && chars.peek() == Some(&'"') => {
                chars.next();
                fields.last_mut().unwrap().push('"');
            }
            '"' => quoted = !quoted,
            ',' if !quoted => fields.push(String::new()),
            c => fields.last_mut().unwrap().push(c),
        }
    }
    fields
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn learns_the_epic_each_ticket_is_logged_under_most() {
        let csv = "\u{feff}\"Description\",\"Billable\",\"Duration\"\n\
            \"SHOP-1126 SHOP-1331\",\"Yes\",\"1:30:00\"\n\
            \"SHOP-1199 SHOP-1331\",\"Yes\",\"0:30:00\"\n\
            \"SHOP-1181 SHOP-1188/ SHOP-1190\",\"Yes\",\"1:00:00\"\n\
            \"Shop-1127 SHOP-1329\",\"Yes\",\"0:15:00\"\n\
            \"MEETING-1 - Squad Meeting\",\"Yes\",\"0:30:00\"\n\
            \"MAINT-1 vendor support and SHOP-815\",\"Yes\",\"0:30:00\"\n\
            \"PTO\",\"No\",\"8:00:00\"\n";
        let epics = logged_epics(csv).unwrap();
        assert_eq!(epics["SHOP-1331"], "SHOP-1126", "the epic with the most time wins");
        assert_eq!((epics["SHOP-1188"].as_str(), epics["SHOP-1190"].as_str()), ("SHOP-1181", "SHOP-1181"));
        assert_eq!(epics["SHOP-1329"], "SHOP-1127", "keys are matched without regard to case");
        assert_eq!(epics.len(), 4, "meetings and free text teach nothing: {epics:?}");
    }

    #[test]
    fn rejects_other_csvs() {
        assert!(logged_epics("a,b\n1,2\n").is_err());
    }
}
