//! The method `worktree_facts`: the git facts of each worktree of a
//! repository, for the worktree list of the nomux daemon. The messages are in
//! `nomux-proto`, because the daemon reads them.
//!
//! The method only reads. A merge check runs in memory, thus it writes no
//! object.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use gix::ObjectId;
use gix::dir::walk::EmissionMode;
use gix::merge::tree::TreatAsUnresolved;
use gix::refs::FullName;
use gix::status::Item as StatusItem;
use gix::status::index_worktree::Item;
use nomux_extension::Permissions;
use nomux_proto::wire::v1::{
    WorktreeFacts, WorktreeFactsRequest, WorktreeFactsResponse, WorktreeMergedBy,
};

use crate::error::{Error, Result};
use crate::path_safety::GrantedDirectories;
use crate::repository::Repository;
use crate::upstream;

/// The largest number of ignored paths of one worktree in a result.
const MAX_IGNORED_PATHS: usize = 256;
/// The count of commits that are not pushed stops here.
const MAX_UNPUSHED_COMMITS: u32 = 1000;

/// Gives the facts of each worktree of the repository of the request.
pub fn worktree_facts(
    permissions: &Permissions,
    request: WorktreeFactsRequest,
) -> Result<WorktreeFactsResponse> {
    let repository = Repository::open(permissions, &request.repository)?;
    let granted = GrantedDirectories::new(&permissions.fs);
    let gix = repository.gix();
    let base = BaseBranch::find(gix, &request.base_branch)?;
    let mut worktrees = vec![main_facts(&gix.main_repo()?, &granted, base.as_ref())?];
    for proxy in gix.worktrees()? {
        worktrees.push(linked_facts(proxy, &granted, base.as_ref())?);
    }
    Ok(WorktreeFactsResponse {
        base_branch: base.map(|base| base.short_name).unwrap_or_default(),
        last_fetch_at_ms: modified_ms(&gix.common_dir().join("FETCH_HEAD")),
        worktrees,
    })
}

/// The facts of the linked worktree whose directory is `path`, with the
/// base branch `base_branch` of the user (empty: the rule), for a change of
/// git that checks the facts again.
pub(crate) fn of_linked(
    repository: &gix::Repository,
    path: &Path,
    granted: &GrantedDirectories,
    base_branch: &str,
) -> Result<WorktreeFacts> {
    let base = BaseBranch::find(repository, base_branch)?;
    for proxy in repository.worktrees()? {
        if proxy.base()?.canonicalize().is_ok_and(|base| base == path) {
            return linked_facts(proxy, granted, base.as_ref());
        }
    }
    Err(Error::not_found(format!(
        "{} is not a linked worktree of the repository",
        path.display()
    )))
}

/// The branch that the merge rules compare with.
struct BaseBranch {
    /// For example `origin/main`.
    short_name: String,
    /// The local branch of the same name, for example `main`. A worktree on
    /// this branch is never merged into the base branch.
    local_name: String,
    commit: ObjectId,
}

impl BaseBranch {
    /// The branch `user_choice` of the user if it exists, else the branch
    /// that `origin/HEAD` names, else `main`, else `master`; the
    /// remote-tracking branch before the local branch.
    fn find(repository: &gix::Repository, user_choice: &str) -> Result<Option<Self>> {
        let mut candidates = Vec::new();
        if !user_choice.is_empty() {
            for name in [
                format!("refs/remotes/{user_choice}"),
                format!("refs/heads/{user_choice}"),
            ] {
                candidates.extend(FullName::try_from(name).ok());
            }
        }
        if let Some(reference) = repository.try_find_reference("refs/remotes/origin/HEAD")?
            && let Some(target) = reference.target().try_name()
        {
            candidates.push(target.to_owned());
        }
        for name in [
            "refs/remotes/origin/main",
            "refs/heads/main",
            "refs/remotes/origin/master",
            "refs/heads/master",
        ] {
            candidates.push(FullName::try_from(name).expect("a valid reference name"));
        }
        for name in candidates {
            let Some(reference) = repository.try_find_reference(name.as_ref())? else {
                continue;
            };
            let short_name = name.as_ref().shorten().to_string();
            let local_name = match name.as_bstr().strip_prefix(b"refs/remotes/") {
                Some(_) => short_name
                    .split_once('/')
                    .map_or(short_name.as_str(), |(_, branch)| branch)
                    .to_owned(),
                None => short_name.clone(),
            };
            return Ok(Some(Self {
                short_name,
                local_name,
                commit: reference.into_fully_peeled_id()?.detach(),
            }));
        }
        Ok(None)
    }
}

fn main_facts(
    main: &gix::Repository,
    granted: &GrantedDirectories,
    base: Option<&BaseBranch>,
) -> Result<WorktreeFacts> {
    let Some(workdir) = main.workdir() else {
        return Ok(WorktreeFacts {
            path: real_path(main.common_dir()),
            is_main: true,
            is_bare: true,
            last_activity_at_ms: last_activity(main.git_dir()),
            ..WorktreeFacts::default()
        });
    };
    let mut facts = WorktreeFacts {
        is_main: true,
        ..WorktreeFacts::default()
    };
    describe(&mut facts, main, workdir)?;
    read_facts(&mut facts, main, granted, base)?;
    Ok(facts)
}

fn linked_facts(
    proxy: gix::worktree::Proxy<'_>,
    granted: &GrantedDirectories,
    base: Option<&BaseBranch>,
) -> Result<WorktreeFacts> {
    let workdir = proxy.base()?;
    let mut facts = WorktreeFacts {
        is_locked: proxy.is_locked(),
        locked_reason: proxy
            .lock_reason()
            .map(|reason| reason.to_string().trim().to_owned())
            .unwrap_or_default(),
        is_missing: !workdir.is_dir(),
        ..WorktreeFacts::default()
    };
    let linked = proxy.into_repo_with_possibly_inaccessible_worktree()?;
    describe(&mut facts, &linked, &workdir)?;
    if !facts.is_missing {
        read_facts(&mut facts, &linked, granted, base)?;
    }
    Ok(facts)
}

/// The facts that come from the common git directory: they are known also
/// for a worktree that the extension may not read.
fn describe(facts: &mut WorktreeFacts, repository: &gix::Repository, workdir: &Path) -> Result<()> {
    facts.path = real_path(workdir);
    facts.last_activity_at_ms = last_activity(repository.git_dir());
    let head = repository.head()?;
    facts.head = head.id().map(|id| id.to_string()).unwrap_or_default();
    facts.branch = head
        .referent_name()
        .map(|name| name.shorten().to_string())
        .unwrap_or_default();
    Ok(())
}

/// The facts that read the worktree itself.
fn read_facts(
    facts: &mut WorktreeFacts,
    repository: &gix::Repository,
    granted: &GrantedDirectories,
    base: Option<&BaseBranch>,
) -> Result<()> {
    if let Err(error) = granted.existing(&facts.path) {
        facts.error = error.message;
        return Ok(());
    }
    facts.has_submodules = Path::new(&facts.path).join(".gitmodules").exists();
    let walk = walk_status(repository)?;
    facts.is_dirty = walk.changed || repository.state().is_some();
    facts.ignored_paths_truncated = walk.ignored.len() > MAX_IGNORED_PATHS;
    facts.ignored_paths = walk.ignored;
    facts.ignored_paths.truncate(MAX_IGNORED_PATHS);
    let head = repository.head()?;
    let Some(commit) = head.id().map(gix::Id::detach) else {
        return Ok(());
    };
    facts.unpushed_commits = unpushed_commits(repository, commit, base)?;
    let Some(branch) = head.referent_name().map(ToOwned::to_owned) else {
        facts.has_detached_commits = has_unreferenced_commits(repository, commit)?;
        return Ok(());
    };
    if let Some(upstream) = upstream::of_branch(repository, branch.as_ref())? {
        facts.upstream_gone = repository.try_find_reference(upstream.as_ref())?.is_none();
    }
    facts.merged_by =
        merged_by(repository, &facts.branch, commit, facts.upstream_gone, base)?.into();
    Ok(())
}

/// What one status walk found.
struct StatusWalk {
    /// A staged, unstaged or untracked change that is not ignored.
    changed: bool,
    /// The ignored paths, a directory with all its content ignored as one
    /// path that ends with `/`.
    ignored: Vec<String>,
}

fn walk_status(repository: &gix::Repository) -> Result<StatusWalk> {
    let platform = repository
        .status(gix::progress::Discard)?
        .dirwalk_options(|options| options.emit_ignored(Some(EmissionMode::CollapseDirectory)));
    let mut walk = StatusWalk {
        changed: false,
        ignored: Vec::new(),
    };
    for item in platform.into_iter(None)? {
        match item? {
            StatusItem::IndexWorktree(Item::DirectoryContents { entry, .. })
                if matches!(entry.status, gix::dir::entry::Status::Ignored(_)) =>
            {
                let mut path = entry.rela_path.to_string();
                if entry.disk_kind.is_some_and(|kind| kind.is_dir()) {
                    path.push('/');
                }
                walk.ignored.push(path);
            }
            _ => walk.changed = true,
        }
    }
    walk.ignored.sort();
    Ok(walk)
}

/// The commits of `commit` that no remote-tracking branch has. Without a
/// remote-tracking branch, the commits that the base branch does not have.
fn unpushed_commits(
    repository: &gix::Repository,
    commit: ObjectId,
    base: Option<&BaseBranch>,
) -> Result<u32> {
    let mut hidden = Vec::new();
    for reference in repository.references()?.remote_branches()? {
        hidden.push(reference?.into_fully_peeled_id()?.detach());
    }
    if hidden.is_empty() {
        hidden.extend(base.map(|base| base.commit));
    }
    let mut count = 0;
    for commit in repository.rev_walk([commit]).with_hidden(hidden).all()? {
        commit?;
        count += 1;
        if count == MAX_UNPUSHED_COMMITS {
            break;
        }
    }
    Ok(count)
}

/// `true` when `commit` has a commit that no local branch and no
/// remote-tracking branch has.
fn has_unreferenced_commits(repository: &gix::Repository, commit: ObjectId) -> Result<bool> {
    let mut tips = Vec::new();
    for reference in repository.references()?.local_branches()? {
        tips.push(reference?.into_fully_peeled_id()?.detach());
    }
    for reference in repository.references()?.remote_branches()? {
        tips.push(reference?.into_fully_peeled_id()?.detach());
    }
    let mut walk = repository.rev_walk([commit]).with_hidden(tips).all()?;
    Ok(walk.next().transpose()?.is_some())
}

fn merged_by(
    repository: &gix::Repository,
    branch: &str,
    commit: ObjectId,
    upstream_gone: bool,
    base: Option<&BaseBranch>,
) -> Result<WorktreeMergedBy> {
    if let Some(base) = base.filter(|base| base.local_name != branch) {
        let Some(merge_base) = repository.merge_base(commit, base.commit)? else {
            return Ok(gone_or_not(upstream_gone));
        };
        if merge_base.detach() == commit {
            return Ok(WorktreeMergedBy::Ancestor);
        }
        if merge_changes_nothing(repository, base.commit, commit)? {
            return Ok(WorktreeMergedBy::Tree);
        }
    }
    Ok(gone_or_not(upstream_gone))
}

fn gone_or_not(upstream_gone: bool) -> WorktreeMergedBy {
    if upstream_gone {
        WorktreeMergedBy::UpstreamGone
    } else {
        WorktreeMergedBy::Unspecified
    }
}

/// `true` when a merge of `commit` into `base` has no conflict and gives the
/// tree of `base`. The merge keeps its objects in memory.
fn merge_changes_nothing(
    repository: &gix::Repository,
    base: ObjectId,
    commit: ObjectId,
) -> Result<bool> {
    let repository = repository.clone().with_object_memory();
    let options = repository.tree_merge_options()?;
    let mut outcome = repository.merge_commits(base, commit, Default::default(), options.into())?;
    if outcome
        .tree_merge
        .has_unresolved_conflicts(TreatAsUnresolved::git())
    {
        return Ok(false);
    }
    let merged = outcome.tree_merge.tree.write()?.detach();
    let base_tree = repository.find_commit(base)?.tree_id()?.detach();
    Ok(merged == base_tree)
}

/// The newest modification time of the files of the git directory of a
/// worktree that change with its activity.
fn last_activity(git_dir: &Path) -> u64 {
    ["HEAD", "index", "logs/HEAD"]
        .iter()
        .map(|name| modified_ms(&git_dir.join(name)))
        .max()
        .unwrap_or(0)
}

/// The modification time of a file in milliseconds after the Unix epoch. 0
/// when the file does not exist.
fn modified_ms(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |since| {
            u64::try_from(since.as_millis()).unwrap_or(u64::MAX)
        })
}

fn real_path(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| PathBuf::from(path))
        .display()
        .to_string()
}
