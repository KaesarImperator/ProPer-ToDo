use std::collections::BTreeSet;

use accounts::{models::Service, AccountsClient};
use caldav_client::{CalDavClient, CalDavError};
use jiff::Timestamp;
use uuid::Uuid;

use crate::config::{radicale_password, RadicaleConnection};
use crate::features::lists::list::List;
use crate::features::tasks::task::{Priority, Recurrence, Task, TrashedTask};
use crate::shared::providers::model::{RemoteTask, RemoteTaskDraft};
use crate::shared::providers::provider::{ProviderCredentials, ProviderError, RemoteTaskProvider};
use crate::shared::store::source::TaskSource;
use crate::shared::store::store::TaskStore;
use crate::shared::store::Store;

use super::google::GoogleTasksProvider;
use super::microsoft::MicrosoftTodoProvider;
use super::radicale::RadicaleProvider;

#[derive(Clone, Debug)]
pub enum SyncStatus {
    Syncing,
    Idle { at: Timestamp, had_errors: bool },
}

#[derive(Clone, Debug, Default)]
pub struct SyncReport {
    pub accounts_synced: usize,
    pub lists_pulled: usize,
    pub tasks_pulled: usize,
    pub tasks_pushed: usize,
    /// Failures: something did not happen and the user may need to act.
    pub errors: Vec<String>,
    /// Informational: the pass succeeded but made a decision worth showing
    /// (e.g. a conflict resolved in favour of the remote version).
    pub warnings: Vec<String>,
}

impl SyncReport {
    /// Only real failures count; warnings never turn a pass into "error".
    pub fn had_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Folds another report into this one (used to merge the CalDAV engine's
    /// pass into the accounts engine's).
    pub fn merge(&mut self, other: SyncReport) {
        self.accounts_synced += other.accounts_synced;
        self.lists_pulled += other.lists_pulled;
        self.tasks_pulled += other.tasks_pulled;
        self.tasks_pushed += other.tasks_pushed;
        self.errors.extend(other.errors);
        self.warnings.extend(other.warnings);
    }
}

/// Everything the sync engine needs to know about whose data it is touching.
///
/// It deliberately replaces `&Account`: CalDAV connections are not COSMIC
/// accounts, and fabricating one would drag OAuth-only fields into a provider
/// that does not use them.
#[derive(Clone, Copy, Debug)]
pub struct SyncTarget<'a> {
    pub id: Uuid,
    pub provider_id: &'a str,
    pub display_name: &'a str,
}

/// Moves every list (and its tasks) mirrored from an account that is no
/// longer "active" — deleted, disabled, or with the Todo capability turned
/// off in the system Accounts app — into trash. `active_account_ids` should
/// contain exactly the accounts currently eligible to sync (account exists,
/// enabled, Todo capability on); anything remote-sourced outside that set is
/// considered orphaned. This does not consult Tasks' own local on/off
/// switch (`AppConfig::disabled_accounts`): turning an account off within
/// Tasks only pauses sync, it never deletes the local mirror.
pub fn purge_orphaned_accounts(store: &Store, active_account_ids: &BTreeSet<Uuid>) -> usize {
    let lists = match store.lists().load_all() {
        Ok(lists) => lists,
        Err(err) => {
            tracing::error!("Failed to load lists for orphan cleanup: {err}");
            return 0;
        }
    };

    let mut purged = 0;
    for list in lists {
        // Radicale lists are not COSMIC accounts: their connections live in
        // AppConfig, never in the system Accounts app, so they can never be
        // in `active` and would be trashed on every app start.
        if !matches!(
            list.source,
            TaskSource::Google { .. } | TaskSource::Microsoft { .. }
        ) {
            continue;
        }
        let Some(account_id) = list.source.account_id() else {
            continue;
        };
        if !active_account_ids.contains(&account_id) {
            match store.trash().trash_list(list.id) {
                Ok(()) => purged += 1,
                Err(err) => tracing::error!("Failed to trash orphaned list: {err}"),
            }
        }
    }
    purged
}

/// Runs a full two-way sync pass against every account with the Tasks service
/// enabled, excluding any the user has turned off within Tasks (`disabled`).
pub async fn run_sync(
    store: Store,
    mut accounts: AccountsClient,
    disabled: &BTreeSet<Uuid>,
) -> SyncReport {
    let mut report = SyncReport::default();

    let enabled = match accounts.list_enabled_accounts(Service::Tasks).await {
        Ok(accounts) => accounts,
        Err(err) => {
            report
                .errors
                .push(format!("Failed to list accounts: {err}"));
            return report;
        }
    };

    for account in enabled.into_iter().filter(|a| !disabled.contains(&a.id)) {
        let Some(provider) = provider_for(&account.provider) else {
            continue;
        };

        if let Err(err) = accounts.ensure_credentials(&account.id).await {
            report.errors.push(format!(
                "{}: failed to refresh credentials: {err}",
                account.display_name
            ));
            continue;
        }

        let token = match accounts
            .get_access_token(&account.id, &Service::Tasks)
            .await
        {
            Ok((token, _expires_at)) => token,
            Err(err) => {
                report.errors.push(format!(
                    "{}: failed to get access token: {err}",
                    account.display_name
                ));
                continue;
            }
        };

        let credentials = ProviderCredentials::Bearer(token);
        let target = SyncTarget {
            id: account.id,
            provider_id: &account.provider,
            display_name: &account.display_name,
        };

        match sync_account(&store, provider.as_ref(), target, &credentials).await {
            Ok(account_report) => {
                report.merge(account_report);
            }
            Err(err) => {
                report
                    .errors
                    .push(format!("{}: {err}", account.display_name));
            }
        }
    }

    report
}

/// Two-way sync pass for the CalDAV connections registered in Tasks itself.
///
/// Deliberately separate from [`run_sync`]: Radicale connections are not
/// COSMIC accounts, so the account loop never sees them; both engines share
/// [`sync_list_tasks`], which is where all the per-list logic lives.
pub async fn run_radicale_sync(store: &Store, connections: &[RadicaleConnection]) -> SyncReport {
    let mut report = SyncReport::default();

    for connection in connections {
        let Some(password) = radicale_password(connection) else {
            report.errors.push(format!(
                "{}: no password (set PROPER_TODO_RADICALE_PASSWORD)",
                connection.name
            ));
            continue;
        };
        let credentials = ProviderCredentials::Basic {
            username: connection.username.clone(),
            password,
        };

        let provider = match RadicaleProvider::from_credentials(&connection.base_url, &credentials)
        {
            Ok(provider) => provider,
            Err(err) => {
                report.errors.push(format!("{}: {err}", connection.name));
                continue;
            }
        };

        // Calendar to sync: the pinned href, or the first one discovered that
        // supports VTODO (`RemoteList` does not expose `supports_vtodo`, so
        // this goes straight through the client). Whatever shape the user
        // configured, it is normalized to a server-relative href: Radicale
        // reports resource hrefs relative to the server root and the engine
        // matches local mirrors against them.
        let (calendar_href, calendar_name) = match &connection.calendar_href {
            Some(href) => {
                let href = CalDavClient::relative_path(href).to_string();
                (href.clone(), None)
            }
            None => match provider.default_calendar().await {
                Ok(Some(calendar)) => (
                    CalDavClient::relative_path(&calendar.href).to_string(),
                    calendar.display_name,
                ),
                Ok(None) => {
                    report.errors.push(format!(
                        "{}: no VTODO calendar found on the server",
                        connection.name
                    ));
                    continue;
                }
                Err(err) => {
                    report.errors.push(format!("{}: {err}", connection.name));
                    continue;
                }
            },
        };

        let Some((list, created)) = radicale_list(store, connection, &calendar_href, calendar_name)
        else {
            continue;
        };
        if created {
            report.lists_pulled += 1;
        }

        let target = SyncTarget {
            id: connection.id,
            provider_id: provider.name(),
            display_name: &connection.name,
        };

        match sync_list_tasks(
            store,
            &provider,
            target,
            &credentials,
            list.id,
            &calendar_href,
            list.sync_token.as_deref(),
        )
        .await
        {
            Ok((list_report, next_token)) => {
                if let Some(token) = next_token {
                    if let Err(err) =
                        store.lists().update(list.id, |l| l.sync_token = Some(token.clone()))
                    {
                        tracing::error!("Failed to save sync token: {err}");
                    }
                }
                report.merge(list_report);
            }
            Err(err) => {
                report.errors.push(format!("{}: {err}", target.display_name));
            }
        }
    }

    report
}

/// Finds the local list mirroring `connection`'s calendar, creating it on
/// first sync. Matching is by `account_id == connection.id`; the calendar the
/// list tracks is whatever `connection.calendar_href` (or discovery) selects.
/// Returns the list and whether it was just created.
fn radicale_list(
    store: &Store,
    connection: &RadicaleConnection,
    calendar_href: &str,
    calendar_name: Option<String>,
) -> Option<(List, bool)> {
    let lists = store.lists().load_all().unwrap_or_default();
    let existing = lists.iter().find(|l| {
        l.source.account_id() == Some(connection.id)
            && matches!(l.source, TaskSource::Radicale { .. })
    });
    if let Some(list) = existing {
        return Some((list.clone(), false));
    }

    let mut list = List::new(calendar_name.unwrap_or_else(|| connection.name.clone()));
    list.source = TaskSource::Radicale {
        account_id: connection.id,
        remote_id: calendar_href.to_string(),
    };
    if let Err(err) = store.lists().save(&list) {
        tracing::error!("Failed to save Radicale list: {err}");
        return None;
    }
    Some((list, true))
}

/// Maps a local task edit onto the provider draft. This is the single write
/// path for pushes, so all fields the app understands survive the trip.
fn draft_from_task(task: &Task, parent_remote_id: Option<String>) -> RemoteTaskDraft {
    RemoteTaskDraft {
        completed: task.is_completed(),
        due_date: task.due_date,
        notes: task.notes.clone(),
        title: task.title.clone(),
        priority: priority_to_remote(task.priority),
        tags: task.tags.clone(),
        percent_complete: None, // the app has no partial-completion state
        parent_remote_id,
        recurrence: recurrence_to_rrule(&task.recurrence),
        // The engine fills this from the remote state it has seen (or leaves
        // it `None` to let the provider re-read the resource).
        etag: None,
    }
}

/// href of the already-synced parent task, if any (`RELATED-TO` carries the
/// parent's href/`remote_id`, matching how the engine links tasks).
fn parent_remote_id_for(task: &Task, locals: &[Task]) -> Option<String> {
    let parent = task.parent_id.and_then(|parent_id| {
        locals
            .iter()
            .find(|l| l.id == parent_id && !l.source.is_local())
    })?;
    parent.source.remote_id().map(str::to_string)
}

fn priority_to_remote(priority: Priority) -> Option<u8> {
    match priority {
        Priority::High => Some(1),
        Priority::Normal => Some(5),
        Priority::Low => Some(9),
    }
}

/// RFC 5545 iCalendar priority → UI priority. `None` (or the `0`
/// "undefined" value) keeps the local value untouched.
fn priority_from_remote(priority: Option<u8>) -> Option<Priority> {
    match priority {
        Some(1..=4) => Some(Priority::High),
        Some(5) => Some(Priority::Normal),
        Some(6..=9) => Some(Priority::Low),
        _ => None,
    }
}

fn recurrence_to_rrule(recurrence: &Recurrence) -> Option<String> {
    let days: Vec<&str> = [
        (recurrence.monday, "MO"),
        (recurrence.tuesday, "TU"),
        (recurrence.wednesday, "WE"),
        (recurrence.thursday, "TH"),
        (recurrence.friday, "FR"),
        (recurrence.saturday, "SA"),
        (recurrence.sunday, "SU"),
    ]
    .into_iter()
    .filter(|(set, _)| *set)
    .map(|(_, code)| code)
    .collect();

    if days.is_empty() {
        None
    } else {
        Some(format!("FREQ=WEEKLY;BYDAY={}", days.join(",")))
    }
}

/// Weekly `RRULE` → `Recurrence`. Non-weekly rules (or missing `BYDAY`) are
/// not understood by the app yet; `None` keeps the local value untouched.
fn recurrence_from_rrule(rrule: Option<&str>) -> Option<Recurrence> {
    let rrule = rrule?;
    let mut weekly = false;
    let mut recurrence = Recurrence::default();

    for part in rrule.split(';') {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim().to_ascii_uppercase().as_str() {
            "FREQ" => weekly = value.trim().eq_ignore_ascii_case("WEEKLY"),
            "BYDAY" => {
                for day in value.trim().split(',') {
                    match day.trim().to_ascii_uppercase().as_str() {
                        "MO" => recurrence.monday = true,
                        "TU" => recurrence.tuesday = true,
                        "WE" => recurrence.wednesday = true,
                        "TH" => recurrence.thursday = true,
                        "FR" => recurrence.friday = true,
                        "SA" => recurrence.saturday = true,
                        "SU" => recurrence.sunday = true,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    let any_day = recurrence.monday
        || recurrence.tuesday
        || recurrence.wednesday
        || recurrence.thursday
        || recurrence.friday
        || recurrence.saturday
        || recurrence.sunday;

    (weekly && any_day).then_some(recurrence)
}

/// Conflict decision for a task that changed on both sides: `true` means the
/// remote version wins and the local edit is dropped (surfaced with a
/// warning), `false` means the local edit is (re-)pushed.
///
/// Equal timestamps mean "the remote did not move since our last pull", so a
/// local edit is simply a normal push, not a conflict. Without any remote
/// timestamp there is nothing proving the local side was written after the
/// remote state just fetched, so the remote wins.
fn remote_wins(local: Option<Timestamp>, remote: Option<Timestamp>) -> bool {
    match (local, remote) {
        (_, None) => true,
        (None, Some(_)) => true,
        (Some(local_at), Some(remote_at)) => remote_at > local_at,
    }
}

/// Whether the remote copy is newer than what the local task last saw.
fn remote_newer(local: &Task, remote: &RemoteTask) -> bool {
    // ETags are exact; timestamps (1s precision) can miss an edit made in the
    // same second as our last sync. Prefer the ETag whenever both are known.
    if let (Some(local_etag), Some(remote_etag)) =
        (local.remote_etag.as_deref(), remote.etag.as_deref())
    {
        return local_etag != remote_etag;
    }
    match local.remote_updated_at {
        Some(local_at) => remote
            .updated_at
            .is_none_or(|remote_at| remote_at > local_at),
        None => true,
    }
}

fn apply_remote_to_local(
    task_store: &TaskStore<'_>,
    local: Option<&Task>,
    remote: &RemoteTask,
    target: SyncTarget<'_>,
    now: Timestamp,
    report: &mut SyncReport,
) {
    let mut task = match local {
        Some(local) => local.clone(),
        None => {
            let mut task = Task::new(&remote.title);
            task.source =
                make_source(target.provider_id, target.id, remote.remote_id.clone());
            task
        }
    };

    task.title = remote.title.clone();
    task.notes = remote.notes.clone();
    task.due_date = remote.due_date;
    task.completion_date = if remote.completed {
        task.completion_date.or(Some(now))
    } else {
        None
    };
    if let Some(priority) = priority_from_remote(remote.priority) {
        task.priority = priority;
    }
    task.tags = remote.tags.clone();
    if let Some(recurrence) = recurrence_from_rrule(remote.recurrence.as_deref()) {
        task.recurrence = recurrence;
    }

    task.remote_updated_at = remote.updated_at.or(Some(now));
    task.remote_etag = remote.etag.clone();
    task.last_synced_at = Some(now);
    task.dirty = false;
    if let Err(err) = task_store.save_synced(&task) {
        tracing::error!("Failed to apply remote task update: {err}");
        return;
    }
    report.tasks_pulled += 1;
}

fn make_source(provider_id: &str, account_id: Uuid, remote_id: String) -> TaskSource {
    match provider_id {
        "google" => TaskSource::Google {
            account_id,
            remote_id,
        },
        "radicale" => TaskSource::Radicale {
            account_id,
            remote_id,
        },
        _ => TaskSource::Microsoft {
            account_id,
            remote_id,
        },
    }
}

fn matches_account(source: &TaskSource, account_id: Uuid) -> bool {
    source.account_id() == Some(account_id)
}

fn provider_for(id: &str) -> Option<Box<dyn RemoteTaskProvider>> {
    match id {
        "google" => Some(Box::new(GoogleTasksProvider::default())),
        "microsoft" => Some(Box::new(MicrosoftTodoProvider::default())),
        "radicale" => None, // CalDAV connections are handled by `run_radicale_sync`, never by the account loop
        _ => None,
    }
}

async fn sync_account(
    store: &Store,
    provider: &dyn RemoteTaskProvider,
    target: SyncTarget<'_>,
    credentials: &ProviderCredentials,
) -> Result<SyncReport, ProviderError> {
    let mut report = SyncReport::default();

    let remote_lists = provider.list_lists(credentials).await?;
    let local_lists = store.lists().load_all().unwrap_or_default();

    // Pull: new/renamed remote lists, and detect remote deletions.
    for remote_list in &remote_lists {
        let existing = local_lists.iter().find(|l| {
            matches_account(&l.source, target.id)
                && l.source.remote_id() == Some(remote_list.remote_id.as_str())
        });

        let list_id = match existing {
            Some(list) => {
                if list.name != remote_list.title {
                    if let Err(err) = store
                        .lists()
                        .update(list.id, |l| l.name = remote_list.title.clone())
                    {
                        tracing::error!("Failed to update list name: {err}");
                    }
                }
                list.id
            }
            None => {
                let mut list = List::new(&remote_list.title);
                list.source =
                    make_source(target.provider_id, target.id, remote_list.remote_id.clone());
                if let Err(err) = store.lists().save(&list) {
                    report.errors.push(format!("Failed to save list: {err}"));
                    continue;
                }
                report.lists_pulled += 1;
                list.id
            }
        };

        match sync_list_tasks(
            store,
            provider,
            target,
            credentials,
            list_id,
            &remote_list.remote_id,
            None,
        )
        .await
        {
            Ok((list_report, next_token)) => {
                save_list_sync_token(store, list_id, next_token);
                report.merge(list_report);
            }
            Err(err) => report.errors.push(format!("{}: {err}", remote_list.title)),
        }
    }

    // Detect remote list deletions: local lists tracking this account whose
    // remote_id is no longer present get moved to trash (non-destructive).
    for local_list in &local_lists {
        if !matches_account(&local_list.source, target.id) {
            continue;
        }
        let still_exists = remote_lists
            .iter()
            .any(|r| Some(r.remote_id.as_str()) == local_list.source.remote_id());
        if !still_exists {
            if let Err(err) = store.trash().trash_list(local_list.id) {
                tracing::error!("Failed to trash remotely-deleted list: {err}");
            }
        }
    }

    // Push: local lists in this account with no remote_id yet (created
    // locally, e.g. copied) aren't auto-created remotely in this pass to
    // avoid surprising list creation; only tasks within already-linked lists
    // are pushed (see `sync_list_tasks`).

    Ok(report)
}

/// Persists a sync token for the next incremental pass; `None` means "no
/// token this pass", in which case the current one is left alone.
fn save_list_sync_token(store: &Store, list_id: Uuid, token: Option<String>) {
    let Some(token) = token else {
        return;
    };
    if let Err(err) = store.lists().update(list_id, |l| l.sync_token = Some(token.clone())) {
        tracing::error!("Failed to save list sync token: {err}");
    }
}

async fn sync_list_tasks(
    store: &Store,
    provider: &dyn RemoteTaskProvider,
    target: SyncTarget<'_>,
    credentials: &ProviderCredentials,
    list_id: Uuid,
    list_remote_id: &str,
    sync_token: Option<&str>,
) -> Result<(SyncReport, Option<String>), ProviderError> {
    let mut report = SyncReport::default();
    let task_store = store.tasks(list_id);
    let now = Timestamp::now();
    let mut pushed = BTreeSet::new();
    let mut next_token = None;

    if let Some(token) = sync_token {
        match provider.sync_changes(credentials, list_remote_id, token).await? {
            // Incremental pass: the server tells us precisely what changed.
            Some(changes) => {
                let locals = task_store.load_all().unwrap_or_default();

                for remote in &changes.changed {
                    let local = local_by_remote(&locals, target.id, &remote.remote_id);
                    match local {
                        Some(local) if local.dirty => {
                            // Both sides changed: the freshness check inside
                            // `push_task` decides whether the remote version
                            // wins or the local edit is pushed.
                            push_task(
                                provider,
                                target,
                                credentials,
                                list_remote_id,
                                &task_store,
                                local,
                                &remote.remote_id,
                                Some(remote),
                                parent_remote_id_for(local, &locals),
                                now,
                                &mut report,
                                &mut pushed,
                            )
                            .await;
                        }
                        Some(local) if remote_newer(&local, remote) => {
                            tracing::debug!(
                                "pull: '{}' (remote {:?} > local {:?})",
                                remote.title,
                                remote.updated_at,
                                local.remote_updated_at
                            );
                            apply_remote_to_local(
                                &task_store,
                                Some(&local),
                                remote,
                                target,
                                now,
                                &mut report,
                            );
                        }
                        Some(_) => {}
                        None => {
                            apply_remote_to_local(
                                &task_store,
                                None,
                                remote,
                                target,
                                now,
                                &mut report,
                            );
                        }
                    }
                }

                for href in &changes.removed {
                    for local in locals.iter().filter(|t| {
                        t.source.remote_id() == Some(href.as_str())
                            && matches_account(&t.source, target.id)
                    }) {
                        trash_removed_task(store, &task_store, local, list_id);
                    }
                }

                next_token = changes.next_token;
            }
            // The provider has no incremental sync (Google/Microsoft, and any
            // CalDAV server without `sync-collection`): full scan instead.
            None => {
                full_scan_pull(
                    store,
                    provider,
                    target,
                    credentials,
                    list_id,
                    list_remote_id,
                    &task_store,
                    now,
                    &mut report,
                    &mut pushed,
                )
                .await;
            }
        }
    } else {
        // First pass with this list (or a re-scan): pull everything, then
        // seed the sync token so the next pass can be incremental. A server
        // without `sync-collection` yields no token and every pass stays a
        // full scan.
        full_scan_pull(
            store,
            provider,
            target,
            credentials,
            list_id,
            list_remote_id,
            &task_store,
            now,
            &mut report,
            &mut pushed,
        )
        .await;
        next_token = provider
            .current_sync_token(credentials, list_remote_id)
            .await
            .unwrap_or_else(|err| {
                tracing::debug!("Provider can't seed a sync token: {err}");
                None
            });
    }

    // Push: edits not covered by the pull loop (tasks the remote did not
    // report as changed), plus local tasks that don't exist remotely yet.
    let locals = task_store.load_all().unwrap_or_default();
    for local in &locals {
        if pushed.contains(&local.id) {
            continue;
        }
        if local.source.is_local() {
            // Created in the app inside a remote-linked list: push it and
            // link it to its freshly-created remote resource.
            let draft = draft_from_task(local, parent_remote_id_for(local, &locals));
            match provider
                .create_task(credentials, list_remote_id, &draft)
                .await
            {
                Ok(created) => {
                    let confirmed = confirm_pushed(
                        provider,
                        credentials,
                        list_remote_id,
                        &created.remote_id,
                    )
                    .await;
                    let mut updated = local.clone();
                    updated.source =
                        make_source(target.provider_id, target.id, created.remote_id);
                    mark_pushed(&task_store, &updated, now, confirmed.as_ref());
                    report.tasks_pushed += 1;
                }
                Err(err) => {
                    report
                        .errors
                        .push(format!("Failed to push new task '{}': {err}", local.title));
                }
            }
        } else if local.dirty && matches_account(&local.source, target.id) {
            let remote_id = local.source.remote_id().unwrap_or_default().to_string();
            if remote_id.is_empty() {
                continue;
            }
            push_task(
                provider,
                target,
                credentials,
                list_remote_id,
                &task_store,
                local,
                &remote_id,
                None,
                parent_remote_id_for(local, &locals),
                now,
                &mut report,
                &mut pushed,
            )
            .await;
        }
    }

    Ok((report, next_token))
}

/// The pull half of a full scan: every remote task of the list is fetched,
/// matched against the local mirror and either pulled or pushed, and local
/// tasks whose remote resource disappeared are trashed.
#[allow(clippy::too_many_arguments)]
async fn full_scan_pull(
    store: &Store,
    provider: &dyn RemoteTaskProvider,
    target: SyncTarget<'_>,
    credentials: &ProviderCredentials,
    list_id: Uuid,
    list_remote_id: &str,
    task_store: &TaskStore<'_>,
    now: Timestamp,
    report: &mut SyncReport,
    pushed: &mut BTreeSet<Uuid>,
) {
    let remote_tasks = match provider.list_tasks(credentials, list_remote_id).await {
        Ok(remote_tasks) => remote_tasks,
        Err(err) => {
            report.errors.push(format!("Failed to list tasks: {err}"));
            return;
        }
    };
    let locals = task_store.load_all().unwrap_or_default();

    // Pull + push per matched remote task.
    for remote in &remote_tasks {
        let local = local_by_remote(&locals, target.id, &remote.remote_id);
        match local {
            Some(local) if local.dirty => {
                push_task(
                    provider,
                    target,
                    credentials,
                    list_remote_id,
                    task_store,
                    local,
                    &remote.remote_id,
                    Some(remote),
                    parent_remote_id_for(local, &locals),
                    now,
                    report,
                    pushed,
                )
                .await;
            }
            Some(local) if remote_newer(&local, remote) => {
                apply_remote_to_local(task_store, Some(&local), remote, target, now, report);
            }
            Some(_) => {}
            None => {
                apply_remote_to_local(task_store, None, remote, target, now, report);
            }
        }
    }

    // Remote deletions: local tasks tracking this list/account whose
    // remote_id is no longer present get moved to trash.
    let present: BTreeSet<&str> = remote_tasks
        .iter()
        .map(|r| r.remote_id.as_str())
        .collect();
    for local in &locals {
        if !matches_account(&local.source, target.id) || local.source.is_local() {
            continue;
        }
        if !present.contains(local.source.remote_id().unwrap_or_default()) {
            trash_removed_task(store, task_store, local, list_id);
        }
    }
}

fn local_by_remote<'t>(locals: &'t [Task], target_id: Uuid, remote_id: &str) -> Option<&'t Task> {
    locals.iter().find(|t| {
        matches_account(&t.source, target_id)
            && t.source.remote_id() == Some(remote_id)
    })
}

/// Moves one locally-mirrored task whose remote resource disappeared into
/// the trash instead of dropping it.
fn trash_removed_task(store: &Store, task_store: &TaskStore<'_>, local: &Task, list_id: Uuid) {
    let trashed = TrashedTask::new(local.clone(), list_id);
    if store.trash().save(&trashed).is_ok() {
        let _ = task_store.delete(local.id);
    } else {
        tracing::error!("Failed to trash locally a remotely-deleted task '{}'", local.title);
    }
}

/// Pushes one dirty local task to its remote resource.
///
/// `seen_remote` is the freshest remote state available (from a full scan or
/// an incremental pull); when present, its ETag guards the write and its
/// timestamp decides the conflict *before* any write: a newer remote version
/// wins and the local edit is dropped with a warning, never silently.
/// Without it the provider re-reads the resource and only mid-air races can
/// conflict (surfaced as `PreconditionFailed` and resolved here, once).
#[allow(clippy::too_many_arguments)]
async fn push_task(
    provider: &dyn RemoteTaskProvider,
    target: SyncTarget<'_>,
    credentials: &ProviderCredentials,
    list_remote_id: &str,
    task_store: &TaskStore<'_>,
    local: &Task,
    remote_id: &str,
    seen_remote: Option<&RemoteTask>,
    parent_remote_id: Option<String>,
    now: Timestamp,
    report: &mut SyncReport,
    pushed: &mut BTreeSet<Uuid>,
) {
    // This entry came from a `sync_changes` report, so the resource changed
    // after the token our mirror was confirmed against. Which of the two
    // changed is decided by the locally stored ETag: identical means the
    // entry is the echo of our own last push (plain push below), different
    // means a foreign change we never saw — the server truth wins, loudly
    // (timestamps alone cannot tell this apart; LAST-MODIFIED usually only
    // has 1s precision).
    if let Some(remote) = seen_remote {
        let server_advanced = match (local.remote_etag.as_deref(), remote.etag.as_deref()) {
            (Some(local_etag), Some(remote_etag)) => local_etag != remote_etag,
            // No ETags to compare (providers without them): fall back to
            // timestamps — a strictly newer remote is a foreign change.
            _ => remote_wins(local.remote_updated_at, remote.updated_at),
        };
        if server_advanced {
            apply_remote_to_local(task_store, Some(local), remote, target, now, report);
            report.warnings.push(format!(
                "conflict on '{}': kept the remote version",
                local.title
            ));
            return;
        }
    }

    let mut draft = draft_from_task(local, parent_remote_id);
    draft.etag = seen_remote.and_then(|remote| remote.etag.clone());

    match provider
        .update_task(credentials, list_remote_id, remote_id, &draft)
        .await
    {
        Ok(()) => {
            let confirmed = confirm_pushed(provider, credentials, list_remote_id, remote_id).await;
            mark_pushed(task_store, local, now, confirmed.as_ref());
            pushed.insert(local.id);
            report.tasks_pushed += 1;
        }
        Err(ProviderError::CalDav(CalDavError::PreconditionFailed(_))) => {
            // The server moved between our fetch and our write. Fetch the
            // fresh state, resolve once, and never loop.
            match provider.get_task(credentials, list_remote_id, remote_id).await {
                Ok(fresh) => {
                    if remote_wins(local.remote_updated_at, fresh.updated_at) {
                        apply_remote_to_local(task_store, Some(local), &fresh, target, now, report);
                        report.warnings.push(format!(
                            "conflict on '{}': kept the remote version",
                            local.title
                        ));
                    } else {
                        let mut retry = draft;
                        retry.etag = fresh.etag;
                        match provider
                            .update_task(credentials, list_remote_id, remote_id, &retry)
                            .await
                        {
                            Ok(()) => {
                                let confirmed =
                                    confirm_pushed(provider, credentials, list_remote_id, remote_id).await;
                                mark_pushed(task_store, local, now, confirmed.as_ref());
                                pushed.insert(local.id);
                                report.tasks_pushed += 1;
                            }
                            Err(err) => {
                                report.errors.push(format!(
                                    "Failed to push task '{}': {err}",
                                    local.title
                                ));
                            }
                        }
                    }
                }
                Err(err) => {
                    report.errors.push(format!(
                        "Failed to resolve conflict on '{}': {err}",
                        local.title
                    ));
                }
            }
        }
        Err(err) => {
            report
                .errors
                .push(format!("Failed to push task '{}': {err}", local.title));
        }
    }
}

/// Marks a local task as cleanly synced after a successful push, stamping it
/// with the server's own timestamp/ETag when available.
fn mark_pushed(
    task_store: &TaskStore<'_>,
    local: &Task,
    now: Timestamp,
    confirmed: Option<&RemoteTask>,
) {
    let mut updated = local.clone();
    updated.dirty = false;
    updated.remote_updated_at = confirmed
        .and_then(|remote| remote.updated_at)
        .or(Some(now));
    updated.remote_etag = confirmed.and_then(|remote| remote.etag.clone());
    updated.last_synced_at = Some(now);
    if let Err(err) = task_store.save_synced(&updated) {
        tracing::error!("Failed to save synced task: {err}");
    }
}

/// After a successful write, reads the resource back once so the local mirror
/// carries the server-assigned `LAST-MODIFIED`/ETag. Stamping the server's
/// own values prevents the next incremental pass from "pulling" our own push
/// again (a wall-clock stamp is always slightly behind the server's).
async fn confirm_pushed(
    provider: &dyn RemoteTaskProvider,
    credentials: &ProviderCredentials,
    list_remote_id: &str,
    remote_id: &str,
) -> Option<RemoteTask> {
    match provider.get_task(credentials, list_remote_id, remote_id).await {
        Ok(fresh) => Some(fresh),
        Err(err) => {
            tracing::debug!("Failed to confirm pushed task state: {err}");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::features::tasks::task::Priority;

    fn task_with(days: &[u8]) -> Recurrence {
        let mut r = Recurrence::default();
        for &day in days {
            match day {
                0 => r.monday = true,
                1 => r.tuesday = true,
                2 => r.wednesday = true,
                3 => r.thursday = true,
                4 => r.friday = true,
                5 => r.saturday = true,
                6 => r.sunday = true,
                _ => {}
            }
        }
        r
    }

    #[test]
    fn draft_maps_priority_tags_and_recurrence() {
        let mut task = Task::new("Con etiquetas");
        task.priority = Priority::High;
        task.tags = vec!["casa".into(), "compras".into()];
        task.recurrence = task_with(&[0, 2]);

        let draft = draft_from_task(&task, None);
        assert_eq!(draft.priority, Some(1));
        assert_eq!(draft.tags, vec!["casa", "compras"]);
        assert_eq!(
            draft.recurrence.as_deref(),
            Some("FREQ=WEEKLY;BYDAY=MO,WE")
        );

        task.priority = Priority::Normal;
        assert_eq!(draft_from_task(&task, None).priority, Some(5));

        task.priority = Priority::Low;
        assert_eq!(draft_from_task(&task, None).priority, Some(9));
    }

    #[test]
    fn draft_without_recurrence_days_maps_to_none() {
        let mut task = Task::new("Sin recurrencia");
        task.recurrence = Recurrence::default();
        let draft = draft_from_task(&task, None);
        assert_eq!(draft.recurrence, None);
    }

    #[test]
    fn warnings_do_not_count_as_errors_and_merge() {
        let mut report = SyncReport::default();
        report.warnings.push("conflict".into());
        assert!(!report.had_errors());

        let mut other = SyncReport::default();
        other.errors.push("boom".into());
        other.warnings.push("another".into());
        report.merge(other);
        assert!(report.had_errors());
        assert_eq!(report.warnings.len(), 2);
        assert_eq!(report.errors.len(), 1);
    }

    #[test]
    fn remote_wins_cases() {
        let older = "2026-01-01T00:00:00Z".parse().unwrap();
        let newer = "2026-06-01T00:00:00Z".parse().unwrap();

        // Remote newer → remote wins.
        assert!(remote_wins(Some(older), Some(newer)));
        // Local newer → local (or equal: plain push) wins.
        assert!(!remote_wins(Some(newer), Some(older)));
        // Equal → not a conflict (remote did not move since the last pull).
        assert!(!remote_wins(Some(older), Some(older)));
        // Both unknown → remote wins (unprovable local edit).
        assert!(remote_wins(None, None));
        // Local without timestamp → remote is the only known truth.
        assert!(remote_wins(None, Some(older)));
        // Remote without timestamp → remote wins.
        assert!(remote_wins(Some(older), None));
    }

    #[test]
    fn maps_remote_priority_to_enum() {
        assert_eq!(priority_from_remote(Some(1)), Some(Priority::High));
        assert_eq!(priority_from_remote(Some(3)), Some(Priority::High));
        assert_eq!(priority_from_remote(Some(5)), Some(Priority::Normal));
        assert_eq!(priority_from_remote(Some(6)), Some(Priority::Low));
        assert_eq!(priority_from_remote(Some(9)), Some(Priority::Low));
        // 0 is "undefined" in RFC 5545; so is a missing priority.
        assert_eq!(priority_from_remote(Some(0)), None);
        assert_eq!(priority_from_remote(None), None);
    }

    #[test]
    fn recurrence_maps_to_weekly_byday() {
        assert_eq!(
            recurrence_to_rrule(&task_with(&[0, 2])),
            Some("FREQ=WEEKLY;BYDAY=MO,WE".to_string())
        );
        // Sorted in weekday order, not insertion order.
        assert_eq!(
            recurrence_to_rrule(&task_with(&[6, 1])),
            Some("FREQ=WEEKLY;BYDAY=TU,SU".to_string())
        );
        assert_eq!(recurrence_to_rrule(&Recurrence::default()), None);
    }

    #[test]
    fn rrule_maps_back_to_recurrence() {
        assert_eq!(
            recurrence_from_rrule(Some("FREQ=WEEKLY;BYDAY=MO,WE")),
            Some(task_with(&[0, 2]))
        );
        // Unknown frequencies/days are not applied.
        assert_eq!(recurrence_from_rrule(Some("FREQ=DAILY")), None);
        assert_eq!(recurrence_from_rrule(Some("FREQ=YEARLY;BYDAY=MO")), None);
        assert_eq!(recurrence_from_rrule(Some("FREQ=WEEKLY")), None);
        assert_eq!(recurrence_from_rrule(None), None);
    }

    #[test]
    fn sync_token_survives_list_ron_roundtrip() {
        let mut list = List::new("CalDAV");
        list.sync_token = Some("http://radicale.org/ns/sync/42".to_string());

        let ron = ron::to_string(&vec![list.clone()]).unwrap();
        let parsed: Vec<List> = ron::from_str(&ron).unwrap();

        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].sync_token.as_deref(), Some("http://radicale.org/ns/sync/42"));
    }

    /// A `lists.ron` written before `sync_token` existed must still load.
    #[test]
    fn legacy_list_ron_without_sync_token_still_loads() {
        let ron = r#"
(
    created_at: "2026-08-01T10:00:00Z",
    description: "",
    hide_completed: false,
    icon: None,
    id: "7c9e6679-7425-40de-944b-e07fc1f90ae7",
    name: "Old list",
    source: Local,
)
"#;
        let list: List = ron::from_str(ron).unwrap();
        assert_eq!(list.name, "Old list");
        assert_eq!(list.sync_token, None);
    }

    /// Live end-to-end smoke of the whole engine, ignored unless the
    /// environment points at a Radicale (docs/03-radicale.md §5):
    ///
    /// ```bash
    /// set -a; . ~/.config/proper-todo/test.env; set +a
    /// export RADICALE_URL=http://192.168.0.35:5232/cesar/<calendar>/
    /// export PROPER_TODO_RADICALE_PASSWORD="$RADICALE_PASS"
    /// cargo test --bin tasks radicale_engine -- --ignored --nocapture
    /// ```
    ///
    /// Covers: list auto-creation, pull, push (create), a no-op incremental
    /// pass (0 pulled / 0 pushed), token persistence, remote edit pull,
    /// remote delete → local trash, local edit push and a both-sides-changed
    /// conflict (remote wins, loudly). Leaves no residue on the server.
    #[test]
    #[ignore = "requires a running Radicale"]
    fn radicale_engine_end_to_end() {
        let Ok(url) = std::env::var("RADICALE_URL") else {
            eprintln!("skipping: RADICALE_URL not set");
            return;
        };
        let user = std::env::var("RADICALE_USER").unwrap_or_default();
        let pass = std::env::var("RADICALE_PASS").unwrap_or_default();
        std::env::set_var("PROPER_TODO_RADICALE_PASSWORD", &pass);

        let connection = RadicaleConnection {
            id: Uuid::new_v4(),
            name: "engine smoke".into(),
            base_url: url.clone(),
            username: user.clone(),
            calendar_href: Some(url.clone()),
        };

        // A fresh client for out-of-band (foreign client) manipulation.
        let foreign = caldav_client::CalDavClient::new(&url, &user, &pass)
            .expect("valid url");
        let calendar = foreign.base_url().path().to_string();
        let href_of = |uid: &str| caldav_client::CalDavClient::vtodo_href(&calendar, uid);
        let clock = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let pull_uid = format!("proper-todo-e2e-{clock}");

        let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
        rt.block_on(async {
            // Snapshot first: whatever happens below (even a panic), dropping
            // the guard leaves the calendar exactly as we found it.
            let guard = caldav_client::testing::CalendarGuard::new(
                caldav_client::CalDavClient::new(&url, &user, &pass).expect("valid url"),
            )
            .await;

            // Seed one task from "another client" (no local mirror yet).
            let seed = caldav_client::Vtodo::new(&pull_uid, "Sembrada por fuera");
            foreign
                .put_vtodo(&calendar, &seed, None)
                .await
                .expect("seed vtodo");

            // First pass: full scan (no token yet), list created, seed pulled.
            let store_dir = std::env::temp_dir().join(format!("proper-todo-engine-{clock}"));
            let store = crate::shared::store::Store::open(&store_dir).unwrap();
            let report = run_radicale_sync(&store, &[connection.clone()]).await;
            println!("pass 1: {report:?}");
            assert!(
                report.errors.is_empty(),
                "pass 1 failed: {:?}",
                report.errors
            );
            assert_eq!(report.lists_pulled, 1, "list must be auto-created");
            assert!(report.tasks_pulled >= 1, "the seeded task must be pulled");
            let list = store
                .lists()
                .load_all()
                .unwrap()
                .into_iter()
                .find(|l| l.source.account_id() == Some(connection.id))
                .expect("radicale list after first pass");
            assert!(list.sync_token.is_some(), "token must be seeded after a full scan");
            assert!(
                store
                    .tasks(list.id)
                    .load_all()
                    .unwrap()
                    .iter()
                    .any(|t| t.source.remote_id() == Some(href_of(&pull_uid).as_str())),
                "the seeded task must be mirrored locally"
            );

            // Second pass: create locally, push, and record the remote href.
            let task_store = store.tasks(list.id);
            let mut task = Task::new("Creada por la app");
            task.notes = "notas de prueba".into();
            task.priority = Priority::High;
            task.tags = vec!["smoke".into()];
            task_store.save(&task).unwrap();
            // Acceptance (docs/01-roadmap.md): app → server in under 5 s.
            let started = std::time::Instant::now();
            let report = run_radicale_sync(&store, &[connection.clone()]).await;
            let push_latency = started.elapsed();
            println!("pass 2: {report:?} in {push_latency:?}");
            assert!(push_latency < std::time::Duration::from_secs(5), "push took {push_latency:?}");
            assert!(report.errors.is_empty(), "pass 2 failed: {:?}", report.errors);
            assert_eq!(report.tasks_pushed, 1);
            let pushed = task_store.get(task.id).unwrap();
            let pushed_href = pushed
                .source
                .remote_id()
                .map(str::to_string)
                .expect("pushed task must be linked to its remote href");
            let _pushed_uid = pushed_href
                .rsplit('/')
                .next()
                .unwrap()
                .trim_end_matches(".ics")
                .to_string();

            // Third pass: nothing changed anywhere → monotone no-op.
            let report = run_radicale_sync(&store, &[connection.clone()]).await;
            println!("pass 3: {report:?}");
            assert!(report.errors.is_empty(), "pass 3 failed: {:?}", report.errors);
            assert_eq!(report.tasks_pulled, 0, "no-op pass must not pull");
            assert_eq!(report.tasks_pushed, 0, "no-op pass must not push");

            // Remote edit (outside the app) → pulled, priority mapped. A real
            // foreign client (Thunderbird, ...) stamps LAST-MODIFIED because
            // Radicale only rewrites resources, never properties.
            let mut remote = foreign
                .get_vtodo(&pushed_href)
                .await
                .expect("fetch pushed for remote edit");
            let etag = remote.etag.clone();
            remote.vtodo.summary = "Editada por fuera".into();
            remote.vtodo.priority = Some(5);
            remote.vtodo.last_modified = Some(Timestamp::now());
            foreign
                .put_vtodo(&calendar, &remote.vtodo, etag.as_deref())
                .await
                .expect("foreign edit");
            let started = std::time::Instant::now();
            let report = run_radicale_sync(&store, &[connection.clone()]).await;
            let pull_latency = started.elapsed();
            println!("pass 4: {report:?} in {pull_latency:?}");
            assert!(pull_latency < std::time::Duration::from_secs(5), "pull took {pull_latency:?}");
            assert_eq!(report.tasks_pulled, 1, "external edit must be pulled");
            let after = task_store.get(task.id).unwrap();
            assert_eq!(after.title, "Editada por fuera");
            assert_eq!(after.priority, Priority::Normal);

            // Local edit (dirty) + concurrent remote edit → conflict, remote
            // wins with a warning, nothing lost silently.
            let mut edited = task_store.get(task.id).unwrap();
            edited.title = "Editada en local".into();
            task_store.save(&edited).unwrap(); // `save` marks the mirror dirty
            let mut remote = foreign
                .get_vtodo(&pushed_href)
                .await
                .expect("fetch for remote edit");
            let etag = remote.etag.clone();
            remote.vtodo.summary = "Editada por fuera (otra vez)".into();
            remote.vtodo.last_modified = Some(Timestamp::now());
            foreign
                .put_vtodo(&calendar, &remote.vtodo, etag.as_deref())
                .await
                .expect("foreign edit 2");
            let report = run_radicale_sync(&store, &[connection.clone()]).await;
            println!("pass 5: {report:?}");
            assert!(report.warnings.iter().any(|warning| {
                warning.contains("conflict") && warning.contains("kept the remote version")
            }), "conflict must surface as a warning: {:?}", report.warnings);
            assert!(report.errors.is_empty(), "a resolved conflict is not an error: {:?}", report.errors);
            assert!(!report.had_errors());
            let after = task_store.get(task.id).unwrap();
            assert_eq!(after.title, "Editada por fuera (otra vez)");
            assert!(!after.dirty, "applied remote clears dirty");

            // Local edit alone (remote untouched) → plain push.
            let mut edited = task_store.get(task.id).unwrap();
            edited.notes = "solo local".into();
            task_store.save(&edited).unwrap();
            let report = run_radicale_sync(&store, &[connection.clone()]).await;
            println!("pass 6: {report:?}");
            assert_eq!(report.tasks_pushed, 1, "dirty edit must be pushed");
            let fetched = foreign.get_vtodo(&pushed_href).await.unwrap();
            assert_eq!(fetched.vtodo.description.as_deref(), Some("solo local"));

            // Remote delete → the local mirror goes to trash, not nowhere.
            foreign.delete_vtodo(&pushed_href, None).await.unwrap();
            let report = run_radicale_sync(&store, &[connection.clone()]).await;
            println!("pass 7: {report:?}");
            assert!(
                report.tasks_pulled == 0 && report.errors.is_empty(),
                "delete pass: {report:?}"
            );
            assert!(
                store
                    .trash()
                    .load_all()
                    .unwrap()
                    .iter()
                    .any(|t| t.task.id == task.id),
                "remotely-deleted task must be trashed locally"
            );
            assert!(task_store.get(task.id).is_err(), "mirror must be removed");

            // Cleanup: the calendar must end exactly as it started.
            guard.clean().await;
            assert!(guard.leftovers().await.is_empty(), "residue left on the calendar");
        });
    }
}
