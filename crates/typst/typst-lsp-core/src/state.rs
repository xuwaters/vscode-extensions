//! Per-document server state.

use lsp_types::{SemanticToken, Uri};

/// What the server remembers about an open document.
#[derive(Debug, Clone)]
pub struct DocumentEntry {
    /// The document's URI, kept so diagnostics can be published without
    /// rebuilding it from the file id.
    pub uri: Uri,
    /// The editor's version. Compile results for older versions are dropped.
    pub version: i32,
}

/// The previous semantic token array for a document, so `full/delta` can send
/// edits instead of thousands of tokens on every keystroke.
#[derive(Debug, Clone, Default)]
pub struct TokenCache {
    /// The id the client will quote when it asks for a delta.
    pub result_id: u64,
    /// The tokens that id refers to.
    pub tokens: Vec<SemanticToken>,
}

impl TokenCache {
    /// Store a new token array and return its result id.
    pub fn store(&mut self, tokens: Vec<SemanticToken>) -> String {
        self.result_id = self.result_id.wrapping_add(1);
        self.tokens = tokens;
        self.result_id.to_string()
    }

    /// Whether the client's quoted id still matches what we hold.
    pub fn matches(&self, result_id: &str) -> bool {
        result_id == self.result_id.to_string()
    }

    /// The current result id.
    pub fn current_id(&self) -> String {
        self.result_id.to_string()
    }
}
