//! Journaley: a local-first project journal.
//!
//! A project is a folder on disk and nothing else. Everything the app knows is
//! re-read from that folder; see [`store`] for the read path and [`watch`] for
//! how external edits get noticed.

pub mod commands;
pub mod dates;
pub mod model;
pub mod paths;
pub mod store;
pub mod video;
pub mod watch;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Startup timing, printed in debug builds only. The gap from process start
    // to "started" is WebView2 coming up; "started" to "finished" is the page
    // and its module graph being served (by Vite, in a dev session). The page
    // itself logs its first render, so the whole black-window interval can be
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
                    "journaley: page load {what} at {} ms",
                    launched.elapsed().as_millis()
                );
            }
        })
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::open_project,
            commands::create_project,
            commands::new_project_target,
            commands::close_project,
            commands::peek_project,
            commands::recent_projects,
            commands::forget_recent,
            commands::restore_recent,
            commands::trash_project,
            commands::restore_project,
            commands::set_project_name,
            commands::set_start_date,
            commands::set_cover,
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
            commands::ffmpeg_status,
            commands::export_begin,
            commands::export_push_frame,
            commands::export_finish,
            commands::export_cancel,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Journaley");
}
