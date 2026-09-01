//! Document sync, configuration, and the workspace file list.

use lsp_types::{
    DidChangeConfigurationParams, DidChangeTextDocumentParams, DidCloseTextDocumentParams,
    DidOpenTextDocumentParams, DidSaveTextDocumentParams, Uri,
};
use serde::Deserialize;
use wgsl_syntax::Language;

use crate::settings::Settings;
use crate::state::Document;
use crate::Server;

/// The file list the host pushes over `wgsl/workspaceFiles`.
///
/// The server has no filesystem — it runs inside WASM — so `workspace/symbol`
/// can only see what the host hands it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct WorkspaceFilesParams {
    /// Files to index, replacing anything held for the same URI.
    pub files: Vec<WorkspaceFile>,
    /// Files to forget — deleted, or renamed away.
    pub removed: Vec<Uri>,
    /// Whether this message is the complete list. A full rescan sets it; an
    /// incremental update after a file watcher event does not.
    pub replace: bool,
}

/// The document `wgsl/validate` names.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ValidateParams {
    pub text_document: lsp_types::TextDocumentIdentifier,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceFile {
    pub uri: Uri,
    pub language_id: String,
    pub text: String,
}

impl Server {
    pub fn did_open(&mut self, params: DidOpenTextDocumentParams) {
        let item = params.text_document;
        let Some(language) = Language::from_id(&item.language_id) else {
            return;
        };

        // The live document supersedes whatever the index holds for this file:
        // keeping both would show every symbol twice in the picker.
        self.index_mut().remove(&item.uri);

        let uri = item.uri.clone();
        let default_version =
            crate::glsl::parse_version(&self.settings().for_language(language).default_version);
        self.insert_document(Document::new(
            item.uri,
            language,
            item.version,
            item.text,
            default_version,
        ));
        // Publish on open regardless of the `onType`/`onSave` settings: those
        // govern *re*validation, and a file with errors should say so the
        // moment it is opened rather than waiting for an edit.
        self.publish_diagnostics_for(&uri);
    }

    pub fn did_change(&mut self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.clone();
        let version = params.text_document.version;
        let Some(document) = self.document_mut(&uri) else {
            return;
        };

        for change in params.content_changes {
            match change.range {
                Some(range) => document.edit(range, &change.text),
                None => document.replace(change.text, version),
            }
        }
        document.version = version;

        if self.validation_enabled(&uri, |v| v.on_type) {
            self.publish_diagnostics_for(&uri);
        }

        // The token cache is deliberately *not* cleared: it holds what the
        // client currently has, which is exactly what the next `full/delta`
        // diffs against. Clearing it would turn every delta into a full
        // resend, which is the cost delta exists to avoid.
    }

    pub fn did_save(&mut self, params: DidSaveTextDocumentParams) {
        let uri = params.text_document.uri;
        if self.validation_enabled(&uri, |v| v.on_save) {
            self.publish_diagnostics_for(&uri);
        }
    }

    pub fn did_close(&mut self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri;
        if let Some(document) = self.remove_document(&uri) {
            // Hand the file back to the index so its symbols stay findable
            // from the workspace picker after the tab is shut.
            let text = document.text().to_string();
            let language = document.language;
            self.index_mut().set_file(&uri, language, &text);
        }
        self.clear_diagnostics(&uri);
    }

    pub fn did_change_configuration(&mut self, params: DidChangeConfigurationParams) {
        // VS Code sends the whole settings tree; the two sections we read may
        // be nested under it or sent on their own.
        let section = params.settings.clone();
        let Ok(settings) = serde_json::from_value::<Settings>(section) else {
            self.notify(
                "window/logMessage",
                serde_json::json!({ "type": 2, "message": "unreadable configuration" }),
            );
            return;
        };

        if self.set_settings(settings) {
            // Switching validation on has to reach the files already open, or
            // nothing happens until each is next touched.
            let uris: Vec<Uri> = self.documents().map(|d| d.uri.clone()).collect();
            for uri in uris {
                self.publish_diagnostics_for(&uri);
            }
        }
    }

    /// Validate one document now, whatever the settings say.
    pub fn validate_now(&mut self, params: ValidateParams) {
        self.publish_diagnostics_for(&params.text_document.uri);
    }

    pub fn did_change_workspace_files(&mut self, params: WorkspaceFilesParams) {
        if params.replace {
            *self.index_mut() = crate::WorkspaceIndex::default();
        }
        for uri in &params.removed {
            self.index_mut().remove(uri);
        }
        for file in &params.files {
            let Some(language) = Language::from_id(&file.language_id) else {
                continue;
            };
            // An open document is the better source; skip its stale copy.
            if self.document(&file.uri).is_some() {
                continue;
            }
            self.index_mut().set_file(&file.uri, language, &file.text);
        }
    }

    /// Whether a validation trigger is switched on for a document's language.
    fn validation_enabled(
        &self,
        uri: &Uri,
        which: impl Fn(&crate::settings::Validate) -> bool,
    ) -> bool {
        let Some(document) = self.document(uri) else {
            return false;
        };
        which(&self.settings().for_language(document.language).validate)
    }
}
