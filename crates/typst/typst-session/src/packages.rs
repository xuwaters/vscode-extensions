//! Package gating.
//!
//! Every file read whose root is a package passes through here first. A package
//! that is not on disk yet cannot be fetched synchronously — WASM has no
//! network and the compile is mid-flight — so the spec is recorded, the read
//! fails with a message the user can act on, and the host downloads it and
//! triggers a recompile.

use std::sync::Mutex;

use ecow::{EcoString, eco_format};
use typst::diag::{FileError, FileResult, PackageError};
use typst::syntax::VirtualRoot;
use typst::syntax::package::PackageSpec;

use crate::ports::{PackageProvider, PackageResolution};

/// Tracks which packages a compile asked for and could not have.
pub struct Packages<P> {
    provider: P,
    pending: Mutex<Vec<PackageSpec>>,
    failed: Mutex<Vec<(PackageSpec, EcoString)>>,
}

impl<P: PackageProvider> Packages<P> {
    /// Wrap a provider.
    pub fn new(provider: P) -> Self {
        Self {
            provider,
            pending: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
        }
    }

    /// The provider, for the Universe index.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    /// Check whether files under this root may be read.
    ///
    /// `Ok(())` for project files and for packages already in the cache.
    pub fn gate(&self, root: &VirtualRoot) -> FileResult<()> {
        let VirtualRoot::Package(spec) = root else { return Ok(()) };

        match self.provider.resolve(spec) {
            PackageResolution::Ready => Ok(()),
            PackageResolution::Pending => {
                let mut pending = self.pending.lock().unwrap();
                if !pending.contains(spec) {
                    pending.push(spec.clone());
                }
                Err(FileError::Package(PackageError::Other(Some(eco_format!(
                    "downloading {spec} — the document will recompile when it is ready"
                )))))
            }
            PackageResolution::Failed(reason) => {
                let mut failed = self.failed.lock().unwrap();
                if !failed.iter().any(|(s, _)| s == spec) {
                    failed.push((spec.clone(), reason.clone()));
                }
                Err(FileError::Package(PackageError::Other(Some(eco_format!(
                    "{spec}: {reason}"
                )))))
            }
        }
    }

    /// Take the packages this compile asked for and did not have.
    pub fn take_pending(&self) -> Vec<PackageSpec> {
        std::mem::take(&mut *self.pending.lock().unwrap())
    }

    /// Take the packages this compile could not resolve at all.
    pub fn take_failed(&self) -> Vec<(PackageSpec, EcoString)> {
        std::mem::take(&mut *self.failed.lock().unwrap())
    }
}
