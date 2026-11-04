//! `RemoteTaskProvider` implementation backed by `caldav-client` (Radicale).
//!
//! Unlike the OAuth providers, a CalDAV connection is identified by a base URL
//! plus HTTP Basic credentials, so the [`CalDavClient`] is built once per
//! connection (see [`RadicaleProvider::from_credentials`]) and reused for every
//! call instead of being reconstructed from a token on each request.

use caldav_client::{CalDavClient, CalDavError, RemoteVtodo, Vtodo, VtodoStatus};
use jiff::Timestamp;
use uuid::Uuid;

use super::model::{RemoteList, RemoteTask, RemoteTaskDraft};
use super::provider::{ProviderCredentials, ProviderError, ProviderResult, RemoteTaskProvider};

/// CalDAV-backed task provider.
///
/// Nothing constructs it yet: CalDAV connections are not COSMIC accounts, so
/// `sync.rs::provider_for` returns `None` for `"radicale"` until A2 wires the
/// connection list into the sync loop.
#[allow(dead_code)]
pub struct RadicaleProvider {
    client: CalDavClient,
}

#[allow(dead_code)]
impl RadicaleProvider {
    pub fn new(client: CalDavClient) -> Self {
        Self { client }
    }

    /// Builds a provider for one CalDAV connection from HTTP Basic credentials.
    pub fn from_credentials(
        base_url: &str,
        credentials: &ProviderCredentials,
    ) -> ProviderResult<Self> {
        match credentials {
            ProviderCredentials::Basic { username, password } => {
                let client = CalDavClient::new(base_url, username.clone(), password.clone())?;
                Ok(Self::new(client))
            }
            ProviderCredentials::Bearer(_) => Err(ProviderError::UnsupportedCredentials),
        }
    }
}

/// Maps a stored VTODO to the provider-agnostic shape. The href doubles as
/// `remote_id` because CalDAV addresses resources by URL, not by UID.
fn vtodo_to_remote(href: &str, vtodo: &Vtodo) -> RemoteTask {
    let completed =
        vtodo.status == Some(VtodoStatus::Completed) || vtodo.percent_complete == Some(100);

    RemoteTask {
        remote_id: href.to_string(),
        title: vtodo.summary.clone(),
        notes: vtodo.description.clone().unwrap_or_default(),
        due_date: vtodo.due,
        completed,
        updated_at: vtodo.last_modified,
        // PRIORITY:0 means "undefined" in RFC 5545, so it is not a real value.
        priority: vtodo.priority.filter(|priority| *priority > 0),
        tags: vtodo.categories.clone(),
        percent_complete: vtodo.percent_complete,
        parent_remote_id: vtodo.parent_uid.clone(),
        recurrence: vtodo.rrule.clone(),
    }
}

/// Applies a draft onto an existing VTODO, leaving fields the draft does not
/// carry (UID, extra properties, creation date) untouched.
fn apply_draft(draft: &RemoteTaskDraft, vtodo: &mut Vtodo) {
    vtodo.summary = draft.title.clone();
    // An empty description must not produce a dangling `DESCRIPTION:` line.
    vtodo.description = if draft.notes.is_empty() {
        None
    } else {
        Some(draft.notes.clone())
    };
    vtodo.due = draft.due_date;

    if draft.completed {
        vtodo.status = Some(VtodoStatus::Completed);
        vtodo.percent_complete = Some(100);
        vtodo.completed_at.get_or_insert_with(Timestamp::now);
    } else {
        vtodo.status = Some(VtodoStatus::NeedsAction);
        vtodo.percent_complete = draft.percent_complete;
        vtodo.completed_at = None;
    }

    vtodo.priority = draft.priority.filter(|priority| *priority > 0);
    vtodo.categories = draft.tags.clone();
    vtodo.parent_uid = draft.parent_remote_id.clone();
    vtodo.rrule = draft.recurrence.clone();
}

#[allow(dead_code)]
#[async_trait::async_trait]
impl RemoteTaskProvider for RadicaleProvider {
    fn name(&self) -> &'static str {
        "radicale"
    }

    async fn create_list(
        &self,
        _credentials: &ProviderCredentials,
        _name: &str,
    ) -> ProviderResult<RemoteList> {
        // Calendars are created on the server (or in A3's settings UI), never
        // implicitly by the sync engine.
        Err(ProviderError::Unsupported)
    }

    async fn delete_list(
        &self,
        _credentials: &ProviderCredentials,
        _remote_id: &str,
    ) -> ProviderResult<()> {
        Err(ProviderError::Unsupported)
    }

    async fn list_lists(
        &self,
        _credentials: &ProviderCredentials,
    ) -> ProviderResult<Vec<RemoteList>> {
        let calendars = self.client.list_calendars().await?;
        Ok(calendars
            .into_iter()
            .map(|calendar| RemoteList {
                title: calendar
                    .display_name
                    .clone()
                    .unwrap_or_else(|| calendar.href.clone()),
                remote_id: calendar.href,
            })
            .collect())
    }

    async fn update_list(
        &self,
        _credentials: &ProviderCredentials,
        _remote_id: &str,
        _name: &str,
    ) -> ProviderResult<()> {
        Err(ProviderError::Unsupported)
    }

    async fn create_task(
        &self,
        _credentials: &ProviderCredentials,
        list_remote_id: &str,
        task: &RemoteTaskDraft,
    ) -> ProviderResult<RemoteTask> {
        let uid = Uuid::new_v4().to_string();
        let mut vtodo = Vtodo::new(&uid, &task.title);
        apply_draft(task, &mut vtodo);
        vtodo.created = Some(Timestamp::now());

        self.client.put_vtodo(list_remote_id, &vtodo, None).await?;

        Ok(vtodo_to_remote(
            &CalDavClient::vtodo_href(list_remote_id, &uid),
            &vtodo,
        ))
    }

    async fn delete_task(
        &self,
        _credentials: &ProviderCredentials,
        _list_remote_id: &str,
        remote_id: &str,
    ) -> ProviderResult<()> {
        match self.client.get_vtodo(remote_id).await {
            Ok(RemoteVtodo { etag, .. }) => {
                self.client.delete_vtodo(remote_id, etag.as_deref()).await?;
                Ok(())
            }
            // Already gone: deleting is idempotent for the sync engine.
            Err(CalDavError::NotFound(_)) => Ok(()),
            Err(err) => Err(err.into()),
        }
    }

    async fn list_tasks(
        &self,
        _credentials: &ProviderCredentials,
        list_remote_id: &str,
    ) -> ProviderResult<Vec<RemoteTask>> {
        let remote = self.client.list_vtodos(list_remote_id).await?;
        Ok(remote
            .into_iter()
            .map(|item| vtodo_to_remote(&item.href, &item.vtodo))
            .collect())
    }

    async fn update_task(
        &self,
        _credentials: &ProviderCredentials,
        list_remote_id: &str,
        remote_id: &str,
        task: &RemoteTaskDraft,
    ) -> ProviderResult<()> {
        // GET+PUT because the trait carries no ETag: correct, but not atomic.
        // A2 fixes this by passing the ETag through the draft.
        let RemoteVtodo {
            etag, mut vtodo, ..
        } = self.client.get_vtodo(remote_id).await?;
        apply_draft(task, &mut vtodo);

        // `put_vtodo` derives the target href from `vtodo.uid`. Tasks this
        // client created live at `vtodo_href(calendar, uid)`, which is exactly
        // `remote_id`; resources written by another client under a different
        // file name would land on a second href (A2 passes the href directly).
        self.client
            .put_vtodo(list_remote_id, &vtodo, etag.as_deref())
            .await?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_remote() -> RemoteTask {
        RemoteTask {
            remote_id: "/cesar/tareas/8f14e45f.ics".to_string(),
            title: "Comprar leche, pan y huevos".to_string(),
            notes: "Línea uno\nLínea dos".to_string(),
            due_date: Some("2026-11-15".parse().unwrap()),
            completed: true,
            priority: Some(1),
            tags: vec!["casa".to_string(), "compras".to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn round_trips_remote_task_through_vtodo() {
        let original = sample_remote();

        let mut vtodo = Vtodo::new("uid-1", &original.title);
        apply_draft(&RemoteTaskDraft::from(&original), &mut vtodo);
        // Serialize and re-parse to also cover escaping/folding.
        let reparsed = Vtodo::from_ical(&vtodo.to_ical()).unwrap();
        let back = vtodo_to_remote(&original.remote_id, &reparsed);

        assert_eq!(back.remote_id, original.remote_id);
        assert_eq!(back.title, original.title);
        assert_eq!(back.notes, original.notes);
        assert_eq!(back.due_date, original.due_date);
        assert_eq!(back.priority, original.priority);
        assert_eq!(back.tags, original.tags);
        assert_eq!(back.completed, original.completed);
    }

    #[test]
    fn completed_tasks_are_written_as_completed() {
        let mut vtodo = Vtodo::new("uid-2", "Terminada");
        apply_draft(
            &RemoteTaskDraft {
                title: "Terminada".to_string(),
                completed: true,
                ..Default::default()
            },
            &mut vtodo,
        );

        let ical = vtodo.to_ical();
        assert!(ical.contains("STATUS:COMPLETED"), "ical: {ical}");
        assert!(ical.contains("PERCENT-COMPLETE:100"), "ical: {ical}");
        assert!(vtodo_to_remote("/c/x.ics", &vtodo).completed);
    }

    #[test]
    fn in_process_task_is_not_completed() {
        let ical = "BEGIN:VCALENDAR\r\n\
VERSION:2.0\r\n\
BEGIN:VTODO\r\n\
UID:uid-3\r\n\
SUMMARY:A medias\r\n\
STATUS:IN-PROCESS\r\n\
PERCENT-COMPLETE:50\r\n\
END:VTODO\r\n\
END:VCALENDAR\r\n";
        let vtodo = Vtodo::from_ical(ical).unwrap();
        let remote = vtodo_to_remote("/c/x.ics", &vtodo);

        assert!(!remote.completed);
        assert_eq!(remote.percent_complete, Some(50));
    }

    #[test]
    fn empty_notes_do_not_emit_description() {
        let mut vtodo = Vtodo::new("uid-4", "Sin notas");
        apply_draft(
            &RemoteTaskDraft {
                title: "Sin notas".to_string(),
                ..Default::default()
            },
            &mut vtodo,
        );

        let ical = vtodo.to_ical();
        assert!(!ical.contains("DESCRIPTION"), "ical: {ical}");

        // …and the reverse direction yields an empty string, not `None`.
        assert_eq!(vtodo_to_remote("/c/x.ics", &vtodo).notes, "");
    }
}
