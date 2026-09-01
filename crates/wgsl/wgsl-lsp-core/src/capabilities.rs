//! What the server tells the client it can do.
//!
//! Capabilities that a setting can switch off are advertised conditionally, so
//! a client does not send requests the server will only decline. They are not
//! per-language, though, and the settings are — so a capability is advertised
//! when *either* language wants it, and the handler declines for a document
//! whose own language does not. See [`Settings::either`].

use lsp_types::{
    CodeActionProviderCapability, CompletionOptions, FoldingRangeProviderCapability,
    HoverProviderCapability, OneOf, RenameOptions, SemanticTokensFullOptions,
    SemanticTokensOptions, SemanticTokensServerCapabilities, ServerCapabilities,
    SignatureHelpOptions, TextDocumentSyncCapability, TextDocumentSyncKind,
    WorkDoneProgressOptions,
};

use crate::features::completion::TRIGGER_CHARACTERS;
use crate::features::semantic_tokens::legend;
use crate::settings::Settings;

pub fn capabilities(settings: &Settings) -> ServerCapabilities {
    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),

        completion_provider: settings.either(|l| l.completion.enabled).then(|| {
            CompletionOptions {
                trigger_characters: Some(
                    TRIGGER_CHARACTERS.iter().map(|c| c.to_string()).collect(),
                ),
                resolve_provider: Some(false),
                ..CompletionOptions::default()
            }
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        document_highlight_provider: Some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: Some(vec![",".into()]),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),

        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(FoldingRangeProviderCapability::Simple(true)),

        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),

        document_formatting_provider: settings
            .either(|l| l.format.enable)
            .then_some(OneOf::Left(true)),
        document_range_formatting_provider: settings
            .either(|l| l.format.enable)
            .then_some(OneOf::Left(true)),

        semantic_tokens_provider: settings.either(|l| l.semantic_tokens).then(|| {
            SemanticTokensServerCapabilities::SemanticTokensOptions(SemanticTokensOptions {
                legend: legend(),
                // Delta is not optional: a shader produces thousands of tokens
                // and a keystroke changes a handful.
                full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
                range: Some(false),
                work_done_progress_options: WorkDoneProgressOptions::default(),
            })
        }),

        inlay_hint_provider: settings
            .either(|l| l.inlay_hints.enabled)
            .then_some(OneOf::Left(true)),

        ..ServerCapabilities::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semantic_tokens_can_be_switched_off() {
        let mut settings = Settings::default();
        assert!(capabilities(&settings).semantic_tokens_provider.is_some());

        settings.wgsl.semantic_tokens = false;
        // Still advertised: GLSL still wants them.
        assert!(capabilities(&settings).semantic_tokens_provider.is_some());

        settings.glsl.semantic_tokens = false;
        assert!(capabilities(&settings).semantic_tokens_provider.is_none());
    }

    #[test]
    fn the_formatter_is_off_until_asked_for() {
        let mut settings = Settings::default();
        assert!(capabilities(&settings).document_formatting_provider.is_none());

        settings.wgsl.format.enable = true;
        let caps = capabilities(&settings);
        assert!(caps.document_formatting_provider.is_some());
        assert!(caps.document_range_formatting_provider.is_some());
    }

    #[test]
    fn inlay_hints_are_off_until_asked_for() {
        let mut settings = Settings::default();
        assert!(capabilities(&settings).inlay_hint_provider.is_none());

        settings.glsl.inlay_hints.enabled = true;
        assert!(capabilities(&settings).inlay_hint_provider.is_some());
    }

    #[test]
    fn every_trigger_character_is_advertised() {
        let caps = capabilities(&Settings::default());
        let triggers = caps.completion_provider.unwrap().trigger_characters.unwrap();
        for expected in TRIGGER_CHARACTERS {
            assert!(triggers.iter().any(|c| c == expected), "missing {expected}");
        }
    }

    /// Incremental sync is what `Document::edit` implements; advertising
    /// anything else would make the two disagree.
    #[test]
    fn document_sync_is_incremental() {
        assert!(matches!(
            capabilities(&Settings::default()).text_document_sync,
            Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::INCREMENTAL))
        ));
    }
}
