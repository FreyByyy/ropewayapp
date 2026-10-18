//! Text clipboard sync for remote control, used on both ends: watches the
//! local clipboard and reports changes, and applies text from the other side
//! without echoing it back.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

const POLL: Duration = Duration::from_millis(400);
/// Larger text is not synced.
pub const MAX_TEXT: usize = 1024 * 1024;

pub struct ClipboardSync {
    last: Arc<Mutex<Option<String>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

fn read() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

impl ClipboardSync {
    /// Calls `on_change` whenever the local clipboard gets new text. Whatever
    /// is on the clipboard at start is not sent.
    pub fn start(on_change: impl Fn(String) + Send + 'static) -> Self {
        let last = Arc::new(Mutex::new(read()));
        let stop = Arc::new(AtomicBool::new(false));
        let (last_t, stop_t) = (last.clone(), stop.clone());
        let thread = std::thread::Builder::new()
            .name("ropeway-clipboard".into())
            .spawn(move || {
                while !stop_t.load(Ordering::Relaxed) {
                    std::thread::sleep(POLL);
                    let Some(text) = read() else { continue };
                    if text.is_empty() || text.len() > MAX_TEXT {
                        continue;
                    }
                    let changed = {
                        let mut last = last_t.lock().unwrap();
                        if last.as_deref() == Some(text.as_str()) {
                            false
                        } else {
                            *last = Some(text.clone());
                            true
                        }
                    };
                    if changed {
                        on_change(text);
                    }
                }
            })
            .ok();
        Self { last, stop, thread }
    }

    /// Puts text from the other side on the local clipboard.
    pub fn set(&self, text: &str) {
        if text.len() > MAX_TEXT {
            return;
        }
        *self.last.lock().unwrap() = Some(text.to_owned());
        match arboard::Clipboard::new() {
            Ok(mut c) => {
                if let Err(e) = c.set_text(text) {
                    tracing::warn!("clipboard: {e}");
                }
            }
            Err(e) => tracing::warn!("clipboard: {e}"),
        }
    }
}

impl Drop for ClipboardSync {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
