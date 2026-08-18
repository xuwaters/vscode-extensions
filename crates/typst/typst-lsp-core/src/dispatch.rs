//! Method name → handler.
//!
//! Transport-agnostic on purpose: the same table serves the WASM binding and
//! the native test harness, so nothing about the feature set depends on how
//! bytes reach the process.

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{Ports, Server};

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
    /// JSON-RPC error code.
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
    serde_json::from_value(params)
        .map_err(|err| ResponseError::invalid_params(err.to_string()))
}

fn ok<T: Serialize>(value: T) -> Result<Value, ResponseError> {
    serde_json::to_value(value).map_err(|err| ResponseError::internal(err.to_string()))
}

/// Route a request.
pub fn request<Q: Ports>(
    server: &mut Server<Q>,
    method: &str,
    params: Value,
) -> Result<Value, ResponseError> {
    match method {
        // ── The typst-ide trio ────────────────────────────────────────────
        "textDocument/completion" => ok(server.completion(parse(params)?)),
        "textDocument/hover" => ok(server.hover(parse(params)?)),
        "textDocument/definition" => ok(server.definition(parse(params)?)),

        // ── Features we assemble ──────────────────────────────────────────
        "textDocument/references" => ok(server.references(parse(params)?)),
        "textDocument/prepareRename" => ok(server.prepare_rename(parse(params)?)),
        "textDocument/rename" => server.rename(parse(params)?).and_then(ok),
        "textDocument/documentSymbol" => ok(server.document_symbols(parse(params)?)),
        "workspace/symbol" => ok(server.workspace_symbols(parse(params)?)),
        "textDocument/semanticTokens/full" => ok(server.semantic_tokens_full(parse(params)?)),
        "textDocument/semanticTokens/full/delta" => {
            ok(server.semantic_tokens_delta(parse(params)?))
        }
        "textDocument/foldingRange" => ok(server.folding_ranges(parse(params)?)),
        "textDocument/selectionRange" => ok(server.selection_ranges(parse(params)?)),
        "textDocument/documentLink" => ok(server.document_links(parse(params)?)),
        "textDocument/formatting" => ok(server.formatting(parse(params)?)),
        "textDocument/rangeFormatting" => ok(server.range_formatting(parse(params)?)),

        // ── Phase 4 ───────────────────────────────────────────────────────
        "textDocument/inlayHint" => ok(server.inlay_hints(parse(params)?)),
        "textDocument/signatureHelp" => ok(server.signature_help(parse(params)?)),
        "textDocument/codeAction" => ok(server.code_actions(parse(params)?)),
        "textDocument/codeLens" => ok(server.code_lenses(parse(params)?)),

        // ── Preview and export ────────────────────────────────────────────
        "typst/renderPages" => ok(server.render_pages(parse(params)?)),
        "typst/documentMetrics" => ok(server.document_metrics(parse(params)?)),
        "typst/jumpFromClick" => ok(server.jump_from_click(parse(params)?)),
        "typst/jumpFromCursor" => ok(server.jump_from_cursor(parse(params)?)),
        "typst/export" => server.export(parse(params)?).and_then(ok),

        // ── Lifecycle ─────────────────────────────────────────────────────
        "shutdown" => ok(Value::Null),

        _ => Err(ResponseError::method_not_found(method)),
    }
}

/// Route a notification. Notifications have no response, so a malformed
/// payload is logged and dropped rather than reported.
pub fn notification<Q: Ports>(server: &mut Server<Q>, method: &str, params: Value) {
    let outcome = match method {
        "textDocument/didOpen" => parse(params).map(|p| server.did_open(p)),
        "textDocument/didChange" => parse(params).map(|p| server.did_change(p)),
        "textDocument/didSave" => parse(params).map(|p| server.did_save(p)),
        "textDocument/didClose" => parse(params).map(|p| server.did_close(p)),
        "workspace/didChangeConfiguration" => {
            parse(params).map(|p| server.did_change_configuration(p))
        }

        // Host-driven: the debounce timer lives in the Node process, because a
        // timer needs a runtime the WASM module does not have.
        "typst/compile" => parse(params).map(|p| server.compile_now(p)),
        "typst/setMain" => parse(params).map(|p| server.set_main(p)),
        "typst/workspaceFiles" => parse(params).map(|p| server.did_change_workspace_files(p)),

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
