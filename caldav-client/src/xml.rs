//! DAV request bodies and `multistatus` response parsing.

use std::collections::BTreeSet;

use crate::error::Result;

pub const PROPFIND_PRINCIPAL: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:current-user-principal/>
    <d:resourcetype/>
  </d:prop>
</d:propfind>"#;

pub const PROPFIND_HOME_SET: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <c:calendar-home-set/>
    <d:displayname/>
  </d:prop>
</d:propfind>"#;

pub const PROPFIND_CALENDARS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/" xmlns:ic="http://apple.com/ns/ical/">
  <d:prop>
    <d:resourcetype/>
    <d:displayname/>
    <d:sync-token/>
    <cs:getctag/>
    <c:calendar-description/>
    <c:supported-calendar-component-set/>
    <ic:calendar-color/>
  </d:prop>
</d:propfind>"#;

pub const PROPFIND_SYNC_TOKEN: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<d:propfind xmlns:d="DAV:">
  <d:prop>
    <d:sync-token/>
  </d:prop>
</d:propfind>"#;

pub const REPORT_VTODOS: &str = r#"<?xml version="1.0" encoding="utf-8"?>
<c:calendar-query xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:prop>
    <d:getetag/>
    <c:calendar-data/>
  </d:prop>
  <c:filter>
    <c:comp-filter name="VCALENDAR">
      <c:comp-filter name="VTODO"/>
    </c:comp-filter>
  </c:filter>
</c:calendar-query>"#;

pub fn report_sync_collection(sync_token: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="utf-8"?>
<d:sync-collection xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav">
  <d:sync-token>{}</d:sync-token>
  <d:sync-level>1</d:sync-level>
  <d:prop>
    <d:getetag/>
    <c:calendar-data/>
  </d:prop>
</d:sync-collection>"#,
        escape_xml(sync_token)
    )
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// One `<response>` entry of a `207 Multi-Status` body.
#[derive(Clone, Debug, Default)]
pub struct DavResource {
    pub href: String,
    pub etag: Option<String>,
    pub calendar_data: Option<String>,
    pub display_name: Option<String>,
    pub calendar_description: Option<String>,
    pub color: Option<String>,
    pub ctag: Option<String>,
    pub sync_token: Option<String>,
    pub is_calendar: bool,
    pub supported_components: Vec<String>,
    /// HTTP status of the response itself, when the server sent one instead
    /// of a `propstat` (used by `sync-collection` to report deletions).
    pub status: Option<u16>,
}

impl DavResource {
    pub fn supports_vtodo(&self) -> bool {
        self.supported_components
            .iter()
            .any(|component| component.eq_ignore_ascii_case("VTODO"))
    }

    pub fn is_deleted(&self) -> bool {
        self.status == Some(404)
    }
}

pub struct MultiStatus {
    pub resources: Vec<DavResource>,
    pub sync_token: Option<String>,
}

pub fn parse_multistatus(xml: &str) -> Result<MultiStatus> {
    let document = roxmltree::Document::parse(xml)?;
    let root = document.root_element();

    let sync_token = root
        .children()
        .find(|node| node.is_element() && node.has_tag_name("sync-token"))
        .and_then(node_text);

    let mut resources = Vec::new();
    for response in root
        .descendants()
        .filter(|node| node.has_tag_name("response"))
    {
        resources.push(parse_response(&response));
    }

    Ok(MultiStatus {
        resources,
        sync_token,
    })
}

fn parse_response(response: &roxmltree::Node<'_, '_>) -> DavResource {
    let mut resource = DavResource::default();

    if let Some(href) = response
        .children()
        .find(|node| node.has_tag_name("href"))
        .and_then(node_text)
    {
        resource.href = href.trim().to_string();
    }

    if let Some(status) = response
        .children()
        .find(|node| node.has_tag_name("status"))
        .and_then(node_text)
        .and_then(|text| status_code(&text))
    {
        resource.status = Some(status);
    }

    for propstat in response
        .children()
        .filter(|node| node.has_tag_name("propstat"))
    {
        let status = propstat
            .children()
            .find(|node| node.has_tag_name("status"))
            .and_then(node_text)
            .and_then(|text| status_code(&text));

        if !matches!(status, Some(code) if (200..300).contains(&code)) {
            continue;
        }

        let Some(prop) = propstat
            .children()
            .find(|node| node.has_tag_name("prop"))
        else {
            continue;
        };

        for node in prop.children().filter(|node| node.is_element()) {
            apply_property(&mut resource, &node);
        }
    }

    resource
}

fn apply_property(resource: &mut DavResource, node: &roxmltree::Node<'_, '_>) {
    match node.tag_name().name() {
        "getetag" => resource.etag = node_text(*node).map(|text| text.trim().to_string()),
        "calendar-data" => resource.calendar_data = node_text(*node),
        "displayname" => resource.display_name = node_text(*node),
        "calendar-description" => resource.calendar_description = node_text(*node),
        "calendar-color" => resource.color = node_text(*node).map(|text| text.trim().to_string()),
        "getctag" => resource.ctag = node_text(*node).map(|text| text.trim().to_string()),
        "sync-token" => resource.sync_token = node_text(*node).map(|text| text.trim().to_string()),
        "resourcetype" => {
            resource.is_calendar = node
                .children()
                .any(|child| child.is_element() && child.has_tag_name("calendar"));
        }
        "supported-calendar-component-set" => {
            let components: BTreeSet<String> = node
                .children()
                .filter(|child| child.is_element() && child.has_tag_name("comp"))
                .filter_map(|child| child.attribute("name"))
                .map(|name| name.to_ascii_uppercase())
                .collect();
            resource.supported_components = components.into_iter().collect();
        }
        _ => {}
    }
}

fn node_text(node: roxmltree::Node<'_, '_>) -> Option<String> {
    node.text().map(|text| text.to_string())
}

fn status_code(text: &str) -> Option<u16> {
    text.split_whitespace().nth(1)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_calendar_propfind() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:" xmlns:c="urn:ietf:params:xml:ns:caldav" xmlns:cs="http://calendarserver.org/ns/">
  <d:response>
    <d:href>/kaesar/tareas/</d:href>
    <d:propstat>
      <d:prop>
        <d:resourcetype><d:collection/><c:calendar/></d:resourcetype>
        <d:displayname>Tareas</d:displayname>
        <d:sync-token>http://radicale.org/ns/sync/abc123</d:sync-token>
        <cs:getctag>abc123</cs:getctag>
        <c:supported-calendar-component-set><c:comp name="VTODO"/></c:supported-calendar-component-set>
      </d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
</d:multistatus>"#;

        let multistatus = parse_multistatus(xml).unwrap();
        assert_eq!(multistatus.resources.len(), 1);

        let calendar = &multistatus.resources[0];
        assert_eq!(calendar.href, "/kaesar/tareas/");
        assert_eq!(calendar.display_name.as_deref(), Some("Tareas"));
        assert!(calendar.is_calendar);
        assert!(calendar.supports_vtodo());
        assert_eq!(calendar.ctag.as_deref(), Some("abc123"));
    }

    #[test]
    fn parses_sync_deletions() {
        let xml = r#"<?xml version="1.0"?>
<d:multistatus xmlns:d="DAV:">
  <d:response>
    <d:href>/kaesar/tareas/gone.ics</d:href>
    <d:status>HTTP/1.1 404 Not Found</d:status>
  </d:response>
  <d:response>
    <d:href>/kaesar/tareas/kept.ics</d:href>
    <d:propstat>
      <d:prop><d:getetag>"etag-1"</d:getetag></d:prop>
      <d:status>HTTP/1.1 200 OK</d:status>
    </d:propstat>
  </d:response>
  <d:sync-token>http://radicale.org/ns/sync/next</d:sync-token>
</d:multistatus>"#;

        let multistatus = parse_multistatus(xml).unwrap();
        assert_eq!(multistatus.sync_token.as_deref(), Some("http://radicale.org/ns/sync/next"));
        assert!(multistatus.resources[0].is_deleted());
        assert_eq!(multistatus.resources[1].etag.as_deref(), Some("\"etag-1\""));
    }
}
