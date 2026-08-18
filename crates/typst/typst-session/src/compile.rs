//! The compile lifecycle.
//!
//! One rule governs this module, and it is not optional:
//!
//! ```text
//! compile, then evict — never the other way round
//! ```
//!
//! Evicting first discards the memoized layout that the compile about to run
//! would have reused. The feasibility spike measured that mistake at 411 ms per
//! keystroke against 7 ms for the correct order — a 50× regression. So the two
//! calls live inside [`Session::compile`] and the ordering is not exposed to
//! any caller. `warm_cold_ratio` in this crate's tests asserts the ratio so a
//! reordering fails loudly rather than quietly.

use std::ops::Range;
use std::sync::Arc;

use typst::syntax::FileId;
use typst::syntax::package::PackageSpec;
use typst_layout::PagedDocument;

use crate::diagnostics::{Diagnostic, resolve_all};
use crate::ports::{ClockProvider, FileProvider, FontProvider, PackageProvider};
use crate::world::SessionWorld;

/// A document version, mirroring LSP's `textDocument.version`.
///
/// Every compile carries the version it started from so results for a
/// superseded document can be dropped instead of published.
pub type DocVersion = i32;

/// What one compile produced.
pub struct CompileOutcome {
    /// The version this compile started from.
    pub version: DocVersion,
    /// Warnings always; errors when the compile failed.
    pub diagnostics: Vec<Diagnostic>,
    /// The last good document — this compile's, or the previous one's if this
    /// one failed. `None` only before the first success.
    pub document: Option<Arc<PagedDocument>>,
    /// Whether *this* compile succeeded.
    pub ok: bool,
    /// Packages the compile asked for and did not have. The host downloads
    /// these and triggers a recompile.
    pub pending_packages: Vec<PackageSpec>,
}

/// Owns the world and the last good document.
pub struct Session<F, T, P, C> {
    world: SessionWorld<F, T, P, C>,
    evict_age: usize,
    last_good: Option<Arc<PagedDocument>>,
    last_good_version: Option<DocVersion>,
}

impl<F, T, P, C> Session<F, T, P, C>
where
    F: FileProvider + Send + Sync,
    T: FontProvider + Send + Sync,
    P: PackageProvider + Send + Sync,
    C: ClockProvider + Send + Sync,
{
    /// Create a session over a world.
    ///
    /// `evict_age` defaults to `1` in the extension — deliberately not
    /// typst-cli's `10`, because per-keystroke editing and whole-file watch
    /// loops have different working sets. See decision record 0005.
    pub fn new(world: SessionWorld<F, T, P, C>, evict_age: usize) -> Self {
        Self { world, evict_age, last_good: None, last_good_version: None }
    }

    /// The world, for IDE features that need to resolve names or read sources.
    pub fn world(&self) -> &SessionWorld<F, T, P, C> {
        &self.world
    }

    /// Mutable access, for document lifecycle events.
    pub fn world_mut(&mut self) -> &mut SessionWorld<F, T, P, C> {
        &mut self.world
    }

    /// The comemo eviction age in force.
    pub fn evict_age(&self) -> usize {
        self.evict_age
    }

    /// Change the eviction age. Takes effect at the next compile.
    pub fn set_evict_age(&mut self, evict_age: usize) {
        self.evict_age = evict_age;
    }

    /// Compile, then evict.
    pub fn compile(&mut self, version: DocVersion) -> CompileOutcome {
        self.world.reset();

        let warned = typst::compile::<PagedDocument>(&self.world);
        // AFTER the compile. See the module docs; this ordering is the whole
        // reason `compile` is a method rather than two public calls.
        comemo::evict(self.evict_age);

        let mut diagnostics = resolve_all(&self.world, warned.warnings.iter());
        let ok = match warned.output {
            Ok(document) => {
                self.last_good = Some(Arc::new(document));
                self.last_good_version = Some(version);
                true
            }
            Err(errors) => {
                diagnostics.extend(resolve_all(&self.world, errors.iter()));
                // Keep the previous good document: a syntax error mid-keystroke
                // must not blank the preview or kill label completions.
                false
            }
        };

        CompileOutcome {
            version,
            diagnostics,
            document: self.last_good.clone(),
            ok,
            pending_packages: self.world.packages().take_pending(),
        }
    }

    /// The document IDE features and the preview read.
    ///
    /// May lag one compile behind the source tree — deliberately, so an LSP
    /// request never has to wait for a compile.
    pub fn last_good(&self) -> Option<&PagedDocument> {
        self.last_good.as_deref()
    }

    /// A shared handle on the last good document.
    pub fn last_good_arc(&self) -> Option<Arc<PagedDocument>> {
        self.last_good.clone()
    }

    /// The version the last good document was compiled from.
    pub fn last_good_version(&self) -> Option<DocVersion> {
        self.last_good_version
    }

    /// Register a document as open at the editor's copy of its text.
    pub fn open(&mut self, id: FileId, text: String) {
        self.world.vfs_mut().open(id, text);
    }

    /// Apply an incremental edit, falling back to a full replace when the range
    /// does not line up.
    pub fn edit(&mut self, id: FileId, range: Range<usize>, with: &str) -> bool {
        self.world.vfs_mut().edit(id, range, with)
    }

    /// Replace an open document's text.
    pub fn replace(&mut self, id: FileId, text: &str) -> bool {
        self.world.vfs_mut().replace(id, text)
    }

    /// Drop a document's overlay.
    pub fn close(&mut self, id: FileId) {
        self.world.vfs_mut().close(id);
    }

    /// Point the compiler at a different entry file.
    ///
    /// Also drops the last good document, because it describes a different
    /// document entirely and would otherwise leak into the preview.
    pub fn set_main(&mut self, main: FileId) {
        if self.world.main_id() != main {
            self.world.set_main(main);
            self.last_good = None;
            self.last_good_version = None;
        }
    }
}
