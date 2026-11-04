use super::model::{RemoteList, RemoteTask, RemoteTaskDraft};

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("network error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("{provider} API error ({status}): {message}")]
    Api {
        provider: &'static str,
        status: u16,
        message: String,
    },
    /// The access token was rejected (expired/revoked); the caller should
    /// force a credential refresh and retry once before surfacing this.
    #[error("authorization expired")]
    Unauthorized,
    /// The provider was handed a credential type it cannot use, e.g. HTTP
    /// Basic credentials for Google (`ProviderCredentials::Bearer` only).
    #[error("provider does not support this credential type")]
    UnsupportedCredentials,
    /// The provider does not implement this operation at all (list
    /// create/update/delete are currently unused by the sync engine, and
    /// CalDAV calendars are managed on the server, not from the app).
    #[error("operation not supported by this provider")]
    Unsupported,
    /// CalDAV transport/parse failure, kept distinct from `Api` so callers can
    /// tell a server-side HTTP status from an iCalendar/XML problem.
    #[error("caldav error: {0}")]
    CalDav(#[from] caldav_client::CalDavError),
}

pub type ProviderResult<T> = Result<T, ProviderError>;

/// Credentials a provider needs to authenticate a call. OAuth providers use a
/// short-lived bearer token refreshed per sync pass; CalDAV servers (Radicale)
/// use HTTP Basic with a long-lived user/password pair.
#[derive(Clone, Debug)]
pub enum ProviderCredentials {
    Bearer(String),
    /// Only built once CalDAV connections exist (A2/A3); until then the sync
    /// loop only ever creates `Bearer`.
    #[allow(dead_code)]
    Basic { username: String, password: String },
}

/// Provider-agnostic remote task backend. Implementors talk directly to the
/// Google Tasks API / Microsoft Graph To Do API / a CalDAV server over HTTP;
/// callers are responsible for obtaining valid credentials (via
/// `AccountsClient::ensure_credentials`/`get_access_token` for OAuth accounts)
/// before calling any method here.
/// List create/update/delete are part of the provider contract but unused by
/// the current sync engine, which only auto-creates lists on pull and never
/// auto-creates/renames/deletes remote lists from local changes (see
/// `sync.rs`); kept here for a future milestone and for provider symmetry.
#[allow(dead_code)]
#[async_trait::async_trait]
pub trait RemoteTaskProvider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn create_list(
        &self,
        credentials: &ProviderCredentials,
        name: &str,
    ) -> ProviderResult<RemoteList>;
    async fn delete_list(
        &self,
        credentials: &ProviderCredentials,
        remote_id: &str,
    ) -> ProviderResult<()>;
    async fn list_lists(
        &self,
        credentials: &ProviderCredentials,
    ) -> ProviderResult<Vec<RemoteList>>;
    async fn update_list(
        &self,
        credentials: &ProviderCredentials,
        remote_id: &str,
        name: &str,
    ) -> ProviderResult<()>;

    async fn create_task(
        &self,
        credentials: &ProviderCredentials,
        list_remote_id: &str,
        task: &RemoteTaskDraft,
    ) -> ProviderResult<RemoteTask>;
    async fn delete_task(
        &self,
        credentials: &ProviderCredentials,
        list_remote_id: &str,
        remote_id: &str,
    ) -> ProviderResult<()>;
    async fn list_tasks(
        &self,
        credentials: &ProviderCredentials,
        list_remote_id: &str,
    ) -> ProviderResult<Vec<RemoteTask>>;
    async fn update_task(
        &self,
        credentials: &ProviderCredentials,
        list_remote_id: &str,
        remote_id: &str,
        task: &RemoteTaskDraft,
    ) -> ProviderResult<()>;
}
