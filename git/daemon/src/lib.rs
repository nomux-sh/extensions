//! The daemon part of the nomux git extension.
//!
//! The nomux daemon starts the program `nomux-git` when a client needs it and
//! calls its methods. The methods that read use `gix`. The methods that
//! write run the `git` program of the user, thus the hooks, the signing and
//! the configuration of the user apply.
//!
//! Each module has one method group. [`extension`] connects them to the
//! extension protocol.

use std::cell::RefCell;
use std::rc::Rc;

use nomux_extension::{Call, CallError, Extension, Permissions};

pub mod blame;
pub mod branches;
pub mod commit;
mod commit_info;
mod content;
pub mod diff;
pub mod error;
mod git_command;
mod line_diff;
pub mod log;
mod path_safety;
pub mod repository;
mod revision;
pub mod staging;
pub mod status;
mod upstream;
pub mod watch;
pub mod worktree_changes;
pub mod worktree_facts;
pub mod worktrees;

use error::Result;
use repository::Repository;
use watch::Watches;
use wire::v1::{
    StatusChanged, StatusRequest, StatusResponse, UnwatchRequest, UnwatchResponse, WatchRequest,
    WatchResponse,
};

/// The messages of the methods and of the event.
pub mod wire {
    /// Package `nomux.git.v1`.
    ///
    /// `buf generate` writes the included file. Do not edit the file. The
    /// file is included with `include!`, thus `rustfmt` does not format it.
    pub mod v1 {
        include!("generated/nomux.git.v1.rs");
    }
}

/// The name of the event that a watch emits.
pub const STATUS_CHANGED: &str = "status_changed";

/// Makes the extension with all its methods.
pub fn extension() -> Extension {
    let watches = Rc::new(RefCell::new(Watches::default()));
    Extension::new("nomux.git", env!("CARGO_PKG_VERSION"))
        .method("status", status(&watches))
        .method("diff", reading(diff::diff))
        .method("stage", writing(&watches, staging::stage))
        .method("unstage", writing(&watches, staging::unstage))
        .method("commit", writing(&watches, commit::commit))
        .method("branches", reading(branches::branches))
        .method("switch", writing(&watches, branches::switch))
        .method("create_branch", writing(&watches, branches::create_branch))
        .method("log", reading(log::log))
        .method("blame", reading(blame::blame))
        .method("worktrees", reading(worktrees::worktrees))
        .method("worktree_add", writing(&watches, worktrees::worktree_add))
        .method(
            "worktree_remove",
            writing(&watches, worktrees::worktree_remove),
        )
        .method("worktree_facts", reading(worktree_facts::worktree_facts))
        .method(
            "worktree_move_to_trash",
            writing(&watches, worktree_changes::worktree_move_to_trash),
        )
        .method(
            "worktree_move_from_trash",
            writing(&watches, worktree_changes::worktree_move_from_trash),
        )
        .method(
            "worktree_delete",
            writing(&watches, worktree_changes::worktree_delete),
        )
        .method(
            "worktree_prune",
            writing(&watches, worktree_changes::worktree_prune),
        )
        .method("watch", watch(&watches))
        .method("unwatch", unwatch(&watches))
}

type Method<Input, Output> = fn(&Permissions, Input) -> Result<Output>;

fn reading<Input, Output>(
    method: Method<Input, Output>,
) -> impl FnMut(&Call, Input) -> std::result::Result<Output, CallError> {
    move |call, input| Ok(method(call.host().permissions(), input)?)
}

/// A method that can change a repository: each watch computes its status
/// again after it.
fn writing<Input, Output>(
    watches: &Rc<RefCell<Watches>>,
    method: Method<Input, Output>,
) -> impl FnMut(&Call, Input) -> std::result::Result<Output, CallError> + use<Input, Output> {
    let watches = Rc::clone(watches);
    move |call, input| {
        let result = method(call.host().permissions(), input);
        watches.borrow().invalidate();
        Ok(result?)
    }
}

fn status(
    watches: &Rc<RefCell<Watches>>,
) -> impl FnMut(&Call, StatusRequest) -> std::result::Result<StatusResponse, CallError> + use<> {
    let watches = Rc::clone(watches);
    move |call, request| {
        let repository = Repository::open(call.host().permissions(), &request.repository)?;
        let status = watches.borrow().status(&repository)?;
        Ok(StatusResponse {
            status: Some(status),
        })
    }
}

fn watch(
    watches: &Rc<RefCell<Watches>>,
) -> impl FnMut(&Call, WatchRequest) -> std::result::Result<WatchResponse, CallError> + use<> {
    let watches = Rc::clone(watches);
    move |call, request| {
        let repository = Repository::open(call.host().permissions(), &request.repository)?;
        let host = call.host().clone();
        let worktree = repository.worktree().display().to_string();
        let emit = move |status: &wire::v1::Status| {
            let event = StatusChanged {
                repository: worktree.clone(),
                status: Some(status.clone()),
            };
            // The event is not sent only when the host closed the connection.
            host.emit(STATUS_CHANGED, &event).is_ok()
        };
        let (subscription, status) = watches.borrow_mut().subscribe(&repository, emit)?;
        Ok(WatchResponse {
            subscription,
            status: Some(status),
        })
    }
}

fn unwatch(
    watches: &Rc<RefCell<Watches>>,
) -> impl FnMut(&Call, UnwatchRequest) -> std::result::Result<UnwatchResponse, CallError> + use<> {
    let watches = Rc::clone(watches);
    move |_, request| {
        watches.borrow_mut().unsubscribe(request.subscription)?;
        Ok(UnwatchResponse {})
    }
}
