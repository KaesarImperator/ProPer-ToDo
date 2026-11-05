//! Passwords for CalDAV connections, kept in the system keyring (Secret
//! Service, or the portal-backed file keyring inside a sandbox). They never
//! touch `config.ron` and never appear in logs.
//!
//! Every call talks D-Bus and may wait for the user to unlock the keyring:
//! only call these from async tasks, never from `view()` or a blocking
//! `update()` path.

use oo7::Keyring;
use uuid::Uuid;

const APPLICATION: &str = "dev.edfloreshz.Tasks";
const KIND: &str = "caldav-password";

/// Keyring failure, already phrased for a log line (never contains the secret).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretError(pub String);

impl std::fmt::Display for SecretError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SecretError {}

impl From<oo7::Error> for SecretError {
    fn from(err: oo7::Error) -> Self {
        Self(format!("keyring unavailable: {err}"))
    }
}

/// Attributes that identify one connection's password. Also what
/// `secret-tool search kind caldav-password` shows.
pub fn attributes(connection_id: Uuid) -> [(&'static str, String); 3] {
    [
        ("application", APPLICATION.to_string()),
        ("kind", KIND.to_string()),
        ("connection-id", connection_id.to_string()),
    ]
}

pub fn label(connection_name: &str) -> String {
    format!("ProPer ToDo — {connection_name}")
}

/// Stores (or replaces) the password for a connection.
pub async fn save_password(
    connection_id: Uuid,
    connection_name: &str,
    password: &str,
) -> Result<(), SecretError> {
    let keyring = Keyring::new().await?;
    keyring
        .create_item(
            &label(connection_name),
            &attributes(connection_id),
            password,
            true,
        )
        .await?;
    Ok(())
}

/// Returns the stored password, or `None` if there is none.
pub async fn load_password(connection_id: Uuid) -> Result<Option<String>, SecretError> {
    let keyring = Keyring::new().await?;
    let items = keyring.search_items(&attributes(connection_id)).await?;
    let Some(item) = items.first() else {
        return Ok(None);
    };
    let secret = item.secret().await?;
    Ok(secret.as_str().map(str::to_string))
}

/// Removes the password. Deleting one that does not exist is not an error.
pub async fn delete_password(connection_id: Uuid) -> Result<(), SecretError> {
    let keyring = Keyring::new().await?;
    keyring.delete(&attributes(connection_id)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attributes_identify_the_connection_and_never_hold_a_secret() {
        let id = Uuid::new_v4();
        let attrs = attributes(id);
        assert!(attrs.contains(&("kind", "caldav-password".to_string())));
        assert!(attrs.contains(&("connection-id", id.to_string())));
        assert_eq!(attrs.len(), 3);
    }

    /// Live round-trip against the session's Secret Service (gnome-keyring
    /// here). Always removes what it created.
    #[test]
    #[ignore = "requires an unlocked Secret Service"]
    fn keyring_save_load_delete_roundtrip() {
        struct Cleanup(Uuid);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                let id = self.0;
                std::thread::scope(|scope| {
                    scope.spawn(move || {
                        if let Ok(rt) = tokio::runtime::Runtime::new() {
                            let _ = rt.block_on(delete_password(id));
                        }
                    });
                });
            }
        }

        let id = Uuid::new_v4();
        let _cleanup = Cleanup(id);
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            assert_eq!(load_password(id).await.unwrap(), None);
            save_password(id, "prueba", "s3cr3t").await.unwrap();
            assert_eq!(load_password(id).await.unwrap().as_deref(), Some("s3cr3t"));
            save_password(id, "prueba", "otra").await.unwrap();
            assert_eq!(load_password(id).await.unwrap().as_deref(), Some("otra"));
            delete_password(id).await.unwrap();
            assert_eq!(load_password(id).await.unwrap(), None);
            // Idempotent.
            delete_password(id).await.unwrap();
        });
    }
}
