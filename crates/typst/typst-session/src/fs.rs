//! `std::fs` implementations of the ports.
//!
//! These exist so the entire compile pipeline — world, VFS, fonts, packages,
//! diagnostics, export — can be exercised by `cargo test` on the host, with no
//! WASM toolchain and no JS in sight. They are also what a future CLI front-end
//! would use.
//!
//! Compiled only under the `fs-ports` feature, which is on by default and off
//! for `typst-lsp-wasm`, so `std::fs` never reaches the WASM artifact.

use std::path::{Path, PathBuf};

use ecow::EcoString;
use typst::diag::{FileError, FileResult};
use typst::foundations::Bytes;
use typst::syntax::package::PackageSpec;
use typst::syntax::{VirtualPath, VirtualRoot};
use typst::text::FontInfo;

use crate::ports::{
    ClockProvider, FaceDescriptor, FileProvider, FontProvider, PackageProvider,
    PackageResolution,
};

/// Reads project files from a root directory and package files from a cache.
pub struct FsFiles {
    root: PathBuf,
    package_cache: Option<PathBuf>,
}

impl FsFiles {
    /// Serve project files out of `root`, with no packages available.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into(), package_cache: None }
    }

    /// Also serve package files out of a typst-cli-compatible cache directory.
    pub fn with_package_cache(mut self, cache: impl Into<PathBuf>) -> Self {
        self.package_cache = Some(cache.into());
        self
    }

    /// The real directory a virtual root maps to.
    fn base(&self, root: &VirtualRoot) -> Option<PathBuf> {
        match root {
            VirtualRoot::Project => Some(self.root.clone()),
            VirtualRoot::Package(spec) => {
                Some(package_dir(self.package_cache.as_deref()?, spec))
            }
        }
    }
}

impl FileProvider for FsFiles {
    fn read(&self, root: &VirtualRoot, vpath: &VirtualPath) -> FileResult<Bytes> {
        let base = self
            .base(root)
            .ok_or_else(|| FileError::Other(Some("no root for this file".into())))?;
        let path = vpath.realize(&base).map_err(FileError::Realize)?;

        let bytes = std::fs::read(&path).map_err(|err| FileError::from_io(err, &path))?;
        Ok(Bytes::new(bytes))
    }

    fn list(&self, root: &VirtualRoot, vpath: &VirtualPath) -> Vec<String> {
        let Some(base) = self.base(root) else { return Vec::new() };
        let Ok(path) = vpath.realize(&base) else { return Vec::new() };
        let Ok(entries) = std::fs::read_dir(path) else { return Vec::new() };

        entries
            .filter_map(|entry| Some(entry.ok()?.file_name().to_str()?.to_string()))
            .collect()
    }
}

/// The directory a package lives in, matching typst-cli's cache layout.
pub fn package_dir(cache: &Path, spec: &PackageSpec) -> PathBuf {
    cache
        .join(spec.namespace.as_str())
        .join(spec.name.as_str())
        .join(spec.version.to_string())
}

/// Fonts read from files on disk.
pub struct FsFonts {
    faces: Vec<FaceDescriptor>,
    files: Vec<PathBuf>,
}

impl FsFonts {
    /// An empty font set. Documents that need a font will report so.
    pub fn empty() -> Self {
        Self { faces: Vec::new(), files: Vec::new() }
    }

    /// Index every font file found under the given directories, recursively.
    pub fn from_dirs<P: AsRef<Path>>(dirs: impl IntoIterator<Item = P>) -> Self {
        let mut fonts = Self::empty();
        for dir in dirs {
            fonts.scan(dir.as_ref());
        }
        fonts
    }

    /// Index one font file, adding every face it contains.
    pub fn add_file(&mut self, path: &Path) {
        let Ok(bytes) = std::fs::read(path) else { return };
        for (index, info) in FontInfo::iter(&bytes).enumerate() {
            self.faces.push(FaceDescriptor { info, index: index as u32 });
            self.files.push(path.to_path_buf());
        }
    }

    fn scan(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        let mut paths: Vec<PathBuf> = entries.filter_map(|e| Some(e.ok()?.path())).collect();
        // Deterministic order, so a font book built twice indexes identically.
        paths.sort();

        for path in paths {
            if path.is_dir() {
                self.scan(&path);
            } else if is_font_file(&path) {
                self.add_file(&path);
            }
        }
    }
}

fn is_font_file(path: &Path) -> bool {
    matches!(
        path.extension().and_then(|ext| ext.to_str()).map(str::to_ascii_lowercase).as_deref(),
        Some("ttf" | "otf" | "ttc" | "otc")
    )
}

impl FontProvider for FsFonts {
    fn faces(&self) -> &[FaceDescriptor] {
        &self.faces
    }

    fn data(&self, face: usize) -> Option<Bytes> {
        let path = self.files.get(face)?;
        Some(Bytes::new(std::fs::read(path).ok()?))
    }
}

/// Packages read from a typst-cli-compatible cache, never downloaded.
pub struct FsPackages {
    cache: Option<PathBuf>,
    index: Vec<(PackageSpec, Option<EcoString>)>,
}

impl FsPackages {
    /// Resolve packages against a cache directory.
    pub fn new(cache: Option<PathBuf>) -> Self {
        Self { cache, index: Vec::new() }
    }

    /// Supply a Universe index for package-name completions.
    pub fn with_index(mut self, index: Vec<(PackageSpec, Option<EcoString>)>) -> Self {
        self.index = index;
        self
    }
}

impl PackageProvider for FsPackages {
    fn resolve(&self, spec: &PackageSpec) -> PackageResolution {
        let Some(cache) = &self.cache else {
            return PackageResolution::Failed("no package cache configured".into());
        };
        if package_dir(cache, spec).is_dir() {
            PackageResolution::Ready
        } else {
            PackageResolution::Failed("package is not in the local cache".into())
        }
    }

    fn index(&self) -> &[(PackageSpec, Option<EcoString>)] {
        &self.index
    }
}

/// The host's wall clock.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl ClockProvider for SystemClock {
    fn now_ms(&self) -> Option<i64> {
        let since_epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?;
        i64::try_from(since_epoch.as_millis()).ok()
    }
}
