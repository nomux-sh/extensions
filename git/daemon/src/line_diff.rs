//! The line diff of two texts. The `diff` method and the staging of lines
//! use the same diff, thus the line numbers of one are valid for the other.

use gix::diff::blob::{Algorithm, InternedInput, diff_with_slider_heuristics};

/// A group of consecutive lines that changed: the lines `before` of the old
/// text become the lines `after` of the new text. The ranges count lines
/// from 0.
pub use gix::diff::blob::Hunk as Change;

/// Splits a text into its lines. Each line keeps its line end.
pub fn lines(text: &[u8]) -> Vec<&[u8]> {
    text.split_inclusive(|byte| *byte == b'\n').collect()
}

/// Computes the changes from `old` to `new`, in the sequence of their lines.
pub fn changes(old: &[u8], new: &[u8]) -> Vec<Change> {
    let input = InternedInput::new(old, new);
    diff_with_slider_heuristics(Algorithm::Histogram, &input)
        .hunks()
        .collect()
}
