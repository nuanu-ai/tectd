use super::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;
use tect_domain::{Error, PublicationOutcome};

#[test]
fn missing_target_is_atomically_created_read_back_and_replay_matches() {
    let fixture = Fixture::new();
    let content = "# Workspace\n\nPreserve exact instructions.\n";
    let created = adapter().publish(&fixture.directory, content).unwrap();
    assert_eq!(created.outcome, PublicationOutcome::Created);
    assert_eq!(created.byte_length, content.len() as u64);
    assert_eq!(created.sha256, hash(content.as_bytes()));
    assert_eq!(fs::read_to_string(fixture.target()).unwrap(), content);
    assert_eq!(
        fs::metadata(fixture.target()).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(stage_names(&fixture.task).is_empty());

    let replay = adapter().publish(&fixture.directory, content).unwrap();
    assert_eq!(replay.outcome, PublicationOutcome::AlreadyMatches);
    assert_eq!(fs::read_to_string(fixture.target()).unwrap(), content);
}

#[test]
fn matching_preexisting_target_is_adopted_without_rewrite() {
    let fixture = Fixture::new();
    let content = "already present";
    fs::write(fixture.target(), content).unwrap();
    let before = fs::metadata(fixture.target()).unwrap();
    let result = adapter().publish(&fixture.directory, content).unwrap();
    let after = fs::metadata(fixture.target()).unwrap();
    assert_eq!(result.outcome, PublicationOutcome::AlreadyMatches);
    assert_eq!(before.ino(), after.ino());
    assert!(stage_names(&fixture.task).is_empty());
}

#[test]
fn target_appearing_after_inspection_is_never_overwritten() {
    let fixture = Fixture::new();
    assert_eq!(
        adapter().inspect(&fixture.directory, 128).unwrap().status,
        tect_domain::SetupFileStatus::Missing
    );
    fs::write(fixture.target(), b"foreign instructions").unwrap();
    assert_eq!(
        adapter().publish(&fixture.directory, "our instructions"),
        Err(Error::SetupFileConflict)
    );
    assert_eq!(fs::read(fixture.target()).unwrap(), b"foreign instructions");
    assert!(stage_names(&fixture.task).is_empty());
}

#[test]
fn concurrent_different_publications_choose_one_without_foreign_loss() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(3));
    let directory_a = fixture.directory.clone();
    let directory_b = fixture.directory.clone();
    let start_a = barrier.clone();
    let start_b = barrier.clone();
    let first = thread::spawn(move || {
        start_a.wait();
        adapter().publish(&directory_a, "content A")
    });
    let second = thread::spawn(move || {
        start_b.wait();
        adapter().publish(&directory_b, "content B")
    });
    barrier.wait();
    let results = [first.join().unwrap(), second.join().unwrap()];
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert_eq!(
        results
            .iter()
            .filter(|result| **result == Err(Error::SetupFileConflict))
            .count(),
        1
    );
    let target = fs::read_to_string(fixture.target()).unwrap();
    assert!(target == "content A" || target == "content B");
    assert!(stage_names(&fixture.task).is_empty());
}

#[test]
fn concurrent_identical_publications_create_then_adopt() {
    let fixture = Fixture::new();
    let barrier = Arc::new(Barrier::new(3));
    let mut handles = Vec::new();
    for directory in [fixture.directory.clone(), fixture.directory.clone()] {
        let start = barrier.clone();
        handles.push(thread::spawn(move || {
            start.wait();
            adapter()
                .publish(&directory, "same content")
                .unwrap()
                .outcome
        }));
    }
    barrier.wait();
    let outcomes: Vec<_> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap())
        .collect();
    assert!(outcomes.contains(&PublicationOutcome::Created));
    assert!(outcomes.contains(&PublicationOutcome::AlreadyMatches));
    assert_eq!(
        fs::read_to_string(fixture.target()).unwrap(),
        "same content"
    );
    assert!(stage_names(&fixture.task).is_empty());
}

#[test]
fn observers_only_see_absence_or_complete_target_bytes() {
    let fixture = Fixture::new();
    let content = "0123456789abcdef".repeat(256 * 1024);
    let expected = Arc::new(content.as_bytes().to_vec());
    let target = fixture.target();
    let stop = Arc::new(AtomicBool::new(false));
    let start = Arc::new(Barrier::new(2));
    let reader_expected = expected.clone();
    let reader_stop = stop.clone();
    let reader_start = start.clone();
    let reader = thread::spawn(move || {
        reader_start.wait();
        while !reader_stop.load(Ordering::Acquire) {
            match fs::read(&target) {
                Ok(bytes) => assert_eq!(bytes, *reader_expected),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => panic!("unexpected read error: {error}"),
            }
            thread::yield_now();
        }
        assert_eq!(fs::read(target).unwrap(), *reader_expected);
    });
    start.wait();
    let published = adapter().publish(&fixture.directory, &content).unwrap();
    assert_eq!(published.outcome, PublicationOutcome::Created);
    stop.store(true, Ordering::Release);
    reader.join().unwrap();
}

#[test]
fn cleanup_removes_only_owned_stage_and_ignores_foreign_orphan() {
    let fixture = Fixture::new();
    let foreign = fixture.task.join(".tectd-agents-foreign.tmp");
    fs::write(&foreign, b"do not remove").unwrap();
    adapter().publish(&fixture.directory, "published").unwrap();
    assert_eq!(fs::read(&foreign).unwrap(), b"do not remove");
    assert_eq!(
        stage_names(&fixture.task),
        vec![".tectd-agents-foreign.tmp"]
    );
}
