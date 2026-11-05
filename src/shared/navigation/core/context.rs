use crate::fl;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ContextPage {
    About,
    #[allow(unused)]
    Accounts,
    Caldav,
    Settings,
    TaskDetails,
}

impl ContextPage {
    pub fn title(&self) -> String {
        match self {
            Self::About => fl!("about"),
            Self::Accounts => fl!("accounts"),
            Self::Caldav => fl!("caldav"),
            Self::Settings => fl!("settings"),
            Self::TaskDetails => fl!("details"),
        }
    }
}
