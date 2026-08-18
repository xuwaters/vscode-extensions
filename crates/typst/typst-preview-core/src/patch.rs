//! Turning "what the client holds" plus "what the client can see" into the
//! smallest set of page updates.
//!
//! A typical keystroke changes one page. Without this the webview would receive
//! 11.4 MB per character on a 30-page document; with it, one page.

use rustc_hash::FxHashMap;
use typst_layout::PagedDocument;

use crate::pages::{PagePatch, RenderMode, hash_hex, page_hash, render_page_in};

/// How pages should be produced for one viewport request.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    /// SVG, PNG, or per-page automatic.
    pub mode: RenderMode,
    /// Raster resolution, when a page is rendered as PNG.
    pub ppi: f64,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self { mode: RenderMode::default(), ppi: 144.0 }
    }
}

/// Compute page patches for a viewport request, in SVG.
pub fn diff(
    document: &PagedDocument,
    want: &[usize],
    known: &FxHashMap<usize, u64>,
) -> Vec<PagePatch> {
    diff_with(document, want, known, RenderOptions::default())
}

/// Compute page patches for a viewport request.
///
/// * `want` — the pages the webview can see, plus its prefetch margin.
/// * `known` — page index → content hash the webview already holds.
///
/// Pages whose hash is unchanged come back as `Unchanged` (no bytes); pages the
/// client holds that no longer exist come back as `Removed` so it can drop
/// them.
pub fn diff_with(
    document: &PagedDocument,
    want: &[usize],
    known: &FxHashMap<usize, u64>,
    options: RenderOptions,
) -> Vec<PagePatch> {
    let page_count = document.pages().len();
    let mut patches = Vec::new();

    for &index in want {
        if index >= page_count {
            // Asked for a page that no longer exists.
            if known.contains_key(&index) {
                patches.push(PagePatch::Removed { index });
            }
            continue;
        }

        let hash = page_hash(&document.pages()[index]);
        if known.get(&index) == Some(&hash) {
            patches.push(PagePatch::Unchanged { index });
            continue;
        }

        let Some((format, content)) =
            render_page_in(document, index, options.mode, options.ppi)
        else {
            continue;
        };
        patches.push(PagePatch::Replace {
            index,
            hash: hash_hex(hash),
            format,
            content,
        });
    }

    // Anything the client still holds past the end of the document must go,
    // even if the viewport did not ask about it — otherwise a document that got
    // shorter leaves orphan pages in the DOM.
    let mut stale: Vec<usize> =
        known.keys().copied().filter(|&index| index >= page_count).collect();
    stale.sort_unstable();
    for index in stale {
        if !want.contains(&index) {
            patches.push(PagePatch::Removed { index });
        }
    }

    patches
}

/// Apply patches to a client-side page map.
///
/// Used by the tests to prove that applying a patch set to the previous page
/// list reproduces the new one; the webview does the same thing to real DOM
/// nodes.
pub fn apply(known: &mut FxHashMap<usize, u64>, patches: &[PagePatch]) {
    for patch in patches {
        match patch {
            PagePatch::Unchanged { .. } => {}
            PagePatch::Replace { index, hash, .. } => {
                if let Ok(hash) = u64::from_str_radix(hash, 16) {
                    known.insert(*index, hash);
                }
            }
            PagePatch::Removed { index } => {
                known.remove(index);
            }
        }
    }
}
