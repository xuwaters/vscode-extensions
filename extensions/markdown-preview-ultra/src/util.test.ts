import { describe, expect, it, vi } from 'vitest';
import { isMarkdownPath } from './util';

// `util` pulls in the VSCode namespace for its editor lookup; the path test
// below touches none of it.
vi.mock('vscode', () => ({}));

describe('isMarkdownPath', () => {
  it.each(['notes.md', 'notes.markdown', 'notes.mdx', 'chat.copilotmd'])(
    'claims %s',
    (path) => {
      expect(isMarkdownPath(path)).toBe(true);
    },
  );

  it('is case-insensitive', () => {
    expect(isMarkdownPath('/docs/README.MD')).toBe(true);
  });

  it.each(['notes.mdown', 'notes.md.txt', 'copilotmd'])(
    'leaves %s alone',
    (path) => {
      expect(isMarkdownPath(path)).toBe(false);
    },
  );
});
