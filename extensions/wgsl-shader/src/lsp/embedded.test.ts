// The virtual-document coordinate model, which is the whole of the embedded
// feature: `sourceOffset === virtualOffset`, with no mapping table anywhere.
// Length preservation is what makes that true, so it is tested as a property
// over generated inputs rather than only as examples.

import { describe, expect, it } from 'vitest';

import {
  blockAt,
  findEmbeddedBlocks,
  hostLanguage,
  virtualDocument,
  type EmbeddedBlock,
} from './embedded.js';

const RUST = `
fn shader() -> &'static str {
    /* wgsl */ r#"
@fragment
fn fs_main() -> @location(0) vec4f {
    return vec4f(1.0, 0.0, 0.0, 1.0);
}
"#
}
`;

const TS = [
  'const frag = /* glsl */ `',
  '#version 450',
  'layout(location = 0) out vec4 colour;',
  'void main() { colour = vec4(${red}, 0.0, 0.0, 1.0); }',
  '`;',
].join('\n');

describe('findEmbeddedBlocks', () => {
  it('finds a tagged Rust raw string and marks it raw', () => {
    const blocks = findEmbeddedBlocks(RUST, 'rust');
    expect(blocks).toHaveLength(1);
    expect(blocks[0]!.language).toBe('wgsl');
    expect(blocks[0]!.raw).toBe(true);
    expect(blocks[0]!.interpolated).toBe(false);
    expect(RUST.slice(blocks[0]!.start, blocks[0]!.end)).toContain('@fragment');
    // The literal ends before its own terminator.
    expect(RUST.slice(blocks[0]!.start, blocks[0]!.end)).not.toContain('"#');
  });

  it('finds a tagged template literal and marks it interpolated', () => {
    const blocks = findEmbeddedBlocks(TS, 'typescript');
    expect(blocks).toHaveLength(1);
    expect(blocks[0]!.language).toBe('glsl');
    expect(blocks[0]!.raw).toBe(false);
    expect(blocks[0]!.interpolated).toBe(true);
  });

  it.each([
    ['a plain string', 'let s = /* wgsl */ "fn f() {}";', 'fn f() {}'],
    ['a byte string', 'let s = /* wgsl */ b"fn f() {}";', 'fn f() {}'],
    ['a raw string with no hashes', 'let s = /* wgsl */ r"fn f() {}";', 'fn f() {}'],
    ['a raw string with hashes', 'let s = /* wgsl */ r##"fn f() {}"##;', 'fn f() {}'],
  ])('reads %s', (_name, source, expected) => {
    const [block] = findEmbeddedBlocks(source, 'rust');
    expect(source.slice(block!.start, block!.end)).toBe(expected);
  });

  it('accepts every spelling of the tag the grammars accept', () => {
    for (const tag of ['/*wgsl*/', '/* wgsl */', '/*  wgsl  */', '/*\twgsl\t*/']) {
      expect(findEmbeddedBlocks(`let s = ${tag} "x";`, 'rust')).toHaveLength(1);
    }
  });

  it('ignores a tag that labels something other than a string', () => {
    expect(findEmbeddedBlocks('let n = /* wgsl */ 42;', 'rust')).toHaveLength(0);
    // A Rust literal is not a template literal.
    expect(findEmbeddedBlocks('let s = /* wgsl */ "x";', 'typescript')).toHaveLength(0);
  });

  it('finds several blocks in one file without them overlapping', () => {
    const source = 'const a = /* wgsl */ `one`;\nconst b = /* glsl */ `two`;\n';
    const blocks = findEmbeddedBlocks(source, 'typescript');
    expect(blocks.map((block) => source.slice(block.start, block.end))).toEqual(['one', 'two']);
  });

  /// An unterminated literal is what a half-typed one looks like.
  it('treats an unterminated literal as running to the end of the file', () => {
    const source = 'const a = /* wgsl */ `fn main() {';
    const [block] = findEmbeddedBlocks(source, 'typescript');
    expect(block!.end).toBe(source.length);
  });
});

describe('virtualDocument', () => {
  it('preserves every offset in the file', () => {
    const [block] = findEmbeddedBlocks(RUST, 'rust');
    const virtual = virtualDocument(RUST, block!);

    expect(virtual).toHaveLength(RUST.length);
    // The shader itself is untouched…
    expect(virtual.slice(block!.start, block!.end)).toBe(RUST.slice(block!.start, block!.end));
    // …and everything else is gone.
    expect(virtual.slice(0, block!.start).trim()).toBe('');
    expect(virtual.slice(block!.end).trim()).toBe('');
  });

  it('keeps line numbers aligned with the host file', () => {
    const [block] = findEmbeddedBlocks(RUST, 'rust');
    const virtual = virtualDocument(RUST, block!);
    const lineOf = (text: string, needle: string) =>
      text.slice(0, text.indexOf(needle)).split('\n').length;
    expect(lineOf(virtual, '@fragment')).toBe(lineOf(RUST, '@fragment'));
    expect(lineOf(virtual, 'return vec4f')).toBe(lineOf(RUST, 'return vec4f'));
  });

  it('replaces an interpolation with an identifier of the same width', () => {
    const [block] = findEmbeddedBlocks(TS, 'typescript');
    const virtual = virtualDocument(TS, block!);
    expect(virtual).toHaveLength(TS.length);
    expect(virtual).toContain('vec4(______, 0.0, 0.0, 1.0)');
    // `${red}` is six characters, and so is what replaced it.
    expect(virtual.indexOf('0.0, 0.0, 1.0')).toBe(TS.indexOf('0.0, 0.0, 1.0'));
  });

  it('blanks escape sequences without moving what follows', () => {
    const source = 'let s = /* wgsl */ "fn f() {}\\nfn g() {}";';
    const [block] = findEmbeddedBlocks(source, 'rust');
    const virtual = virtualDocument(source, block!);
    expect(virtual).toHaveLength(source.length);
    expect(virtual).toContain('fn f() {}  fn g() {}');
  });

  it('leaves a raw string alone, backslashes included', () => {
    const source = 'let s = /* wgsl */ r#"a \\n b"#;';
    const [block] = findEmbeddedBlocks(source, 'rust');
    expect(virtualDocument(source, block!)).toContain('a \\n b');
  });

  /// Two blocks must not be spliced into one shader, or the second one's
  /// declarations resolve against the first one's scope.
  it('shows one block at a time', () => {
    const source = 'const a = /* wgsl */ `first`;\nconst b = /* wgsl */ `second`;\n';
    const blocks = findEmbeddedBlocks(source, 'typescript');
    const first = virtualDocument(source, blocks[0]!);
    expect(first).toContain('first');
    expect(first).not.toContain('second');
  });
});

// ── The property the whole model rests on ──────────────────────────────────

/** Deterministic pseudo-random generator: tests must not flake. */
function mulberry32(seed: number): () => number {
  let a = seed;
  return () => {
    a |= 0;
    a = (a + 0x6d2b79f5) | 0;
    let t = Math.imul(a ^ (a >>> 15), 1 | a);
    t = (t + Math.imul(t ^ (t >>> 7), 61 | t)) ^ t;
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function generate(seed: number): string {
  const random = mulberry32(seed);
  const pieces = [
    'fn main() {\n',
    '  let x = 1.0;\n',
    'vec3(1.0, 0.0, 0.0)',
    '  // a comment with ✕ and ⌘\n',
    '${uniforms.scale}',
    '${a ? `${b}` : c}',
    '\\n',
    '\\u{1F600}',
    '\\t',
    '  return;\n',
  ];

  let body = '';
  const count = 3 + Math.floor(random() * 12);
  for (let i = 0; i < count; i += 1) {
    body += pieces[Math.floor(random() * pieces.length)];
  }
  return `const before = 1;\nconst shader = /* wgsl */ \`${body}\`;\nconst after = 2;\n`;
}

describe('the offset model', () => {
  it('never moves a character, for any generated template', () => {
    for (let seed = 0; seed < 300; seed += 1) {
      const source = generate(seed);
      const blocks = findEmbeddedBlocks(source, 'typescript');
      expect(blocks, `seed ${seed}`).toHaveLength(1);

      const block = blocks[0]!;
      const virtual = virtualDocument(source, block);

      expect(virtual, `seed ${seed}`).toHaveLength(source.length);
      // Line structure is identical, which is what makes a Position in one
      // document mean the same thing in the other.
      expect(virtual.split('\n').length, `seed ${seed}`).toBe(source.split('\n').length);
      for (let i = 0; i < source.length; i += 1) {
        if (source[i] === '\n') {
          expect(virtual[i], `seed ${seed} offset ${i}`).toBe('\n');
        }
      }
      // Nothing outside the block survives.
      expect(virtual.slice(0, block.start).trim(), `seed ${seed}`).toBe('');
      expect(virtual.slice(block.end).trim(), `seed ${seed}`).toBe('');
    }
  });
});

describe('blockAt', () => {
  it('finds the block a cursor is in, edges included', () => {
    const blocks: EmbeddedBlock[] = [
      { language: 'wgsl', start: 10, end: 20, raw: true, interpolated: false },
    ];
    expect(blockAt(blocks, 9)).toBeUndefined();
    expect(blockAt(blocks, 10)).toBe(blocks[0]);
    // The trailing edge counts: that is where the cursor sits after typing the
    // last character of the shader.
    expect(blockAt(blocks, 20)).toBe(blocks[0]);
    expect(blockAt(blocks, 21)).toBeUndefined();
  });
});

describe('hostLanguage', () => {
  it('covers exactly the languages the injection grammars target', () => {
    expect(hostLanguage('rust')).toBe('rust');
    for (const id of ['typescript', 'typescriptreact', 'javascript', 'javascriptreact']) {
      expect(hostLanguage(id)).toBe('typescript');
    }
    expect(hostLanguage('python')).toBeUndefined();
    expect(hostLanguage('wgsl')).toBeUndefined();
  });
});
