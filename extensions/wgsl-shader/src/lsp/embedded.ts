// Shaders written inside Rust and TS/JS string literals — the ones the
// injection grammars in `syntaxes/` already colour.
//
// The coordinate model is the whole trick, and it is the one fast-element-ultra
// uses for `html` templates: **nothing is ever re-mapped**. `virtualDocument`
// returns a buffer the same length as the host file, with everything outside
// the shader replaced by whitespace, so a position in the virtual document is
// the same position in the host file. No offset table, no drift, nothing to get
// wrong when an edit lands between two blocks.
//
// Interpolations and escape sequences are blanked *in place* for the same
// reason: `${uniforms.scale}` becomes an identifier of exactly its own length,
// so the text after it does not move.
//
// Kept free of the `vscode` module so it can be unit tested.

/** The languages a shader can be embedded in. */
export type HostLanguage = 'rust' | 'typescript';

export type ShaderLanguage = 'wgsl' | 'glsl';

/** One tagged string literal found in a host file. */
export interface EmbeddedBlock {
  language: ShaderLanguage;
  /** Offset of the first character *inside* the literal. */
  start: number;
  /** Offset one past the last character inside the literal. */
  end: number;
  /** A raw literal has no escape sequences to blank. */
  raw: boolean;
  /** Whether `${…}` interpolation applies. */
  interpolated: boolean;
}

/**
 * The comment tag the injection grammars key off, in its spelling variants.
 *
 * Deliberately the same shape as `SHADER_TAG` in `rustHint.ts` — if the two
 * disagree, a file gets shader colouring with no language features, or the
 * reverse.
 */
const TAG = /\/\*\s*(wgsl|glsl)\s*\*\//g;

/** Every embedded shader in a host file, in source order. */
export function findEmbeddedBlocks(source: string, host: HostLanguage): EmbeddedBlock[] {
  const blocks: EmbeddedBlock[] = [];
  TAG.lastIndex = 0;

  let match: RegExpExecArray | null;
  while ((match = TAG.exec(source)) !== null) {
    const language = match[1] as ShaderLanguage;
    // Only whitespace may separate the tag from the literal it labels.
    let cursor = match.index + match[0].length;
    while (cursor < source.length && /\s/.test(source[cursor]!)) cursor += 1;

    const literal = host === 'rust' ? rustLiteral(source, cursor) : templateLiteral(source, cursor);
    if (!literal) continue;

    blocks.push({ language, ...literal });
    // Resume after the literal, so a tag inside a shader cannot open a second
    // block overlapping the first.
    TAG.lastIndex = literal.end;
  }

  return blocks;
}

type Literal = Pick<EmbeddedBlock, 'start' | 'end' | 'raw' | 'interpolated'>;

/** `r#"…"#`, `r"…"`, `b"…"` or `"…"`, starting at `at`. */
function rustLiteral(source: string, at: number): Literal | undefined {
  let cursor = at;
  if (source[cursor] === 'b') cursor += 1;

  if (source[cursor] === 'r') {
    cursor += 1;
    let hashes = 0;
    while (source[cursor + hashes] === '#') hashes += 1;
    cursor += hashes;
    if (source[cursor] !== '"') return undefined;

    const start = cursor + 1;
    const terminator = '"' + '#'.repeat(hashes);
    const end = source.indexOf(terminator, start);
    // An unterminated literal is what a half-typed one looks like; treat the
    // rest of the file as its body rather than losing the block entirely.
    return { start, end: end === -1 ? source.length : end, raw: true, interpolated: false };
  }

  if (source[cursor] !== '"') return undefined;
  const start = cursor + 1;
  return { start, end: findUnescaped(source, start, '"'), raw: false, interpolated: false };
}

/** A `` `…` `` template literal starting at `at`. */
function templateLiteral(source: string, at: number): Literal | undefined {
  if (source[at] !== '`') return undefined;
  const start = at + 1;
  return { start, end: findUnescaped(source, start, '`'), raw: false, interpolated: true };
}

/** The offset of the next unescaped `terminator`, or the end of the source. */
function findUnescaped(source: string, from: number, terminator: string): number {
  for (let i = from; i < source.length; i += 1) {
    if (source[i] === '\\') {
      i += 1;
      continue;
    }
    if (source[i] === terminator) return i;
  }
  return source.length;
}

/**
 * The host file rewritten so only `block` remains, at exactly the offsets it
 * already occupies.
 *
 * Newlines are kept everywhere so line numbers survive; every other character
 * outside the block becomes a space. Inside it, interpolations become
 * identifiers and escape sequences become spaces — both the same length as what
 * they replace.
 */
export function virtualDocument(source: string, block: EmbeddedBlock): string {
  const out: string[] = new Array(source.length);

  for (let i = 0; i < source.length; i += 1) {
    const ch = source[i]!;
    out[i] = ch === '\n' || ch === '\r' ? ch : ' ';
  }
  for (let i = block.start; i < block.end; i += 1) {
    out[i] = source[i]!;
  }

  if (!block.raw) {
    blankEscapes(source, out, block);
  }
  if (block.interpolated) {
    blankInterpolations(source, out, block);
  }

  return out.join('');
}

/**
 * `\n` and friends become whitespace of the same width.
 *
 * A `\n` in a single-line literal really is a line break in the shader, and
 * turning it into two spaces flattens the shader onto one line. That costs
 * nothing: whitespace is whitespace to both grammars, and preserving the
 * *host* file's line structure is what keeps positions aligned.
 */
function blankEscapes(source: string, out: string[], block: EmbeddedBlock): void {
  for (let i = block.start; i < block.end - 1; i += 1) {
    if (source[i] !== '\\') continue;

    let width = 2;
    const kind = source[i + 1];
    if (kind === 'x') {
      width = 4;
    } else if (kind === 'u') {
      // Rust spells it `\u{1F600}`; JS spells it `\u0041`.
      const brace = source.indexOf('}', i);
      width = source[i + 2] === '{' && brace !== -1 && brace < block.end ? brace - i + 1 : 6;
    }

    for (let j = i; j < Math.min(i + width, block.end); j += 1) out[j] = ' ';
    i += width - 1;
  }
}

/**
 * `${expr}` becomes an identifier of the same width.
 *
 * Underscores rather than spaces because an interpolation almost always stands
 * where a value goes — `vec3(${r}, 0.0, 0.0)` — and a blank there is a syntax
 * error the user cannot fix, while an identifier is merely undefined.
 */
function blankInterpolations(source: string, out: string[], block: EmbeddedBlock): void {
  for (let i = block.start; i < block.end - 1; i += 1) {
    if (source[i] !== '$' || source[i + 1] !== '{') continue;

    let depth = 0;
    let end = i;
    for (; end < block.end; end += 1) {
      if (source[end] === '{') depth += 1;
      else if (source[end] === '}') {
        depth -= 1;
        if (depth === 0) {
          end += 1;
          break;
        }
      }
    }
    for (let j = i; j < end; j += 1) {
      // Newlines are never overwritten: a multi-line interpolation must not
      // collapse the lines below it onto the wrong numbers.
      out[j] = source[j] === '\n' || source[j] === '\r' ? source[j]! : '_';
    }
    i = end - 1;
  }
}

/** The block containing `offset`, if the cursor is inside one. */
export function blockAt(blocks: EmbeddedBlock[], offset: number): EmbeddedBlock | undefined {
  return blocks.find((block) => offset >= block.start && offset <= block.end);
}

/** The host language a VS Code language id belongs to, if it can embed shaders. */
export function hostLanguage(languageId: string): HostLanguage | undefined {
  if (languageId === 'rust') return 'rust';
  if (
    languageId === 'typescript' ||
    languageId === 'typescriptreact' ||
    languageId === 'javascript' ||
    languageId === 'javascriptreact'
  ) {
    return 'typescript';
  }
  return undefined;
}
