//! What the server tells the client it can do.

use lsp_types::{
    CodeActionProviderCapability, CodeLensOptions, CompletionOptions, HoverProviderCapability,
    OneOf, RenameOptions, SemanticTokensFullOptions, SemanticTokensOptions,
    SemanticTokensServerCapabilities, ServerCapabilities, SignatureHelpOptions,
    TextDocumentSyncCapability, TextDocumentSyncKind, WorkDoneProgressOptions,
};

use crate::features::completion::TRIGGER_CHARACTERS;
use crate::features::semantic_tokens::legend;
use crate::settings::{FormatterMode, SemanticTokensMode, Settings};

/// Build the capability set for a settings object.
///
/// Capabilities that a setting can switch off are advertised conditionally, so
/// a client does not send requests the server will only decline.
pub fn capabilities(settings: &Settings) -> ServerCapabilities {
    let formatting = settings.formatter.mode != FormatterMode::Off;

    ServerCapabilities {
        text_document_sync: Some(TextDocumentSyncCapability::Kind(
            TextDocumentSyncKind::INCREMENTAL,
        )),

        completion_provider: Some(CompletionOptions {
            trigger_characters: Some(
                TRIGGER_CHARACTERS.iter().map(|c| c.to_string()).collect(),
            ),
            resolve_provider: Some(false),
            ..CompletionOptions::default()
        }),
        hover_provider: Some(HoverProviderCapability::Simple(true)),
        definition_provider: Some(OneOf::Left(true)),
        references_provider: Some(OneOf::Left(true)),
        rename_provider: Some(OneOf::Right(RenameOptions {
            prepare_provider: Some(true),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        })),
        document_symbol_provider: Some(OneOf::Left(true)),
        workspace_symbol_provider: Some(OneOf::Left(true)),
        folding_range_provider: Some(lsp_types::FoldingRangeProviderCapability::Simple(true)),
        selection_range_provider: Some(lsp_types::SelectionRangeProviderCapability::Simple(
            true,
        )),
        document_link_provider: Some(lsp_types::DocumentLinkOptions {
            resolve_provider: Some(false),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),

        document_formatting_provider: formatting.then_some(OneOf::Left(true)),
        document_range_formatting_provider: formatting.then_some(OneOf::Left(true)),

        semantic_tokens_provider: (settings.semantic_tokens == SemanticTokensMode::Enable)
            .then(|| {
                SemanticTokensServerCapabilities::SemanticTokensOptions(
                    SemanticTokensOptions {
                        legend: legend(),
                        // Delta is not optional: a document produces thousands
                        // of tokens and a keystroke changes a handful.
                        full: Some(SemanticTokensFullOptions::Delta { delta: Some(true) }),
                        range: Some(false),
                        work_done_progress_options: WorkDoneProgressOptions::default(),
                    },
                )
            }),

        inlay_hint_provider: settings
            .inlay_hints
            .enabled
            .then_some(OneOf::Left(true)),
        signature_help_provider: Some(SignatureHelpOptions {
            trigger_characters: Some(vec!["(".into(), ",".into()]),
            retrigger_characters: Some(vec![",".into()]),
            work_done_progress_options: WorkDoneProgressOptions::default(),
        }),
        code_action_provider: Some(CodeActionProviderCapability::Simple(true)),
        code_lens_provider: Some(CodeLensOptions { resolve_provider: Some(false) }),

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

        settings.semantic_tokens = SemanticTokensMode::Disable;
        assert!(capabilities(&settings).semantic_tokens_provider.is_none());
    }

    #[test]
    fn the_formatter_can_be_switched_off() {
        let mut settings = Settings::default();
        assert!(capabilities(&settings).document_formatting_provider.is_some());

        settings.formatter.mode = FormatterMode::Off;
        let caps = capabilities(&settings);
        assert!(caps.document_formatting_provider.is_none());
        assert!(caps.document_range_formatting_provider.is_none());
    }

    #[test]
    fn inlay_hints_are_off_until_asked_for() {
        let mut settings = Settings::default();
        assert!(capabilities(&settings).inlay_hint_provider.is_none());

        settings.inlay_hints.enabled = true;
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
}
