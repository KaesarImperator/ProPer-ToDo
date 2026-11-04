//! VTODO <-> iCalendar mapping.
//!
//! Keeps unknown properties in `extra` so a round-trip through this client
//! does not silently drop fields written by other CalDAV clients (Thunderbird,
//! Nextcloud Tasks, Errands, ...).

use jiff::{civil::Date, Timestamp};

use crate::error::{CalDavError, Result};
use crate::ical::{self, Property};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VtodoStatus {
    NeedsAction,
    InProcess,
    Completed,
    Cancelled,
}

impl VtodoStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            VtodoStatus::NeedsAction => "NEEDS-ACTION",
            VtodoStatus::InProcess => "IN-PROCESS",
            VtodoStatus::Completed => "COMPLETED",
            VtodoStatus::Cancelled => "CANCELLED",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_uppercase().as_str() {
            "NEEDS-ACTION" => Some(VtodoStatus::NeedsAction),
            "IN-PROCESS" => Some(VtodoStatus::InProcess),
            "COMPLETED" => Some(VtodoStatus::Completed),
            "CANCELLED" => Some(VtodoStatus::Cancelled),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Vtodo {
    pub uid: String,
    pub summary: String,
    pub description: Option<String>,
    pub status: Option<VtodoStatus>,
    pub percent_complete: Option<u8>,
    pub priority: Option<u8>,
    pub due: Option<Date>,
    pub start: Option<Date>,
    pub completed_at: Option<Timestamp>,
    pub created: Option<Timestamp>,
    pub last_modified: Option<Timestamp>,
    pub categories: Vec<String>,
    pub rrule: Option<String>,
    pub parent_uid: Option<String>,
    /// Properties this client does not model, preserved verbatim.
    pub extra: Vec<Property>,
}

impl Vtodo {
    pub fn new(uid: impl Into<String>, summary: impl Into<String>) -> Self {
        Self {
            uid: uid.into(),
            summary: summary.into(),
            status: Some(VtodoStatus::NeedsAction),
            ..Default::default()
        }
    }

    /// Parses the first `VTODO` found in an iCalendar payload.
    pub fn from_ical(input: &str) -> Result<Self> {
        let lines = ical::unfold(input);
        let start = lines
            .iter()
            .position(|line| line.eq_ignore_ascii_case("BEGIN:VTODO"))
            .ok_or_else(|| CalDavError::Ical("no VTODO component found".into()))?;
        let end = lines[start..]
            .iter()
            .position(|line| line.eq_ignore_ascii_case("END:VTODO"))
            .map(|offset| start + offset)
            .ok_or_else(|| CalDavError::Ical("unterminated VTODO component".into()))?;

        let properties = ical::parse_properties(&lines[start + 1..end]);
        Ok(Self::from_properties(properties))
    }

    fn from_properties(properties: Vec<Property>) -> Self {
        let mut vtodo = Vtodo::default();

        for property in properties {
            match property.name.as_str() {
                "UID" => vtodo.uid = property.value.trim().to_string(),
                "SUMMARY" => vtodo.summary = ical::unescape_text(&property.value),
                "DESCRIPTION" => {
                    vtodo.description = Some(ical::unescape_text(&property.value));
                }
                "STATUS" => vtodo.status = VtodoStatus::parse(&property.value),
                "PERCENT-COMPLETE" => vtodo.percent_complete = property.value.trim().parse().ok(),
                "PRIORITY" => vtodo.priority = property.value.trim().parse().ok(),
                "DUE" => vtodo.due = parse_date(&property.value),
                "DTSTART" => vtodo.start = parse_date(&property.value),
                "COMPLETED" => vtodo.completed_at = parse_utc(&property.value),
                "CREATED" => vtodo.created = parse_utc(&property.value),
                "LAST-MODIFIED" => vtodo.last_modified = parse_utc(&property.value),
                "CATEGORIES" => {
                    vtodo.categories.extend(
                        property
                            .value
                            .split(',')
                            .map(|item| ical::unescape_text(item.trim()))
                            .filter(|item| !item.is_empty()),
                    );
                }
                "RRULE" => vtodo.rrule = Some(property.value.clone()),
                "RELATED-TO"
                    if property
                        .param("RELTYPE")
                        .map(|kind| kind.eq_ignore_ascii_case("PARENT"))
                        .unwrap_or(true) =>
                {
                    vtodo.parent_uid = Some(property.value.trim().to_string());
                }
                _ => vtodo.extra.push(property),
            }
        }

        vtodo
    }

    /// Serializes a complete `VCALENDAR`/`VTODO` payload ready to `PUT`.
    pub fn to_ical(&self) -> String {
        let mut body = String::new();
        let mut push = |line: String| ical::fold_line(&line, &mut body);

        push("BEGIN:VCALENDAR".into());
        push("VERSION:2.0".into());
        push("PRODID:-//ProPer ToDo//CalDAV client//EN".into());
        push("BEGIN:VTODO".into());
        push(format!("UID:{}", self.uid));
        push(format!("SUMMARY:{}", ical::escape_text(&self.summary)));

        if let Some(description) = &self.description {
            if !description.is_empty() {
                push(format!(
                    "DESCRIPTION:{}",
                    ical::escape_text(description)
                ));
            }
        }
        if let Some(status) = self.status {
            push(format!("STATUS:{}", status.as_str()));
        }
        if let Some(percent) = self.percent_complete {
            push(format!("PERCENT-COMPLETE:{percent}"));
        }
        if let Some(priority) = self.priority {
            push(format!("PRIORITY:{priority}"));
        }
        if let Some(due) = self.due {
            push(format!("DUE;VALUE=DATE:{}", format_date(due)));
        }
        if let Some(start) = self.start {
            push(format!("DTSTART;VALUE=DATE:{}", format_date(start)));
        }
        if let Some(completed) = self.completed_at {
            push(format!("COMPLETED:{}", format_utc(completed)));
        }
        if let Some(created) = self.created {
            push(format!("CREATED:{}", format_utc(created)));
        }
        if let Some(modified) = self.last_modified {
            push(format!("LAST-MODIFIED:{}", format_utc(modified)));
        }
        if !self.categories.is_empty() {
            let joined = self
                .categories
                .iter()
                .map(|category| ical::escape_text(category))
                .collect::<Vec<_>>()
                .join(",");
            push(format!("CATEGORIES:{joined}"));
        }
        if let Some(rrule) = &self.rrule {
            push(format!("RRULE:{rrule}"));
        }
        if let Some(parent) = &self.parent_uid {
            push(format!("RELATED-TO;RELTYPE=PARENT:{parent}"));
        }

        for property in &self.extra {
            let mut line = property.name.clone();
            for (key, value) in &property.params {
                line.push(';');
                line.push_str(key);
                line.push('=');
                line.push_str(value);
            }
            line.push(':');
            line.push_str(&property.value);
            push(line);
        }

        push("END:VTODO".into());
        push("END:VCALENDAR".into());
        body
    }
}

fn parse_date(value: &str) -> Option<Date> {
    let value = value.trim();
    let date_part = value.split('T').next()?;
    if date_part.len() != 8 {
        return None;
    }
    let year: i16 = date_part[0..4].parse().ok()?;
    let month: i8 = date_part[4..6].parse().ok()?;
    let day: i8 = date_part[6..8].parse().ok()?;
    Date::new(year, month, day).ok()
}

fn format_date(date: Date) -> String {
    // `Date` displays as YYYY-MM-DD; iCalendar DATE is YYYYMMDD.
    date.to_string().replace('-', "")
}

fn parse_utc(value: &str) -> Option<Timestamp> {
    let value = value.trim();
    if value.len() != 16 || !value.ends_with('Z') || value.as_bytes().get(8) != Some(&b'T') {
        return None;
    }
    let normalized = format!(
        "{}-{}-{}T{}:{}:{}Z",
        &value[0..4],
        &value[4..6],
        &value[6..8],
        &value[9..11],
        &value[11..13],
        &value[13..15],
    );
    normalized.parse::<Timestamp>().ok()
}

fn format_utc(timestamp: Timestamp) -> String {
    // `Timestamp` displays as RFC 3339 (2026-11-04T12:00:00Z, optionally with
    // fractional seconds); iCalendar DATE-TIME wants YYYYMMDDTHHMMSSZ.
    let rfc3339 = timestamp.to_string();
    let without_fraction = rfc3339.split('.').next().unwrap_or(&rfc3339);
    let mut out: String = without_fraction
        .chars()
        .filter(|ch| !matches!(ch, '-' | ':'))
        .collect();
    if !out.ends_with('Z') {
        out.push('Z');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VTODO\r\n\
UID:8f14e45f-ea0f-4f6a-9f0e-1c2d3e4f5a6b\r\n\
SUMMARY:Comprar leche\\, pan y huevos\r\n\
DESCRIPTION:Línea uno\\nLínea dos\r\n\
STATUS:IN-PROCESS\r\n\
PERCENT-COMPLETE:40\r\n\
PRIORITY:5\r\n\
DUE;VALUE=DATE:20261115\r\n\
CATEGORIES:casa,compras\r\n\
RELATED-TO;RELTYPE=PARENT:parent-uid\r\n\
X-CUSTOM:keep me\r\n\
END:VTODO\r\n\
END:VCALENDAR\r\n";

    #[test]
    fn parses_sample_vtodo() {
        let vtodo = Vtodo::from_ical(SAMPLE).unwrap();

        assert_eq!(vtodo.uid, "8f14e45f-ea0f-4f6a-9f0e-1c2d3e4f5a6b");
        assert_eq!(vtodo.summary, "Comprar leche, pan y huevos");
        assert_eq!(vtodo.description.as_deref(), Some("Línea uno\nLínea dos"));
        assert_eq!(vtodo.status, Some(VtodoStatus::InProcess));
        assert_eq!(vtodo.percent_complete, Some(40));
        assert_eq!(vtodo.priority, Some(5));
        assert_eq!(vtodo.due.map(format_date).as_deref(), Some("20261115"));
        assert_eq!(vtodo.categories, vec!["casa", "compras"]);
        assert_eq!(vtodo.parent_uid.as_deref(), Some("parent-uid"));
        assert_eq!(vtodo.extra.len(), 1);
        assert_eq!(vtodo.extra[0].name, "X-CUSTOM");
    }

    #[test]
    fn round_trips_through_serialization() {
        let original = Vtodo::from_ical(SAMPLE).unwrap();
        let reparsed = Vtodo::from_ical(&original.to_ical()).unwrap();

        assert_eq!(original.uid, reparsed.uid);
        assert_eq!(original.summary, reparsed.summary);
        assert_eq!(original.description, reparsed.description);
        assert_eq!(original.status, reparsed.status);
        assert_eq!(original.priority, reparsed.priority);
        assert_eq!(original.due, reparsed.due);
        assert_eq!(original.categories, reparsed.categories);
        assert_eq!(original.parent_uid, reparsed.parent_uid);
    }

    #[test]
    fn rejects_payload_without_vtodo() {
        let err = Vtodo::from_ical("BEGIN:VCALENDAR\r\nEND:VCALENDAR\r\n").unwrap_err();
        assert!(matches!(err, CalDavError::Ical(_)));
    }
}
