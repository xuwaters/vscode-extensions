import { describe, expect, it } from 'vitest';
import { resolveMode } from './modeState';

describe('resolveMode', () => {
  it('is edit with nothing open', () => {
    expect(
      resolveMode({ previewEditorActive: false, hasPanel: false }),
    ).toBe('edit');
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
      resolveMode({
        previewEditorActive: false,
        hasPanel: true,
        sourceColumn: 1,
      }),
    ).toBe('split');
  });

  it('is preview for a preview-editor tab', () => {
    expect(
      resolveMode({ previewEditorActive: true, hasPanel: false }),
    ).toBe('preview');
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
