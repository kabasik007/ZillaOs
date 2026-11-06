//! Shared app IDs. The Desktop, Dock, Start menu and App Manager agree on these.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum App {
    Files,
    Terminal,
    Settings,
    AppCenter,
    Notepad,
}
impl App {
    pub const ALL: [App; 5] = [App::Files, App::Terminal, App::Settings, App::AppCenter, App::Notepad];
    pub const COUNT: usize = Self::ALL.len();
    pub const fn index(self) -> usize {
        match self {
            Self::Files => 0, Self::Terminal => 1,
            Self::Settings => 2, Self::AppCenter => 3,
            Self::Notepad => 4,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Files => "FILES", Self::Terminal => "TERMINAL",
            Self::Settings => "SETTINGS", Self::AppCenter => "APP CENTER",
            Self::Notepad => "NOTEPAD",
        }
    }
}
