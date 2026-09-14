//! Choosing the window's theme and background before the window exists.
//!
//! The page themes itself from `localStorage`, which only the webview can read
//! and only once it is running. Two things are already on screen by then: the
//! title bar, drawn by the OS, and the rectangle behind the page. Both take
//! their colour from how the window was *built*, and changing either after the
//! fact repaints the frame in front of the user — so the only clean moment to
//! get them right is creation, which is why the window is built in `lib.rs`
//! rather than declared in `tauri.conf.json`.
//!
//! The frontend mirrors its choice into `settings.json`, and this is what reads
//! it back.

use tauri::window::Color;
use tauri::Theme;

/// `--bg` for the dark theme, from `src/styles.css`. Change both together.
const DARK_BG: Color = Color(0x0d, 0x0d, 0x10, 0xff);

/// `--bg` for the light theme, from `src/styles.css`. Change both together.
const LIGHT_BG: Color = Color(0xf1, 0xf0, 0xed, 0xff);

/// What to build the window with, from the stored choice.
pub struct WindowDress {
    /// `None` leaves the frame to the OS, which is what `system` means.
    pub theme: Option<Theme>,
    /// The colour behind the page until the page paints over it.
    pub background: Color,
}

/// Resolve the raw setting — `light`, `dark`, `system`, or nothing at all on a
/// first run — into a frame theme and a background.
///
/// For `system` the frame is left to the OS, but a background still has to be
/// chosen now, so the OS preference is read directly. The page will resolve the
/// same preference through `prefers-color-scheme` a moment later; they agree
/// because both read the same Windows setting.
pub fn dress_for(choice: Option<&str>) -> WindowDress {
    let theme = match choice {
        Some("light") => Some(Theme::Light),
        Some("dark") => Some(Theme::Dark),
        _ => None,
    };
    let resolved = theme.unwrap_or_else(os_theme);
    WindowDress {
        theme,
        background: match resolved {
            Theme::Light => LIGHT_BG,
            _ => DARK_BG,
        },
    }
}

/// The OS-wide light/dark preference, as Windows stores it.
///
/// `AppsUseLightTheme` is the switch under Settings → Personalisation →
/// Colours; `1` is light. Anything unreadable is taken as dark, which is what
/// the app looked like before it had a light theme at all.
#[cfg(windows)]
fn os_theme() -> Theme {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let light = RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|key| key.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|value| value == 1)
        .unwrap_or(false);
    if light {
        Theme::Light
    } else {
        Theme::Dark
    }
}

#[cfg(not(windows))]
fn os_theme() -> Theme {
    // There is no window yet to ask, and no portable way to ask without one.
    Theme::Dark
}
