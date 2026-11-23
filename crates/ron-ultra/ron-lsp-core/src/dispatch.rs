//! Method name → handler.

use crate::Server;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;

/// A notification the server wants to send the client.
#[derive(Debug, Clone, Serialize)]
pub struct Outbound {
    pub method: String,
    pub params: Value,
}

/// A JSON-RPC error.
#[derive(Debug, Clone, Serialize)]
pub struct ResponseError {
    pub code: i32,
    pub message: String,
}

impl ResponseError {
    pub fn method_not_found(method: &str) -> Self {
        Self { code: -32601, message: format!("unknown method: {method}") }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self { code: -32602, message: message.into() }
    }

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

/// The requests [`request`] answers, for the host to register. `shutdown`
/// is answered too but left off: the host's LSP library owns the
/// shutdown/exit handshake.
pub const REQUESTS: &[&str] = &[
    "textDocument/hover",
    "textDocument/completion",
    "textDocument/definition",
    "textDocument/documentSymbol",
    "textDocument/foldingRange",
    "textDocument/selectionRange",
    "textDocument/formatting",
];

/// The notifications [`notification`] handles.
pub const NOTIFICATIONS: &[&str] = &[
    "textDocument/didOpen",
    "textDocument/didChange",
    "textDocument/didClose",
    "workspace/didChangeConfiguration",
    "workspace/didChangeWatchedFiles",
];

pub fn request(server: &mut Server, method: &str, params: Value) -> Result<Value, ResponseError> {
    match method {
        "textDocument/hover" => ok(server.hover(parse(params)?)),
        "textDocument/completion" => ok(server.completion(parse(params)?)),
        "textDocument/definition" => ok(server.definition(parse(params)?)),
        "textDocument/documentSymbol" => ok(server.document_symbols(parse(params)?)),
        "textDocument/foldingRange" => ok(server.folding_ranges(parse(params)?)),
        "textDocument/selectionRange" => ok(server.selection_ranges(parse(params)?)),
        "textDocument/formatting" => ok(server.formatting(parse(params)?)),
        "shutdown" => ok(Value::Null),
        _ => Err(ResponseError::method_not_found(method)),
    }
}

/// Notifications have no response, so a malformed payload is logged and
/// dropped.
pub fn notification(server: &mut Server, method: &str, params: Value) {
    let outcome = match method {
        "textDocument/didOpen" => parse(params).map(|p| server.did_open(p)),
        "textDocument/didChange" => parse(params).map(|p| server.did_change(p)),
        "textDocument/didClose" => parse(params).map(|p| server.did_close(p)),
        "workspace/didChangeConfiguration" => parse(params).map(|p| server.did_change_configuration(p)),
        "workspace/didChangeWatchedFiles" => parse(params).map(|p| server.did_change_watched_files(p)),
        _ => Ok(()),
    };
    if let Err(error) = outcome {
        server.notify(
            "window/logMessage",
            serde_json::json!({ "type": 2, "message": format!("{method}: {}", error.message) }),
        );
    }
}
