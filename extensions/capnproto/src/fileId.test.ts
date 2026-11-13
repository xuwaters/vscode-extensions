import { describe, expect, it } from 'vitest';
import { fileIdInsertion, findFileId, generateFileId } from './fileId';

const ID = '@0x9eb32e19f86ee174';

function apply(text: string): string {
  const { offset, text: insert } = fileIdInsertion(text, ID);
  return text.slice(0, offset) + insert + text.slice(offset);
}

describe('generateFileId', () => {
  it('prints 16 hex digits with the top bit set', () => {
    for (let i = 0; i < 100; i++) {
      const id = generateFileId();
      expect(id).toMatch(/^@0x[89a-f][0-9a-f]{15}$/);
    }
  });

  it('sets the top bit even when the random bytes are zero', () => {
    expect(generateFileId((n) => new Uint8Array(n))).toBe('@0x8000000000000000');
  });
});

describe('findFileId', () => {
  it('finds the id after leading comments', () => {
    const text = `# header\n\n${ID};\n\nstruct A {}\n`;
    const found = findFileId(text);
    expect(found?.id).toBe(ID);
    expect(text.slice(found!.start, found!.end)).toBe(ID);
  });

  it('ignores ordinals and ids inside comments', () => {
    expect(findFileId('struct A {\n  x @0 :Text;\n}\n')).toBeNull();
    expect(findFileId(`# ${ID};\nstruct A {}\n`)).toBeNull();
  });
});

describe('fileIdInsertion', () => {
  it('fills an empty file', () => {
    expect(apply('')).toBe(`${ID};\n`);
  });

  it('goes above the first declaration with a blank line between', () => {
    expect(apply('struct A {}\n')).toBe(`${ID};\n\nstruct A {}\n`);
    expect(apply('\n\nstruct A {}\n')).toBe(`${ID};\n\n\nstruct A {}\n`);
  });

  it('goes after a leading comment block', () => {
    expect(apply('# licence\n# more\n\nstruct A {}\n')).toBe(
      `# licence\n# more\n\n${ID};\n\nstruct A {}\n`,
    );
    expect(apply('# licence\nstruct A {}\n')).toBe(`# licence\n\n${ID};\n\nstruct A {}\n`);
  });

  it('handles a file that is only comments', () => {
    expect(apply('# just a note')).toBe(`# just a note\n\n${ID};\n`);
  });
});
