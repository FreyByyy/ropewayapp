use std::path::{Path, PathBuf};
use std::time::Duration;

use ropeway_core::protocol::ChangeKind;
use ropeway_core::{ChangeStatus, Event, Node, NodeOptions, Notice, TransferStatus, TransferView};
use tokio::sync::broadcast::Receiver;

fn temp_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ropeway-files-{tag}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn start_in(dir: &Path) -> Node {
    let node = Node::start(NodeOptions {
        data_dir: dir.join("data"),
        port: Some(0),
        discovery: false,
        use_recycle_bin: false,
    })
    .await
    .unwrap();
    node.set_download_dir(Some(dir.join("downloads"))).unwrap();
    node.set_shared_dir(Some(dir.join("shared"))).unwrap();
    node
}

async fn pair() -> (Node, Node, PathBuf, PathBuf) {
    let (a_dir, b_dir) = (temp_dir("a"), temp_dir("b"));
    let a = start_in(&a_dir).await;
    let b = start_in(&b_dir).await;
    b.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    // Let A register the connection and hello.
    tokio::time::sleep(Duration::from_millis(200)).await;
    (a, b, a_dir, b_dir)
}

async fn wait_for<T>(rx: &mut Receiver<Event>, secs: u64, mut pick: impl FnMut(Event) -> Option<T>) -> T {
    tokio::time::timeout(Duration::from_secs(secs), async {
        loop {
            match rx.recv().await {
                Ok(e) => {
                    if let Some(v) = pick(e) {
                        return v;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(e) => panic!("event channel closed: {e}"),
            }
        }
    })
    .await
    .expect("event did not arrive in time")
}

async fn wait_transfer(rx: &mut Receiver<Event>, id: &str, status: TransferStatus) -> TransferView {
    let id = id.to_owned();
    wait_for(rx, 60, move |e| match e {
        Event::Transfer(t) if t.id == id && t.status == status => Some(t),
        _ => None,
    })
    .await
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn pattern(len: usize, seed: u8) -> Vec<u8> {
    (0..len).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed)).collect()
}

#[tokio::test(flavor = "multi_thread")]
async fn folder_offer_needs_approval_then_arrives_intact() {
    let (a, b, a_dir, b_dir) = pair().await;
    let src = a_dir.join("outbox");
    write(&src.join("Проект/readme.txt"), "привет".as_bytes());
    write(&src.join("Проект/data/big.bin"), &pattern(3 * 1024 * 1024 + 17, 7));
    write(&src.join("Проект/empty.txt"), b"");
    std::fs::create_dir_all(src.join("Проект/пустая папка")).unwrap();
    write(&src.join("single.txt"), b"one");

    let mut b_events = b.subscribe();
    let mut a_events = a.subscribe();
    let sent = a.send_paths(b.id(), vec![src.join("Проект"), src.join("single.txt")]).await.unwrap();
    assert_eq!(sent.file_count, 4);

    // Not trusted: B has to decide.
    let offer = wait_for(&mut b_events, 10, |e| match e {
        Event::Notice(Notice::OfferReceived { transfer }) => Some(transfer),
        _ => None,
    })
    .await;
    assert_eq!(offer.status, TransferStatus::Pending);
    assert_eq!(offer.root_count, 2);

    b.accept_transfer(&offer.id).unwrap();
    let done = wait_transfer(&mut b_events, &offer.id, TransferStatus::Completed).await;
    assert_eq!(done.transferred, done.total);
    wait_transfer(&mut a_events, &sent.id, TransferStatus::Completed).await;

    let dl = b_dir.join("downloads");
    assert_eq!(std::fs::read(dl.join("Проект/readme.txt")).unwrap(), "привет".as_bytes());
    assert_eq!(std::fs::read(dl.join("Проект/data/big.bin")).unwrap(), pattern(3 * 1024 * 1024 + 17, 7));
    assert_eq!(std::fs::read(dl.join("Проект/empty.txt")).unwrap(), b"");
    assert!(dl.join("Проект/пустая папка").is_dir());
    assert_eq!(std::fs::read(dl.join("single.txt")).unwrap(), b"one");

    // Sending the same folder again does not overwrite: it becomes "Проект (1)".
    b.set_trusted(a.id(), true).unwrap();
    let again = a.send_paths(b.id(), vec![src.join("Проект")]).await.unwrap();
    wait_transfer(&mut a_events, &again.id, TransferStatus::Completed).await;
    assert!(dl.join("Проект (1)/data/big.bin").is_file());

    a.shutdown().await;
    b.shutdown().await;
    let _ = std::fs::remove_dir_all(&a_dir);
    let _ = std::fs::remove_dir_all(&b_dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn rejected_offer_is_reported_to_sender() {
    let (a, b, a_dir, _) = pair().await;
    write(&a_dir.join("x.txt"), b"x");
    let mut a_events = a.subscribe();
    let mut b_events = b.subscribe();
    let sent = a.send_paths(b.id(), vec![a_dir.join("x.txt")]).await.unwrap();
    let offer = wait_for(&mut b_events, 10, |e| match e {
        Event::Notice(Notice::OfferReceived { transfer }) => Some(transfer),
        _ => None,
    })
    .await;
    b.reject_transfer(&offer.id).unwrap();
    wait_transfer(&mut a_events, &sent.id, TransferStatus::Rejected).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn interrupted_download_resumes_after_restart() {
    let (a, b, a_dir, b_dir) = pair().await;
    b.set_trusted(a.id(), true).unwrap();
    let size = 96 * 1024 * 1024;
    let content = pattern(size, 3);
    write(&a_dir.join("movie.bin"), &content);

    let mut b_events = b.subscribe();
    let mut a_events = a.subscribe();
    let sent = a.send_paths(b.id(), vec![a_dir.join("movie.bin")]).await.unwrap();

    // Stop B somewhere in the middle of the file.
    let id = sent.id.clone();
    wait_for(&mut b_events, 30, |e| match e {
        Event::Transfer(t) if t.id == id && t.transferred > 0 && t.transferred < t.total => Some(()),
        _ => None,
    })
    .await;
    b.shutdown().await;
    drop(b);
    tokio::time::sleep(Duration::from_millis(500)).await;

    let part = b_dir.join("downloads/movie.bin.ropeway-part");
    let partial = std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0);
    assert!(partial < size as u64, "file finished before the restart");

    // B restarts with the same identity and reconnects: the transfer resumes.
    let b2 = Node::start(NodeOptions { data_dir: b_dir.join("data"), port: Some(0), discovery: false, use_recycle_bin: false })
        .await
        .unwrap();
    let mut b2_events = b2.subscribe();
    assert!(b2.transfers().iter().any(|t| t.id == sent.id && t.status == TransferStatus::Interrupted));
    b2.connect_address(&format!("127.0.0.1:{}", a.port())).await.unwrap();
    wait_transfer(&mut b2_events, &sent.id, TransferStatus::Completed).await;
    wait_transfer(&mut a_events, &sent.id, TransferStatus::Completed).await;
    assert_eq!(std::fs::read(b_dir.join("downloads/movie.bin")).unwrap(), content);
    assert!(!part.exists());

    a.shutdown().await;
    b2.shutdown().await;
    let _ = std::fs::remove_dir_all(&a_dir);
    let _ = std::fs::remove_dir_all(&b_dir);
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_folder_copy_edit_and_delete_with_owner_approval() {
    let (a, b, a_dir, b_dir) = pair().await;
    let shared = a_dir.join("shared");
    write(&shared.join("docs/a.txt"), b"alpha");
    write(&shared.join("docs/sub/b.bin"), &pattern(70_000, 1));
    write(&shared.join("top.txt"), b"version 1");

    // Listing.
    let listing = b.peer_shared(a.id()).await.unwrap();
    assert!(listing.enabled);
    let paths: Vec<&str> = listing.entries.iter().map(|e| e.path.as_str()).collect();
    for expected in ["docs", "docs/a.txt", "docs/sub", "docs/sub/b.bin", "top.txt"] {
        assert!(paths.contains(&expected), "missing {expected} in {paths:?}");
    }

    // Copy a folder.
    let mut b_events = b.subscribe();
    let copy = b.download_shared(a.id(), "docs").await.unwrap();
    wait_transfer(&mut b_events, &copy.id, TransferStatus::Completed).await;
    assert_eq!(std::fs::read(b_dir.join("downloads/docs/a.txt")).unwrap(), b"alpha");
    assert_eq!(std::fs::read(b_dir.join("downloads/docs/sub/b.bin")).unwrap(), pattern(70_000, 1));

    // Edit a copy and propose it; the owner's file changes only after approval.
    b.edit_shared(a.id(), "top.txt").await.unwrap();
    let local = wait_for(&mut b_events, 20, |e| match e {
        Event::Notice(Notice::EditReady { path }) => Some(PathBuf::from(path)),
        _ => None,
    })
    .await;
    assert_eq!(std::fs::read(&local).unwrap(), b"version 1");
    std::fs::write(&local, b"version 2 from B").unwrap();
    assert!(b.edits().iter().any(|e| e.path == "top.txt" && e.modified));

    let mut a_events = a.subscribe();
    b.propose_replace(a.id(), "top.txt").unwrap();
    let request = wait_for(&mut a_events, 10, |e| match e {
        Event::Notice(Notice::ChangeRequested { change }) => Some(change),
        _ => None,
    })
    .await;
    assert_eq!(request.kind, ChangeKind::Replace);
    assert!(!request.conflict);
    assert_eq!(std::fs::read(shared.join("top.txt")).unwrap(), b"version 1");

    a.approve_change(&request.id).unwrap();
    let answer = wait_for(&mut b_events, 20, |e| match e {
        Event::Notice(Notice::ChangeAnswered { change }) => Some(change),
        _ => None,
    })
    .await;
    assert_eq!(answer.status, ChangeStatus::Applied, "{:?}", answer.error);
    assert_eq!(std::fs::read(shared.join("top.txt")).unwrap(), b"version 2 from B");
    assert!(b.edits().is_empty());
    // The old version was kept, not destroyed.
    let removed = a_dir.join("data/removed");
    assert!(std::fs::read_dir(&removed).unwrap().count() == 1);

    // Delete: first rejected, then approved.
    b.propose_delete(a.id(), "docs/sub").unwrap();
    let request = wait_for(&mut a_events, 10, |e| match e {
        Event::Notice(Notice::ChangeRequested { change }) => Some(change),
        _ => None,
    })
    .await;
    a.reject_change(&request.id).unwrap();
    let answer = wait_for(&mut b_events, 10, |e| match e {
        Event::Notice(Notice::ChangeAnswered { change }) => Some(change),
        _ => None,
    })
    .await;
    assert_eq!(answer.status, ChangeStatus::Rejected);
    assert!(shared.join("docs/sub/b.bin").exists());

    b.propose_delete(a.id(), "docs/sub").unwrap();
    let request = wait_for(&mut a_events, 10, |e| match e {
        Event::Notice(Notice::ChangeRequested { change }) => Some(change),
        _ => None,
    })
    .await;
    a.approve_change(&request.id).unwrap();
    let answer = wait_for(&mut b_events, 10, |e| match e {
        Event::Notice(Notice::ChangeAnswered { change }) => Some(change),
        _ => None,
    })
    .await;
    assert_eq!(answer.status, ChangeStatus::Applied, "{:?}", answer.error);
    assert!(!shared.join("docs/sub").exists());
    assert!(shared.join("docs/a.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_folder_cannot_be_escaped() {
    let (a, b, a_dir, _) = pair().await;
    write(&a_dir.join("secret.txt"), b"do not leak");
    let err = b.download_shared(a.id(), "../secret.txt").await.unwrap_err();
    assert!(err.to_string().contains("invalid path"), "{err}");
    b.propose_delete(a.id(), "../secret.txt").unwrap_err();
    assert!(a_dir.join("secret.txt").exists());
}

#[tokio::test(flavor = "multi_thread")]
async fn shared_folder_can_be_limited_to_trusted_devices() {
    let (a, b, a_dir, _b_dir) = pair().await;
    write(&a_dir.join("shared/note.txt"), b"hi");
    a.set_shared_trusted_only(true).unwrap();

    let err = b.peer_shared(a.id()).await.unwrap_err().to_string();
    assert!(err.contains("trusted devices only"), "{err}");
    assert!(b.download_shared(a.id(), "note.txt").await.is_err());

    a.set_trusted(b.id(), true).unwrap();
    let listing = b.peer_shared(a.id()).await.unwrap();
    assert!(listing.entries.iter().any(|e| e.path == "note.txt"));
}

#[tokio::test(flavor = "multi_thread")]
async fn files_from_untrusted_devices_are_marked_as_downloaded() {
    let (a, b, a_dir, b_dir) = pair().await;
    write(&a_dir.join("shared/tool.bat"), b"echo hi");
    let mut b_events = b.subscribe();
    let t = b.download_shared(a.id(), "tool.bat").await.unwrap();
    wait_transfer(&mut b_events, &t.id, TransferStatus::Completed).await;
    let file = b_dir.join("downloads/tool.bat");
    assert_eq!(std::fs::read(&file).unwrap(), b"echo hi");
    #[cfg(windows)]
    {
        let zone = std::fs::read_to_string(format!("{}:Zone.Identifier", file.display())).unwrap();
        assert!(zone.contains("ZoneId=3"), "{zone}");
    }
}

