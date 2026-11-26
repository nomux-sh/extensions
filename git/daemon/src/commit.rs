//! The `commit` method.

use nomux_extension::Permissions;

use crate::error::{Error, Result};
use crate::repository::Repository;
use crate::wire::v1::{CommitRequest, CommitResponse};

/// Commits the index with the `git` program of the user, thus the hooks and
/// the signing of the user apply. The extension adds nothing to the message.
pub fn commit(permissions: &Permissions, request: CommitRequest) -> Result<CommitResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let mut command = repository.git()?.arg("commit").arg("--quiet");
    if request.amend {
        command = command.arg("--amend");
    }
    command = match (request.message.is_empty(), request.amend) {
        (true, true) => command.arg("--no-edit"),
        (true, false) => return Err(Error::invalid_argument("the commit message is empty")),
        // `verbatim` keeps each byte of the message.
        (false, _) => command
            .arg("--cleanup=verbatim")
            .arg("--file=-")
            .input(request.message.into_bytes()),
    };
    command.run()?;
    Ok(CommitResponse {
        commit_id: repository.gix().head_id()?.to_string(),
    })
}
