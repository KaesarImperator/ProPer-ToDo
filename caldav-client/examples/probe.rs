//! Manual probe against a live server. Not part of the test suite.
//!
//! ```bash
//! RADICALE_URL=http://localhost:5232/testuser/agenda/ \
//! RADICALE_USER=testuser RADICALE_PASS=testuser \
//! cargo run --example probe
//! ```

use caldav_client::CalDavClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("RADICALE_URL")?;
    let user = std::env::var("RADICALE_USER").unwrap_or_default();
    let pass = std::env::var("RADICALE_PASS").unwrap_or_default();

    let client = CalDavClient::new(&url, user, pass)?;
    let calendar = client.base_url().path().to_string();

    println!("base: {url}");
    println!("principal: {:?}", client.principal_href().await?);
    println!("home: {:?}", client.calendar_home_href().await?);
    println!("calendars: {:#?}", client.list_calendars().await?);
    println!("sync-token: {:?}", client.sync_token(&calendar).await?);

    let vtodos = client.list_vtodos(&calendar).await?;
    println!("vtodos: {}", vtodos.len());
    for remote in &vtodos {
        println!("  {} -> {}", remote.href, remote.vtodo.summary);
    }

    Ok(())
}
