//! Minimal no_std three-language UI catalogue shared by the desktop renderer.
//! UTF-8 labels use the kernel bitmap font; no OS locale or IME is claimed.
use crate::setup::Language;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextId {
    Desktop, File, View, Windows, Files, Terminal, Settings, Home, Apps, About,
    Documents, Pictures, Projects, Appearance, ThemeDark, ThemeLight,
    Language, LangUkrainian, LangEnglish, LangRussian, Display, BootResolution,
    BootHint, ModeChangedAfterReboot, Launcher, Active, WallpaperTitle,
    WallpaperSubtitle, Preview, Shortcuts, System, ColorDepth, NoDisk,
    FilePreview, TerminalPreview, ClickToChange, KeyboardHint,
}

pub const fn text(language: Language, id: TextId) -> &'static str {
    use crate::setup::Language::{Ukrainian, English, Russian};
    use TextId::*;
    match (language, id) {
        (Ukrainian, Desktop) => "СТІЛ", (English, Desktop) => "DESKTOP", (Russian, Desktop) => "СТОЛ",
        (Ukrainian, File) => "ФАЙЛ", (English, File) => "FILE", (Russian, File) => "ФАЙЛ",
        (Ukrainian, View) => "ВИГЛЯД", (English, View) => "VIEW", (Russian, View) => "ВИД",
        (Ukrainian, Windows) => "ВІКНА", (English, Windows) => "WINDOWS", (Russian, Windows) => "ОКНА",
        (Ukrainian, Files) => "ФАЙЛИ", (English, Files) => "FILES", (Russian, Files) => "ФАЙЛЫ",
        (Ukrainian, Terminal) => "ТЕРМІНАЛ", (English, Terminal) => "TERMINAL", (Russian, Terminal) => "ТЕРМИНАЛ",
        (Ukrainian, Settings) => "НАЛАШТУВАННЯ", (English, Settings) => "SETTINGS", (Russian, Settings) => "НАСТРОЙКИ",
        (Ukrainian, Home) => "ГОЛОВНА", (English, Home) => "HOME", (Russian, Home) => "ГЛАВНАЯ",
        (Ukrainian, Apps) => "ПРОГРАМИ", (English, Apps) => "APPS", (Russian, Apps) => "ПРОГРАММЫ",
        (Ukrainian, About) => "ПРО СИСТЕМУ", (English, About) => "ABOUT", (Russian, About) => "О СИСТЕМЕ",
        (Ukrainian, Documents) => "ДОКУМЕНТИ", (English, Documents) => "DOCUMENTS", (Russian, Documents) => "ДОКУМЕНТЫ",
        (Ukrainian, Pictures) => "ЗОБРАЖЕННЯ", (English, Pictures) => "PICTURES", (Russian, Pictures) => "ИЗОБРАЖЕНИЯ",
        (Ukrainian, Projects) => "ПРОЄКТИ", (English, Projects) => "PROJECTS", (Russian, Projects) => "ПРОЕКТЫ",
        (Ukrainian, Appearance) => "ОФОРМЛЕННЯ", (English, Appearance) => "APPEARANCE", (Russian, Appearance) => "ОФОРМЛЕНИЕ",
        (Ukrainian, ThemeDark) => "ТЕМНА ТЕМА", (English, ThemeDark) => "DARK THEME", (Russian, ThemeDark) => "ТЁМНАЯ ТЕМА",
        (Ukrainian, ThemeLight) => "СВІТЛА ТЕМА", (English, ThemeLight) => "LIGHT THEME", (Russian, ThemeLight) => "СВЕТЛАЯ ТЕМА",
        (Ukrainian, Language) => "МОВА", (English, Language) => "LANGUAGE", (Russian, Language) => "ЯЗЫК",
        (Ukrainian, LangUkrainian) => "УКРАЇНСЬКА", (English, LangUkrainian) => "UKRAINIAN", (Russian, LangUkrainian) => "УКРАИНСКИЙ",
        (Ukrainian, LangEnglish) => "АНГЛІЙСЬКА", (English, LangEnglish) => "ENGLISH", (Russian, LangEnglish) => "АНГЛИЙСКИЙ",
        (Ukrainian, LangRussian) => "РОСІЙСЬКА", (English, LangRussian) => "RUSSIAN", (Russian, LangRussian) => "РУССКИЙ",
        (Ukrainian, Display) => "ЕКРАН", (English, Display) => "DISPLAY", (Russian, Display) => "ЭКРАН",
        (Ukrainian, BootResolution) => "РОЗДІЛЬНІСТЬ", (English, BootResolution) => "RESOLUTION", (Russian, BootResolution) => "РАЗРЕШЕНИЕ",
        (Ukrainian, BootHint) => "ОБЕРИ ПРИ СТАРТІ", (English, BootHint) => "CHOOSE AT BOOT", (Russian, BootHint) => "ВЫБЕРИ ПРИ СТАРТЕ",
        (Ukrainian, ModeChangedAfterReboot) => "ПІСЛЯ ПЕРЕЗАПУСКУ", (English, ModeChangedAfterReboot) => "AFTER REBOOT", (Russian, ModeChangedAfterReboot) => "ПОСЛЕ ПЕРЕЗАГРУЗКИ",
        (Ukrainian, Launcher) => "МЕНЮ ZILLA", (English, Launcher) => "ZILLA LAUNCHER", (Russian, Launcher) => "МЕНЮ ZILLA",
        (Ukrainian, Active) => "АКТИВНЕ", (English, Active) => "ACTIVE", (Russian, Active) => "АКТИВНО",
        (Ukrainian, WallpaperTitle) => "СТВОРЮЙ НОВЕ.", (English, WallpaperTitle) => "MAKE SOMETHING.", (Russian, WallpaperTitle) => "СОЗДАВАЙ НОВОЕ.",
        (Ukrainian, WallpaperSubtitle) => "ZILLA DESKTOP / ПЕРЕГЛЯД", (English, WallpaperSubtitle) => "ZILLA DESKTOP / PREVIEW", (Russian, WallpaperSubtitle) => "ZILLA DESKTOP / ПРОСМОТР",
        (Ukrainian, Preview) => "ПЕРЕГЛЯД", (English, Preview) => "PREVIEW", (Russian, Preview) => "ПРОСМОТР",
        (Ukrainian, Shortcuts) => "ШВИДКІ ДІЇ", (English, Shortcuts) => "SHORTCUTS", (Russian, Shortcuts) => "БЫСТРЫЕ ДЕЙСТВИЯ",
        (Ukrainian, System) => "СИСТЕМА", (English, System) => "SYSTEM", (Russian, System) => "СИСТЕМА",
        (Ukrainian, ColorDepth) => "КОЛІР RGB32", (English, ColorDepth) => "COLOR RGB32", (Russian, ColorDepth) => "ЦВЕТ RGB32",
        (Ukrainian, NoDisk) => "БЕЗ ФАЙЛОВОЇ СИСТЕМИ", (English, NoDisk) => "NO FILESYSTEM YET", (Russian, NoDisk) => "НЕТ ФАЙЛОВОЙ СИСТЕМЫ",
        (Ukrainian, FilePreview) => "ПРИКЛАД ПАПОК", (English, FilePreview) => "SAMPLE FOLDERS", (Russian, FilePreview) => "ПРИМЕР ПАПОК",
        (Ukrainian, TerminalPreview) => "ТЕРМІНАЛ: ПЕРЕГЛЯД", (English, TerminalPreview) => "TERMINAL: PREVIEW", (Russian, TerminalPreview) => "ТЕРМИНАЛ: ПРОСМОТР",
        (Ukrainian, ClickToChange) => "НАТИСНИ ДЛЯ ЗМІНИ", (English, ClickToChange) => "CLICK TO CHANGE", (Russian, ClickToChange) => "НАЖМИ ДЛЯ СМЕНЫ",
        (Ukrainian, KeyboardHint) => "F8 МОВА F5 ТЕМА", (English, KeyboardHint) => "F8 LANGUAGE F5 THEME", (Russian, KeyboardHint) => "F8 ЯЗЫК F5 ТЕМА",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn localization_is_complete_and_utf8() {
        let ids = [TextId::Desktop, TextId::File, TextId::View, TextId::Windows,
            TextId::Files, TextId::Terminal, TextId::Settings, TextId::Home,
            TextId::Apps, TextId::About, TextId::Documents, TextId::Pictures,
            TextId::Projects, TextId::Appearance, TextId::ThemeDark,
            TextId::ThemeLight, TextId::Language, TextId::LangUkrainian,
            TextId::LangEnglish, TextId::LangRussian, TextId::Display,
            TextId::BootResolution, TextId::BootHint, TextId::ModeChangedAfterReboot,
            TextId::Launcher, TextId::Active, TextId::WallpaperTitle,
            TextId::WallpaperSubtitle, TextId::Preview, TextId::Shortcuts,
            TextId::System, TextId::ColorDepth, TextId::NoDisk,
            TextId::FilePreview, TextId::TerminalPreview, TextId::ClickToChange,
            TextId::KeyboardHint];
        for lang in [Language::Ukrainian, Language::English, Language::Russian] {
            for id in ids { assert!(!text(lang, id).is_empty()); }
        }
        assert_eq!(text(Language::Ukrainian, TextId::Language), "МОВА");
        assert_eq!(text(Language::Russian, TextId::Language), "ЯЗЫК");
    }
}
