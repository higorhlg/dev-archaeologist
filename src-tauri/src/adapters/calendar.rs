//! Meetings from the macOS Calendar app through EventKit. Any account already added
//! to Calendar (Google Workspace, iCloud, Exchange) works; nothing logs in here.

use chrono::{DateTime, TimeZone, Utc};
use serde::Serialize;
use serde_json::json;

use crate::event::{clip, kind, WorkEvent};

pub const SOURCE: &str = "calendar";
/// Meetings carry a pseudo path, so the work/personal rules can read the account.
pub const PATH_PREFIX: &str = "calendar://";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Access {
    NotAsked,
    Granted,
    Denied,
    Unsupported,
}

/// One calendar event as EventKit reports it, before the meeting rules run.
#[derive(Debug, Clone)]
pub struct RawEvent {
    pub id: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    pub busy: bool,
    pub declined: bool,
    pub attendees: usize,
    pub has_video_link: bool,
    pub calendar: String,
    pub account: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalendarInfo {
    pub title: String,
    pub account: String,
}

/// A meeting is a busy, accepted, timed event with other people or a video link.
/// Focus blocks, reminders and all-day events are not meetings.
pub fn is_meeting(e: &RawEvent) -> bool {
    !e.all_day && e.busy && !e.declined && (e.attendees >= 2 || e.has_video_link) && e.end > e.start
}

pub fn has_video_link(text: &str) -> bool {
    ["meet.google.com", "zoom.us/", "teams.microsoft.com", "teams.live.com", "webex.com", "whereby.com", "around.co"]
        .iter()
        .any(|d| text.contains(d))
}

/// One meeting becomes one event that lasts the meeting. A recurring meeting has one id,
/// so the start time joins the source id.
pub fn to_work_event(e: &RawEvent) -> WorkEvent {
    let mut w = WorkEvent::new(SOURCE, format!("{}@{}", e.id, e.start.timestamp()), e.start, kind::MEETING);
    w.detail = Some(clip(&e.title, 160));
    w.path = Some(format!("{PATH_PREFIX}{}", e.account));
    w.meta = json!({
        "dur_ms": (e.end - e.start).num_milliseconds(),
        "attendees": e.attendees,
        "calendar": e.calendar,
    });
    w
}

/// Meetings between two times, from calendars the user did not exclude.
pub fn meetings(from: DateTime<Utc>, to: DateTime<Utc>, excluded_calendars: &[String]) -> Vec<WorkEvent> {
    sys::events(from, to)
        .into_iter()
        .filter(|e| !excluded_calendars.contains(&e.calendar))
        .filter(is_meeting)
        .map(|e| to_work_event(&e))
        .collect()
}

pub use sys::{access, calendars, request_access};

fn ts(secs: f64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt((secs * 1000.0) as i64).single().unwrap_or_default()
}

#[cfg(target_os = "macos")]
mod sys {
    use std::sync::mpsc;

    use block2::RcBlock;
    use chrono::{DateTime, Utc};
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_event_kit::{
        EKAuthorizationStatus, EKEntityType, EKEvent, EKEventAvailability, EKEventStore, EKParticipantStatus,
    };
    use objc2_foundation::{NSDate, NSError};

    use super::{has_video_link, ts, Access, CalendarInfo, RawEvent};

    fn store() -> Retained<EKEventStore> {
        unsafe { EKEventStore::new() }
    }

    pub fn access() -> Access {
        let status = unsafe { EKEventStore::authorizationStatusForEntityType(EKEntityType::Event) };
        match status {
            EKAuthorizationStatus::NotDetermined => Access::NotAsked,
            EKAuthorizationStatus::FullAccess => Access::Granted,
            _ => Access::Denied,
        }
    }

    /// Shows the macOS Calendar permission prompt once and waits for the answer.
    pub fn request_access() -> Access {
        let (tx, rx) = mpsc::channel();
        let block = RcBlock::new(move |granted: Bool, _err: *mut NSError| {
            let _ = tx.send(granted.as_bool());
        });
        let s = store();
        unsafe { s.requestFullAccessToEventsWithCompletion(&*block as *const _ as *mut _) };
        match rx.recv_timeout(std::time::Duration::from_secs(120)) {
            Ok(true) => Access::Granted,
            _ => Access::Denied,
        }
    }

    pub fn calendars() -> Vec<CalendarInfo> {
        if access() != Access::Granted {
            return vec![];
        }
        let s = store();
        let list = unsafe { s.calendarsForEntityType(EKEntityType::Event) };
        list.iter()
            .map(|c| unsafe {
                CalendarInfo {
                    title: c.title().to_string(),
                    account: c.source().map(|src| src.title().to_string()).unwrap_or_default(),
                }
            })
            .collect()
    }

    fn read(e: &EKEvent) -> Option<RawEvent> {
        unsafe {
            let start = e.startDate().timeIntervalSince1970();
            let end = e.endDate().timeIntervalSince1970();
            let attendees = e.attendees();
            let declined = attendees.as_ref().is_some_and(|list| {
                list.iter().any(|p| p.isCurrentUser() && p.participantStatus() == EKParticipantStatus::Declined)
            });
            let text = [
                e.location().map(|l| l.to_string()),
                e.notes().map(|n| n.to_string()),
                e.URL().and_then(|u| u.absoluteString()).map(|u| u.to_string()),
            ]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" ");
            let calendar = e.calendar();
            Some(RawEvent {
                id: e.eventIdentifier().map(|i| i.to_string()).unwrap_or_default(),
                title: e.title().to_string(),
                start: ts(start),
                end: ts(end),
                all_day: e.isAllDay(),
                busy: e.availability() != EKEventAvailability::Free,
                declined,
                attendees: attendees.map(|a| a.count()).unwrap_or(0),
                has_video_link: has_video_link(&text),
                calendar: calendar.as_ref().map(|c| c.title().to_string()).unwrap_or_default(),
                account: calendar.and_then(|c| c.source()).map(|s| s.title().to_string()).unwrap_or_default(),
            })
        }
    }

    pub fn events(from: DateTime<Utc>, to: DateTime<Utc>) -> Vec<RawEvent> {
        if access() != Access::Granted {
            return vec![];
        }
        let s = store();
        unsafe {
            let start = NSDate::dateWithTimeIntervalSince1970(from.timestamp() as f64);
            let end = NSDate::dateWithTimeIntervalSince1970(to.timestamp() as f64);
            let predicate = s.predicateForEventsWithStartDate_endDate_calendars(&start, &end, None);
            s.eventsMatchingPredicate(&predicate).iter().filter_map(|e| read(&e)).collect()
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod sys {
    use chrono::{DateTime, Utc};

    use super::{Access, CalendarInfo, RawEvent};

    pub fn access() -> Access {
        Access::Unsupported
    }
    pub fn request_access() -> Access {
        Access::Unsupported
    }
    pub fn calendars() -> Vec<CalendarInfo> {
        vec![]
    }
    pub fn events(_: DateTime<Utc>, _: DateTime<Utc>) -> Vec<RawEvent> {
        vec![]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw() -> RawEvent {
        let start = Utc.with_ymd_and_hms(2026, 10, 8, 14, 0, 0).unwrap();
        RawEvent {
            id: "e1".into(),
            title: "SHOP-1261 sync".into(),
            start,
            end: start + chrono::Duration::minutes(30),
            all_day: false,
            busy: true,
            declined: false,
            attendees: 3,
            has_video_link: false,
            calendar: "Work".into(),
            account: "me@company.com".into(),
        }
    }

    #[test]
    fn meeting_rules() {
        assert!(is_meeting(&raw()));
        assert!(!is_meeting(&RawEvent { declined: true, ..raw() }));
        assert!(!is_meeting(&RawEvent { all_day: true, ..raw() }));
        assert!(!is_meeting(&RawEvent { busy: false, ..raw() }));
        assert!(!is_meeting(&RawEvent { attendees: 0, ..raw() }), "a solo focus block");
        assert!(is_meeting(&RawEvent { attendees: 0, has_video_link: true, ..raw() }));
    }

    #[test]
    fn recurring_meetings_get_one_event_each() {
        let a = to_work_event(&raw());
        let b = to_work_event(&RawEvent { start: raw().start + chrono::Duration::days(7), end: raw().end + chrono::Duration::days(7), ..raw() });
        assert_ne!(a.source_id, b.source_id);
        assert_eq!(a.meta_i64("dur_ms"), Some(30 * 60 * 1000));
        assert_eq!(a.path.as_deref(), Some("calendar://me@company.com"));
    }
}
