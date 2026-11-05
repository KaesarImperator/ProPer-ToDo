use cosmic::{widget, Element};

use crate::{
    app::{AppModel, ContextPage, Message},
    fl,
};

use super::AccountsAction;

/// "Manage accounts" entry point, added to the Settings page.
pub fn _settings_entry(app: &AppModel) -> Element<'_, Message> {
    let subtitle = if !app.accounts_daemon_checked {
        String::new()
    } else if app.accounts.is_none() {
        fl!("accounts-daemon-unavailable")
    } else if todo_accounts(app).is_empty() {
        fl!("no-accounts-connected")
    } else {
        fl!("manage-accounts")
    };

    widget::settings::section()
        .title(fl!("accounts"))
        .add(
            widget::settings::item::builder(fl!("accounts"))
                .description(subtitle)
                .control(
                    widget::button::standard(fl!("accounts"))
                        .on_press(Message::ToggleContextPage(ContextPage::Accounts)),
                ),
        )
        .into()
}

/// Full-page account manager, rendered in the `ContextPage::Accounts` drawer.
/// Tasks does not create/remove accounts here — that's the system Accounts
/// app's job; this page only lets the user flip a local on/off switch per
/// account and jump to the Accounts app to manage connections.
pub fn page(app: &AppModel) -> Element<'_, Message> {
    let mut section = widget::settings::section().title(fl!("accounts"));

    section = section.add(widget::settings::item::item(
        fl!("manage-accounts"),
        widget::button::standard(fl!("open"))
            .on_press(Message::Accounts(AccountsAction::OpenAccountsApp)),
    ));

    if !app.accounts_daemon_checked {
        return widget::scrollable(section).into();
    }

    if app.accounts.is_none() {
        section = section.add(widget::settings::item::item(
            fl!("accounts-daemon-unavailable"),
            widget::text::body(""),
        ));
        return widget::scrollable(section).into();
    }

    let accounts = todo_accounts(app);

    if accounts.is_empty() {
        section = section.add(widget::settings::item::item(
            fl!("no-accounts-connected"),
            widget::text::body(""),
        ));
        return widget::scrollable(section).into();
    }

    for account in accounts {
        let id = account.id;
        let locally_enabled = !app.config.disabled_accounts.contains(&id);
        let label = if let Some(email) = &account.email {
            format!("{} ({})", account.display_name, email)
        } else {
            account.display_name.clone()
        };

        section = section.add(widget::settings::item::builder(label).control(
            widget::toggler(locally_enabled).on_toggle(move |val| {
                Message::Accounts(AccountsAction::SetLocallyEnabled(id, val))
            }),
        ));
    }

    section = section.add(sync_row(app));

    widget::scrollable(section).into()
}

/// Accounts with the Todo capability enabled in the system Accounts app.
/// Tasks only ever shows/syncs accounts that pass this filter; an account
/// with Todo turned off there is invisible here, not just unsynced.
fn todo_accounts(app: &AppModel) -> Vec<&accounts::models::Account> {
    app.remote_accounts
        .iter()
        .filter(|a| {
            a.services
                .get(&accounts::models::Service::Tasks)
                .copied()
                .unwrap_or(false)
        })
        .collect()
}

/// "Last synced / Sync now" row, shared by every page that can trigger a
/// sync (accounts and CalDAV). Shows the last error under the button so the
/// user never has to read logs, and the count of informational notices.
pub fn sync_row(app: &AppModel) -> Element<'_, Message> {
    use crate::shared::providers::sync::SyncStatus;

    let (label, syncing, detail) = match &app.sync_status {
        Some(SyncStatus::Syncing) => (fl!("syncing"), true, None),
        Some(SyncStatus::Idle {
            at,
            had_errors,
            warnings,
            last_error,
        }) => {
            let base = if *had_errors {
                fl!("sync-error")
            } else {
                fl!("last-synced")
            };
            let mut text = format!(
                "{base} ({})",
                crate::features::tasks::task::Task::format_timestamp(at)
            );
            if *warnings > 0 {
                text = format!("{text} · {}", fl!("sync-notices", count = (*warnings as i64)));
            }
            (text, false, last_error.clone())
        }
        None => (String::new(), false, None),
    };

    let mut column = widget::column::with_capacity(2).push(widget::settings::item::item(
        label,
        widget::button::standard(fl!("sync"))
            .on_press_maybe((!syncing).then_some(Message::Accounts(AccountsAction::SyncNow))),
    ));
    if let Some(detail) = detail {
        column = column.push(widget::text::caption(detail));
    }
    column.spacing(4).into()
}
