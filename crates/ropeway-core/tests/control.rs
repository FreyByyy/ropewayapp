use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ropeway_core::screen::{PKT_CLIPBOARD, PKT_VIDEO};
use ropeway_core::{BroadcastSink, CaptureOptions, Event, InputEvent, Node, NodeOptions, Notice, ScreenProvider, StreamInfo};
use tokio::sync::broadcast::Receiver;

/// Records input; in control sessions it also publishes clipboard text.
#[derive(Default)]
struct RecordingProvider {
    inputs: Mutex<Vec<InputEvent>>,
    stop: Arc<AtomicBool>,
}

impl ScreenProvider for RecordingProvider {
    fn start(&self, _session_id: &str, options: &CaptureOptions, sink: BroadcastSink) -> anyhow::Result<()> {
        let stop = self.stop.clone();
        let control = options.control;
        std::thread::spawn(move || {
            sink.info(&StreamInfo { width: 64, height: 48, audio: false, ..Default::default() });
            while !stop.load(Ordering::Relaxed) && !sink.is_closed() {
                let key = sink.take_keyframe_request();
                sink.video(key, 0, b"frame");
                if control {
                    sink.clipboard("скопировано на A");
                }
                std::thread::sleep(Duration::from_millis(20));
            }
        });
        Ok(())
    }

    fn stop(&self, _session_id: &str) {}

    fn input(&self, _session_id: &str, event: &InputEvent) {
        self.inputs.lock().unwrap().push(event.clone());
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ropeway-control-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn start(tag: &str, provider: Arc<RecordingProvider>) -> Node {
    let node = Node::start(NodeOptions { data_dir: temp_dir(tag), port: Some(0), discovery: false, use_recycle_bin: false })
        .await
        .unwrap();
    node.set_screen_provider(provider);
    node
}

async fn wait_for<T>(rx: &mut Receiver<Event>, mut pick: impl FnMut(Event) -> Option<T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await {
                Ok(e) => {
                    if let Some(v) = pick(e) {
                        return v;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(e) => panic!("{e}"),
            }
        }
    })
    .await
    .expect("event did not arrive in time")
}

async fn wait_until(mut check: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(10), async {
        while !check() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("condition not reached in time");
}

#[tokio::test(flavor = "multi_thread")]
async fn controller_input_is_applied_only_after_approval() {
    let provider = Arc::new(RecordingProvider::default());
    let a = start("a", provider.clone()).await;
    let b = start("b", Arc::new(RecordingProvider::default())).await;
    b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    let mut a_events = a.subscribe();
    let mut b_events = b.subscribe();

    b.ask_screen(a.id(), true).unwrap();
    let ask = wait_for(&mut a_events, |e| match e {
        Event::Notice(Notice::ScreenAsked { ask }) => Some(ask),
        _ => None,
    })
    .await;
    assert!(ask.control);
    a.answer_screen_ask(&ask.id, true).unwrap();

    let share = wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenInvite { share }) => Some(share),
        _ => None,
    })
    .await;
    assert!(share.requested && share.control);

    let mut watch = b.watch_screen(a.id(), &share.id).await.unwrap();
    // Clipboard text reaches the controller.
    let mut clipboard = None;
    for _ in 0..50 {
        let p = watch.packets.recv().await.unwrap();
        if p[0] == PKT_CLIPBOARD {
            clipboard = Some(String::from_utf8(p[14..].to_vec()).unwrap());
            break;
        }
    }
    assert_eq!(clipboard.as_deref(), Some("скопировано на A"));

    let events = vec![
        InputEvent::MouseMove { x: 0.25, y: 0.75 },
        InputEvent::MouseButton { button: 0, down: true },
        InputEvent::MouseButton { button: 0, down: false },
        InputEvent::Key { code: "KeyA".into(), down: true },
        InputEvent::Key { code: "KeyA".into(), down: false },
        InputEvent::Clipboard { text: "с B".into() },
    ];
    for e in &events {
        watch.input.send(e.clone()).await.unwrap();
    }
    wait_until(|| provider.inputs.lock().unwrap().len() >= events.len()).await;
    assert_eq!(&provider.inputs.lock().unwrap()[..events.len()], &events[..]);
    // The sharer shows who is in control.
    assert!(a.screen().sharing[0].controllers.iter().any(|c| c.id == b.id()));

    // Closing the viewer releases held keys and ends the share made for it.
    drop(watch);
    wait_until(|| provider.inputs.lock().unwrap().last() == Some(&InputEvent::ReleaseAll)).await;
    wait_until(|| a.screen().sharing.is_empty()).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn view_only_viewer_cannot_control() {
    let provider = Arc::new(RecordingProvider::default());
    let a = start("va", provider.clone()).await;
    let b = start("vb", Arc::new(RecordingProvider::default())).await;
    b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Even asking for control through the plain share API grants none.
    let options = CaptureOptions { video: Default::default(), source: None, system_audio: false, microphone: false, control: true };
    let session = a.start_screen_share(options, vec![b.id().to_owned()], false).unwrap();
    let mut watch = b.watch_screen(a.id(), &session).await.unwrap();
    for _ in 0..10 {
        let p = watch.packets.recv().await.unwrap();
        assert_ne!(p[0], PKT_CLIPBOARD);
        if p[0] == PKT_VIDEO {
            break;
        }
    }
    watch.input.send(InputEvent::Key { code: "KeyA".into(), down: true }).await.unwrap();
    watch.input.send(InputEvent::MouseMove { x: 0.5, y: 0.5 }).await.unwrap();
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(provider.inputs.lock().unwrap().is_empty());
    assert!(a.screen().sharing[0].controllers.is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn requester_may_change_video_but_not_send_input() {
    let provider = Arc::new(RecordingProvider::default());
    let a = start("ra", provider.clone()).await;
    let b = start("rb", Arc::new(RecordingProvider::default())).await;
    b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    a.set_trusted(b.id(), true).unwrap();
    a.set_trusted_view_without_asking(true).unwrap();
    let mut b_events = b.subscribe();

    b.ask_screen(a.id(), false).unwrap();
    let share = wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenInvite { share }) => Some(share),
        _ => None,
    })
    .await;
    assert!(!share.control);
    let watch = b.watch_screen(a.id(), &share.id).await.unwrap();
    let video = ropeway_core::screen::VideoSettings {
        fps: 60,
        resolution: ropeway_core::screen::Resolution::P1080,
        quality: ropeway_core::screen::Quality::High,
    };
    watch.input.send(InputEvent::MouseMove { x: 0.1, y: 0.1 }).await.unwrap();
    watch.input.send(InputEvent::SetVideo { video }).await.unwrap();
    wait_until(|| !provider.inputs.lock().unwrap().is_empty()).await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(*provider.inputs.lock().unwrap(), vec![InputEvent::SetVideo { video }]);
}

#[tokio::test(flavor = "multi_thread")]
async fn trusted_device_still_asks_for_control() {
    let provider = Arc::new(RecordingProvider::default());
    let a = start("ta", provider.clone()).await;
    let b = start("tb", Arc::new(RecordingProvider::default())).await;
    b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    a.set_trusted(b.id(), true).unwrap();
    let mut a_events = a.subscribe();
    let mut b_events = b.subscribe();

    b.ask_screen(a.id(), true).unwrap();
    let ask = wait_for(&mut a_events, |e| match e {
        Event::Notice(Notice::ScreenAsked { ask }) => Some(ask),
        _ => None,
    })
    .await;
    assert!(ask.control);
    assert!(a.screen().sharing.is_empty());
    a.answer_screen_ask(&ask.id, true).unwrap();
    let share = wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenInvite { share }) => Some(share),
        _ => None,
    })
    .await;
    assert!(share.control);
    let watch = b.watch_screen(a.id(), &share.id).await.unwrap();
    watch.input.send(InputEvent::Wheel { dx: 0.0, dy: 1.0 }).await.unwrap();
    wait_until(|| !provider.inputs.lock().unwrap().is_empty()).await;
}
