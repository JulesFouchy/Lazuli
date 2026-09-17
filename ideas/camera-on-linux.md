---
summary: The camera is denied on Linux, because wry leaves WebKitGTK's permission request unhandled
affects: [src/camera.ts]
---

# Camera on Linux

"Take a photo" works on Windows and macOS and fails on Linux. `getUserMedia` raises a `permission-request` signal on the `WebKitWebView`, and wry 0.55 connects nothing to it on the GTK backend — an unhandled signal denies. So the request comes back `NotAllowedError` and the picker says the app was not allowed to use the camera, which is true and unhelpful, because there is nothing the user can do about it.

Windows is fine because WebView2 shows its own prompt when the host leaves the permission state at default, and remembers the answer per profile. macOS is fine because WKWebView asks the system, which is why `src-tauri/Info.plist` carries `NSCameraUsageDescription`.

## How

Either wry grows a media-permission hook and Lazuli answers it, or Lazuli reaches the `WebKitWebView` itself — `WebviewWindow::gtk_webview()` hands it over on Linux — and connects `permission-request`, allowing `WebKitUserMediaPermissionRequest` for video. The second is a `#[cfg(target_os = "linux")]` block of a dozen lines plus a `webkit2gtk` dependency that is already in the tree through wry.

Prompting is not required: the OS does not gate camera access on Linux, and the app only opens the camera when the user pressed a button that says so.

## When

When there is a Linux user, or when someone can test it — the check needs a machine with a camera and a WebKitGTK build, and a fix nobody has watched work is not worth the dependency.
