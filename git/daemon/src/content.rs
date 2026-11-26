//! The content of one file in a tree, in the index or in the worktree.

use std::fs::File;
use std::io::Read;
use std::path::Path;

use gix::ObjectId;
use gix::bstr::BStr;
use gix::index::entry::Mode;

use crate::error::{Error, Result};

/// The largest file that the extension reads to compare or to stage lines.
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;

/// The number of bytes at the start of a file that decide if it is text, as
/// in git.
const BINARY_CHECK_BYTES: usize = 8000;

/// A place that has a version of a file.
#[derive(Clone, Copy)]
pub enum Version {
    /// A tree of a commit.
    Tree(ObjectId),
    /// The index.
    Index,
    /// The worktree. The content goes through the filters that `git add`
    /// applies, thus it compares with a blob.
    Worktree,
}

/// One version of a file.
pub enum Content {
    /// The version does not have the file.
    Missing,
    /// The bytes of a text file.
    Text(Vec<u8>),
    /// The file is not text: it has a zero byte, or it is a submodule or a
    /// directory.
    Binary,
    /// The file is larger than [`MAX_FILE_BYTES`].
    TooLarge,
}

/// Reads versions of the files of one repository.
pub struct Contents<'repo> {
    repository: &'repo gix::Repository,
    index: gix::worktree::Index,
    filters: Option<gix::filter::Pipeline<'repo>>,
}

impl<'repo> Contents<'repo> {
    /// Loads the index of the repository.
    pub fn new(repository: &'repo gix::Repository) -> Result<Self> {
        Ok(Self {
            repository,
            index: repository.index_or_empty()?,
            filters: None,
        })
    }

    /// Reads the file at `path`, relative to the worktree.
    pub fn read(&mut self, version: Version, path: &str) -> Result<Content> {
        match version {
            Version::Tree(tree) => self.in_tree(tree, path),
            Version::Index => self.in_index(path),
            Version::Worktree => self.in_worktree(path),
        }
    }

    /// The mode of the file in the index.
    pub fn index_mode(&self, path: &str) -> Option<Mode> {
        self.index
            .entry_by_path(BStr::new(path))
            .map(|entry| entry.mode)
    }

    fn in_tree(&self, tree: ObjectId, path: &str) -> Result<Content> {
        let Some(entry) = self
            .repository
            .find_tree(tree)?
            .lookup_entry_by_path(path)?
        else {
            return Ok(Content::Missing);
        };
        if !entry.mode().is_blob_or_symlink() {
            return Ok(Content::Binary);
        }
        self.blob(entry.object_id())
    }

    fn in_index(&self, path: &str) -> Result<Content> {
        let Some(entry) = self.index.entry_by_path(BStr::new(path)) else {
            return Ok(Content::Missing);
        };
        if !matches!(
            entry.mode,
            Mode::FILE | Mode::FILE_EXECUTABLE | Mode::SYMLINK
        ) {
            return Ok(Content::Binary);
        }
        self.blob(entry.id)
    }

    fn blob(&self, id: ObjectId) -> Result<Content> {
        if self.repository.find_header(id)?.size() > MAX_FILE_BYTES {
            return Ok(Content::TooLarge);
        }
        Ok(classify(self.repository.find_blob(id)?.take_data()))
    }

    fn in_worktree(&mut self, path: &str) -> Result<Content> {
        let worktree = self
            .repository
            .workdir()
            .ok_or_else(|| Error::failed("the repository has no worktree"))?;
        let file = worktree.join(path);
        let metadata = match file.symlink_metadata() {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Content::Missing);
            }
            Err(error) => return Err(error.into()),
        };
        if metadata.is_symlink() {
            let target = file.read_link()?;
            return Ok(Content::Text(target.into_os_string().into_encoded_bytes()));
        }
        if !metadata.is_file() {
            return Ok(Content::Binary);
        }
        if metadata.len() > MAX_FILE_BYTES {
            return Ok(Content::TooLarge);
        }
        let filters = match &mut self.filters {
            Some(filters) => filters,
            None => self
                .filters
                .insert(self.repository.filter_pipeline(None)?.0),
        };
        let mut data = Vec::new();
        filters
            .convert_to_git(File::open(&file)?, Path::new(path), &self.index)?
            .read_to_end(&mut data)?;
        Ok(classify(data))
    }
}

fn classify(data: Vec<u8>) -> Content {
    let checked = &data[..data.len().min(BINARY_CHECK_BYTES)];
    if checked.contains(&0) {
        return Content::Binary;
    }
    Content::Text(data)
}
