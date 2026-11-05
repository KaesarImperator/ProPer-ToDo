use cosmic::{widget, Element};

use crate::{
    app::{AppModel, ContextPage, Message},
    features::accounts::views::sync_row,
    fl,
};

use super::{
    action::CaldavAction,
    form::{Field, Sensitive, TestFailure, TestState},
};

/// Entry in the Settings page.
pub fn settings_entry(app: &AppModel) -> Element<'_, Message> {
    let count = app.config.radicale_connections.len();
    let subtitle = if count == 0 {
        fl!("caldav-none")
    } else {
        fl!("caldav-count", count = (count as i64))
    };
    widget::settings::section()
        .title(fl!("caldav"))
        .add(
            widget::settings::item::builder(fl!("caldav"))
                .description(subtitle)
                .control(
                    widget::button::standard(fl!("manage"))
                        .on_press(Message::ToggleContextPage(ContextPage::Caldav)),
                ),
        )
        .into()
}

/// Drawer page: connection list, form and sync status. It does not depend on
/// the COSMIC accounts daemon.
pub fn page(app: &AppModel) -> Element<'_, Message> {
    let form = &app.caldav_form;
    let mut content = widget::column::with_capacity(4).spacing(16);

    let mut list = widget::settings::section().title(fl!("caldav"));
    if app.config.radicale_connections.is_empty() {
        list = list.add(widget::text::body(fl!("caldav-none")));
    }
    for connection in &app.config.radicale_connections {
        let id = connection.id;
        let controls: Element<'_, Message> = if form.pending_delete == Some(id) {
            widget::row::with_capacity(3)
                .push(widget::text::caption(fl!("caldav-delete-confirm")))
                .push(
                    widget::button::destructive(fl!("delete"))
                        .on_press(Message::Caldav(CaldavAction::ConfirmDelete(id))),
                )
                .push(
                    widget::button::standard(fl!("cancel"))
                        .on_press(Message::Caldav(CaldavAction::CancelDelete)),
                )
                .spacing(8)
                .align_y(cosmic::iced::Alignment::Center)
                .into()
        } else {
            widget::row::with_capacity(2)
                .push(
                    widget::button::standard(fl!("edit"))
                        .on_press(Message::Caldav(CaldavAction::Edit(id))),
                )
                .push(
                    widget::button::destructive(fl!("delete"))
                        .on_press(Message::Caldav(CaldavAction::AskDelete(id))),
                )
                .spacing(8)
                .into()
        };
        list = list.add(
            widget::settings::item::builder(connection.name.clone())
                .description(format!("{} · {}", connection.username, connection.base_url))
                .control(controls),
        );
    }
    if !form.open {
        list = list.add(
            widget::button::suggested(fl!("caldav-add"))
                .on_press(Message::Caldav(CaldavAction::New)),
        );
    }
    content = content.push(list);

    if form.open {
        content = content.push(form_section(app));
    }

    if !app.config.radicale_connections.is_empty() {
        content = content.push(widget::settings::section().add(sync_row(app)));
    }

    widget::scrollable(content).into()
}

fn form_section(app: &AppModel) -> Element<'_, Message> {
    let form = &app.caldav_form;
    let title = if form.editing.is_some() {
        fl!("caldav-edit")
    } else {
        fl!("caldav-add")
    };
    let mut section = widget::settings::section().title(title);

    section = section.add(widget::settings::item::item(
        fl!("caldav-name"),
        widget::text_input(fl!("caldav-name"), form.name.as_str())
            .on_input(|v| Message::Caldav(CaldavAction::TextChanged(Field::Name, v))),
    ));
    section = section.add(widget::settings::item::item(
        fl!("caldav-url"),
        widget::text_input("http://192.168.0.35:5232/user/", form.base_url.as_str())
            .on_input(|v| Message::Caldav(CaldavAction::TextChanged(Field::Url, v))),
    ));
    section = section.add(widget::settings::item::item(
        fl!("caldav-username"),
        widget::text_input(fl!("caldav-username"), form.username.as_str())
            .on_input(|v| Message::Caldav(CaldavAction::TextChanged(Field::Username, v))),
    ));
    let password_placeholder = if form.editing.is_some() {
        fl!("caldav-password-keep")
    } else {
        fl!("caldav-password")
    };
    section = section.add(widget::settings::item::item(
        fl!("caldav-password"),
        widget::secure_input(
            password_placeholder,
            form.password.expose().to_string(),
            Some(Message::Caldav(CaldavAction::TogglePasswordVisible)),
            !form.show_password,
        )
        .on_input(|v| Message::Caldav(CaldavAction::PasswordChanged(Sensitive::new(v)))),
    ));

    let testing = matches!(form.test, TestState::Testing);
    section = section.add(widget::settings::item::item(
        match &form.test {
            TestState::Idle => String::new(),
            TestState::Testing => fl!("caldav-testing"),
            TestState::Ok { calendars, .. } => fl!("caldav-test-ok", count = (calendars.len() as i64)),
            TestState::Failed(failure) => failure_text(failure),
        },
        widget::button::standard(fl!("caldav-test"))
            .on_press_maybe((form.can_test() && !testing).then_some(Message::Caldav(
                CaldavAction::TestConnection,
            ))),
    ));

    if let TestState::Ok {
        calendars,
        selected,
    } = &form.test
    {
        let names: Vec<String> = calendars.iter().map(|c| c.name.clone()).collect();
        section = section.add(widget::settings::item::item(
            fl!("caldav-calendar"),
            widget::dropdown(names, Some(*selected), |index| {
                Message::Caldav(CaldavAction::SelectCalendar(index))
            }),
        ));
    }

    let buttons = widget::row::with_capacity(2)
        .push(
            widget::button::standard(fl!("cancel"))
                .on_press(Message::Caldav(CaldavAction::CloseForm)),
        )
        .push(
            widget::button::suggested(fl!("save"))
                .on_press_maybe(form.can_save().then_some(Message::Caldav(CaldavAction::Save))),
        )
        .spacing(8);
    section = section.add(buttons);

    section.into()
}

fn failure_text(failure: &TestFailure) -> String {
    match failure {
        TestFailure::BadCredentials => fl!("caldav-err-credentials"),
        TestFailure::Timeout => fl!("caldav-err-timeout"),
        TestFailure::InvalidUrl => fl!("caldav-err-url"),
        TestFailure::NoVtodoCalendars => fl!("caldav-err-no-calendars"),
        TestFailure::MissingPassword => fl!("caldav-err-missing-password"),
        TestFailure::Keyring(detail) => fl!("caldav-err-keyring", detail = detail.as_str()),
        TestFailure::Server { status } => fl!("caldav-err-server", status = (*status as i64)),
        TestFailure::Other(detail) => fl!("caldav-err-other", detail = detail.as_str()),
    }
}
