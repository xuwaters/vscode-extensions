//! PDF and HTML export.
//!
//! SVG and PNG live in `typst-preview-core`, which already depends on
//! `typst-svg` and `typst-render` for the live preview; keeping them there
//! saves the LSP crate two dependencies it has no other use for.

use ecow::{EcoString, EcoVec, eco_format};
use typst::diag::{FileResult, SourceDiagnostic};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Feature, Library, LibraryExt, World};
use typst_layout::PagedDocument;
use typst_pdf::{PdfOptions, PdfStandard, PdfStandards};

/// What the caller can vary about a PDF export.
#[derive(Debug, Clone, Default)]
pub struct PdfExportOptions {
    /// A stable document identifier. `None` lets typst derive one from the
    /// title and author, which is what `typst compile` does.
    pub ident: Option<String>,
    /// PDF standards to enforce, e.g. `PDF/A-2b`.
    pub standards: Vec<PdfStandard>,
}

/// Export a compiled document to PDF bytes.
///
/// Errors carry the compiler's own message, because they are almost always
/// something the user can fix (an unsupported feature for the chosen standard,
/// say) rather than an internal failure.
pub fn export_pdf(
    document: &PagedDocument,
    options: &PdfExportOptions,
) -> Result<Vec<u8>, EcoString> {
    let standards = PdfStandards::new(&options.standards)
        .map_err(|err| eco_format!("{}", err.message()))?;

    let opts = PdfOptions {
        ident: match &options.ident {
            Some(ident) => typst::foundations::Smart::Custom(ident.clone()),
            None => typst::foundations::Smart::Auto,
        },
        standards,
        ..PdfOptions::default()
    };

    typst_pdf::pdf(document, &opts).map_err(|errors| {
        errors
            .first()
            .map(|error| error.message.clone())
            .unwrap_or_else(|| "PDF export failed".into())
    })
}

/// Export a document to HTML — P4-07.
///
/// HTML is a **separate compilation target**, not a rendering of the paged
/// document: `typst::compile::<HtmlDocument>` runs the whole pipeline again
/// against `Target::Html`, and a document written for print may lay out
/// differently or refuse outright. So this takes the world rather than a
/// `PagedDocument`, and reports the compiler's own diagnostics when the target
/// does not suit the document.
///
/// It is also still **experimental upstream**, gated behind `Feature::Html` —
/// typst-cli spells that `--features html`. The session's own library
/// deliberately does *not* enable it, because that would make `html.*`
/// available in ordinary documents and quietly diverge from what
/// `typst compile` accepts. Instead the export runs against a world that is the
/// session's in every respect except its library.
pub fn export_html(world: &dyn World) -> Result<String, Vec<EcoString>> {
    let html_world = HtmlWorld::new(world);
    let warned = typst::compile::<typst_html::HtmlDocument>(&html_world);

    let document = warned.output.map_err(collect_messages)?;

    typst_html::html(&document, &typst_html::HtmlOptions { pretty: true })
        .map_err(collect_messages)
}

fn collect_messages(errors: EcoVec<SourceDiagnostic>) -> Vec<EcoString> {
    errors.iter().map(|error| error.message.clone()).collect()
}

/// A world that is another world with the HTML feature turned on.
///
/// Everything but `library()` delegates, so the VFS, fonts, packages, and clock
/// are the session's — only the standard library differs.
struct HtmlWorld<'a> {
    inner: &'a dyn World,
    library: LazyHash<Library>,
}

impl<'a> HtmlWorld<'a> {
    fn new(inner: &'a dyn World) -> Self {
        let library = Library::builder()
            .with_features([Feature::Html].into_iter().collect())
            .build();
        Self { inner, library: LazyHash::new(library) }
    }
}

impl World for HtmlWorld<'_> {
    fn library(&self) -> &LazyHash<Library> {
        &self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.inner.book()
    }

    fn main(&self) -> FileId {
        self.inner.main()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        self.inner.source(id)
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        self.inner.file(id)
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.inner.font(index)
    }

    fn today(&self, offset: Option<Duration>) -> Option<Datetime> {
        self.inner.today(offset)
    }
}
