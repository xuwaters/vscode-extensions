// highlight.js grammar for Cap'n Proto schemas. highlight.js ships no Cap'n
// Proto language, so this mirrors the token model of the `capnp-analyzer`
// crate's lexer (crates/capnp-analyzer/src/lexer.rs) and the TextMate grammar
// of the sibling `capnproto` extension.
import type { HLJSApi, Language } from 'highlight.js';

const IDENT = '[A-Za-z_][A-Za-z0-9_]*';

/** `@0` field/method ordinals plus the `@0x…` file and declaration ids. */
const ORDINAL = /@(?:0[xX][0-9a-fA-F]+|\d+)/;

/** Types the language names itself; every other capitalised name is a user type. */
const BUILT_IN_TYPES = [
  'Void',
  'Bool',
  'Int8',
  'Int16',
  'Int32',
  'Int64',
  'UInt8',
  'UInt16',
  'UInt32',
  'UInt64',
  'Float32',
  'Float64',
  'Text',
  'Data',
  'List',
  'AnyPointer',
  'Capability',
];

export default function capnp(hljs: HLJSApi): Language {
  return {
    name: "Cap'n Proto",
    aliases: ['capnproto', 'capn-proto'],
    keywords: {
      keyword: [
        'struct',
        'enum',
        'interface',
        'union',
        'group',
        'const',
        'annotation',
        'using',
        'import',
        'extends',
        'stream',
      ],
      literal: ['true', 'false', 'void', 'inf', 'nan'],
    },
    contains: [
      // `#` to end of line — Cap'n Proto has no block comments.
      hljs.HASH_COMMENT_MODE,
      // `0x"62 61 72"` is one Data literal, not `0x` followed by a string.
      {
        scope: 'string',
        begin: /0[xX]"/,
        end: /"/,
        contains: [hljs.BACKSLASH_ESCAPE],
      },
      hljs.QUOTE_STRING_MODE,
      // `struct Person`, `interface Directory`, `enum Type`
      {
        begin: [/\b(?:struct|interface|enum)\b/, /\s+/, IDENT],
        beginScope: { 1: 'keyword', 3: 'title.class' },
      },
      // `using Cxx = import "/capnp/c++.capnp";`
      {
        begin: [/\busing\b/, /\s+/, IDENT],
        beginScope: { 1: 'keyword', 3: 'title.class' },
      },
      // `const pi :Float64 = …;`, `annotation name (struct) :Text;`
      {
        begin: [/\b(?:const|annotation)\b/, /\s+/, IDENT],
        beginScope: { 1: 'keyword', 3: 'title' },
      },
      // `lookup @0 (name :Text)` — a method; the `(` stays outside the match
      // so the parameter list keeps its own highlighting.
      {
        begin: [`\\b${IDENT}`, /\s*/, ORDINAL, /\s*(?=\()/],
        beginScope: { 1: 'title.function', 3: 'symbol' },
      },
      // `name @1 :Text;` / `mobile @0;` — a field, parameter or enumerant.
      {
        begin: [`\\b${IDENT}`, /\s*/, ORDINAL],
        beginScope: { 1: 'attribute', 3: 'symbol' },
      },
      // A bare `@0xdbb9ad1f14bf0b36;` file id, or an id on a declaration.
      { scope: 'symbol', match: ORDINAL },
      // `$Cxx.namespace("addressbook")` — only the annotation path; its
      // arguments are highlighted by the rules below.
      { scope: 'meta', match: new RegExp(`\\$${IDENT}(?:\\.${IDENT})*`) },
      {
        scope: 'built_in',
        match: new RegExp(`\\b(?:${BUILT_IN_TYPES.join('|')})\\b`),
      },
      { scope: 'title.class', match: /\b[A-Z][A-Za-z0-9_]*\b/, relevance: 0 },
      {
        scope: 'number',
        match:
          /\b0[xX][0-9a-fA-F]+\b|\b\d+(?:\.\d+)?(?:[eE][+-]?\d+)?\b|\B\.\d+(?:[eE][+-]?\d+)?/,
        relevance: 0,
      },
      // The method-return arrow, e.g. `-> (person :Person)`.
      { scope: 'keyword', match: /->/ },
    ],
  };
}
