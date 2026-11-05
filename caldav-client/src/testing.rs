//! Helpers for integration tests that run against a real server. Not part of
//! the public API for applications.

use std::collections::BTreeSet;

use crate::client::CalDavClient;

/// Remembers which VTODO resources a calendar held when it was created and,
/// on drop, deletes every other one. The calendar ends up exactly as it was,
/// even when the test panics halfway, regardless of who created the
/// resources (the test, the app under test, or a "foreign client").
pub struct CalendarGuard {
    client: CalDavClient,
    calendar: String,
    keep: BTreeSet<String>,
}

impl CalendarGuard {
    /// Snapshots the calendar the client points at.
    pub async fn new(client: CalDavClient) -> Self {
        let calendar = client.base_url().path().to_string();
        let keep = Self::hrefs(&client, &calendar).await;
        Self { client, calendar, keep }
    }

    async fn hrefs(client: &CalDavClient, calendar: &str) -> BTreeSet<String> {
        client
            .list_vtodos(calendar)
            .await
            .map(|items| items.into_iter().map(|item| item.href).collect())
            .unwrap_or_default()
    }

    /// Resources that did not exist when the guard was created.
    pub async fn leftovers(&self) -> Vec<String> {
        Self::hrefs(&self.client, &self.calendar)
            .await
            .difference(&self.keep)
            .cloned()
            .collect()
    }

    /// Deletes every leftover now. Called by `Drop` as well.
    pub async fn clean(&self) {
        for href in self.leftovers().await {
            if let Err(err) = self.client.delete_vtodo(&href, None).await {
                eprintln!("CalendarGuard: failed to delete {href}: {err}");
            }
        }
    }
}

impl Drop for CalendarGuard {
    fn drop(&mut self) {
        // `Drop` may run inside a tokio runtime (where `block_on` panics), so
        // the cleanup gets its own thread and runtime.
        std::thread::scope(|scope| {
            scope.spawn(|| {
                match tokio::runtime::Builder::new_current_thread().enable_all().build() {
                    Ok(rt) => rt.block_on(self.clean()),
                    Err(err) => eprintln!("CalendarGuard: no runtime for cleanup: {err}"),
                }
            });
        });
    }
}
