//! The `watch` and `unwatch` methods: the watches of the worktrees that have
//! a subscriber.
//!
//! A worktree without a subscriber has no watch, no thread and no cached
//! status.

mod coverage;
mod repository_watch;

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub use repository_watch::{Debounce, RepositoryWatch};

use crate::error::{Error, Result};
use crate::repository::Repository;
use crate::status;
use crate::wire::v1::Status;

/// The watches of the extension, one for each worktree that has a
/// subscriber.
#[derive(Default)]
pub struct Watches {
    last_subscription: u64,
    watched: HashMap<PathBuf, Watched>,
}

struct Watched {
    watch: RepositoryWatch,
    subscriptions: HashSet<u64>,
}

impl Watches {
    /// Adds a subscriber to a worktree and returns the subscription and the
    /// status. The first subscriber of a worktree starts its watch, and the
    /// watch calls `on_change` with each new status.
    pub fn subscribe(
        &mut self,
        repository: &Repository,
        on_change: impl FnMut(&Status) -> bool + Send + 'static,
    ) -> Result<(u64, Status)> {
        let status = match self.watched.get(repository.worktree()) {
            Some(_) => self.status(repository)?,
            None => {
                let (watch, status) =
                    RepositoryWatch::start(repository.worktree(), Debounce::default(), on_change)?;
                let watched = Watched {
                    watch,
                    subscriptions: HashSet::new(),
                };
                self.watched
                    .insert(repository.worktree().to_owned(), watched);
                status
            }
        };
        self.last_subscription += 1;
        if let Some(watched) = self.watched.get_mut(repository.worktree()) {
            watched.subscriptions.insert(self.last_subscription);
        }
        Ok((self.last_subscription, status))
    }

    /// Ends a subscription. The watch of a worktree stops with its last
    /// subscription.
    pub fn unsubscribe(&mut self, subscription: u64) -> Result<()> {
        let worktree = self
            .watched
            .iter_mut()
            .find_map(|(worktree, watched)| {
                watched
                    .subscriptions
                    .remove(&subscription)
                    .then(|| (worktree.clone(), watched.subscriptions.is_empty()))
            })
            .ok_or_else(|| {
                Error::not_found(format!("the subscription {subscription} does not exist"))
            })?;
        if let (worktree, true) = worktree {
            self.watched.remove(&worktree);
        }
        Ok(())
    }

    /// The status of a worktree: the cached status of its watch if it is
    /// current, or a new computation.
    pub fn status(&self, repository: &Repository) -> Result<Status> {
        let cached = self
            .watched
            .get(repository.worktree())
            .and_then(|watched| watched.watch.current_status());
        match cached {
            Some(status) => Ok(status),
            None => status::status(repository.gix()),
        }
    }

    /// Tells each watch that a method changed a repository.
    pub fn invalidate(&self) {
        for watched in self.watched.values() {
            watched.watch.invalidate();
        }
    }

    /// The number of worktrees that have a watch.
    pub fn watched_worktrees(&self) -> usize {
        self.watched.len()
    }

    /// The number of status computations of the watch of a worktree.
    pub fn computations(&self, worktree: &Path) -> Option<u64> {
        self.watched
            .get(worktree)
            .map(|watched| watched.watch.computations())
    }
}
