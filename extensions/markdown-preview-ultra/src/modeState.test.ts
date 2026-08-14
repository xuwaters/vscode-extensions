import { describe, expect, it } from 'vitest';
import {
  resolveMode,
  shouldHandOffToSource,
  shouldPromoteToPreview,
  toggleEditPreview,
} from './modeState';

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

describe('shouldHandOffToSource', () => {
  /** Source in column 1, preview panel beside it in column 2. */
  const split = { hasPanel: true, panelColumn: 2, sourceColumn: 1 };

  it('claims the source column of a split for the text editor', () => {
    expect(shouldHandOffToSource(split, 1)).toBe(true);
  });

  it('leaves a tab that opened in another column alone', () => {
    expect(shouldHandOffToSource(split, 2)).toBe(false);
    expect(shouldHandOffToSource(split, 3)).toBe(false);
  });

  it('claims it too when the panel has yet to report a column of its own', () => {
    expect(shouldHandOffToSource({ hasPanel: true, sourceColumn: 1 }, 1)).toBe(
      true,
    );
  });

  it('leaves the tab alone with no panel open — Preview mode opened it', () => {
    expect(shouldHandOffToSource({ hasPanel: false }, 1)).toBe(false);
  });

  it('leaves the tab alone when the panel holds the source column', () => {
    expect(
      shouldHandOffToSource(
        { hasPanel: true, panelColumn: 1, sourceColumn: 1 },
        1,
      ),
    ).toBe(false);
  });
});

describe('shouldPromoteToPreview', () => {
  /** Nothing else on screen: the reader just opened a file. */
  const alone = { hasPanel: false };
  /** Source in column 1, preview panel beside it in column 2. */
  const split = { hasPanel: true, panelColumn: 2, sourceColumn: 1 };
  const opened = { column: 1, hasSelection: false };

  it('sends a plain open on to the preview', () => {
    expect(shouldPromoteToPreview(alone, opened, true)).toBe(true);
  });

  it('leaves a file opened at a search match on the source', () => {
    expect(
      shouldPromoteToPreview(alone, { column: 1, hasSelection: true }, true),
    ).toBe(false);
  });

  it('leaves the source column of a split alone', () => {
    expect(shouldPromoteToPreview(split, opened, true)).toBe(false);
  });

  it('still sends a file opened elsewhere than the split on', () => {
    expect(shouldPromoteToPreview(split, { ...opened, column: 3 }, true)).toBe(
      true,
    );
  });

  it('sends nothing on when the reader has turned it off', () => {
    expect(shouldPromoteToPreview(alone, opened, false)).toBe(false);
  });

  it('never contradicts the claim the source column makes', () => {
    for (const column of [1, 2, 3]) {
      const promote = shouldPromoteToPreview(split, { ...opened, column }, true);
      const handOff = shouldHandOffToSource(split, column);
      expect(promote && handOff).toBe(false);
    }
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
