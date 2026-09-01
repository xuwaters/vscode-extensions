// rust-analyzer paints whole string literals with a `string` semantic token,
// and semantic tokens win over TextMate scopes — so the shader colouring this
// extension injects into tagged Rust strings stays invisible until
// `rust-analyzer.semanticHighlighting.strings.enable` is turned off.
//
// These rules are kept free of the `vscode` module so they can be unit tested.

export const RUST_ANALYZER_EXTENSION_ID = 'rust-lang.rust-analyzer';
export const RUST_STRING_TOKENS_SETTING = 'rust-analyzer.semanticHighlighting.strings.enable';
export const HINT_SETTING = 'rust.highlightHint';

// The block-comment tags the injection grammars key off, in their spelling
// variants: slash-star, optional spaces, `wgsl` or `glsl`, optional spaces,
// star-slash.
const SHADER_TAG = /\/\*\s*(?:wgsl|glsl)\s*\*\//;

export interface RustHintState {
  languageId: string;
  /** Document text; only the tag matters, so a prefix is enough. */
  text: string;
  hasRustAnalyzer: boolean;
  /** Effective value of `RUST_STRING_TOKENS_SETTING`. */
  stringTokensEnabled: boolean;
  /** Effective value of `wgsl.rust.highlightHint`. */
  hintEnabled: boolean;
}

/**
 * Whether to offer turning off rust-analyzer's string tokens. Only worth asking
 * when the file actually embeds a shader and the setting is the thing hiding it.
 */
export function shouldOfferStringTokenFix(state: RustHintState): boolean {
  return (
    state.hintEnabled &&
    state.hasRustAnalyzer &&
    state.stringTokensEnabled &&
    state.languageId === 'rust' &&
    SHADER_TAG.test(state.text)
  );
}

export const HINT_ACTIONS = {
  workspace: 'Turn Off in This Workspace',
  global: 'Turn Off Everywhere',
  never: "Don't Show Again",
} as const;

export type HintAction = (typeof HINT_ACTIONS)[keyof typeof HINT_ACTIONS];

/**
 * The setting has no per-string granularity — a workspace folder is as narrow as
 * it gets — so prefer the workspace when there is one and keep the global switch
 * as the second choice.
 */
export function hintActions(hasWorkspace: boolean): HintAction[] {
  return hasWorkspace
    ? [HINT_ACTIONS.workspace, HINT_ACTIONS.global, HINT_ACTIONS.never]
    : [HINT_ACTIONS.global, HINT_ACTIONS.never];
}
