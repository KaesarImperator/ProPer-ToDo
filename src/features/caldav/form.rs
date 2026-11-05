//! State and pure rules of the CalDAV connection form. Nothing here touches
//! the network, the keyring or the config, so all of it is unit-testable.

use caldav_client::{CalDavClient, CalDavError};
use uuid::Uuid;

use crate::config::RadicaleConnection;

/// A password typed by the user. `Debug` is redacted because every `Message`
/// derives `Debug` and may end up in a log.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct Sensitive(String);

impl Sensitive {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl std::fmt::Debug for Sensitive {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sensitive([REDACTED])")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Url,
    Username,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CalendarChoice {
    pub href: String,
    pub name: String,
}

/// Why "Test connection" failed, independent of the UI language.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestFailure {
    BadCredentials,
    Timeout,
    InvalidUrl,
    NoVtodoCalendars,
    MissingPassword,
    Keyring(String),
    Server { status: u16 },
    Other(String),
}

impl TestFailure {
    pub fn from_error(err: &CalDavError) -> Self {
        match err {
            CalDavError::Unauthorized { .. } => Self::BadCredentials,
            CalDavError::Url(_) => Self::InvalidUrl,
            CalDavError::Http(inner) if inner.is_timeout() => Self::Timeout,
            CalDavError::Status { status, .. } => Self::Server { status: *status },
            other => Self::Other(other.to_string()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum TestState {
    #[default]
    Idle,
    Testing,
    Ok {
        calendars: Vec<CalendarChoice>,
        selected: usize,
    },
    Failed(TestFailure),
}

/// The add/edit form. No `Debug` derive on purpose (holds the password).
#[derive(Clone, Default)]
pub struct CaldavForm {
    /// Whether the form is shown at all.
    pub open: bool,
    /// `Some` when editing an existing connection.
    pub editing: Option<Uuid>,
    pub name: String,
    pub base_url: String,
    pub username: String,
    pub password: Sensitive,
    pub show_password: bool,
    pub test: TestState,
    /// Calendar to preselect after a successful test (when editing).
    pub preferred_href: Option<String>,
    pub saving: bool,
    pub pending_delete: Option<Uuid>,
}

impl CaldavForm {
    pub fn blank() -> Self {
        Self {
            open: true,
            ..Self::default()
        }
    }

    pub fn for_connection(connection: &RadicaleConnection) -> Self {
        Self {
            open: true,
            editing: Some(connection.id),
            name: connection.name.clone(),
            base_url: connection.base_url.clone(),
            username: connection.username.clone(),
            preferred_href: connection.calendar_href.clone(),
            ..Self::default()
        }
    }

    /// Any edit to the inputs invalidates a previous test result.
    pub fn invalidate_test(&mut self) {
        self.test = TestState::Idle;
    }

    /// `http(s)` URL that the client accepts.
    pub fn url_is_valid(&self) -> bool {
        match CalDavClient::new(self.base_url.trim(), "", "") {
            Ok(client) => matches!(client.base_url().scheme(), "http" | "https")
                && client.base_url().host_str().is_some(),
            Err(_) => false,
        }
    }

    /// Editing may leave the password empty (keep the saved one).
    pub fn password_present(&self) -> bool {
        !self.password.is_empty() || self.editing.is_some()
    }

    pub fn can_test(&self) -> bool {
        self.url_is_valid()
            && !self.username.trim().is_empty()
            && self.password_present()
            && !matches!(self.test, TestState::Testing)
    }

    pub fn selected_calendar(&self) -> Option<&CalendarChoice> {
        match &self.test {
            TestState::Ok {
                calendars,
                selected,
            } => calendars.get(*selected),
            _ => None,
        }
    }

    pub fn can_save(&self) -> bool {
        !self.saving && !self.name.trim().is_empty() && self.selected_calendar().is_some()
    }

    /// Builds the connection to persist (never includes the password).
    pub fn to_connection(&self) -> Option<RadicaleConnection> {
        let calendar = self.selected_calendar()?;
        Some(RadicaleConnection {
            id: self.editing.unwrap_or_else(Uuid::new_v4),
            name: self.name.trim().to_string(),
            base_url: self.base_url.trim().to_string(),
            username: self.username.trim().to_string(),
            calendar_href: Some(calendar.href.clone()),
        })
    }

    /// Applies a successful test: keeps the previously chosen calendar when
    /// it is still there.
    pub fn apply_test_result(&mut self, calendars: Vec<CalendarChoice>) {
        let selected = self
            .preferred_href
            .as_ref()
            .and_then(|href| {
                calendars
                    .iter()
                    .position(|c| same_collection(&c.href, href))
            })
            .unwrap_or(0);
        self.test = TestState::Ok {
            calendars,
            selected,
        };
    }
}

fn same_collection(a: &str, b: &str) -> bool {
    CalDavClient::relative_path(a).trim_end_matches('/')
        == CalDavClient::relative_path(b).trim_end_matches('/')
}

/// Whether saving `new` over `old` points the engine at different data, so
/// the local mirror must be re-scanned from scratch.
pub fn needs_full_rescan(old: &RadicaleConnection, new: &RadicaleConnection) -> bool {
    old.base_url != new.base_url
        || old.username != new.username
        || match (&old.calendar_href, &new.calendar_href) {
            (Some(a), Some(b)) => !same_collection(a, b),
            (None, None) => false,
            _ => true,
        }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn choice(href: &str) -> CalendarChoice {
        CalendarChoice {
            href: href.into(),
            name: href.into(),
        }
    }

    fn filled() -> CaldavForm {
        let mut form = CaldavForm::blank();
        form.name = "Casa".into();
        form.base_url = "http://192.168.0.35:5232/cesar/".into();
        form.username = "cesar".into();
        form.password = Sensitive::new("pw");
        form
    }

    #[test]
    fn errors_map_to_failures() {
        assert_eq!(
            TestFailure::from_error(&CalDavError::Unauthorized { url: "x".into() }),
            TestFailure::BadCredentials
        );
        // `CalDavError::Url` wraps `url::ParseError`; get a real one through
        // the client so the app does not need the `url` crate.
        let url_err = CalDavClient::new("not a url", "", "")
            .err()
            .expect("'not a url' must not parse");
        assert_eq!(TestFailure::from_error(&url_err), TestFailure::InvalidUrl);
        assert_eq!(
            TestFailure::from_error(&CalDavError::Status {
                method: "PROPFIND".into(),
                url: "x".into(),
                status: 503,
                body: String::new(),
            }),
            TestFailure::Server { status: 503 }
        );
        assert!(matches!(
            TestFailure::from_error(&CalDavError::MissingField("href")),
            TestFailure::Other(_)
        ));
    }

    #[test]
    fn url_validation() {
        let mut form = filled();
        assert!(form.url_is_valid());
        form.base_url = "".into();
        assert!(!form.url_is_valid());
        form.base_url = "ftp://servidor/".into();
        assert!(!form.url_is_valid());
        form.base_url = "no es una url".into();
        assert!(!form.url_is_valid());
    }

    #[test]
    fn save_is_blocked_without_a_successful_test() {
        let mut form = filled();
        assert!(form.can_test());
        assert!(!form.can_save(), "nothing tested yet");

        form.apply_test_result(vec![choice("/cesar/tareas/")]);
        assert!(form.can_save());

        form.name = "  ".into();
        assert!(!form.can_save(), "name is required");

        form.name = "Casa".into();
        form.invalidate_test();
        assert!(!form.can_save(), "editing a field invalidates the test");
    }

    #[test]
    fn new_connection_requires_a_password_but_editing_does_not() {
        let mut form = filled();
        form.password = Sensitive::default();
        assert!(!form.can_test());

        form.editing = Some(Uuid::new_v4());
        assert!(form.can_test(), "empty password = keep the saved one");
    }

    #[test]
    fn test_result_preselects_the_previous_calendar() {
        let mut form = filled();
        form.preferred_href = Some("http://host:5232/cesar/b/".into());
        form.apply_test_result(vec![choice("/cesar/a/"), choice("/cesar/b/")]);
        assert_eq!(form.selected_calendar().unwrap().href, "/cesar/b/");
    }

    #[test]
    fn connection_never_carries_the_password() {
        let mut form = filled();
        form.apply_test_result(vec![choice("/cesar/tareas/")]);
        let connection = form.to_connection().unwrap();
        let ron = ron::to_string(&connection).unwrap();
        assert!(!ron.contains("pw"), "{ron}");
        assert!(!ron.to_lowercase().contains("password"), "{ron}");
        assert_eq!(connection.calendar_href.as_deref(), Some("/cesar/tareas/"));
    }

    #[test]
    fn sensitive_debug_is_redacted() {
        let secret = Sensitive::new("hunter2");
        assert!(!format!("{secret:?}").contains("hunter2"));
    }

    #[test]
    fn rescan_only_when_the_target_changes() {
        let old = RadicaleConnection {
            id: Uuid::new_v4(),
            name: "a".into(),
            base_url: "http://h/u/".into(),
            username: "u".into(),
            calendar_href: Some("/u/c/".into()),
        };
        let mut renamed = old.clone();
        renamed.name = "b".into();
        assert!(!needs_full_rescan(&old, &renamed));

        let mut other_calendar = old.clone();
        other_calendar.calendar_href = Some("/u/d/".into());
        assert!(needs_full_rescan(&old, &other_calendar));

        let mut other_user = old.clone();
        other_user.username = "v".into();
        assert!(needs_full_rescan(&old, &other_user));
    }
}
