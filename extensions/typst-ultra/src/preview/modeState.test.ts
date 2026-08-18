import { describe, expect, it } from 'vitest';
import { CYCLE, resolveMode, toggleEditPreview } from './modeState.js';

describe('resolveMode', () => {
  it('is edit with nothing open', () => {
    expect(resolveMode({ previewEditorActive: false, hasPanel: false })).toBe(
      'edit',
    );
  });

  it('is preview when the panel holds the source column', () => {
    expect(
      resolveMode({
        previewEditorActive: false,
        hasPanel: true,
        panelColumn: 1,
        sourceColumn: 1,
      }),
    ).toBe('preview');
  });

  it('is split when the panel sits in another column', () => {
    expect(
      resolveMode({
        previewEditorActive: false,
        hasPanel: true,
        panelColumn: 2,
        sourceColumn: 1,
      }),
    ).toBe('split');
  });

  it('is split when the panel column is not yet known', () => {
    expect(
      resolveMode({ previewEditorActive: false, hasPanel: true, sourceColumn: 1 }),
    ).toBe('split');
  });

  it('is preview for a preview-editor tab', () => {
    expect(resolveMode({ previewEditorActive: true, hasPanel: false })).toBe(
      'preview',
    );
  });

  it('lets the preview-editor tab win over a panel open elsewhere', () => {
    expect(
      resolveMode({
        previewEditorActive: true,
        hasPanel: true,
        panelColumn: 2,
        sourceColumn: 1,
      }),
    ).toBe('preview');
  });
});

describe('toggleEditPreview', () => {
  it('shows the preview from the editor', () => {
    expect(toggleEditPreview('edit')).toBe('preview');
  });

  it('shows the editor from the preview', () => {
    expect(toggleEditPreview('preview')).toBe('edit');
  });

  it('leaves split for the preview, so the key still means "the other one"', () => {
    expect(toggleEditPreview('split')).toBe('preview');
  });

  it('returns to where it started when pressed twice', () => {
    for (const mode of ['edit', 'preview'] as const) {
      expect(toggleEditPreview(toggleEditPreview(mode))).toBe(mode);
    }
  });
});

describe('the cycle', () => {
  it('visits all three modes and comes back', () => {
    let mode: 'edit' | 'split' | 'preview' = 'edit';
    const seen: string[] = [mode];
    for (let step = 0; step < 3; step += 1) {
      mode = CYCLE[mode];
      seen.push(mode);
    }
    expect(seen).toEqual(['edit', 'split', 'preview', 'edit']);
  });
});
