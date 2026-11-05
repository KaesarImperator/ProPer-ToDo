use std::collections::BTreeSet;

use cosmic::{
    cosmic_config::{self, cosmic_config_derive::CosmicConfigEntry, CosmicConfigEntry},
    theme,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const CONFIG_VERSION: u64 = 1;
#[derive(Clone, CosmicConfigEntry, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[version = 1]
pub struct AppConfig {
    pub app_theme: AppTheme,

    /// Accounts the user has turned off *within Tasks only* (e.g. via the
    /// Accounts page toggle). This is local app state, separate from
    /// whether the account's Todo capability is enabled in the system
    /// Accounts app — that's read-only from here. An account absent from
    /// this set is synced by default as long as its Todo capability is on.
    #[serde(default)]
    pub disabled_accounts: BTreeSet<Uuid>,

    pub hide_completed: bool,
    pub last_list_id: Option<Uuid>,
    pub list_sort_by: ListSortBy,
    /// CalDAV connections managed by Tasks itself (A3 adds the settings
    /// UI); they are deliberately *not* COSMIC accounts, so the sync engine
    /// handles them in a separate pass (see `sync.rs::run_radicale_sync`).
    #[serde(default)]
    pub radicale_connections: Vec<RadicaleConnection>,
    pub show_favorites: bool,
    pub show_trash: bool,
    pub sort_by: SortBy,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            app_theme: AppTheme::default(),
            disabled_accounts: BTreeSet::new(),
            hide_completed: false,
            last_list_id: None,
            list_sort_by: ListSortBy::default(),
            radicale_connections: Vec::new(),
            show_favorites: true,
            show_trash: true,
            sort_by: SortBy::default(),
        }
    }
}

/// One CalDAV (Radicale) connection as configured by the user. Stored in
/// `config.ron`; the password never is (see `radicale_password`).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct RadicaleConnection {
    pub id: Uuid,
    pub name: String,
    /// URL base with which the `CalDavClient` is built, e.g.
    /// `http://192.168.0.35:5232/cesar/`.
    pub base_url: String,
    pub username: String,
    /// href of the calendar. When `None`, the engine discovers the first
    /// calendar on the server that supports VTODO.
    pub calendar_href: Option<String>,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum ListSortBy {
    Manual,
    #[default]
    NameAsc,
    NameDesc,
}

impl From<usize> for ListSortBy {
    fn from(value: usize) -> Self {
        match value {
            1 => ListSortBy::NameDesc,
            2 => ListSortBy::Manual,
            _ => ListSortBy::NameAsc,
        }
    }
}

impl From<ListSortBy> for usize {
    fn from(value: ListSortBy) -> Self {
        match value {
            ListSortBy::Manual => 2,
            ListSortBy::NameAsc => 0,
            ListSortBy::NameDesc => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum SortBy {
    #[default]
    DateAsc,
    DateDesc,
    Manual,
    NameAsc,
    NameDesc,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub enum AppTheme {
    Dark,
    Light,
    #[default]
    System,
}

impl From<usize> for AppTheme {
    fn from(value: usize) -> Self {
        match value {
            1 => AppTheme::Dark,
            2 => AppTheme::Light,
            _ => AppTheme::System,
        }
    }
}

impl From<AppTheme> for usize {
    fn from(value: AppTheme) -> Self {
        match value {
            AppTheme::Dark => 1,
            AppTheme::Light => 2,
            AppTheme::System => 0,
        }
    }
}

impl AppTheme {
    pub fn theme(&self) -> theme::Theme {
        match self {
            Self::Dark => theme::system_dark(),
            Self::Light => theme::system_light(),
            Self::System => theme::system_preference(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `config.ron` written before `radicale_connections` existed must keep
    /// loading; cosmic-config only reads what Serde derives here.
    #[test]
    fn legacy_config_ron_without_radicale_connections_still_loads() {
        let ron = r#"
(
    app_theme: System,
    disabled_accounts: [],
    hide_completed: false,
    last_list_id: None,
    list_sort_by: NameAsc,
    show_favorites: true,
    show_trash: true,
    sort_by: DateAsc,
)
"#;
        let config: AppConfig = ron::from_str(ron).expect("legacy config must still parse");
        assert!(config.radicale_connections.is_empty());
    }

    #[test]
    fn radicale_connection_survives_config_ron_roundtrip() {
        let config = AppConfig {
            radicale_connections: vec![RadicaleConnection {
                id: Uuid::new_v4(),
                name: "Radicale casero".into(),
                base_url: "http://192.168.0.35:5232/cesar/".into(),
                username: "cesar".into(),
                calendar_href: Some("/cesar/tareas/".into()),
            }],
            ..Default::default()
        };

        let ron = ron::to_string(&config).unwrap();
        let parsed: AppConfig = ron::from_str(&ron).unwrap();
        assert_eq!(parsed.radicale_connections, config.radicale_connections);
    }
}
