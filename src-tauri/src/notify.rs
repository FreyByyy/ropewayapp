//! System notifications that open the right place in the app when clicked.

#[cfg(desktop)]
use tauri::Emitter;
use tauri::AppHandle;
use tauri_plugin_notification::NotificationExt;

/// Tells the window what to open: `chat:<conversation>` or `shared`.
#[cfg(desktop)]
pub const OPEN_EVENT: &str = "ropeway-open";

/// Where a click on a notification leads.
pub enum Target<'a> {
    Chat(&'a str),
    Shared,
    App,
}

pub fn show(app: &AppHandle, title: String, body: String, target: Target) {
    let open = match target {
        Target::Chat(conversation) => format!("chat:{conversation}"),
        Target::Shared => "shared".to_string(),
        Target::App => String::new(),
    };
    let _ = app.notification().builder().title(title).body(body).extra("open", open).show();
}

/// Brings the window forward on a click and passes the target to the page.
/// On Windows a click is reported while the toast is on screen. On Android
/// the page listens to the plugin's `actionPerformed` itself.
#[cfg(mobile)]
pub fn init(_app: &AppHandle) {}

#[cfg(desktop)]
pub fn init(app: &AppHandle) {
    let handle = app.clone();
    let _ = app.notification().on_action(move |performed| {
        let open = performed
            .notification()
            .and_then(|n| n.extra().get("open"))
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        let app = handle.clone();
        let _ = handle.run_on_main_thread(move || {
            crate::show_main(&app);
            if !open.is_empty() {
                let _ = app.emit_to("main", OPEN_EVENT, open);
            }
        });
    });
}
