//! Block-level diffing: turn the previous and current renders' block-hash
//! sequences into a compact patch script the webview can apply as DOM surgery.

use serde::Serialize;
use similar::{Algorithm, DiffOp, capture_diff_slices};

/// One step of the patch script, applied with a cursor walking the preview's
/// current top-level block list.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Patch {
    /// Advance the cursor over `count` unchanged blocks.
    Keep { count: usize },
    /// Remove the next `count` blocks.
    Delete { count: usize },
    /// Insert these blocks at the cursor.
    Insert { html: Vec<String> },
    /// Replace the next `count` blocks with these blocks.
    Replace { count: usize, html: Vec<String> },
}

/// Diff two hash sequences, emitting patches whose inserted HTML comes from
/// `new_html` (parallel to `new_hashes`).
pub fn diff(old_hashes: &[u64], new_hashes: &[u64], new_html: &[String]) -> Vec<Patch> {
    debug_assert_eq!(new_hashes.len(), new_html.len());
    let ops = capture_diff_slices(Algorithm::Myers, old_hashes, new_hashes);
    let mut patches = Vec::with_capacity(ops.len());
    for op in ops {
        match op {
            DiffOp::Equal { len, .. } => patches.push(Patch::Keep { count: len }),
            DiffOp::Delete { old_len, .. } => patches.push(Patch::Delete { count: old_len }),
            DiffOp::Insert {
                new_index, new_len, ..
            } => patches.push(Patch::Insert {
                html: new_html[new_index..new_index + new_len].to_vec(),
            }),
            DiffOp::Replace {
                old_len,
                new_index,
                new_len,
                ..
            } => patches.push(Patch::Replace {
                count: old_len,
                html: new_html[new_index..new_index + new_len].to_vec(),
            }),
        }
    }
    patches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn apply(old: &[String], patches: &[Patch]) -> Vec<String> {
        let mut out = Vec::new();
        let mut cursor = 0usize;
        for p in patches {
            match p {
                Patch::Keep { count } => {
                    out.extend_from_slice(&old[cursor..cursor + count]);
                    cursor += count;
                }
                Patch::Delete { count } => cursor += count,
                Patch::Insert { html } => out.extend(html.iter().cloned()),
                Patch::Replace { count, html } => {
                    cursor += count;
                    out.extend(html.iter().cloned());
                }
            }
        }
        out.extend_from_slice(&old[cursor..]);
        out
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    fn hashes(items: &[String]) -> Vec<u64> {
        items.iter().map(|s| crate::hash_block(s)).collect()
    }

    /// Property: applying the patch script to the old block list reproduces
    /// the new one.
    fn check(old: &[&str], new: &[&str]) {
        let old = strings(old);
        let new = strings(new);
        let patches = diff(&hashes(&old), &hashes(&new), &new);
        assert_eq!(apply(&old, &patches), new, "patches: {patches:?}");
    }

    #[test]
    fn edit_at_start() {
        check(&["a", "b", "c"], &["x", "b", "c"]);
    }

    #[test]
    fn edit_in_middle() {
        check(&["a", "b", "c"], &["a", "x", "c"]);
    }

    #[test]
    fn edit_at_end() {
        check(&["a", "b", "c"], &["a", "b", "x"]);
    }

    #[test]
    fn insert_and_delete_multi_region() {
        check(&["a", "b", "c", "d", "e"], &["a", "x", "b", "d", "y"]);
    }

    #[test]
    fn empty_to_full_and_back() {
        check(&[], &["a", "b"]);
        check(&["a", "b"], &[]);
    }

    #[test]
    fn identical_produces_single_keep() {
        let old = strings(&["a", "b"]);
        let patches = diff(&hashes(&old), &hashes(&old), &old);
        assert_eq!(patches, vec![Patch::Keep { count: 2 }]);
    }

    #[test]
    fn single_edit_is_one_replace() {
        let old = strings(&["a", "b", "c"]);
        let new = strings(&["a", "x", "c"]);
        let patches = diff(&hashes(&old), &hashes(&new), &new);
        assert_eq!(
            patches,
            vec![
                Patch::Keep { count: 1 },
                Patch::Replace {
                    count: 1,
                    html: vec!["x".into()]
                },
                Patch::Keep { count: 1 },
            ]
        );
    }
}
