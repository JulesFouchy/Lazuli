//! Lazuli: a local-first project journal.
//!
//! A project is a folder on disk and nothing else. Everything the app knows is
//! re-read from that folder; see [`store`] for the read path and [`watch`] for
//! how external edits get noticed.

pub mod atomic;
pub mod camera;
pub mod commands;
pub mod dates;
pub mod keys;
pub mod library;
pub mod model;
pub mod paths;
pub mod store;
pub mod theme;
pub mod trashcan;
pub mod updates;
pub mod watch;

use tauri::{WebviewUrl, WebviewWindowBuilder};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Startup timing, printed in debug builds only. The gap from process start
    // to "started" is WebView2 coming up; "started" to "finished" is the page
    // and its module graph being served (by Vite, in a dev session). The page
    // itself logs when its script has run, so the whole black-window interval can be
    // split into who was waiting on what.
    let launched = std::time::Instant::now();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .on_page_load(move |_webview, payload| {
            if cfg!(debug_assertions) {
                let what = match payload.event() {
                    tauri::webview::PageLoadEvent::Started => "started",
                    tauri::webview::PageLoadEvent::Finished => "finished",
                };
                eprintln!(
                    "lazuli: page load {what} at {} ms",
                    launched.elapsed().as_millis()
                );
            }
        })
        // The window is built here rather than declared in `tauri.conf.json`,
        // because its frame theme and background colour have to be known at
        // creation: set afterwards, each one repaints the window in front of
        // the user, and hiding it meanwhile only turns the flicker into a
        // window that appears, vanishes and appears again.
        .setup(|app| {
            // Registered here rather than in the chain above because the crate
            // is only a dependency on desktop targets — see `Cargo.toml`.
            #[cfg(desktop)]
            app.handle()
                .plugin(tauri_plugin_updater::Builder::new().build())?;

            // Before the window: on Windows this is where a downloaded update
            // is applied, and it must happen while there is nothing on screen
            // to close. See `updates.rs`.
            updates::at_launch(app.handle());

            let dress =
                theme::dress_for(&commands::appearance_preference(app.handle()));
            // "main" is the label the capabilities file grants permissions to.
            let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
                .title("Lazuli")
                .icon(icon()?)?
                .inner_size(1100.0, 820.0)
                .min_inner_size(640.0, 480.0)
                .maximized(true)
                // The bar that minimises, maximises and closes the window is
                // drawn by the page — see `src/titlebar.ts`. Only the caption
                // goes: the resize frame is a separate window style, so edges,
                // corners and Aero snap all still work.
                .decorations(false)
                // Hidden only for the length of the dance below.
                .visible(false)
                .theme(dress.theme)
                .background_color(dress.background)
                .build()?;

            // An undecorated window gets its resize edges from an overlay child
            // window Tauri puts along its border (`TAURI_DRAG_RESIZE_BORDERS`),
            // and a maximised window should have none — there is nothing to
            // resize. Created maximised, it keeps one: the top four pixels of
            // the client area are covered, the page is never told the pointer
            // is in them, and so the title bar can never be hovered into view,
            // which is the only gesture that reveals it. Maximising the window
            // again collapses the overlay, and nothing else measured does.
            //
            // Hence the hidden window: this is the restore-and-maximise the
            // user would otherwise have to do by hand, done before there is
            // anything on screen to see it happen.
            window.unmaximize()?;
            window.maximize()?;
            window.show()?;

            // Before anything can ask: the webview answers permission
            // questions itself, and the page is free to ask from its first
            // frame.
            camera::allow(&window);

            // Looks for a newer version a few seconds from now, downloads it
            // in the background if there is one, and says nothing.
            updates::start(app.handle().clone());
            Ok(())
        })
        // Only on macOS and Linux does this do anything: a downloaded update
        // is applied as the window closes. On Windows it was applied at launch.
        .on_window_event(updates::on_window_event)
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::create_project,
            commands::new_project_target,
            commands::close_project,
            commands::peek_project,
            commands::project_tabs,
            commands::add_project,
            commands::move_project,
            commands::forget_project,
            commands::restore_listing,
            commands::add_tab,
            commands::rename_tab,
            commands::delete_tab,
            commands::restore_tab,
            commands::move_tab,
            commands::trash_project,
            commands::restore_project,
            commands::trash_contents,
            commands::restore_trashed,
            commands::set_project_name,
            commands::set_start_date,
            commands::set_cover,
            commands::set_date_format,
            commands::set_sort_order,
            commands::show_spelling_suggestions,
            commands::create_entry,
            commands::update_entry,
            commands::import_images,
            commands::import_image_bytes,
            commands::trash_image,
            commands::trash_entry,
            commands::undo_delete,
            commands::journal_today,
            commands::startup_project,
            commands::default_projects_dir,
            commands::set_theme_preference,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Lazuli");
}

/// The window's icon — its taskbar button, and Alt-Tab.
///
/// Given explicitly rather than left to the executable's own icon resource.
/// That resource is embedded by the build script, which only re-runs when
/// `tauri.conf.json` changes — replace the files in `icons/` alone and the
/// binary keeps the icon it was first built with, which is exactly how the old
/// one survived the rename. This reads the PNG the icons were generated from,
/// so the window cannot disagree with the folder.
fn icon() -> tauri::Result<tauri::image::Image<'static>> {
    tauri::image::Image::from_bytes(include_bytes!("../icons/128x128.png"))
}
