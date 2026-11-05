use caldav_client::CalDavClient;
use cosmic::app;
use uuid::Uuid;

use crate::{
    app::{AppModel, Message},
    config::RadicaleConnection,
    fl,
    shared::{secrets, store::source::TaskSource},
};

use super::{
    action::CaldavAction,
    form::{needs_full_rescan, CalendarChoice, CaldavForm, TestFailure, TestState},
};

impl AppModel {
    pub fn update_caldav(&mut self, action: CaldavAction) -> app::Task<Message> {
        match action {
            CaldavAction::New => {
                self.caldav_form = CaldavForm::blank();
            }
            CaldavAction::Edit(id) => {
                if let Some(connection) = self.config.radicale_connections.iter().find(|c| c.id == id)
                {
                    self.caldav_form = CaldavForm::for_connection(connection);
                }
            }
            CaldavAction::CloseForm => {
                self.caldav_form = CaldavForm::default();
            }
            CaldavAction::TextChanged(field, value) => {
                use super::form::Field;
                match field {
                    Field::Name => self.caldav_form.name = value,
                    Field::Url => {
                        self.caldav_form.base_url = value;
                        self.caldav_form.invalidate_test();
                    }
                    Field::Username => {
                        self.caldav_form.username = value;
                        self.caldav_form.invalidate_test();
                    }
                }
            }
            CaldavAction::PasswordChanged(password) => {
                self.caldav_form.password = password;
                self.caldav_form.invalidate_test();
            }
            CaldavAction::TogglePasswordVisible => {
                self.caldav_form.show_password = !self.caldav_form.show_password;
            }
            CaldavAction::TestConnection => {
                if !self.caldav_form.can_test() {
                    return app::Task::none();
                }
                self.caldav_form.test = TestState::Testing;
                let url = self.caldav_form.base_url.trim().to_string();
                let username = self.caldav_form.username.trim().to_string();
                let typed_password = self.caldav_form.password.expose().to_string();
                let editing = self.caldav_form.editing;
                return cosmic::task::future(async move {
                    Message::Caldav(CaldavAction::TestFinished(
                        test_connection(url, username, typed_password, editing).await,
                    ))
                });
            }
            CaldavAction::TestFinished(result) => match result {
                Ok(calendars) => self.caldav_form.apply_test_result(calendars),
                Err(failure) => self.caldav_form.test = TestState::Failed(failure),
            },
            CaldavAction::SelectCalendar(index) => {
                if let TestState::Ok { calendars, selected } = &mut self.caldav_form.test {
                    if index < calendars.len() {
                        *selected = index;
                    }
                }
            }
            CaldavAction::Save => {
                if !self.caldav_form.can_save() {
                    return app::Task::none();
                }
                let Some(connection) = self.caldav_form.to_connection() else {
                    return app::Task::none();
                };
                self.caldav_form.saving = true;
                let typed_password = self.caldav_form.password.expose().to_string();
                return cosmic::task::future(async move {
                    // Password first: if the keyring refuses, the connection
                    // must not be saved (there is no plain-text fallback).
                    let result = if typed_password.is_empty() {
                        Ok(connection)
                    } else {
                        secrets::save_password(connection.id, &connection.name, &typed_password)
                            .await
                            .map(|()| connection)
                            .map_err(|err| err.to_string())
                    };
                    Message::Caldav(CaldavAction::SaveFinished(result))
                });
            }
            CaldavAction::SaveFinished(result) => {
                self.caldav_form.saving = false;
                match result {
                    Err(err) => {
                        tracing::error!("caldav: could not save the password: {err}");
                        self.caldav_form.test = TestState::Failed(TestFailure::Keyring(err));
                    }
                    Ok(connection) => return self.persist_connection(connection),
                }
            }
            CaldavAction::AskDelete(id) => self.caldav_form.pending_delete = Some(id),
            CaldavAction::CancelDelete => self.caldav_form.pending_delete = None,
            CaldavAction::ConfirmDelete(id) => return self.delete_connection(id),
            CaldavAction::DeleteFinished(error) => {
                if let Some(error) = error {
                    // Not fatal: the connection is already gone; an orphan
                    // keyring entry is harmless and the user can be told.
                    tracing::warn!("caldav: could not remove the saved password: {error}");
                }
            }
        }
        app::Task::none()
    }

    fn persist_connection(&mut self, connection: RadicaleConnection) -> app::Task<Message> {
        let mut connections = self.config.radicale_connections.clone();
        let previous = connections.iter().find(|c| c.id == connection.id).cloned();

        match connections.iter_mut().find(|c| c.id == connection.id) {
            Some(existing) => *existing = connection.clone(),
            None => connections.push(connection.clone()),
        }
        if let Err(err) = self
            .config
            .set_radicale_connections(&self.handler, connections)
        {
            tracing::error!("caldav: failed to save connections: {err}");
            self.caldav_form.test = TestState::Failed(TestFailure::Other(err.to_string()));
            return app::Task::none();
        }

        // Pointing at different data: forget the incremental state and the
        // calendar the local list was linked to, so the next pass is a full
        // scan against the new target.
        if previous
            .as_ref()
            .is_some_and(|old| needs_full_rescan(old, &connection))
        {
            self.relink_local_list(&connection);
        }

        self.caldav_form = CaldavForm::default();
        let toast = self
            .toasts
            .push(cosmic::widget::Toast::new(fl!("caldav-saved")))
            .map(cosmic::Action::App);
        app::Task::batch(vec![
            toast,
            cosmic::task::message(Message::Accounts(
                crate::features::accounts::AccountsAction::SyncNow,
            )),
        ])
    }

    fn relink_local_list(&self, connection: &RadicaleConnection) {
        let Some(href) = connection.calendar_href.as_deref() else {
            return;
        };
        let href = CalDavClient::relative_path(href).to_string();
        let lists = self.store.lists().load_all().unwrap_or_default();
        for list in lists.iter().filter(|l| {
            matches!(l.source, TaskSource::Radicale { .. })
                && l.source.account_id() == Some(connection.id)
        }) {
            let result = self.store.lists().update(list.id, |l| {
                l.sync_token = None;
                l.source = TaskSource::Radicale {
                    account_id: connection.id,
                    remote_id: href.clone(),
                };
            });
            if let Err(err) = result {
                tracing::error!("caldav: failed to relink the local list: {err}");
            }
        }
    }

    fn delete_connection(&mut self, id: Uuid) -> app::Task<Message> {
        self.caldav_form.pending_delete = None;
        let connections: Vec<_> = self
            .config
            .radicale_connections
            .iter()
            .filter(|c| c.id != id)
            .cloned()
            .collect();
        if let Err(err) = self
            .config
            .set_radicale_connections(&self.handler, connections)
        {
            tracing::error!("caldav: failed to remove the connection: {err}");
            return app::Task::none();
        }
        if self.caldav_form.editing == Some(id) {
            self.caldav_form = CaldavForm::default();
        }

        // Local mirror → trash (never the server).
        let lists = self.store.lists().load_all().unwrap_or_default();
        for list in lists.iter().filter(|l| {
            matches!(l.source, TaskSource::Radicale { .. }) && l.source.account_id() == Some(id)
        }) {
            if let Err(err) = self.store.trash().trash_list(list.id) {
                tracing::error!("caldav: failed to trash the local list: {err}");
            }
        }

        app::Task::batch(vec![
            cosmic::task::future(async move {
                let error = secrets::delete_password(id).await.err().map(|e| e.to_string());
                Message::Caldav(CaldavAction::DeleteFinished(error))
            }),
            cosmic::task::message(Message::Tasks(
                crate::shared::navigation::nav::TasksAction::SyncFromDisk,
            )),
        ])
    }
}

/// Connects with the typed credentials (or the saved password when editing
/// with an empty field) and returns the calendars that can hold tasks.
async fn test_connection(
    url: String,
    username: String,
    typed_password: String,
    editing: Option<Uuid>,
) -> Result<Vec<CalendarChoice>, TestFailure> {
    let password = if !typed_password.is_empty() {
        typed_password
    } else if let Some(id) = editing {
        match secrets::load_password(id).await {
            Ok(Some(saved)) => saved,
            Ok(None) => return Err(TestFailure::MissingPassword),
            Err(err) => return Err(TestFailure::Keyring(err.to_string())),
        }
    } else {
        return Err(TestFailure::MissingPassword);
    };

    let client = CalDavClient::new(&url, username, password)
        .map_err(|err| TestFailure::from_error(&err))?;
    let calendars = client
        .list_calendars()
        .await
        .map_err(|err| TestFailure::from_error(&err))?;

    let choices: Vec<CalendarChoice> = calendars
        .into_iter()
        .filter(|c| c.supports_vtodo)
        .map(|c| CalendarChoice {
            name: c.display_name.clone().unwrap_or_else(|| c.href.clone()),
            href: c.href,
        })
        .collect();
    if choices.is_empty() {
        return Err(TestFailure::NoVtodoCalendars);
    }
    Ok(choices)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Live check of "Test connection" against the designated test server:
    ///
    /// ```bash
    /// set -a; . ~/.config/proper-todo/test.env; set +a
    /// export RADICALE_URL=http://192.168.0.35:5232/cesar/
    /// cargo test --bin tasks test_connection -- --ignored --nocapture
    /// ```
    #[test]
    #[ignore = "requires a running Radicale"]
    fn test_connection_against_a_real_server() {
        let Ok(url) = std::env::var("RADICALE_URL") else {
            eprintln!("skipping: RADICALE_URL not set");
            return;
        };
        let user = std::env::var("RADICALE_USER").unwrap_or_default();
        let pass = std::env::var("RADICALE_PASS").unwrap_or_default();
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let ok = test_connection(url.clone(), user.clone(), pass.clone(), None).await;
            let calendars = ok.expect("valid credentials must connect");
            assert!(!calendars.is_empty());

            let bad = test_connection(url.clone(), user.clone(), "definitely-wrong".into(), None).await;
            assert_eq!(bad, Err(TestFailure::BadCredentials));

            let missing = test_connection(url.clone(), user.clone(), String::new(), None).await;
            assert_eq!(missing, Err(TestFailure::MissingPassword));

            // Nothing listens there: must fail fast and readably, not hang.
            let started = std::time::Instant::now();
            let closed = test_connection("http://127.0.0.1:9/".into(), user, pass, None).await;
            assert!(closed.is_err());
            assert!(started.elapsed() < std::time::Duration::from_secs(35));
        });
    }
}
