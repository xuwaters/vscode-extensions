import { describe, expect, it } from 'vitest';
import { namesTypstSource } from './commandTarget.js';

describe('the URI a command was invoked with', () => {
  it('accepts a typst file', () => {
    expect(namesTypstSource({ path: '/w/paper.typ' })).toBe(true);
    expect(namesTypstSource({ path: '/w/lib.typc' })).toBe(true);
    expect(namesTypstSource({ path: '/w/Paper.TYP' })).toBe(true);
    expect(namesTypstSource({ path: '/w/a b/dotted.name.typ' })).toBe(true);
  });

  // The one that mattered: in split mode the preview panel is the active tab,
  // so its title-bar Export arrives with the webview's own resource.
  it('rejects the preview webview', () => {
    expect(
      namesTypstSource({
        path: 'webview-panel/webview-typstUltra.preview-78f17c48-dee4-48c1-87e9-4dd89af70d62',
      }),
    ).toBe(false);
  });

  // The language server's command arguments are JSON, so a code lens hands over
  // the URI as a string rather than as a `Uri`.
  it('accepts the string a code lens sends', () => {
    expect(namesTypstSource('file:///w/paper.typ')).toBe(true);
    expect(namesTypstSource('file:///w/lib.typc')).toBe(true);
    expect(namesTypstSource('file:///w/notes.md')).toBe(false);
  });

  // A bibliography is a project file the server speaks BibTeX for, but it is
  // not a document: `Client.mainPath` asks this before making the file that
  // started the server the compile root.
  it('rejects a bibliography', () => {
    expect(namesTypstSource({ path: '/w/refs.bib' })).toBe(false);
    expect(namesTypstSource('file:///w/refs.bib')).toBe(false);
  });

  it('rejects anything else that is not a document', () => {
    expect(namesTypstSource({ path: '/w/paper.pdf' })).toBe(false);
    expect(namesTypstSource({ path: '/w/typ' })).toBe(false);
    expect(namesTypstSource({ path: '/w/paper.typ.bak' })).toBe(false);
    expect(namesTypstSource({ path: '' })).toBe(false);
    expect(namesTypstSource(undefined)).toBe(false);
    expect(namesTypstSource(null)).toBe(false);
    expect(namesTypstSource({})).toBe(false);
    expect(namesTypstSource(42)).toBe(false);
  });
});
