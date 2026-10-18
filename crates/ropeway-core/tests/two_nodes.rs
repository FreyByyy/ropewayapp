use std::path::PathBuf;
use std::time::Duration;

use ropeway_core::{Event, MessageStatus, Node, NodeOptions, GENERAL};
use tokio::sync::broadcast::Receiver;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ropeway-test-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn start(tag: &str) -> Node {
    Node::start(NodeOptions {
        data_dir: temp_dir(tag),
        port: Some(0),
        discovery: false,
        use_recycle_bin: false,
    })
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
async fn direct_and_general_chat() {
    let a = start("a").await;
    let b = start("b").await;
    a.set_device_name("Alpha").unwrap();
    b.set_device_name("Beta").unwrap();
    let mut a_events = a.subscribe();
    let mut b_events = b.subscribe();

    // B connects to A by address; both sides learn each other's name.
    let peer = b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    assert_eq!(peer.id, a.id());
    assert_eq!(peer.name, "Alpha");
    assert!(peer.online);
    wait_for(&mut a_events, |e| match e {
        Event::Peers(list) => list.iter().any(|p| p.id == b.id() && p.name == "Beta" && p.online).then_some(()),
        _ => None,
    })
    .await;

    // Direct message B -> A.
    let sent = b.send_chat(a.id(), "привет").unwrap();
    let got = wait_for(&mut a_events, |e| match e {
        Event::Chat(m) if !m.outgoing => Some(m),
        _ => None,
    })
    .await;
    assert_eq!(got.text, "привет");
    assert_eq!(got.conversation, b.id());
    assert_eq!(got.from_name, "Beta");
    wait_for(&mut b_events, |e| match e {
        Event::Chat(m) if m.id == sent.id && m.status == MessageStatus::Sent => Some(()),
        _ => None,
    })
    .await;

    // A replies over the connection B opened, without knowing B's port up front.
    a.send_chat(b.id(), "hi").unwrap();
    let reply = wait_for(&mut b_events, |e| match e {
        Event::Chat(m) if !m.outgoing => Some(m),
        _ => None,
    })
    .await;
    assert_eq!(reply.conversation, a.id());

    // General chat.
    a.send_chat(GENERAL, "всем").unwrap();
    let general = wait_for(&mut b_events, |e| match e {
        Event::Chat(m) if !m.outgoing && m.conversation == GENERAL => Some(m),
        _ => None,
    })
    .await;
    assert_eq!(general.text, "всем");
    assert_eq!(b.history(GENERAL).len(), 1);

    // Trust survives as a setting.
    b.set_trusted(a.id(), true).unwrap();
    assert!(b.peers().iter().any(|p| p.id == a.id() && p.trusted));
}

#[tokio::test(flavor = "multi_thread")]
async fn queued_message_is_delivered_when_peer_returns() {
    let a = start("qa").await;
    let a_dir = a.data_dir().clone();
    let a_id = a.id().to_owned();
    let a_port = a.port();
    let b = start("qb").await;
    b.connect_address(&format!("127.0.0.1:{a_port}")).await.unwrap();

    a.shutdown().await;
    drop(a);
    tokio::time::sleep(Duration::from_millis(300)).await;

    let mut b_events = b.subscribe();
    let msg = b.send_chat(&a_id, "ты тут?").unwrap();
    wait_for(&mut b_events, |e| match e {
        Event::Chat(m) if m.id == msg.id && m.status == MessageStatus::Failed => Some(()),
        _ => None,
    })
    .await;

    // A comes back on the same port with the same identity.
    let a2 = Node::start(NodeOptions { data_dir: a_dir, port: Some(a_port), discovery: false, use_recycle_bin: false })
        .await
        .unwrap();
    assert_eq!(a2.id(), a_id);
    let mut a2_events = a2.subscribe();
    b.connect_address(&format!("127.0.0.1:{a_port}")).await.unwrap();
    let got = wait_for(&mut a2_events, |e| match e {
        Event::Chat(m) if !m.outgoing => Some(m),
        _ => None,
    })
    .await;
    assert_eq!(got.text, "ты тут?");
}
