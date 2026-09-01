//! Method name → handler.
//!
//! Transport-agnostic on purpose: the same table serves the WASM binding and
//! the native test harness, so nothing about the feature set depends on how
//! bytes reach the process.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::Server;

/// A notification the server wants to send the client.
///
/// Queued rather than sent, and drained after the response is written, so a
/// handler never re-enters the host mid-call.
#[derive(Debug, Clone, Serialize)]
pub struct Outbound {
    /// LSP method name.
    pub method: String,
    /// Its params.
    pub params: Value,
}

/// A JSON-RPC error.
#[derive(Debug, Clone, Serialize)]
pub struct ResponseError {
    pub code: i32,
    /// A message for the log, not usually for the user.
    pub message: String,
}

impl ResponseError {
    /// -32601: the server does not implement this method.
    pub fn method_not_found(method: &str) -> Self {
        Self { code: -32601, message: format!("unknown method: {method}") }
    }

    /// -32602: the params did not deserialize.
    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self { code: -32602, message: message.into() }
    }

    /// -32603: something went wrong inside a handler.
    pub fn internal(message: impl Into<String>) -> Self {
        Self { code: -32603, message: message.into() }
    }
}

fn parse<T: DeserializeOwned>(params: Value) -> Result<T, ResponseError> {
    serde_json::from_value(params).map_err(|err| ResponseError::invalid_params(err.to_string()))
}

fn ok<T: Serialize>(value: T) -> Result<Value, ResponseError> {
    serde_json::to_value(value).map_err(|err| ResponseError::internal(err.to_string()))
}

/// Route a request.
pub fn request(
    server: &mut Server,
    method: &str,
    params: Value,
) -> Result<Value, ResponseError> {
    match method {
        // ── Reading the source ────────────────────────────────────────────
        "textDocument/completion" => ok(server.completion(parse(params)?)),
        "textDocument/hover" => ok(server.hover(parse(params)?)),
        "textDocument/definition" => ok(server.definition(parse(params)?)),
        "textDocument/references" => ok(server.references(parse(params)?)),
        "textDocument/documentHighlight" => ok(server.document_highlights(parse(params)?)),
        "textDocument/signatureHelp" => ok(server.signature_help(parse(params)?)),

        // ── Structure ─────────────────────────────────────────────────────
        "textDocument/documentSymbol" => ok(server.document_symbols(parse(params)?)),
        "workspace/symbol" => ok(server.workspace_symbols(parse(params)?)),
        "textDocument/foldingRange" => ok(server.folding_ranges(parse(params)?)),

        // ── Editing the source ────────────────────────────────────────────
        "textDocument/prepareRename" => ok(server.prepare_rename(parse(params)?)),
        "textDocument/rename" => ok(server.rename(parse(params)?)),
        "textDocument/codeAction" => ok(server.code_actions(parse(params)?)),
        "textDocument/formatting" => ok(server.formatting(parse(params)?)),
        "textDocument/rangeFormatting" => ok(server.range_formatting(parse(params)?)),

        // ── Decoration ────────────────────────────────────────────────────
        "textDocument/semanticTokens/full" => ok(server.semantic_tokens_full(parse(params)?)),
        "textDocument/semanticTokens/full/delta" => {
            ok(server.semantic_tokens_delta(parse(params)?))
        }
        "textDocument/inlayHint" => ok(server.inlay_hints(parse(params)?)),

        // ── Ours ──────────────────────────────────────────────────────────
        // GLSL carries no record of its own stage, so the status bar has to
        // ask what the server decided to treat the file as.
        "wgsl/shaderInfo" => ok(server.shader_info(parse(params)?)),

        "shutdown" => ok(Value::Null),

        _ => Err(ResponseError::method_not_found(method)),
    }
}

/// Route a notification. Notifications have no response, so a malformed
/// payload is logged and dropped rather than reported.
pub fn notification(server: &mut Server, method: &str, params: Value) {
    let outcome = match method {
        "textDocument/didOpen" => parse(params).map(|p| server.did_open(p)),
        "textDocument/didChange" => parse(params).map(|p| server.did_change(p)),
        "textDocument/didSave" => parse(params).map(|p| server.did_save(p)),
        "textDocument/didClose" => parse(params).map(|p| server.did_close(p)),
        "workspace/didChangeConfiguration" => {
            parse(params).map(|p| server.did_change_configuration(p))
        }

        // Host-driven: the server has no filesystem, so the host walks the
        // workspace and pushes what it finds.
        "wgsl/workspaceFiles" => parse(params).map(|p| server.did_change_workspace_files(p)),
        // "Validate Current File". Distinct from `didSave`, which honours the
        // `validate.onSave` setting — asking for validation explicitly should
        // validate whatever the settings say.
        "wgsl/validate" => parse(params).map(|p| server.validate_now(p)),

        "initialized" | "exit" | "$/cancelRequest" | "$/setTrace" => Ok(()),
        _ => Ok(()),
    };

    if let Err(error) = outcome {
        server.notify(
            "window/logMessage",
            serde_json::json!({
                "type": 2,
                "message": format!("{method}: {}", error.message),
            }),
        );
    }
}
