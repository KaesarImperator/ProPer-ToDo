use uuid::Uuid;

use crate::config::RadicaleConnection;

use super::form::{CalendarChoice, Field, Sensitive, TestFailure};

#[derive(Clone, Debug)]
pub enum CaldavAction {
    /// Opens the form for a new connection.
    New,
    /// Opens the form pre-filled with an existing connection.
    Edit(Uuid),
    CloseForm,
    TextChanged(Field, String),
    PasswordChanged(Sensitive),
    TogglePasswordVisible,
    TestConnection,
    TestFinished(Result<Vec<CalendarChoice>, TestFailure>),
    SelectCalendar(usize),
    Save,
    SaveFinished(Result<RadicaleConnection, String>),
    /// First click on "Delete": asks for confirmation inline.
    AskDelete(Uuid),
    CancelDelete,
    ConfirmDelete(Uuid),
    /// Keyring cleanup finished; `Some` carries a loggable error.
    DeleteFinished(Option<String>),
}
