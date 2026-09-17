//! Letting the page use the machine's camera.
//!
//! `getUserMedia` asks the host for permission, and WebView2 hands that
//! question to whoever built the webview. wry answers only the clipboard one,
//! so a camera request went unanswered and the promise in `src/camera.ts` hung
//! for ever — no preview, no error, nothing to report.
//!
//! Lazuli answers it here, and answers yes. The only thing that ever asks is
//! the button in the image picker that says "Take a photo…", so the user has
//! already said what this would be a second prompt about; and there is no
//! remote content in the window that could ask on its own.

use tauri::WebviewWindow;

/// Answer the webview's permission questions for the lifetime of the window.
///
/// Only Windows needs this today. macOS asks the operating system, which
/// prompts on the app's behalf — see `NSCameraUsageDescription` in
/// `Info.plist`. Linux denies, and putting that right needs a WebKitGTK
/// handler: see `ideas/camera-on-linux.md`.
pub fn allow(window: &WebviewWindow) {
    #[cfg(windows)]
    {
        // A failure here costs the camera button and nothing else, so it is
        // reported rather than fatal: the app is a journal first.
        if let Err(err) = windows_impl::allow(window) {
            eprintln!("lazuli: could not grant camera permission: {err}");
        }
    }
    #[cfg(not(windows))]
    let _ = window;
}

#[cfg(windows)]
mod windows_impl {
    use tauri::WebviewWindow;
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PERMISSION_KIND, COREWEBVIEW2_PERMISSION_KIND_CAMERA,
        COREWEBVIEW2_PERMISSION_STATE_ALLOW,
    };
    use webview2_com::PermissionRequestedEventHandler;

    pub fn allow(window: &WebviewWindow) -> tauri::Result<()> {
        window.with_webview(|webview| unsafe {
            let core = webview.controller().CoreWebView2();
            let Ok(core) = core else { return };
            let mut token = 0;
            let _ = core.add_PermissionRequested(
                &PermissionRequestedEventHandler::create(Box::new(|_, args| {
                    let Some(args) = args else { return Ok(()) };
                    let mut kind = COREWEBVIEW2_PERMISSION_KIND::default();
                    args.PermissionKind(&mut kind)?;
                    // Only the camera. Everything else keeps WebView2's own
                    // answer, which is to ask.
                    if kind == COREWEBVIEW2_PERMISSION_KIND_CAMERA {
                        args.SetState(COREWEBVIEW2_PERMISSION_STATE_ALLOW)?;
                    }
                    Ok(())
                })),
                &mut token,
            );
        })
    }
}
