/// Provider-agnostic representation of a remote task list, mapped from
/// either Google Tasks or Microsoft Graph To Do API responses.
#[derive(Clone, Debug)]
pub struct RemoteList {
    pub remote_id: String,
    pub title: String,
}

/// Provider-agnostic representation of a remote task.
///
/// Only `remote_id`, `title`, `notes`, `due_date`, `completed` and
/// `updated_at` are understood by every provider; the fields after them are
/// CalDAV extras that Google/Microsoft leave at their defaults.
#[derive(Clone, Debug, Default)]
pub struct RemoteTask {
    pub completed: bool,
    pub due_date: Option<jiff::civil::Date>,
    pub notes: String,
    pub remote_id: String,
    pub title: String,
    /// Provider's last-modified timestamp, used for last-write-wins conflict
    /// resolution against the local `Task::remote_updated_at`.
    pub updated_at: Option<jiff::Timestamp>,
    /// 1 (highest) .. 9 (lowest); 0 means "undefined" in iCalendar, so it is
    /// mapped to `None` by the CalDAV provider.
    pub priority: Option<u8>,
    /// iCalendar `CATEGORIES`.
    pub tags: Vec<String>,
    pub percent_complete: Option<u8>,
    /// UID of the parent VTODO (`RELATED-TO;RELTYPE=PARENT`).
    pub parent_remote_id: Option<String>,
    /// Raw `RRULE`, kept as text until real recurrence support lands.
    pub recurrence: Option<String>,
    /// The resource's current ETag, as returned by the server. Present on
    /// Calendar entries; `None` on writes (the provider renders it).
    pub etag: Option<String>,
}

/// Fields sent when creating/updating a remote task. Kept separate from
/// `RemoteTask` since providers ignore `remote_id`/`updated_at` on write.
#[derive(Clone, Debug, Default)]
pub struct RemoteTaskDraft {
    pub completed: bool,
    pub due_date: Option<jiff::civil::Date>,
    pub notes: String,
    pub title: String,
    pub priority: Option<u8>,
    pub tags: Vec<String>,
    pub percent_complete: Option<u8>,
    pub parent_remote_id: Option<String>,
    pub recurrence: Option<String>,
    /// ETag last seen for the resource this draft overwrites. When set, the
    /// provider writes with `If-Match`, so a server-side change since the
    /// fetch fails with `PreconditionFailed` instead of clobbering it. When
    /// `None` the provider re-reads the resource and uses its ETag.
    pub etag: Option<String>,
}

/// Incremental changes reported by `RemoteTaskProvider::sync_changes`.
#[derive(Clone, Debug, Default)]
pub struct RemoteChanges {
    pub changed: Vec<RemoteTask>,
    /// `remote_id`s of resources that no longer exist remotely.
    pub removed: Vec<String>,
    /// Token to persist for the next incremental pass; `None` means the
    /// provider has no notion of tokens (full scan will be used).
    pub next_token: Option<String>,
}

impl From<&RemoteTask> for RemoteTaskDraft {
    fn from(task: &RemoteTask) -> Self {
        Self {
            completed: task.completed,
            due_date: task.due_date,
            notes: task.notes.clone(),
            title: task.title.clone(),
            priority: task.priority,
            tags: task.tags.clone(),
            percent_complete: task.percent_complete,
            parent_remote_id: task.parent_remote_id.clone(),
            recurrence: task.recurrence.clone(),
            etag: task.etag.clone(),
        }
    }
}
