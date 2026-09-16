//! Pressing a key at the OS, for the one thing the page cannot do for itself.

use anyhow::Result;

/// Open the webview's own context menu where the caret is.
///
/// This exists for the spelling suggestions. Chromium computes them, and
/// nothing in the page can reach them: the list is never exposed to
/// JavaScript, and a `contextmenu` event dispatched from a script is untrusted
/// and opens nothing. The menu answers to exactly two gestures — a right-click
/// and the keyboard's Menu key — and neither can be synthesised from the page
/// either. So the key is pressed at the OS, and the webview takes it as it
/// takes any other keystroke.
///
/// Whatever modifiers the shortcut that called this was held with are released
/// first. Chromium opens the menu only for an *unmodified* Menu key, so
/// Ctrl still being down — which it is, the shortcut being Ctrl+; — means the
/// key arrives and nothing happens. Only the modifiers actually down are
/// touched, and they are not pressed again: the user releasing them afterwards
/// sends a second release, which is harmless, whereas pressing Ctrl again
/// would send a keystroke into the menu that has just opened.
#[cfg(windows)]
pub fn press_context_menu_key() -> Result<()> {
    use anyhow::bail;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_APPS, VK_LCONTROL, VK_LMENU,
        VK_LSHIFT, VK_RCONTROL, VK_RMENU, VK_RSHIFT,
    };

    // The Windows key is deliberately absent: releasing it on its own opens the
    // Start menu, which is a worse outcome than the shortcut not working.
    const MODIFIERS: &[VIRTUAL_KEY] = &[
        VK_LCONTROL,
        VK_RCONTROL,
        VK_LSHIFT,
        VK_RSHIFT,
        VK_LMENU,
        VK_RMENU,
    ];

    fn stroke(key: VIRTUAL_KEY, flags: KEYBD_EVENT_FLAGS) -> INPUT {
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: key,
                    wScan: 0,
                    dwFlags: flags,
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        }
    }

    let mut strokes: Vec<INPUT> = MODIFIERS
        .iter()
        // The high bit says the key is down right now.
        .filter(|key| unsafe { GetAsyncKeyState(key.0 as i32) } as u16 & 0x8000 != 0)
        .map(|key| stroke(*key, KEYEVENTF_KEYUP))
        .collect();
    strokes.push(stroke(VK_APPS, KEYBD_EVENT_FLAGS(0)));
    strokes.push(stroke(VK_APPS, KEYEVENTF_KEYUP));

    let sent = unsafe { SendInput(&strokes, std::mem::size_of::<INPUT>() as i32) };
    if sent as usize != strokes.len() {
        bail!("the keystroke was blocked before it reached the window");
    }
    Ok(())
}

#[cfg(not(windows))]
pub fn press_context_menu_key() -> Result<()> {
    anyhow::bail!("there is no Menu key to press on this platform")
}
