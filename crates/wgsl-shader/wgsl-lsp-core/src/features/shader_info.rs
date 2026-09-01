//! `wgsl/shaderInfo` — the one request that is not standard LSP.
//!
//! GLSL has no in-language way to say which stage a file is: it comes from a
//! `#pragma shader_stage`, or the file extension, or a guess at the builtins
//! the source uses. The status bar shows which, because a file analysed as the
//! wrong stage produces errors that make no sense — and it shows whether the
//! stage was *told* or *guessed*, because a guess never produces an error in
//! the first place.
//!
//! What it no longer carries is a reason validation was skipped. Before RFC
//! 012 phase 5 a GLSL ES or OpenGL-style source went unvalidated, because
//! naga's front end implements Vulkan GLSL alone; the honest answer then was
//! "not checked". Every dialect is checked now, by our own analyzer.

use lsp_types::{TextDocumentIdentifier, Uri};
use serde::{Deserialize, Serialize};
use wgsl_syntax::Language;

use crate::Server;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderInfoParams {
    pub text_document: TextDocumentIdentifier,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShaderInfo {
    /// `"wgsl"` or `"glsl"`.
    pub language: &'static str,
    /// The stage the file was analysed as. Absent for WGSL, which has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<&'static str>,
    /// Whether that stage was guessed from the file's contents rather than
    /// declared by a `#pragma shader_stage` or the extension. A guessed stage
    /// never produces a diagnostic.
    pub stage_guessed: bool,
    /// The GLSL version in force, as the spec writes it: `4.50`, `3.00 es`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Whether the analysis found no errors.
    pub ok: bool,
    /// How many error-severity problems it found.
    pub problems: usize,
    /// How many warnings.
    pub warnings: usize,
    /// Who did the checking, for the status bar's tooltip.
    pub validator: String,
    /// The naga version, which is what validates WGSL.
    pub naga: &'static str,
}

impl Server {
    pub fn shader_info(&mut self, params: ShaderInfoParams) -> Option<ShaderInfo> {
        let uri: &Uri = &params.text_document.uri;
        let document = self.document(uri)?;

        if let Some(glsl) = document.glsl() {
            let problems = crate::features::diagnostics::diagnostics(document);
            let errors = problems
                .iter()
                .filter(|d| d.severity == Some(lsp_types::DiagnosticSeverity::ERROR))
                .count();
            return Some(ShaderInfo {
                language: Language::Glsl.id(),
                stage: Some(glsl.stage_label()),
                stage_guessed: !glsl.stage_known,
                version: Some(glsl.context().version_label()),
                ok: errors == 0,
                problems: errors,
                warnings: problems.len() - errors,
                validator: format!("glsl-analysis {}", crate::GLSL_ANALYZER_VERSION),
                naga: crate::NAGA_VERSION,
            });
        }

        let analysis = document.analysis();
        Some(ShaderInfo {
            language: document.language.id(),
            stage: None,
            stage_guessed: false,
            version: None,
            ok: analysis.problems.is_empty(),
            problems: analysis.problems.len(),
            warnings: 0,
            validator: format!("naga {}", crate::NAGA_VERSION),
            naga: crate::NAGA_VERSION,
        })
    }
}
