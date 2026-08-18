//! The Typst compile session.
//!
//! This crate owns everything between the host and the typst compiler: the
//! [`World`](typst::World) implementation, the virtual file system, the lazy
//! font book, package gating, and the compile-then-evict lifecycle. It knows
//! nothing about LSP, nothing about WASM, and does no I/O of its own — every
//! byte it reads arrives through one of the traits in [`ports`].
//!
//! That is what makes the whole server testable with `cargo test`: [`fs`]
//! implements the ports over `std::fs`, `typst-lsp-wasm` implements them over
//! JS callbacks, and the engine in between is the same code either way.
//!
//! ```no_run
//! use typst_session::fs::{FsFiles, FsFonts, FsPackages, SystemClock};
//! use typst_session::{Session, SessionWorld};
//! use typst::syntax::VirtualPath;
//!
//! let main = SessionWorld::<FsFiles, FsFonts, FsPackages, SystemClock>::project_file(
//!     VirtualPath::new("main.typ").unwrap(),
//! );
//! let world = SessionWorld::new(
//!     FsFiles::new("/project"),
//!     FsFonts::from_dirs(["/project/fonts"]),
//!     FsPackages::new(None),
//!     SystemClock,
//!     main,
//! );
//! let mut session = Session::new(world, 1);
//! let outcome = session.compile(0);
//! println!("{} diagnostics", outcome.diagnostics.len());
//! ```

pub mod compile;
pub mod diagnostics;
pub mod export;
pub mod fonts;
pub mod packages;
pub mod ports;
pub mod vfs;
pub mod world;

#[cfg(feature = "fs-ports")]
pub mod fs;

pub use compile::{CompileOutcome, DocVersion, Session};
pub use diagnostics::{Diagnostic, Related};
pub use export::{PdfExportOptions, export_html, export_pdf};
pub use fonts::FontSlots;
pub use packages::Packages;
pub use ports::{
    ClockProvider, FaceDescriptor, FileProvider, FixedClock, FontProvider, NoClock,
    NoPackages, PackageProvider, PackageResolution,
};
pub use vfs::Vfs;
pub use world::SessionWorld;

/// The upstream typst version this session is built against.
pub const TYPST_VERSION: &str = "0.15.1";
