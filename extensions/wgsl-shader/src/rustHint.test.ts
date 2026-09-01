import { describe, expect, it } from 'vitest';
import {
  HINT_ACTIONS,
  hintActions,
  shouldOfferStringTokenFix,
  type RustHintState,
} from './rustHint';

const TAGGED = 'const S: &str = /* wgsl */ r#"\n  fn main() {}\n"#;';

function state(overrides: Partial<RustHintState> = {}): RustHintState {
  return {
    languageId: 'rust',
    text: TAGGED,
    hasRustAnalyzer: true,
    stringTokensEnabled: true,
    hintEnabled: true,
    ...overrides,
  };
}

describe('rust-analyzer string token hint', () => {
  it('offers the fix for a Rust file that embeds WGSL', () => {
    expect(shouldOfferStringTokenFix(state())).toBe(true);
  });

  it('accepts the tag spelling variations the grammar accepts', () => {
    for (const tag of ['/*wgsl*/', '/*  wgsl  */', '/* wgsl */', '/*glsl*/', '/* glsl */']) {
      expect(shouldOfferStringTokenFix(state({ text: `let s = ${tag} r#""#;` })), tag).toBe(true);
    }
  });

  it('stays quiet for Rust files with no embedded shader', () => {
    expect(shouldOfferStringTokenFix(state({ text: 'fn main() { println!("hi"); }' }))).toBe(false);
  });

  it('stays quiet when rust-analyzer is not installed', () => {
    expect(shouldOfferStringTokenFix(state({ hasRustAnalyzer: false }))).toBe(false);
  });

  it('stays quiet once the setting is already off', () => {
    expect(shouldOfferStringTokenFix(state({ stringTokensEnabled: false }))).toBe(false);
  });

  it('stays quiet when the user dismissed the hint', () => {
    expect(shouldOfferStringTokenFix(state({ hintEnabled: false }))).toBe(false);
  });

  it('ignores non-Rust documents that happen to carry the tag', () => {
    expect(shouldOfferStringTokenFix(state({ languageId: 'typescript' }))).toBe(false);
  });

  it('offers the workspace first when there is one to scope the change to', () => {
    expect(hintActions(true)[0]).toBe(HINT_ACTIONS.workspace);
    expect(hintActions(true)).toContain(HINT_ACTIONS.global);
  });

  it('offers only the global switch for a loose file', () => {
    expect(hintActions(false)).not.toContain(HINT_ACTIONS.workspace);
    expect(hintActions(false)[0]).toBe(HINT_ACTIONS.global);
  });
});
