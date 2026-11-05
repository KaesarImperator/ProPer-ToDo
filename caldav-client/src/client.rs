//! Async CalDAV client targeting Radicale and other RFC 4791 servers.

use reqwest::{Method, StatusCode};
use url::Url;

use crate::error::{CalDavError, Result};
use crate::vtodo::Vtodo;
use crate::xml::{self, DavResource, MultiStatus};

/// A calendar collection discovered on the server.
#[derive(Clone, Debug, Default)]
pub struct Calendar {
    pub href: String,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub color: Option<String>,
    pub ctag: Option<String>,
    pub sync_token: Option<String>,
    pub supports_vtodo: bool,
}

impl Calendar {
    fn from_resource(resource: DavResource) -> Self {
        let supports_vtodo = resource.supports_vtodo();
        Self {
            href: resource.href,
            display_name: resource.display_name,
            description: resource.calendar_description,
            color: resource.color,
            ctag: resource.ctag,
            sync_token: resource.sync_token,
            supports_vtodo,
        }
    }
}

/// A VTODO as stored on the server, together with its ETag.
#[derive(Clone, Debug)]
pub struct RemoteVtodo {
    pub href: String,
    pub etag: Option<String>,
    pub vtodo: Vtodo,
}

/// Incremental changes reported by `sync-collection`.
#[derive(Clone, Debug, Default)]
pub struct SyncChanges {
    pub changed: Vec<RemoteVtodo>,
    pub removed: Vec<String>,
    pub new_token: Option<String>,
}

pub struct CalDavClient {
    http: reqwest::Client,
    base: Url,
    username: String,
    password: String,
}

impl CalDavClient {
    pub fn new(base_url: &str, username: impl Into<String>, password: impl Into<String>) -> Result<Self> {
        let mut base = Url::parse(base_url)?;
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }

        let http = reqwest::Client::builder()
            .user_agent(concat!("ProPer-ToDo/", env!("CARGO_PKG_VERSION")))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;

        Ok(Self {
            http,
            base,
            username: username.into(),
            password: password.into(),
        })
    }

    pub fn base_url(&self) -> &Url {
        &self.base
    }

    /// Resolves a possibly relative `href` returned by the server.
    pub fn absolute(&self, href: &str) -> Result<Url> {
        Ok(self.base.join(href)?)
    }

    /// Reduces a possibly absolute URL to its server-relative path.
    ///
    /// Radicale reports resource hrefs relative to the server root
    /// (`/user/calendar/task.ics`), and the sync engine matches stored
    /// `remote_id`s against exactly those, so whatever the user configured
    /// (a full `http://host/user/calendar/` URL is natural to paste) must be
    /// reduced to the same relative shape.
    pub fn relative_path(href: &str) -> &str {
        match href.find("://") {
            Some(scheme) => {
                let authority = &href[scheme + 3..];
                match authority.find('/') {
                    Some(path) => &authority[path..],
                    None => "/",
                }
            }
            None => href,
        }
    }

    // -- discovery ---------------------------------------------------------

    /// `current-user-principal` for the configured base URL.
    ///
    /// Falls back to `/{username}/` when the server cannot resolve a principal
    /// from the base URL — which is what Radicale does with `auth.type = none`,
    /// and what a user typing `http://host:5232/` will hit.
    pub async fn principal_href(&self) -> Result<String> {
        let url = self.base.clone();
        let response = self
            .dav_request(Method::from_bytes(b"PROPFIND").unwrap(), url.clone(), Some("0"), Some(xml::PROPFIND_PRINCIPAL))
            .await?;
        let body = self.expect_multistatus(response, &url).await?;

        let multistatus = xml::parse_multistatus(&body)?;
        let principal = multistatus
            .resources
            .first()
            .and_then(|resource| resource.href.clone().into());

        match principal {
            Some(href) if href.trim_matches('/').is_empty() && !self.username.is_empty() => {
                Ok(format!("/{}/", self.username))
            }
            Some(href) => Ok(href),
            None if !self.username.is_empty() => Ok(format!("/{}/", self.username)),
            None => Ok(self.base.path().to_string()),
        }
    }

    /// `calendar-home-set` for the current user.
    pub async fn calendar_home_href(&self) -> Result<String> {
        let principal = self.principal_href().await?;
        let url = self.absolute(&principal)?;
        let response = self
            .dav_request(Method::from_bytes(b"PROPFIND").unwrap(), url.clone(), Some("0"), Some(xml::PROPFIND_HOME_SET))
            .await?;
        let body = self.expect_multistatus(response, &url).await?;

        let document = roxmltree::Document::parse(&body)?;
        let home = document
            .descendants()
            .find(|node| node.has_tag_name("calendar-home-set"))
            .and_then(|node| {
                node.children()
                    .find(|child| child.has_tag_name("href"))
                    .and_then(|child| child.text())
            })
            .map(|text| text.trim().to_string());

        Ok(home.unwrap_or(principal))
    }

    /// Every calendar collection under the user's calendar home.
    pub async fn list_calendars(&self) -> Result<Vec<Calendar>> {
        let home = self.calendar_home_href().await?;
        let url = self.absolute(&home)?;
        let response = self
            .dav_request(Method::from_bytes(b"PROPFIND").unwrap(), url.clone(), Some("1"), Some(xml::PROPFIND_CALENDARS))
            .await?;
        let body = self.expect_multistatus(response, &url).await?;
        let multistatus = xml::parse_multistatus(&body)?;

        Ok(multistatus
            .resources
            .into_iter()
            .filter(|resource| resource.is_calendar)
            .map(Calendar::from_resource)
            .collect())
    }

    // -- VTODO operations --------------------------------------------------

    /// All VTODOs in a calendar, including their `calendar-data`.
    pub async fn list_vtodos(&self, calendar_href: &str) -> Result<Vec<RemoteVtodo>> {
        let url = self.absolute(calendar_href)?;
        let response = self
            .dav_request(Method::from_bytes(b"REPORT").unwrap(), url.clone(), Some("1"), Some(xml::REPORT_VTODOS))
            .await?;
        let body = self.expect_multistatus(response, &url).await?;
        let multistatus = xml::parse_multistatus(&body)?;

        Ok(collect_vtodos(multistatus.resources))
    }

    /// Fetches a single VTODO by href.
    pub async fn get_vtodo(&self, href: &str) -> Result<RemoteVtodo> {
        let url = self.absolute(href)?;
        let response = self.authorized(self.http.get(url.clone())).send().await?;

        if response.status() == StatusCode::NOT_FOUND {
            return Err(CalDavError::NotFound(href.to_string()));
        }
        let response = self.check_status(response, &url, "GET").await?;

        let etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string());
        let body = response.text().await?;

        Ok(RemoteVtodo {
            href: href.to_string(),
            etag,
            vtodo: Vtodo::from_ical(&body)?,
        })
    }

    /// Server-relative href a VTODO with `uid` would live at inside
    /// `calendar_href`. Radicale and most servers accept any file name, so the
    /// UID doubles as the resource name.
    pub fn vtodo_href(calendar_href: &str, uid: &str) -> String {
        let calendar = calendar_href.trim_end_matches('/');
        format!("{calendar}/{}.ics", sanitize_uid(uid))
    }

    /// Creates or updates a VTODO under a calendar collection, addressing the
    /// resource by a href derived from the UID (see [`Self::vtodo_href`]).
    ///
    /// Prefer [`Self::put_vtodo_at`] when the href is known: it is the only
    /// safe write path for resources created by another client, which may
    /// name the file differently than `sanitize_uid` would.
    ///
    /// Pass the current ETag to update an existing resource (sends
    /// `If-Match`, so a concurrent edit fails with `PreconditionFailed`
    /// instead of being silently overwritten), or `None` to create it
    /// (`If-None-Match: *`, so an existing resource is never clobbered).
    pub async fn put_vtodo(
        &self,
        calendar_href: &str,
        vtodo: &Vtodo,
        etag: Option<&str>,
    ) -> Result<String> {
        self.put_vtodo_at(&Self::vtodo_href(calendar_href, &vtodo.uid), vtodo, etag)
            .await
    }

    /// Writes a VTODO at an explicit href, ignoring [`Self::vtodo_href`]'s
    /// UID-derived file name. The href must belong to a calendar collection
    /// on this server (it is resolved with [`Self::absolute`]).
    pub async fn put_vtodo_at(
        &self,
        href: &str,
        vtodo: &Vtodo,
        etag: Option<&str>,
    ) -> Result<String> {
        let url = self.absolute(href)?;

        let mut request = self
            .http
            .put(url.clone())
            .header(reqwest::header::CONTENT_TYPE, "text/calendar; charset=utf-8")
            .body(vtodo.to_ical());

        request = match etag {
            Some(etag) => request.header(reqwest::header::IF_MATCH, etag),
            None => request.header(reqwest::header::IF_NONE_MATCH, "*"),
        };

        let response = self.authorized(request).send().await?;
        if response.status() == StatusCode::PRECONDITION_FAILED {
            return Err(CalDavError::PreconditionFailed(url.to_string()));
        }
        let response = self.check_status(response, &url, "PUT").await?;

        let new_etag = response
            .headers()
            .get(reqwest::header::ETAG)
            .and_then(|value| value.to_str().ok())
            .map(|value| value.to_string());

        Ok(new_etag.unwrap_or_default())
    }

    /// Deletes a VTODO, guarding the operation with its ETag.
    pub async fn delete_vtodo(&self, href: &str, etag: Option<&str>) -> Result<()> {
        let url = self.absolute(href)?;
        let mut request = self.http.delete(url.clone());

        if let Some(etag) = etag {
            request = request.header(reqwest::header::IF_MATCH, etag);
        }

        let response = self.authorized(request).send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(());
        }
        if response.status() == StatusCode::PRECONDITION_FAILED {
            return Err(CalDavError::PreconditionFailed(url.to_string()));
        }
        self.check_status(response, &url, "DELETE").await?;

        Ok(())
    }

    // -- incremental sync --------------------------------------------------

    /// Current `sync-token` for a collection.
    pub async fn sync_token(&self, calendar_href: &str) -> Result<Option<String>> {
        let url = self.absolute(calendar_href)?;
        let response = self
            .dav_request(Method::from_bytes(b"PROPFIND").unwrap(), url.clone(), Some("0"), Some(xml::PROPFIND_SYNC_TOKEN))
            .await?;
        let body = self.expect_multistatus(response, &url).await?;
        let multistatus = xml::parse_multistatus(&body)?;

        Ok(multistatus
            .sync_token
            .or_else(|| multistatus.resources.first().and_then(|r| r.sync_token.clone())))
    }

    /// `sync-collection` REPORT: changes since `sync_token`.
    ///
    /// `calendar-data` is requested alongside the ETag so complying servers
    /// inline the body and no per-resource `GET` is needed; entries that come
    /// back without data (older servers, or servers that ignore the prop)
    /// carry a default `Vtodo` and must be resolved by the caller via
    /// [`Self::get_vtodo`].
    pub async fn sync_collection(&self, calendar_href: &str, sync_token: &str) -> Result<SyncChanges> {
        let url = self.absolute(calendar_href)?;
        let body = xml::report_sync_collection(sync_token);
        let response = self
            .dav_request(Method::from_bytes(b"REPORT").unwrap(), url.clone(), Some("1"), Some(&body))
            .await?;
        let response_body = self.expect_multistatus(response, &url).await?;
        let MultiStatus {
            resources,
            sync_token,
        } = xml::parse_multistatus(&response_body)?;

        let mut changes = SyncChanges {
            new_token: sync_token,
            ..Default::default()
        };

        for resource in resources {
            if resource.is_deleted() {
                changes.removed.push(resource.href);
            } else if let Some(data) = &resource.calendar_data {
                if let Ok(vtodo) = Vtodo::from_ical(data) {
                    changes.changed.push(RemoteVtodo {
                        href: resource.href,
                        etag: resource.etag,
                        vtodo,
                    });
                }
            } else {
                changes.changed.push(RemoteVtodo {
                    href: resource.href.clone(),
                    etag: resource.etag,
                    vtodo: Vtodo::default(),
                });
            }
        }

        Ok(changes)
    }

    // -- internals ---------------------------------------------------------

    fn authorized(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        request.basic_auth(&self.username, Some(&self.password))
    }

    async fn dav_request(
        &self,
        method: Method,
        url: Url,
        depth: Option<&str>,
        body: Option<&str>,
    ) -> Result<reqwest::Response> {
        let mut request = self.authorized(self.http.request(method.clone(), url.clone()));

        if let Some(depth) = depth {
            request = request.header("Depth", depth);
        }
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/xml; charset=utf-8")
                .body(body.to_string());
        }

        let response = request.send().await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Err(CalDavError::NotFound(url.to_string()));
        }
        self.check_status(response, &url, method.as_str()).await
    }

    async fn check_status(
        &self,
        response: reqwest::Response,
        url: &Url,
        method: &str,
    ) -> Result<reqwest::Response> {
        let status = response.status();

        if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            return Err(CalDavError::Unauthorized {
                url: url.to_string(),
            });
        }
        if status.is_success() {
            return Ok(response);
        }

        let body = response.text().await.unwrap_or_default();
        Err(CalDavError::Status {
            method: method.to_string(),
            url: url.to_string(),
            status: status.as_u16(),
            body: body.chars().take(500).collect(),
        })
    }

    async fn expect_multistatus(&self, response: reqwest::Response, url: &Url) -> Result<String> {
        if response.status() == StatusCode::MULTI_STATUS {
            return Ok(response.text().await?);
        }
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        Err(CalDavError::Status {
            method: "PROPFIND/REPORT".to_string(),
            url: url.to_string(),
            status: status.as_u16(),
            body: body.chars().take(500).collect(),
        })
    }
}

fn collect_vtodos(resources: Vec<DavResource>) -> Vec<RemoteVtodo> {
    resources
        .into_iter()
        .filter_map(|resource| {
            let data = resource.calendar_data?;
            let vtodo = Vtodo::from_ical(&data).ok()?;
            Some(RemoteVtodo {
                href: resource.href,
                etag: resource.etag,
                vtodo,
            })
        })
        .collect()
}

/// Keeps UIDs filesystem and URL safe while staying stable per task.
fn sanitize_uid(uid: &str) -> String {
    uid.chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_base_url_trailing_slash() {
        let client = CalDavClient::new("http://192.168.1.10:5232/kaesar", "u", "p").unwrap();
        assert_eq!(client.base_url().as_str(), "http://192.168.1.10:5232/kaesar/");
    }

    #[test]
    fn resolves_relative_hrefs() {
        let client = CalDavClient::new("http://localhost:5232/kaesar/", "u", "p").unwrap();
        assert_eq!(
            client.absolute("/kaesar/tareas/x.ics").unwrap().as_str(),
            "http://localhost:5232/kaesar/tareas/x.ics"
        );
    }

    #[test]
    fn sanitizes_uids_for_file_names() {
        assert_eq!(sanitize_uid("abc-123_def.4"), "abc-123_def.4");
        assert_eq!(sanitize_uid("a/b:c d"), "a_b_c_d");
    }
}
