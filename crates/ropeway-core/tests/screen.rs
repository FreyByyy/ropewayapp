use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ropeway_core::screen::{FLAG_KEYFRAME, PKT_AUDIO, PKT_INFO, PKT_VIDEO};
use ropeway_core::{BroadcastSink, CaptureOptions, Event, Node, NodeOptions, Notice, ScreenProvider, StreamInfo};
use tokio::sync::broadcast::Receiver;

/// Pretends to capture: a keyframe whenever one is requested, small delta
/// frames otherwise, and a little audio.
#[derive(Default)]
struct FakeProvider {
    running: Mutex<HashMap<String, Arc<std::sync::atomic::AtomicBool>>>,
}

impl ScreenProvider for FakeProvider {
    fn start(&self, session_id: &str, options: &CaptureOptions, sink: BroadcastSink) -> anyhow::Result<()> {
        let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.running.lock().unwrap().insert(session_id.to_owned(), stop.clone());
        let audio = options.system_audio;
        std::thread::spawn(move || {
            sink.info(&StreamInfo { width: 64, height: 48, audio, ..Default::default() });
            let mut ts = 0u64;
            while !stop.load(std::sync::atomic::Ordering::Relaxed) && !sink.is_closed() {
                let key = sink.take_keyframe_request();
                sink.video(key, ts, if key { b"KEYFRAME" } else { b"delta" });
                if audio {
                    sink.audio(ts, &[0u8; 3840]);
                }
                ts += 33_000;
                std::thread::sleep(Duration::from_millis(15));
            }
        });
        Ok(())
    }

    fn stop(&self, session_id: &str) {
        if let Some(flag) = self.running.lock().unwrap().remove(session_id) {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
        }
    }
}

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ropeway-screen-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn start(tag: &str) -> Node {
    let node = Node::start(NodeOptions { data_dir: temp_dir(tag), port: Some(0), discovery: false, use_recycle_bin: false })
        .await
        .unwrap();
    node.set_screen_provider(Arc::new(FakeProvider::default()));
    node
}

async fn pair() -> (Node, Node) {
    let a = start("a").await;
    let b = start("b").await;
    b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    (a, b)
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

fn options() -> CaptureOptions {
    CaptureOptions { video: Default::default(), source: None, system_audio: true, microphone: false, control: false }
}

#[tokio::test(flavor = "multi_thread")]
async fn invited_viewer_receives_stream_until_share_stops() {
    let (a, b) = pair().await;
    let mut b_events = b.subscribe();
    let session = a.start_screen_share(options(), vec![b.id().to_owned()], false).unwrap();

    let share = wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenInvite { share }) => Some(share),
        _ => None,
    })
    .await;
    assert_eq!(share.id, session);
    assert_eq!(share.peer, a.id());
    assert!(!share.requested);

    let mut rx = b.watch_screen(a.id(), &session).await.unwrap().packets;
    let first = rx.recv().await.unwrap();
    assert_eq!(first[0], PKT_INFO);
    let info: StreamInfo = serde_json::from_slice(&first[14..]).unwrap();
    assert_eq!((info.width, info.height, info.audio), (64, 48, true));

    // The first video packet a new viewer gets is a keyframe.
    let mut saw_audio = false;
    let first_video = loop {
        let p = rx.recv().await.unwrap();
        match p[0] {
            PKT_VIDEO => break p,
            PKT_AUDIO => saw_audio = true,
            _ => {}
        }
    };
    assert_eq!(first_video[1] & FLAG_KEYFRAME, FLAG_KEYFRAME);
    assert_eq!(&first_video[14..], b"KEYFRAME");
    for _ in 0..5 {
        let p = rx.recv().await.unwrap();
        saw_audio |= p[0] == PKT_AUDIO;
    }
    assert!(saw_audio);

    // The sharer sees who is watching.
    let view = a.screen();
    assert_eq!(view.sharing.len(), 1);
    assert!(view.sharing[0].viewers.iter().any(|v| v.id == b.id()));

    a.stop_screen_share(&session);
    tokio::time::timeout(Duration::from_secs(5), async { while rx.recv().await.is_some() {} })
        .await
        .expect("stream did not end");
    wait_for(&mut b_events, |e| match e {
        Event::Screen(v) if v.available.is_empty() => Some(()),
        _ => None,
    })
    .await;
}

#[tokio::test(flavor = "multi_thread")]
async fn uninvited_device_cannot_watch() {
    let (a, b) = pair().await;
    let session = a.start_screen_share(options(), vec![], false).unwrap();
    let err = b.watch_screen(a.id(), &session).await.unwrap_err();
    assert!(err.to_string().contains("not shared"), "{err}");
}

#[tokio::test(flavor = "multi_thread")]
async fn asking_to_see_a_screen_needs_approval_unless_trusted() {
    let (a, b) = pair().await;
    let mut a_events = a.subscribe();
    let mut b_events = b.subscribe();

    // Untrusted: A decides.
    b.ask_screen(a.id(), false).unwrap();
    let ask = wait_for(&mut a_events, |e| match e {
        Event::Notice(Notice::ScreenAsked { ask }) => Some(ask),
        _ => None,
    })
    .await;
    assert_eq!(ask.peer, b.id());
    assert!(a.screen().sharing.is_empty());
    a.answer_screen_ask(&ask.id, true).unwrap();
    let share = wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenInvite { share }) => Some(share),
        _ => None,
    })
    .await;
    assert!(share.requested);
    assert!(b.screen().asked.is_empty());
    let mut rx = b.watch_screen(a.id(), &share.id).await.unwrap().packets;
    assert!(rx.recv().await.is_some());
    a.stop_screen_share(&share.id);

    // Declined.
    b.ask_screen(a.id(), false).unwrap();
    let ask = wait_for(&mut a_events, |e| match e {
        Event::Notice(Notice::ScreenAsked { ask }) => Some(ask),
        _ => None,
    })
    .await;
    a.answer_screen_ask(&ask.id, false).unwrap();
    let accepted = wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenAskAnswered { accepted, .. }) => Some(accepted),
        _ => None,
    })
    .await;
    assert!(!accepted);

    // Trusted: A is still asked, unless A lets trusted devices watch.
    a.set_trusted(b.id(), true).unwrap();
    b.ask_screen(a.id(), false).unwrap();
    let ask = wait_for(&mut a_events, |e| match e {
        Event::Notice(Notice::ScreenAsked { ask }) => Some(ask),
        _ => None,
    })
    .await;
    assert!(a.screen().sharing.is_empty());
    a.answer_screen_ask(&ask.id, false).unwrap();
    wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenAskAnswered { .. }) => Some(()),
        _ => None,
    })
    .await;

    a.set_trusted_view_without_asking(true).unwrap();
    b.ask_screen(a.id(), false).unwrap();
    let share = wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenInvite { share }) => Some(share),
        _ => None,
    })
    .await;
    assert!(share.requested);
    assert!(a.screen().asks.is_empty());
    assert_eq!(a.screen().sharing.len(), 1);
}

/// Whether a viewer's stream ends within a few seconds.
async fn ends(rx: &mut tokio::sync::mpsc::Receiver<Vec<u8>>) -> bool {
    tokio::time::timeout(Duration::from_secs(5), async {
        while rx.recv().await.is_some() {}
    })
    .await
    .is_ok()
}

#[tokio::test(flavor = "multi_thread")]
async fn revoking_trust_ends_screen_viewing() {
    let (a, b) = pair().await;
    let mut b_events = b.subscribe();
    a.set_trusted(b.id(), true).unwrap();
    a.set_trusted_view_without_asking(true).unwrap();

    // A share made for B's ask.
    b.ask_screen(a.id(), false).unwrap();
    let asked = wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenInvite { share }) => Some(share),
        _ => None,
    })
    .await;
    let mut asked_rx = b.watch_screen(a.id(), &asked.id).await.unwrap().packets;
    assert!(asked_rx.recv().await.is_some());

    // And one A started for B by hand.
    let manual = a.start_screen_share(options(), vec![b.id().to_owned()], false).unwrap();
    let mut manual_rx = b.watch_screen(a.id(), &manual).await.unwrap().packets;
    assert!(manual_rx.recv().await.is_some());

    a.set_trusted(b.id(), false).unwrap();
    assert!(ends(&mut asked_rx).await, "the asked-for share kept streaming");
    assert!(ends(&mut manual_rx).await, "the manual share kept streaming");
    // The share made for B's ask is gone; the other one stays, without B.
    let sharing = a.screen().sharing;
    assert_eq!(sharing.len(), 1);
    assert_eq!(sharing[0].id, manual);
    assert!(sharing[0].invited.is_empty());
    assert!(b.watch_screen(a.id(), &manual).await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn public_share_invites_devices_that_connect_later() {
    let a = start("pa").await;
    let session = a.start_screen_share(options(), vec![], true).unwrap();
    let b = start("pb").await;
    let mut b_events = b.subscribe();
    b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    let share = wait_for(&mut b_events, |e| match e {
        Event::Notice(Notice::ScreenInvite { share }) => Some(share),
        _ => None,
    })
    .await;
    assert_eq!(share.id, session);
    let mut rx = b.watch_screen(a.id(), &session).await.unwrap().packets;
    assert!(rx.recv().await.is_some());
}
