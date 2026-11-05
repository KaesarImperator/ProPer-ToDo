//! Integration tests against a real Radicale server.
//!
//! Skipped unless the environment points at one:
//!
//! ```bash
//! export RADICALE_URL=http://localhost:5232/testuser/agenda/
//! export RADICALE_USER=testuser
//! export RADICALE_PASS=testuser
//! cargo test --test radicale -- --ignored --nocapture
//! ```
//!
//! See `docs/03-radicale.md` for a disposable Docker setup.

use std::time::{SystemTime, UNIX_EPOCH};

use caldav_client::{CalDavClient, CalDavError, Vtodo, VtodoStatus};

fn client() -> Option<CalDavClient> {
    let url = std::env::var("RADICALE_URL").ok()?;
    let user = std::env::var("RADICALE_USER").unwrap_or_default();
    let pass = std::env::var("RADICALE_PASS").unwrap_or_default();

    Some(CalDavClient::new(&url, user, pass).expect("RADICALE_URL must be a valid URL"))
}

fn unique_uid() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system clock before unix epoch")
        .as_nanos();
    format!("proper-todo-test-{nanos}")
}

/// The collection the client was pointed at, as a server-relative href.
fn calendar_href(client: &CalDavClient) -> String {
    client.base_url().path().to_string()
}

/// Best-effort removal so failed assertions do not litter the server.
async fn cleanup(client: &CalDavClient, calendar: &str, uid: &str) {
    let href = CalDavClient::vtodo_href(calendar, uid);
    let _ = client.delete_vtodo(&href, None).await;
}

#[tokio::test]
#[ignore = "requires a running Radicale (docs/03-radicale.md)"]
async fn discovers_calendars() {
    let Some(client) = client() else {
        eprintln!("skipping: RADICALE_URL not set");
        return;
    };

    let calendars = client.list_calendars().await.expect("list calendars");
    assert!(
        !calendars.is_empty(),
        "server exposed no calendar collections"
    );

    let vtodo_calendars: Vec<_> = calendars.iter().filter(|c| c.supports_vtodo).collect();
    assert!(
        !vtodo_calendars.is_empty(),
        "no calendar advertises VTODO support; found: {:?}",
        calendars
            .iter()
            .map(|c| (&c.href, &c.display_name, c.supports_vtodo))
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ignore = "requires a running Radicale (docs/03-radicale.md)"]
async fn full_vtodo_lifecycle() {
    let Some(client) = client() else {
        eprintln!("skipping: RADICALE_URL not set");
        return;
    };

    let calendar = calendar_href(&client);
    let uid = unique_uid();

    let outcome = lifecycle(&client, &calendar, &uid).await;
    cleanup(&client, &calendar, &uid).await;
    outcome
}

async fn lifecycle(client: &CalDavClient, calendar: &str, uid: &str) {
    // -- create ------------------------------------------------------------
    let mut vtodo = Vtodo::new(uid, "Comprar leche, pan y huevos");
    vtodo.description = Some("Línea uno\nLínea dos; con separadores, y \\ barras".into());
    vtodo.priority = Some(1);
    vtodo.categories = vec!["casa".into(), "compras".into()];
    vtodo.status = Some(VtodoStatus::NeedsAction);

    let etag = client
        .put_vtodo(calendar, &vtodo, None)
        .await
        .expect("create vtodo");
    assert!(!etag.is_empty(), "server returned no ETag on create");

    // -- list --------------------------------------------------------------
    let listed = client.list_vtodos(calendar).await.expect("list vtodos");
    let created = listed
        .iter()
        .find(|remote| remote.vtodo.uid == uid)
        .expect("created vtodo is not listed");

    assert_eq!(created.vtodo.summary, vtodo.summary);
    assert_eq!(created.vtodo.description, vtodo.description);
    assert_eq!(created.vtodo.priority, Some(1));
    assert_eq!(created.vtodo.categories, vec!["casa", "compras"]);

    // -- read --------------------------------------------------------------
    let fetched = client.get_vtodo(&created.href).await.expect("get vtodo");
    assert_eq!(fetched.vtodo.uid, uid);
    let current_etag = fetched.etag.clone().expect("get returns an ETag");

    // -- update ------------------------------------------------------------
    let mut updated = fetched.vtodo.clone();
    updated.summary = "Comprar leche (actualizado)".into();
    updated.status = Some(VtodoStatus::InProcess);
    updated.percent_complete = Some(50);

    let new_etag = client
        .put_vtodo(calendar, &updated, Some(&current_etag))
        .await
        .expect("update vtodo");
    assert_ne!(new_etag, current_etag, "ETag must change after an update");

    let after_update = client.get_vtodo(&created.href).await.expect("get updated");
    assert_eq!(after_update.vtodo.summary, "Comprar leche (actualizado)");
    assert_eq!(after_update.vtodo.percent_complete, Some(50));

    // -- stale etag must be rejected --------------------------------------
    let stale = client
        .put_vtodo(calendar, &updated, Some(&current_etag))
        .await;
    assert!(
        matches!(stale, Err(CalDavError::PreconditionFailed(_))),
        "writing with a stale ETag must fail, got: {stale:?}"
    );

    // -- delete ------------------------------------------------------------
    client
        .delete_vtodo(&created.href, Some(&new_etag))
        .await
        .expect("delete vtodo");

    let remaining = client.list_vtodos(calendar).await.expect("list after delete");
    assert!(
        remaining.iter().all(|remote| remote.vtodo.uid != uid),
        "vtodo still present after delete"
    );

    // Deleting twice is idempotent.
    client
        .delete_vtodo(&created.href, None)
        .await
        .expect("delete is idempotent");
}

#[tokio::test]
#[ignore = "requires a running Radicale (docs/03-radicale.md)"]
async fn incremental_sync_reports_changes() {
    let Some(client) = client() else {
        eprintln!("skipping: RADICALE_URL not set");
        return;
    };

    let calendar = calendar_href(&client);

    // A freshly created, never-written collection may not expose a
    // `sync-token` yet; seed one with a throwaway task if needed.
    let mut token = client
        .sync_token(&calendar)
        .await
        .expect("read sync token");

    let mut seed_uid = None;
    if token.is_none() {
        let uid = unique_uid();
        let seed = Vtodo::new(uid.clone(), "seed");
        client
            .put_vtodo(&calendar, &seed, None)
            .await
            .expect("seed vtodo");
        seed_uid = Some(uid);
        token = client
            .sync_token(&calendar)
            .await
            .expect("read sync token after seed");
    }

    let Some(token) = token else {
        eprintln!("skipping: server does not expose sync-token (needs Radicale >= 3.1)");
        if let Some(uid) = &seed_uid {
            cleanup(&client, &calendar, uid).await;
        }
        return;
    };

    let uid = unique_uid();
    let vtodo = Vtodo::new(uid.clone(), "Tarea para sync incremental");
    client
        .put_vtodo(&calendar, &vtodo, None)
        .await
        .expect("create vtodo");

    let changes = client
        .sync_collection(&calendar, &token)
        .await
        .expect("sync collection");

    assert!(
        changes.new_token.is_some(),
        "sync-collection must return a new token"
    );
    let created = changes
        .changed
        .iter()
        .find(|remote| remote.vtodo.uid == uid || remote.href.contains(&uid))
        .expect("created vtodo missing from sync changes");

    // Radicale inlines `calendar-data` when asked; a placeholder without it
    // (empty UID) would mean the engine has to GET every change.
    assert_eq!(
        created.vtodo.summary, "Tarea para sync incremental",
        "sync-collection must return calendar-data inline, got a bare href+etag entry"
    );

    cleanup(&client, &calendar, &uid).await;
    if let Some(uid) = &seed_uid {
        cleanup(&client, &calendar, uid).await;
    }
}

#[tokio::test]
#[ignore = "requires a running Radicale (docs/03-radicale.md)"]
async fn rejects_bad_credentials() {
    let Some(_) = client() else {
        eprintln!("skipping: RADICALE_URL not set");
        return;
    };

    let url = std::env::var("RADICALE_URL").unwrap();
    let bad = CalDavClient::new(&url, "nobody", "definitely-wrong").expect("valid url");

    // With auth.type=none the server accepts anything, so only assert when
    // the deployment actually enforces authentication.
    match bad.list_calendars().await {
        Err(CalDavError::Unauthorized { .. }) => {}
        Ok(_) => eprintln!("skipping: server does not enforce authentication"),
        Err(other) => panic!("unexpected error: {other:?}"),
    }
}
