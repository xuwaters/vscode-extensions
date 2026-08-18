//! Document lifecycle, configuration, and the compile trigger.
//!
//! The debounce timer lives in the Node host — WASM has no runtime to hang one
//! on — so the host sends `typst/compile` when the quiet period expires. What
//! lives here is the *policy*: whether the trigger applies at all, and the rule
//! that results for superseded versions are dropped.

use lsp_types::{
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, Uri,
};
use serde::{Deserialize, Serialize};
use typst::syntax::FileId;

use crate::convert::range_from_lsp;
use crate::settings::Settings;
use crate::state::DocumentEntry;
use crate::{Ports, Server};

/// `typst/compile`: the host's debounce has expired, compile now.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CompileParams {
    /// The document that triggered the compile. Used to pick the compile root
    /// when no main file is pinned.
    pub uri: Option<Uri>,
}

/// `typst/setMain`: pin, or unpin, the compile root.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SetMainParams {
    /// The file to compile. `None` goes back to following the focused editor.
    pub uri: Option<Uri>,
}

/// `typst/workspaceFiles`: the set of `.typ` files the host can see.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFilesParams {
    /// Every known typst file in the workspace.
    pub uris: Vec<Uri>,
}

impl<Q: Ports> Server<Q> {
    /// `textDocument/didOpen`.
    pub fn did_open(&mut self, params: DidOpenTextDocumentParams) {
        let document = params.text_document;
        let Some(id) = self.uris().to_file_id(&document.uri) else {
            // Outside the compile root. Decision 0008 calls this "not in
            // project"; the status bar says so and nothing else happens.
            return;
        };

        self.session_mut().open(id, document.text);
        self.documents.insert(
            id,
            DocumentEntry { uri: document.uri, version: document.version },
        );
        self.tokens.remove(&id);

        // The first open of a file with no pinned main makes it the root, so
        // opening a `.typ` and seeing errors needs no configuration at all.
        if self.pinned_main.is_none() {
            self.session_mut().set_main(id);
        }
    }

    /// `textDocument/didChange`, incremental where the client offers it.
    pub fn did_change(&mut self, params: DidChangeTextDocumentParams) {
        let Some(id) = self.uris().to_file_id(&params.text_document.uri) else { return };

        for change in params.content_changes {
            match change.range {
                Some(range) => {
                    // Resolve the range against the text as it stands *now*,
                    // before this edit, which is what LSP specifies.
                    let Some(source) = self.session().world().vfs().opened(id).cloned()
                    else {
                        continue;
                    };
                    let byte_range = range_from_lsp(&source, range);
                    if !self.session_mut().edit(id, byte_range, &change.text) {
                        // The range did not line up — resynchronize rather than
                        // letting the trees drift apart silently.
                        self.session_mut().replace(id, &change.text);
                    }
                }
                None => {
                    self.session_mut().replace(id, &change.text);
                }
            }
        }

        if let Some(entry) = self.documents.get_mut(&id) {
            entry.version = params.text_document.version;
        }

        // The token cache is deliberately *not* cleared here: it holds what the
        // client currently has, which is exactly what the next `full/delta`
        // diffs against. Clearing it would turn every delta request into a full
        // resend, which is the cost delta exists to avoid.
    }

    /// `textDocument/didSave`.
    pub fn did_save(&mut self, params: DidSaveTextDocumentParams) {
        let Some(id) = self.uris().to_file_id(&params.text_document.uri) else { return };
        if let Some(text) = params.text {
            self.session_mut().replace(id, &text);
        }
    }

    /// `textDocument/didClose`.
    pub fn did_close(&mut self, params: DidCloseTextDocumentParams) {
        let Some(id) = self.uris().to_file_id(&params.text_document.uri) else { return };
        self.session_mut().close(id);
        self.documents.remove(&id);
        self.tokens.remove(&id);
    }

    /// `workspace/didChangeConfiguration`.
    ///
    /// The client sends the whole `typstUltra` section; anything it omits falls
    /// back to the documented default.
    pub fn did_change_configuration(&mut self, params: DidChangeConfigurationParams) {
        let section = params
            .settings
            .get("typstUltra")
            .cloned()
            .unwrap_or(params.settings.clone());

        let Ok(settings) = serde_json::from_value::<Settings>(section) else {
            self.notify(
                "window/logMessage",
                serde_json::json!({ "type": 2, "message": "unreadable configuration" }),
            );
            return;
        };

        if self.set_settings(settings) {
            self.compile_now(CompileParams { uri: None });
        }
    }

    /// `typst/compile`: run a compile and publish what it produced.
    pub fn compile_now(&mut self, params: CompileParams) {
        // Follow the focused editor unless a main file is pinned.
        if self.pinned_main.is_none()
            && let Some(uri) = &params.uri
            && let Some(id) = self.uris().to_file_id(uri)
        {
            self.session_mut().set_main(id);
        }

        let version = self.main_version();

        self.notify(
            "typst/compileStatus",
            serde_json::json!({ "state": "compiling" }),
        );

        let outcome = self.session_mut().compile(version);

        // A newer edit landed while this compile ran: its diagnostics describe a
        // document that no longer exists, so they are dropped rather than
        // published. The compile itself cannot be interrupted — one thread, no
        // yield points — so supersession is the cancellation mechanism.
        if self.main_version() != outcome.version {
            return;
        }

        let page_count = outcome.document.as_ref().map(|d| d.pages().len()).unwrap_or(0);
        if let Some(document) = outcome.document.clone() {
            self.preview.measure(&document);
        }

        for spec in &outcome.pending_packages {
            self.notify(
                "typst/packageStatus",
                serde_json::json!({ "spec": spec.to_string(), "state": "downloading" }),
            );
        }

        self.publish_diagnostics(&outcome);
        self.last_published_version = Some(outcome.version);

        self.notify(
            "typst/compileStatus",
            serde_json::json!({
                "state": if outcome.ok { "ok" } else { "error" },
                "pageCount": page_count,
            }),
        );
    }

    /// `typst/setMain`: pin or unpin the compile root.
    pub fn set_main(&mut self, params: SetMainParams) {
        match params.uri.as_ref().and_then(|uri| self.uris().to_file_id(uri)) {
            Some(id) => {
                self.pinned_main = Some(id);
                self.session_mut().set_main(id);
            }
            None => self.pinned_main = None,
        }
        self.compile_now(CompileParams { uri: None });
    }

    /// `typst/workspaceFiles`.
    pub fn did_change_workspace_files(&mut self, params: WorkspaceFilesParams) {
        self.set_workspace_files(&params.uris);
    }

    /// The editor version of whichever file is currently the compile root.
    ///
    /// This is what compile results are stamped with, and what makes a
    /// superseded result recognisable when it comes back.
    fn main_version(&self) -> i32 {
        self.documents
            .get(&self.session().world().main_id())
            .map(|entry| entry.version)
            .unwrap_or(0)
    }

    /// Which file the compile root is pinned to, if any.
    pub fn pinned_main(&self) -> Option<FileId> {
        self.pinned_main
    }
}
