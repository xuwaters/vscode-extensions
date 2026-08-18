//! Test scaffolding: the ports, wired to the fixture corpus and to typst's
//! bundled fonts.
//!
//! The point of the ports pattern is that this file is the *only* difference
//! between a test run and the real WASM server. Everything below the ports is
//! the same code either way.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use typst::foundations::Bytes;
use typst::syntax::{VirtualPath, VirtualRoot};
use typst::text::FontInfo;
use typst_session::fs::FsFiles;
use typst_session::ports::{
    ClockProvider, FaceDescriptor, FontProvider, PackageProvider, PackageResolution,
};
use typst_session::{Session, SessionWorld};

/// The fixture corpus root.
pub fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// typst's bundled default fonts, held in memory.
///
/// The real server reads these from `assets/fonts/` through the host, but the
/// bytes and therefore the layout are identical — which is the whole point of
/// bundling them (decision 0004).
pub struct BundledFonts {
    faces: Vec<FaceDescriptor>,
    data: Vec<Bytes>,
}

impl BundledFonts {
    pub fn new() -> Self {
        let mut faces = Vec::new();
        let mut data = Vec::new();
        for file in typst_assets::fonts() {
            let bytes = Bytes::new(file);
            for (index, info) in FontInfo::iter(file).enumerate() {
                faces.push(FaceDescriptor { info, index: index as u32 });
                data.push(bytes.clone());
            }
        }
        Self { faces, data }
    }
}

impl Default for BundledFonts {
    fn default() -> Self {
        Self::new()
    }
}

impl FontProvider for BundledFonts {
    fn faces(&self) -> &[FaceDescriptor] {
        &self.faces
    }

    fn data(&self, face: usize) -> Option<Bytes> {
        self.data.get(face).cloned()
    }
}

/// A package provider that reports every package as still downloading, so the
/// `Pending` path can be exercised without a network.
pub struct AlwaysPending;

impl PackageProvider for AlwaysPending {
    fn resolve(
        &self,
        _spec: &typst::syntax::package::PackageSpec,
    ) -> PackageResolution {
        PackageResolution::Pending
    }
}

/// A clock frozen at 2026-08-17T12:00:00Z, so snapshots stay stable.
pub struct FrozenClock;

impl ClockProvider for FrozenClock {
    fn now_ms(&self) -> Option<i64> {
        Some(1_787_054_400_000)
    }
}

/// The session type the tests use.
pub type TestSession<P = AlwaysPending> =
    Session<FsFiles, BundledFonts, P, FrozenClock>;

/// Build a session rooted at a fixture directory, compiling `main`.
pub fn session_at(root: &Path, main: &str) -> TestSession {
    session_with(root, main, AlwaysPending)
}

/// Build a session with a specific package provider.
pub fn session_with<P: PackageProvider + Send + Sync>(
    root: &Path,
    main: &str,
    packages: P,
) -> TestSession<P> {
    let main = file_id(main);
    let world = SessionWorld::new(
        FsFiles::new(root),
        BundledFonts::new(),
        packages,
        FrozenClock,
        main,
    );
    Session::new(world, 1)
}

/// A project-rooted file id for a workspace-relative path.
pub fn file_id(path: &str) -> typst::syntax::FileId {
    typst::syntax::FileId::new(typst::syntax::RootedPath::new(
        VirtualRoot::Project,
        VirtualPath::new(path).expect("valid virtual path"),
    ))
}

/// Render diagnostics as a stable, human-readable block for snapshotting.
pub fn render_diagnostics(diagnostics: &[typst_session::Diagnostic]) -> String {
    let mut out = String::new();
    for diagnostic in diagnostics {
        let file = diagnostic
            .file
            .map(|id| id.get().vpath().get_with_slash().to_string())
            .unwrap_or_else(|| "<detached>".into());
        let range = diagnostic
            .range
            .as_ref()
            .map(|r| format!("{}..{}", r.start, r.end))
            .unwrap_or_else(|| "-".into());
        out.push_str(&format!(
            "{:?} {file} [{range}] {}\n",
            diagnostic.severity, diagnostic.message
        ));
        for related in &diagnostic.related {
            out.push_str(&format!(
                "    ↳ {} [{}..{}] {}\n",
                related.file.get().vpath().get_with_slash(),
                related.range.start,
                related.range.end,
                related.message
            ));
        }
    }
    if out.is_empty() {
        out.push_str("(no diagnostics)\n");
    }
    out
}
