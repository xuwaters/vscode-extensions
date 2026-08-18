//! Everything the engine needs from the outside world.
//!
//! The engine never performs I/O itself. Every file read, font byte, package
//! resolution, and clock reading crosses one of these traits. `typst-lsp-wasm`
//! implements them over synchronous JS callbacks; [`crate::fs`] implements them
//! over `std::fs` so the whole compile pipeline is testable with `cargo test`.
//!
//! All implementations are **synchronous** — they are called from inside
//! `World::file` in the middle of a compile. Anything that cannot be
//! synchronous (package downloads, system font indexing) is deferred: the
//! provider reports [`PackageResolution::Pending`], the compile finishes with a
//! diagnostic, and the host recompiles once the resource lands.

use ecow::EcoString;
use typst::diag::FileResult;
use typst::foundations::Bytes;
use typst::syntax::package::PackageSpec;
use typst::syntax::{VirtualPath, VirtualRoot};
use typst::text::FontInfo;

/// Reads project and package files.
pub trait FileProvider {
    /// Read a file. `root` distinguishes project files from package files.
    fn read(&self, root: &VirtualRoot, vpath: &VirtualPath) -> FileResult<Bytes>;

    /// List the entries of a directory, for path completions.
    ///
    /// Optional; the default returns nothing, which only costs path
    /// completions.
    fn list(&self, _root: &VirtualRoot, _vpath: &VirtualPath) -> Vec<String> {
        Vec::new()
    }
}

/// One font face the host knows about.
///
/// `FontInfo` is `Serialize`/`Deserialize` upstream, so the host can cache a
/// whole index on disk and hand it back without re-parsing any font file.
#[derive(Debug, Clone)]
pub struct FaceDescriptor {
    /// Metadata: family, variant, coverage.
    pub info: FontInfo,
    /// The face's index within its container file (non-zero only for
    /// collections such as `.ttc`).
    pub index: u32,
}

/// Supplies font metadata eagerly and font bytes lazily.
pub trait FontProvider {
    /// Metadata for every known face. Built once by the host, cached on disk.
    fn faces(&self) -> &[FaceDescriptor];

    /// Bytes for one face's container file. Called lazily — only for faces a
    /// document actually selects, so a machine with 400 MB of installed fonts
    /// contributes zero bytes to the heap until one is used.
    fn data(&self, face: usize) -> Option<Bytes>;
}

/// What the host can tell us about a package right now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PackageResolution {
    /// The package is present in the cache and its files can be read.
    Ready,
    /// The package is not present yet. The engine records the spec, emits a
    /// diagnostic, and the host downloads it and triggers a recompile.
    Pending,
    /// The package could not be made available, with a reason to show the user.
    Failed(EcoString),
}

/// Resolves package specs to readable roots.
pub trait PackageProvider {
    /// Resolve a package to a readable root, or report why not.
    fn resolve(&self, spec: &PackageSpec) -> PackageResolution;

    /// The Universe package index, for package-name completions.
    ///
    /// Optional; empty means no package completions.
    fn index(&self) -> &[(PackageSpec, Option<EcoString>)] {
        &[]
    }
}

/// Supplies the current time.
///
/// Split out from the other ports because WASM has no clock of its own —
/// `SystemTime::now()` traps on `wasm32-unknown-unknown`. The host reads
/// `Date.now()` and its timezone offset and passes both across.
pub trait ClockProvider {
    /// Milliseconds since the Unix epoch, UTC.
    ///
    /// `None` makes typst's `datetime.today()` return an error, which is the
    /// documented behaviour for a world without a clock.
    fn now_ms(&self) -> Option<i64>;

    /// Minutes east of UTC for the host's local timezone, used when a document
    /// asks for `today()` without an explicit offset.
    fn local_offset_minutes(&self) -> i64 {
        0
    }
}

/// A clock that always reports "no time available".
///
/// Useful in tests that want byte-stable output from documents which do not
/// call `datetime.today()`.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoClock;

impl ClockProvider for NoClock {
    fn now_ms(&self) -> Option<i64> {
        None
    }
}

/// A clock pinned to a fixed instant. Snapshot tests use this.
#[derive(Debug, Clone, Copy)]
pub struct FixedClock {
    /// Milliseconds since the Unix epoch.
    pub now_ms: i64,
    /// Minutes east of UTC.
    pub offset_minutes: i64,
}

impl ClockProvider for FixedClock {
    fn now_ms(&self) -> Option<i64> {
        Some(self.now_ms)
    }

    fn local_offset_minutes(&self) -> i64 {
        self.offset_minutes
    }
}

/// A package provider that resolves nothing.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoPackages;

impl PackageProvider for NoPackages {
    fn resolve(&self, _spec: &PackageSpec) -> PackageResolution {
        PackageResolution::Failed("package support is disabled".into())
    }
}
