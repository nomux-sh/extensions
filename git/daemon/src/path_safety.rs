//! The rules that keep each path of a request in the directories that the
//! user approved.

use std::path::{Component, Path, PathBuf};

use crate::error::{Error, Result};

/// The directories that the user approved for the extension, without
/// symbolic links.
pub struct GrantedDirectories(Vec<PathBuf>);

impl GrantedDirectories {
    /// Reads the `fs` permission. A directory that does not exist grants
    /// nothing.
    pub fn new(directories: &[String]) -> Self {
        Self(
            directories
                .iter()
                .filter_map(|directory| Path::new(directory).canonicalize().ok())
                .collect(),
        )
    }

    /// Returns the real path of a directory that exists, if it is in a
    /// granted directory.
    pub fn existing(&self, path: &str) -> Result<PathBuf> {
        let path = absolute(path)?;
        let real = path
            .canonicalize()
            .map_err(|error| Error::not_found(format!("{}: {error}", path.display())))?;
        self.admit(real)
    }

    /// Returns the real path of a directory that does not exist yet, if its
    /// parent directory exists and the path is in a granted directory.
    pub fn new_directory(&self, path: &str) -> Result<PathBuf> {
        let path = absolute(path)?;
        let (Some(parent), Some(Component::Normal(name))) =
            (path.parent(), path.components().next_back())
        else {
            return Err(Error::invalid_argument(format!(
                "{} does not name a new directory",
                path.display()
            )));
        };
        let parent = parent
            .canonicalize()
            .map_err(|error| Error::not_found(format!("{}: {error}", parent.display())))?;
        self.admit(parent.join(name))
    }

    fn admit(&self, real: PathBuf) -> Result<PathBuf> {
        if self.0.iter().any(|granted| real.starts_with(granted)) {
            return Ok(real);
        }
        Err(Error::permission_denied(format!(
            "{} is not in a directory that the user approved",
            real.display()
        )))
    }
}

/// Makes a path of a request relative to the worktree, with `/` between the
/// parts.
///
/// The function refuses a path with a `..` part, an absolute path that is not
/// in the worktree, and a path that leaves the worktree through a symbolic
/// link. The last part can be a symbolic link: git records the link, not its
/// target.
pub fn worktree_path(worktree: &Path, path: &str) -> Result<String> {
    let escapes = || Error::invalid_argument(format!("the path `{path}` is not in the worktree"));
    let given = Path::new(path);
    let relative = if given.is_absolute() {
        given.strip_prefix(worktree).map_err(|_| escapes())?
    } else {
        given
    };
    let mut parts = Vec::new();
    for component in relative.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str().ok_or_else(escapes)?),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(escapes());
            }
        }
    }
    if parts.is_empty() {
        return Err(Error::invalid_argument("the path is empty"));
    }
    let parent = worktree.join(parts[..parts.len() - 1].join("/"));
    if !nearest_existing(&parent)?.starts_with(worktree) {
        return Err(escapes());
    }
    Ok(parts.join("/"))
}

fn absolute(path: &str) -> Result<&Path> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Ok(path);
    }
    Err(Error::invalid_argument(format!(
        "`{}` is not an absolute path",
        path.display()
    )))
}

/// The real path of the directory, or of its nearest parent that exists.
fn nearest_existing(directory: &Path) -> Result<PathBuf> {
    for ancestor in directory.ancestors() {
        match ancestor.canonicalize() {
            Ok(real) => return Ok(real),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Err(Error::not_found(format!(
        "{} has no parent directory that exists",
        directory.display()
    )))
}
