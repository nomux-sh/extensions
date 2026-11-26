//! The watch of a worktree: the events after changes, the changes that
//! compute no status, and the number of computations for many changes.

mod repositories;

use std::fs;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use nomux_extension::ErrorCode;
use nomux_git::repository::Repository;
use nomux_git::staging::stage;
use nomux_git::watch::{Debounce, RepositoryWatch, Watches};
use nomux_git::wire::v1::{ChangeKind, StageRequest, Status};
use repositories::TestRepository;

/// Longer than the longest time from a change to its event.
const EVENT_TIMEOUT: Duration = Duration::from_secs(20);

/// Longer than the wait of a watch after an event.
const QUIET_TIME: Duration = Duration::from_millis(1200);

struct Subscriber {
    watches: Watches,
    subscription: u64,
    statuses: Receiver<Status>,
}

impl Subscriber {
    fn new(repository: &TestRepository) -> Self {
        let mut watches = Watches::default();
        let (subscription, statuses) = subscribe(&mut watches, repository);
        Self {
            watches,
            subscription,
            statuses,
        }
    }

    /// Waits for the first event whose status passes the check.
    fn wait_for(&self, is_expected: impl Fn(&Status) -> bool) -> Status {
        loop {
            let status = self
                .statuses
                .recv_timeout(EVENT_TIMEOUT)
                .expect("the watch emitted no status that passes the check");
            if is_expected(&status) {
                return status;
            }
        }
    }

    fn computations(&self, repository: &TestRepository) -> u64 {
        self.watches.computations(&repository.root()).unwrap()
    }
}

fn subscribe(watches: &mut Watches, repository: &TestRepository) -> (u64, Receiver<Status>) {
    let opened = Repository::open(&repository.permissions(), &repository.path()).unwrap();
    let (sender, statuses) = mpsc::channel();
    let on_change = move |status: &Status| sender.send(status.clone()).is_ok();
    let (subscription, _) = watches.subscribe(&opened, on_change).unwrap();
    (subscription, statuses)
}

fn unstaged(status: &Status, path: &str) -> ChangeKind {
    status
        .files
        .iter()
        .find(|file| file.path == path)
        .map_or(ChangeKind::Unspecified, |file| file.unstaged())
}

#[test]
fn an_edit_of_a_file_emits_the_new_status() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let subscriber = Subscriber::new(&repository);

    repository.write("a.txt", "b\n");

    let status = subscriber.wait_for(|_| true);
    assert_eq!(unstaged(&status, "a.txt"), ChangeKind::Modified);
    assert_eq!(status.files.len(), 1);
}

#[test]
fn a_commit_from_a_different_program_emits_the_new_head() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let subscriber = Subscriber::new(&repository);
    repository.write("a.txt", "b\n");
    subscriber.wait_for(|status| !status.files.is_empty());

    repository.commit_all("second");

    let head = repository.git(&["rev-parse", "HEAD"]);
    let status = subscriber.wait_for(|status| status.head == head.trim());
    assert_eq!(status.files, []);
}

#[test]
fn a_branch_switch_from_a_different_program_emits_the_new_branch() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    repository.git(&["branch", "other"]);
    let subscriber = Subscriber::new(&repository);

    repository.git(&["switch", "--quiet", "other"]);

    let status = subscriber.wait_for(|_| true);
    assert_eq!(status.branch, "other");
}

#[test]
fn a_change_in_an_ignored_directory_computes_no_status() {
    let repository = TestRepository::with_files(&[(".gitignore", "target/\n"), ("a.txt", "a\n")]);
    repository.write("target/old.o", "x");
    let subscriber = Subscriber::new(&repository);
    // A file watcher can report the writes from before its start.
    std::thread::sleep(QUIET_TIME);
    let computations = subscriber.computations(&repository);

    for number in 0..200 {
        repository.write(&format!("target/debug/{number}.o"), "x");
    }
    repository.write("target/old.o", "y");

    assert_eq!(
        subscriber.statuses.recv_timeout(QUIET_TIME),
        Err(RecvTimeoutError::Timeout)
    );
    assert_eq!(subscriber.computations(&repository), computations);
}

#[test]
fn a_file_in_a_directory_that_is_new_after_the_start_emits_an_event() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let subscriber = Subscriber::new(&repository);
    repository.write("new/deep/file.txt", "a\n");
    subscriber.wait_for(|status| unstaged(status, "new/") == ChangeKind::Untracked);
    repository.git(&["add", "new"]);
    subscriber.wait_for(|status| {
        status
            .files
            .iter()
            .any(|file| file.staged() == ChangeKind::Added)
    });

    repository.write("new/deep/file.txt", "b\n");

    let status = subscriber.wait_for(|_| true);
    assert_eq!(unstaged(&status, "new/deep/file.txt"), ChangeKind::Modified);
}

#[test]
fn a_change_that_keeps_the_status_computes_it_and_emits_no_event() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let subscriber = Subscriber::new(&repository);
    repository.write("a.txt", "b\n");
    subscriber.wait_for(|status| unstaged(status, "a.txt") == ChangeKind::Modified);
    let computations = subscriber.computations(&repository);

    repository.write("a.txt", "c\n");

    assert_eq!(
        subscriber.statuses.recv_timeout(QUIET_TIME),
        Err(RecvTimeoutError::Timeout)
    );
    assert!(subscriber.computations(&repository) > computations);
}

#[test]
fn a_watch_does_not_give_its_cached_status_after_it_is_invalidated() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let on_change = |_: &Status| true;
    let (watch, status) =
        RepositoryWatch::start(&repository.root(), Debounce::default(), on_change).unwrap();
    assert_eq!(watch.current_status(), Some(status.clone()));

    watch.invalidate();

    assert_eq!(watch.current_status(), None);
    std::thread::sleep(QUIET_TIME);
    assert_eq!(watch.current_status(), Some(status));
    assert!(watch.computations() > 1);
}

#[test]
fn the_status_after_a_method_that_writes_is_not_the_cached_status() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let subscriber = Subscriber::new(&repository);
    repository.write("a.txt", "b\n");
    subscriber.wait_for(|status| unstaged(status, "a.txt") == ChangeKind::Modified);
    let opened = Repository::open(&repository.permissions(), &repository.path()).unwrap();
    let cached = subscriber.watches.status(&opened).unwrap();
    assert_eq!(unstaged(&cached, "a.txt"), ChangeKind::Modified);

    let request = StageRequest {
        repository: repository.path(),
        paths: vec!["a.txt".to_owned()],
        ..StageRequest::default()
    };
    stage(&repository.permissions(), request).unwrap();
    subscriber.watches.invalidate();
    let status = subscriber.watches.status(&opened).unwrap();

    assert_eq!(status.files[0].staged(), ChangeKind::Modified);
    assert_eq!(unstaged(&status, "a.txt"), ChangeKind::Unspecified);
}

#[test]
fn a_watch_runs_from_its_first_subscription_to_its_last() {
    let repository = TestRepository::with_files(&[("a.txt", "a\n")]);
    let mut subscriber = Subscriber::new(&repository);
    let (second, _) = subscribe(&mut subscriber.watches, &repository);
    assert_eq!(subscriber.watches.watched_worktrees(), 1);

    subscriber.watches.unsubscribe(second).unwrap();
    repository.write("a.txt", "b\n");
    subscriber.wait_for(|status| unstaged(status, "a.txt") == ChangeKind::Modified);

    subscriber
        .watches
        .unsubscribe(subscriber.subscription)
        .unwrap();
    assert_eq!(subscriber.watches.watched_worktrees(), 0);
    repository.write("a.txt", "c\n");
    // The thread of the watch ended and dropped the sender.
    assert_eq!(
        subscriber.statuses.recv_timeout(QUIET_TIME),
        Err(RecvTimeoutError::Disconnected)
    );
    let error = subscriber.watches.unsubscribe(second).unwrap_err();
    assert_eq!(error.code, ErrorCode::NotFound);
}

#[test]
fn ten_thousand_file_changes_compute_the_status_a_small_number_of_times() {
    let repository = TestRepository::new();
    for directory in 0..100 {
        fs::create_dir_all(repository.root().join(format!("source/{directory}"))).unwrap();
        for file in 0..100 {
            repository.write(&format!("source/{directory}/{file}.txt"), "before\n");
        }
    }
    repository.commit_all("ten thousand files");
    let subscriber = Subscriber::new(&repository);
    // A file watcher can report the writes from before its start.
    std::thread::sleep(QUIET_TIME);
    let before = subscriber.computations(&repository);
    let started = Instant::now();

    for directory in 0..100 {
        for file in 0..100 {
            repository.write(&format!("source/{directory}/{file}.txt"), "after\n");
        }
    }
    // The time of the burst sets the limit, not the number of changes: one
    // computation for each longest wait, and the first and the last one.
    let longest_wait = Debounce::default().longest;
    let limit = started.elapsed().as_millis() / longest_wait.as_millis() + 2;

    let status = subscriber.wait_for(|status| status.files.len() == 10_000);
    assert!(
        status
            .files
            .iter()
            .all(|file| file.unstaged() == ChangeKind::Modified)
    );
    assert_eq!(
        subscriber.statuses.recv_timeout(QUIET_TIME),
        Err(RecvTimeoutError::Timeout)
    );
    let computations = u128::from(subscriber.computations(&repository) - before);
    assert!(
        (1..=limit).contains(&computations),
        "{computations} computations, the limit is {limit}"
    );
}
