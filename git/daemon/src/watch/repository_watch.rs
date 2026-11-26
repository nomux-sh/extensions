//! The watch of one worktree: a thread that waits for file events, waits
//! until they stop, and computes the status one time.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use gix_notify::{Budget, Loss, Watcher};

use crate::error::Result;
use crate::status;
use crate::watch::coverage::{Coverage, Relevance};
use crate::wire::v1::Status;

/// The times of the wait between a file event and the computation of the
/// status.
#[derive(Clone, Copy, Debug)]
pub struct Debounce {
    /// The time without a relevant event after which the status is computed.
    pub quiet: Duration,
    /// The longest time from the first relevant event to the computation,
    /// when events arrive without a pause.
    pub longest: Duration,
}

impl Default for Debounce {
    fn default() -> Self {
        Self {
            quiet: Duration::from_millis(300),
            longest: Duration::from_millis(1500),
        }
    }
}

enum Signal {
    /// The watcher has file events.
    Events,
    /// A method changed the repository.
    Changed,
    Stop,
}

/// The state that the thread of the watch and the thread of the methods
/// share.
struct Shared {
    /// The last status that the watch computed. The watch holds the lock
    /// during a computation, thus a reader waits for the new status.
    status: Mutex<Option<Status>>,
    /// True from a relevant event to the start of the next computation.
    is_stale: AtomicBool,
    computations: AtomicU64,
}

/// A running watch of one worktree. The watch stops when the value is
/// dropped.
pub struct RepositoryWatch {
    shared: Arc<Shared>,
    signals: Sender<Signal>,
    thread: Option<JoinHandle<()>>,
}

impl RepositoryWatch {
    /// Starts to watch the worktree and returns its status.
    ///
    /// After each group of relevant file events, the watch computes the
    /// status one time. If the status changed, it calls `on_change`. The
    /// watch stops when `on_change` returns `false`.
    pub fn start(
        worktree: &Path,
        debounce: Debounce,
        on_change: impl FnMut(&Status) -> bool + Send + 'static,
    ) -> Result<(Self, Status)> {
        let repository = gix::open(worktree)?;
        let (signals, inbox) = mpsc::channel();
        let mut watcher = Watcher::new(gix_notify::Options::default())?;
        let wake = signals.clone();
        watcher.set_waker(move || {
            // The send fails only after the thread of the watch ended.
            let _ = wake.send(Signal::Events);
        });
        // The watches start before the first status, thus no change between
        // the two is lost.
        let coverage = Coverage::discover(&repository)?;
        watcher.replace(coverage.watches())?;
        let status = status::status(&repository)?;
        let shared = Arc::new(Shared {
            status: Mutex::new(Some(status.clone())),
            is_stale: AtomicBool::new(false),
            computations: AtomicU64::new(1),
        });
        let worker = Worker {
            worktree: worktree.to_owned(),
            debounce,
            watcher,
            coverage,
            is_coverage_current: true,
            shared: Arc::clone(&shared),
            on_change: Box::new(on_change),
        };
        let thread = thread::Builder::new()
            .name("watch".to_owned())
            .spawn(move || worker.run(&inbox))?;
        let watch = Self {
            shared,
            signals,
            thread: Some(thread),
        };
        Ok((watch, status))
    }

    /// The status of the worktree, if no relevant event arrived after its
    /// computation.
    pub fn current_status(&self) -> Option<Status> {
        let status = self
            .shared
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if self.shared.is_stale.load(Ordering::SeqCst) {
            return None;
        }
        status.clone()
    }

    /// Tells the watch that a method changed the repository. The watch does
    /// not wait for the file events of the change.
    pub fn invalidate(&self) {
        self.shared.is_stale.store(true, Ordering::SeqCst);
        // The send fails only after the thread of the watch ended.
        let _ = self.signals.send(Signal::Changed);
    }

    /// The number of status computations from the start of the watch.
    pub fn computations(&self) -> u64 {
        self.shared.computations.load(Ordering::SeqCst)
    }
}

impl Drop for RepositoryWatch {
    fn drop(&mut self) {
        // The send fails only after the thread of the watch ended.
        let _ = self.signals.send(Signal::Stop);
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            eprintln!("nomux-git: the thread of a watch panicked");
        }
    }
}

/// The time of the next computation.
struct Due {
    at: Instant,
    latest: Instant,
}

struct Worker {
    worktree: PathBuf,
    debounce: Debounce,
    watcher: Watcher,
    coverage: Coverage,
    is_coverage_current: bool,
    shared: Arc<Shared>,
    on_change: Box<dyn FnMut(&Status) -> bool + Send>,
}

impl Worker {
    fn run(mut self, inbox: &Receiver<Signal>) {
        let mut due: Option<Due> = None;
        loop {
            let signal = match &due {
                None => inbox.recv().map_err(|_| RecvTimeoutError::Disconnected),
                Some(due) => inbox.recv_timeout(due.at.saturating_duration_since(Instant::now())),
            };
            match signal {
                Ok(Signal::Events) if !self.has_relevant_events() => {}
                Ok(Signal::Events | Signal::Changed) => due = Some(self.postpone(due)),
                Err(RecvTimeoutError::Timeout) => {
                    due = None;
                    if !self.publish_status() {
                        return;
                    }
                }
                Ok(Signal::Stop) | Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }

    /// Moves the computation to `quiet` after this event, but not later than
    /// `longest` after the first event.
    fn postpone(&self, due: Option<Due>) -> Due {
        self.shared.is_stale.store(true, Ordering::SeqCst);
        let now = Instant::now();
        let latest = due.map_or(now + self.debounce.longest, |due| due.latest);
        Due {
            at: (now + self.debounce.quiet).min(latest),
            latest,
        }
    }

    /// Takes the events from the watcher. The queue of the watcher has a
    /// limit: when it loses events, the coverage is discovered again.
    fn has_relevant_events(&mut self) -> bool {
        let mut is_relevant = false;
        loop {
            let batch = self.watcher.drain(Budget::default());
            if let Some(error) = &batch.error {
                eprintln!("nomux-git: watch of {}: {error}", self.worktree.display());
            }
            let lost_events = batch.loss.is_some_and(|loss| loss != Loss::WatchSetChanged);
            let relevance = batch
                .events
                .iter()
                .map(|event| self.coverage.relevance(event))
                .max()
                .unwrap_or(Relevance::None);
            if lost_events || batch.error.is_some() || relevance == Relevance::Coverage {
                self.is_coverage_current = false;
            }
            is_relevant |= lost_events || batch.error.is_some() || relevance != Relevance::None;
            if !batch.more {
                return is_relevant;
            }
        }
    }

    /// Computes the status and gives it to the subscriber if it changed.
    /// Returns `false` when the subscriber is gone.
    fn publish_status(&mut self) -> bool {
        match self.compute_status() {
            Ok(Some(status)) => (self.on_change)(&status),
            Ok(None) => true,
            Err(error) => {
                self.shared.is_stale.store(true, Ordering::SeqCst);
                eprintln!("nomux-git: watch of {}: {error}", self.worktree.display());
                true
            }
        }
    }

    /// Returns the new status, or `None` when it is equal to the last one.
    fn compute_status(&mut self) -> Result<Option<Status>> {
        let repository = gix::open(&self.worktree)?;
        if !self.is_coverage_current {
            self.coverage = Coverage::discover(&repository)?;
            self.watcher.replace(self.coverage.watches())?;
            self.is_coverage_current = true;
        }
        let mut last = self
            .shared
            .status
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.shared.is_stale.store(false, Ordering::SeqCst);
        let status = status::status(&repository)?;
        self.shared.computations.fetch_add(1, Ordering::SeqCst);
        if last.as_ref() == Some(&status) {
            return Ok(None);
        }
        *last = Some(status.clone());
        Ok(Some(status))
    }
}
