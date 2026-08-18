//! The `typst/*` protocol extensions: page rendering, metrics, jump mapping,
//! and export.
//!
//! These are what the preview webview and the export commands talk to. They all
//! read the **last good document** — never triggering a compile of their own, so
//! a preview refresh cannot stall a keystroke, and an export of a document that
//! currently has errors reports that rather than silently writing a stale file.

use lsp_types::Uri;
use rustc_hash::FxHashMap;
use serde::{Deserialize, Serialize};
use typst_preview_core::{
    DocumentPoint, JumpTarget, PageMetrics, PagePatch, RenderMode, RenderOptions, jump,
};
use typst_session::PdfExportOptions;

use crate::convert::offset_to_position;
use crate::dispatch::ResponseError;
use crate::{Ports, Server};

/// `typst/renderPages`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPagesParams {
    /// The document being previewed.
    pub uri: Uri,
    /// The pages in the viewport plus its prefetch margin.
    pub pages: Vec<usize>,
    /// Page index → the content hash the webview already holds, as hex.
    #[serde(default)]
    pub known_hashes: FxHashMap<usize, String>,
    /// `typstUltra.preview.renderMode`.
    #[serde(default)]
    pub mode: RenderMode,
    /// Raster resolution, when a page is rendered as PNG. The host sends the
    /// current zoom step so a zoomed-in raster page is not blurry.
    #[serde(default)]
    pub ppi: Option<f64>,
}

/// `typst/renderPages` result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderPagesResult {
    /// One entry per requested page.
    pub patches: Vec<PagePatch>,
    /// How many pages the document has, so the webview can size its scrollbar.
    pub page_count: usize,
}

/// `typst/documentMetrics`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetricsParams {
    /// The document being previewed.
    pub uri: Uri,
}

/// `typst/documentMetrics` result.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetricsResult {
    /// How many pages.
    pub page_count: usize,
    /// Dimensions and content hash per page.
    pub pages: Vec<PageMetricsWire>,
}

/// A page's metrics on the wire, with the hash as hex so JSON keeps it exact.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageMetricsWire {
    /// Zero-based page index.
    pub index: usize,
    /// Width in typographic points.
    pub width_pt: f64,
    /// Height in typographic points.
    pub height_pt: f64,
    /// Content hash, lowercase hex.
    pub hash: String,
}

impl From<&PageMetrics> for PageMetricsWire {
    fn from(metrics: &PageMetrics) -> Self {
        Self {
            index: metrics.index,
            width_pt: metrics.width_pt,
            height_pt: metrics.height_pt,
            hash: typst_preview_core::hash_hex(metrics.hash),
        }
    }
}

/// `typst/jumpFromClick`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JumpFromClickParams {
    /// Zero-based page index.
    pub page: usize,
    /// Horizontal offset in points, already divided by the webview's zoom.
    pub x_pt: f64,
    /// Vertical offset in points.
    pub y_pt: f64,
}

/// Where a click resolved to.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum JumpFromClickResult {
    /// A position in a source file.
    #[serde(rename_all = "camelCase")]
    Source {
        /// The file to reveal.
        uri: Uri,
        /// Where in it.
        position: lsp_types::Position,
    },
    /// An external link. The host applies its scheme allowlist.
    Url {
        /// The target.
        url: String,
    },
    /// Another place in the same document.
    Page(DocumentPoint),
}

/// `typst/jumpFromCursor`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JumpFromCursorParams {
    /// The document the cursor is in.
    pub uri: Uri,
    /// The cursor.
    pub position: lsp_types::Position,
}

/// `typst/export`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportParams {
    /// What to produce.
    pub format: ExportFormat,
    /// Page index, for single-page PNG export.
    #[serde(default)]
    pub page: Option<usize>,
    /// Pixels per inch for PNG.
    #[serde(default)]
    pub ppi: Option<f64>,
}

/// An export target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// A PDF of the whole document.
    Pdf,
    /// One SVG containing every page.
    Svg,
    /// PNG, one file per page.
    Png,
    /// HTML. A separate compilation target, so a document written for print may
    /// not produce one.
    Html,
}

/// `typst/export` result: base64 payloads, one per output file.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExportResult {
    /// The produced files, base64-encoded, in page order.
    pub files: Vec<String>,
    /// The extension the host should use.
    pub extension: String,
}

impl<Q: Ports> Server<Q> {
    /// `typst/renderPages`.
    pub fn render_pages(&mut self, params: RenderPagesParams) -> Option<RenderPagesResult> {
        let document = self.session().last_good_arc()?;

        let known: FxHashMap<usize, u64> = params
            .known_hashes
            .iter()
            .filter_map(|(index, hash)| Some((*index, u64::from_str_radix(hash, 16).ok()?)))
            .collect();

        let patches: Vec<PagePatch> = self.preview.render(
            &document,
            &params.pages,
            &known,
            RenderOptions {
                mode: params.mode,
                ppi: params.ppi.unwrap_or(144.0),
            },
        );

        Some(RenderPagesResult { patches, page_count: document.pages().len() })
    }

    /// `typst/documentMetrics`.
    pub fn document_metrics(
        &mut self,
        _params: DocumentMetricsParams,
    ) -> Option<DocumentMetricsResult> {
        let document = self.session().last_good_arc()?;
        let metrics = self.preview.measure(&document);

        Some(DocumentMetricsResult {
            page_count: metrics.len(),
            pages: metrics.iter().map(PageMetricsWire::from).collect(),
        })
    }

    /// `typst/jumpFromClick`.
    pub fn jump_from_click(
        &mut self,
        params: JumpFromClickParams,
    ) -> Option<JumpFromClickResult> {
        let document = self.session().last_good_arc()?;
        let at = DocumentPoint {
            page: params.page,
            x_pt: params.x_pt,
            y_pt: params.y_pt,
        };

        match jump::from_click(self.session().world(), &document, at)? {
            JumpTarget::Source { file, offset } => {
                let source = typst::World::source(self.session().world(), file).ok()?;
                Some(JumpFromClickResult::Source {
                    uri: self.uris().to_uri(file)?,
                    position: offset_to_position(&source, offset),
                })
            }
            JumpTarget::Url(url) => Some(JumpFromClickResult::Url { url }),
            JumpTarget::Page(point) => Some(JumpFromClickResult::Page(point)),
        }
    }

    /// `typst/jumpFromCursor`.
    pub fn jump_from_cursor(
        &mut self,
        params: JumpFromCursorParams,
    ) -> Option<Vec<DocumentPoint>> {
        let document = self.session().last_good_arc()?;
        let (_, source, cursor) = self.locate(&params.uri, params.position)?;
        Some(jump::from_cursor(&document, &source, cursor))
    }

    /// `typst/export`.
    pub fn export(&mut self, params: ExportParams) -> Result<ExportResult, ResponseError> {
        let Some(document) = self.session().last_good_arc() else {
            return Err(ResponseError::internal(
                "nothing to export yet — the document has not compiled successfully",
            ));
        };

        // Export never triggers a compile of its own. If the document is
        // currently broken, say so rather than writing a stale file that looks
        // current.
        if !self.session().last_good_version().is_some() {
            return Err(ResponseError::internal("the document has no successful compile"));
        }

        let (files, extension) = match params.format {
            ExportFormat::Pdf => {
                let bytes = typst_session::export_pdf(&document, &PdfExportOptions::default())
                    .map_err(|err| ResponseError::internal(err.to_string()))?;
                (vec![bytes], "pdf")
            }
            ExportFormat::Svg => {
                let svg = typst_preview_core::export_svg(&document, 12.0);
                (vec![svg.into_bytes()], "svg")
            }
            ExportFormat::Png => {
                let ppi = params.ppi.unwrap_or(144.0);
                let pages = match params.page {
                    Some(page) => vec![
                        typst_preview_core::export_png(&document, page, ppi)
                            .map_err(|err| ResponseError::internal(err.to_string()))?,
                    ],
                    None => typst_preview_core::export_png_all(&document, ppi)
                        .map_err(|err| ResponseError::internal(err.to_string()))?,
                };
                (pages, "png")
            }
            ExportFormat::Html => {
                // HTML re-runs the whole pipeline against a different target, so
                // unlike the others it does not read the last good paged
                // document — and it can fail on a document that paginates fine.
                let html = typst_session::export_html(self.session().world())
                    .map_err(|errors| ResponseError::internal(errors.join("; ")))?;
                (vec![html.into_bytes()], "html")
            }
        };

        Ok(ExportResult {
            files: files.iter().map(|bytes| typst_preview_core::base64::encode(bytes)).collect(),
            extension: extension.to_string(),
        })
    }
}
