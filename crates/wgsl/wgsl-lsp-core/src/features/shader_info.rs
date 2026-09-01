//! `wgsl/shaderInfo` — the one request that is not standard LSP.
//!
//! GLSL has no in-language way to say which stage a file is: it comes from a
//! `#pragma shader_stage`, or the file extension, or a guess at the built-ins
//! the source uses. The status bar shows which, because a file silently
//! validated as the wrong stage produces errors that make no sense.
//!
//! It also carries the reason validation was *skipped*, which is the honest
//! answer for a `#version 300 es` source: naga does not implement that dialect,
//! so the absence of squiggles means "not checked", not "correct".

use lsp_types::{TextDocumentIdentifier, Uri};
use serde::{Deserialize, Serialize};

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
    /// The stage the file was validated as. Absent for WGSL, which has none.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stage: Option<&'static str>,
    /// Why nothing was validated, when nothing was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    /// Whether naga parsed and validated the source cleanly.
    pub ok: bool,
    /// How many problems it found.
    pub problems: usize,
    /// The naga version doing the checking.
    pub naga: &'static str,
}

impl Server {
    pub fn shader_info(&mut self, params: ShaderInfoParams) -> Option<ShaderInfo> {
        let uri: &Uri = &params.text_document.uri;
        let document = self.document(uri)?;
        let analysis = document.analysis();

        Some(ShaderInfo {
            language: document.language.id(),
            stage: analysis.stage_label,
            skipped: analysis.skipped.clone(),
            ok: analysis.problems.is_empty() && analysis.skipped.is_none(),
            problems: analysis.problems.len(),
            naga: crate::NAGA_VERSION,
        })
    }
}
