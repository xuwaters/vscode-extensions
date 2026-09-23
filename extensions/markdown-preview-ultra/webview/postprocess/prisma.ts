// highlight.js grammar for Prisma schema files. highlight.js ships no Prisma
// language, so this follows the Prisma Schema Language reference: `model`,
// `view`, `type` and `enum` blocks, the `datasource`/`generator` config
// blocks, `@`/`@@` attributes and `//` / `///` comments.
import type { HLJSApi, Language, Mode } from 'highlight.js';

const IDENT = '[A-Za-z_][A-Za-z0-9_]*';

/** Declarations start a line; the leading indent is matched but unscoped. */
const LINE_START = /^[ \t]*/;

/** Types the language names itself; every other type is a model, type or enum. */
const SCALAR_TYPES = [
  'String',
  'Boolean',
  'Int',
  'BigInt',
  'Float',
  'Decimal',
  'DateTime',
  'Json',
  'Bytes',
  'Unsupported',
];

/** Functions usable in `@default(…)` and config values such as `env(…)`. */
const FUNCTIONS = [
  'autoincrement',
  'sequence',
  'dbgenerated',
  'cuid',
  'uuid',
  'ulid',
  'nanoid',
  'now',
  'auto',
  'env',
];

/** Bare enum values taken by `@relation(onDelete: …)` and `sort: …`. */
const ARGUMENT_CONSTANTS = [
  'Cascade',
  'Restrict',
  'NoAction',
  'SetNull',
  'SetDefault',
  'Asc',
  'Desc',
];

const KEYWORDS = { literal: ['true', 'false', 'null'] };

export default function prisma(hljs: HLJSApi): Language {
  // `//` and `///` (doc) comments; Prisma has no block comments.
  const COMMENT = hljs.C_LINE_COMMENT_MODE;

  // `@id`, `@db.VarChar`, `@@index` — only the attribute name; its arguments
  // are highlighted by the rules below.
  const ATTRIBUTE: Mode = {
    scope: 'meta',
    match: new RegExp(`@@?${IDENT}(?:\\.${IDENT})*`),
  };

  const VALUES: Mode[] = [
    hljs.QUOTE_STRING_MODE,
    ATTRIBUTE,
    {
      scope: 'built_in',
      match: new RegExp(`\\b(?:${FUNCTIONS.join('|')})(?=\\s*\\()`),
    },
    // `fields: [authorId]`, `onDelete: Cascade`
    { scope: 'attr', match: new RegExp(`\\b${IDENT}(?=\\s*:)`) },
    {
      scope: 'literal',
      match: new RegExp(`\\b(?:${ARGUMENT_CONSTANTS.join('|')})\\b`),
    },
    {
      scope: 'built_in',
      match: new RegExp(`\\b(?:${SCALAR_TYPES.join('|')})\\b`),
    },
    { scope: 'title.class', match: /\b[A-Z][A-Za-z0-9_]*\b/, relevance: 0 },
    { scope: 'number', match: /-?\b\d+(?:\.\d+)?\b/, relevance: 0 },
  ];

  // `email String? @unique`, `posts Post[]` — a field name and its type. Model
  // names may be lowercase (`db pull` keeps table names), so the type is taken
  // by position rather than by capitalisation.
  const FIELDS: Mode[] = [
    {
      begin: [
        LINE_START,
        IDENT,
        /[ \t]+/,
        new RegExp(`(?:${SCALAR_TYPES.join('|')})\\b`),
      ],
      beginScope: { 2: 'attribute', 4: 'built_in' },
    },
    {
      begin: [LINE_START, IDENT, /[ \t]+/, IDENT],
      beginScope: { 2: 'attribute', 4: 'title.class' },
    },
  ];

  /** `<keyword> <Name> { … }`; the `{` stays in the block body. */
  function block(keyword: string, nameScope: string, body: Mode[]): Mode {
    return {
      begin: [
        LINE_START,
        new RegExp(`(?:${keyword})\\b`),
        /[ \t]+/,
        IDENT,
        /(?=\s*\{)/,
      ],
      beginScope: { 2: 'keyword', 4: nameScope },
      end: /\}/,
      keywords: KEYWORDS,
      contains: [COMMENT, ...body],
    };
  }

  return {
    name: 'Prisma',
    keywords: KEYWORDS,
    contains: [
      COMMENT,
      block('model|view|type', 'title.class', [...FIELDS, ...VALUES]),
      block('enum', 'title.class', [
        // `ADMIN` / `USER @map("user")` — one enumerant per line.
        {
          begin: [LINE_START, IDENT],
          beginScope: { 2: 'variable.constant' },
        },
        ...VALUES,
      ]),
      block('datasource|generator', 'title', [
        // `provider = "postgresql"`
        {
          begin: [LINE_START, IDENT, /(?=\s*=)/],
          beginScope: { 2: 'attr' },
        },
        ...VALUES,
      ]),
      // A fence may hold a bare excerpt of fields with no enclosing block.
      ...FIELDS,
      ...VALUES,
    ],
  };
}
