//! Chat deletion, invites (QR codes), temporary codes and hiding a device.
//!
//! Temporary codes travel over UDP; this test binary moves them to another
//! port so it never talks to Ropeway on the real network.

use std::path::PathBuf;
use std::time::Duration;

use ropeway_core::{Event, Node, NodeOptions, Settings};
use tokio::sync::broadcast::Receiver;

const TEST_DISCOVERY_PORT: &str = "47893";

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ropeway-test-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn start(tag: &str) -> Node {
    Node::start(NodeOptions { data_dir: temp_dir(tag), port: Some(0), discovery: false, use_recycle_bin: false })
        .await
        .unwrap()
}

async fn wait_for<T>(rx: &mut Receiver<Event>, mut pick: impl FnMut(Event) -> Option<T>) -> T {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Some(v) = pick(rx.recv().await.unwrap()) {
                return v;
            }
        }
    })
    .await
    .expect("event did not arrive in time")
}

#[tokio::test(flavor = "multi_thread")]
async fn messages_can_be_deleted_for_everyone_but_only_by_their_author() {
    let a = start("a").await;
    let b = start("b").await;
    b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let mut a_events = a.subscribe();
    let mut b_events = b.subscribe();

    let mine = b.send_chat(a.id(), "удали меня").unwrap();
    let keep = b.send_chat(a.id(), "оставь").unwrap();
    for _ in 0..2 {
        wait_for(&mut a_events, |e| matches!(e, Event::Chat(m) if !m.outgoing).then_some(())).await;
    }
    let theirs = a.send_chat(b.id(), "ответ").unwrap();
    wait_for(&mut b_events, |e| matches!(e, Event::Chat(m) if !m.outgoing).then_some(())).await;

    // B deletes its own message and A's reply for everyone: only its own
    // goes away on A; A's reply goes only on B.
    b.delete_messages(a.id(), vec![mine.id.clone(), theirs.id.clone()], true).unwrap();
    let gone = wait_for(&mut a_events, |e| match e {
        Event::ChatDeleted { conversation, ids } if conversation == b.id() => Some(ids),
        _ => None,
    })
    .await;
    assert_eq!(gone, vec![mine.id.clone()]);
    let on_a: Vec<String> = a.history(b.id()).into_iter().map(|m| m.id).collect();
    assert_eq!(on_a, vec![keep.id.clone(), theirs.id.clone()]);
    let on_b: Vec<String> = b.history(a.id()).into_iter().map(|m| m.id).collect();
    assert_eq!(on_b, vec![keep.id.clone()]);

    // Only here: A keeps its copy of nothing B removes.
    a.delete_messages(b.id(), vec![keep.id.clone()], false).unwrap();
    assert!(a.history(b.id()).iter().all(|m| m.id != keep.id));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(b.history(a.id()).iter().any(|m| m.id == keep.id));

    b.clear_chat(a.id()).unwrap();
    assert!(b.history(a.id()).is_empty());
}

#[tokio::test(flavor = "multi_thread")]
async fn invite_reaches_exactly_the_device_it_names() {
    let a = start("ia").await;
    let b = start("ib").await;
    let link = a.me().invite;
    assert!(link.starts_with("ropeway://connect?"));

    let peer = b.connect_invite(&link).await.unwrap();
    assert_eq!(peer.id, a.id());
    assert!(peer.online);
    assert!(!peer.trusted, "an invite does not make a device trusted");

    // The same addresses with another device's id: the key does not match.
    let c = start("ic").await;
    let forged = link.replace(a.id(), c.id());
    assert!(b.connect_invite(&forged).await.is_err());
    assert!(b.connect_invite("not a code").await.is_err());
}

#[tokio::test(flavor = "multi_thread")]
async fn temporary_code_finds_a_hidden_device() {
    std::env::set_var("ROPEWAY_DISCOVERY_PORT", TEST_DISCOVERY_PORT);
    // A listens for codes but is hidden, so it never announces itself.
    let dir = temp_dir("ca");
    let mut settings = Settings::default();
    settings.discoverable = false;
    settings.save(&dir.join("settings.json")).unwrap();
    let a = Node::start(NodeOptions { data_dir: dir, port: Some(0), discovery: true, use_recycle_bin: false })
        .await
        .unwrap();
    let b = start("cb").await;

    assert!(b.connect_code("123456").await.is_err(), "no code is shown yet");
    let (code, expires) = a.start_pairing_code();
    assert!(expires > 0);
    let wrong = if code == "000000" { "111111" } else { "000000" };
    assert!(b.connect_code(wrong).await.is_err());

    let peer = b.connect_code(&format!("{} {}", &code[..3], &code[3..])).await.unwrap();
    assert_eq!(peer.id, a.id());
    assert!(!peer.trusted);

    a.stop_pairing_code();
    let c = start("cc").await;
    assert!(c.connect_code(&code).await.is_err(), "a stopped code finds nothing");
}

#[tokio::test(flavor = "multi_thread")]
async fn favorites_are_kept_apart_from_trust() {
    let a = start("fa").await;
    let b = start("fb").await;
    b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    b.set_favorite(a.id(), true).unwrap();
    let p = b.peers().into_iter().find(|p| p.id == a.id()).unwrap();
    assert!(p.favorite && !p.trusted);
    assert!(b.settings().is_favorite(a.id()));
    b.set_favorite(a.id(), false).unwrap();
    assert!(!b.peers().into_iter().find(|p| p.id == a.id()).unwrap().favorite);
}
