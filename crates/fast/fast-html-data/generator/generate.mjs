// Generates ../src/generated.rs from @vscode/web-custom-data's HTML data plus
// the curated SVG/MathML tables in svg-data.json (hand-written from MDN —
// web-custom-data has no SVG dataset and the corpus is full of inline SVG).
//
// The generated file is committed, so a clean cargo build does not need the
// npm packages present — the same arrangement as typst-ultra's embedded
// grammar. Re-run with `pnpm run generate:htmldata` from
// extensions/fast-element-ultra after upgrading @vscode/web-custom-data.

import { createRequire } from 'node:module';
import { writeFileSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const extensionDir = join(here, '..', '..', '..', '..', 'extensions', 'fast-element-ultra');
const require = createRequire(join(extensionDir, 'package.json'));

const htmlData = require('@vscode/web-custom-data/data/browsers.html-data.json');
const svgData = JSON.parse(readFileSync(join(here, 'svg-data.json'), 'utf8'));
const version = require('@vscode/web-custom-data/package.json').version;

/** Rust string literal. Unicode is embedded raw — Rust source is UTF-8. */
function rustStr(s) {
  const escaped = String(s ?? '')
    .replace(/\\/g, '\\\\')
    .replace(/"/g, '\\"')
    .replace(/\n/g, '\\n')
    .replace(/\r/g, '')
    .replace(/\t/g, '\\t');
  return `"${escaped}"`;
}

/** First paragraph only: hovers want a sentence, not an article. */
function describe(d) {
  const raw = typeof d === 'string' ? d : (d?.value ?? '');
  const cut = raw.indexOf('\n\n');
  return cut === -1 ? raw : raw.slice(0, cut);
}

const valueSets = new Map((htmlData.valueSets ?? []).map((v) => [v.name, v.values.map((x) => x.name)]));

/** @returns {string[]} the enumerated values for an attribute, resolved. */
function valuesOf(attr) {
  if (attr.values?.length) return attr.values.map((v) => v.name);
  if (attr.valueSet && attr.valueSet !== 'v') return valueSets.get(attr.valueSet) ?? [];
  return [];
}

function attributeRust(attr) {
  const boolean = attr.valueSet === 'v';
  const values = valuesOf(attr);
  return `A { name: ${rustStr(attr.name)}, description: ${rustStr(describe(attr.description))}, boolean: ${boolean}, values: &[${values.map(rustStr).join(', ')}] }`;
}

function elementRust(tag, ns) {
  const attrs = (tag.attributes ?? []).map(attributeRust).join(',\n        ');
  return `    ${rustStr(tag.name)} => E {
        name: ${rustStr(tag.name)},
        description: ${rustStr(describe(tag.description))},
        void: ${tag.void === true},
        namespace: Namespace::${ns},
        attributes: &[${attrs ? `\n        ${attrs},\n    ` : ''}],
    }`;
}

const events = new Map();
for (const attr of htmlData.globalAttributes ?? []) {
  if (attr.name.startsWith('on') && attr.name.length > 2) {
    events.set(attr.name.slice(2), describe(attr.description));
  }
}
for (const [name, description] of Object.entries(svgData.extraEvents)) {
  if (!events.has(name)) events.set(name, description);
}

const globalAttributes = (htmlData.globalAttributes ?? []).filter((a) => !a.name.startsWith('on'));

const htmlElements = htmlData.tags.map((t) => elementRust(t, 'Html'));
const svgElements = svgData.svgElements.map((t) => elementRust(t, 'Svg'));
const mathElements = svgData.mathElements.map((t) => elementRust(t, 'MathMl'));

const out = `//! GENERATED FILE — do not edit by hand.
//!
//! Built by generator/generate.mjs from @vscode/web-custom-data ${version}
//! (HTML tags, global attributes, value sets, events) and the crate's own
//! curated SVG/MathML tables (generator/svg-data.json). Regenerate with
//! \`pnpm run generate:htmldata\` in extensions/fast-element-ultra.
//!
//! The three namespaces get separate maps because a handful of names —
//! \`title\` for one — exist in more than one, with different meanings.

use crate::{AttributeData as A, ElementData as E, Namespace};

pub static HTML_ELEMENTS: phf::Map<&'static str, E> = phf::phf_map! {
${htmlElements.join(',\n')},
};

pub static SVG_ELEMENTS: phf::Map<&'static str, E> = phf::phf_map! {
${svgElements.join(',\n')},
};

pub static MATHML_ELEMENTS: phf::Map<&'static str, E> = phf::phf_map! {
${mathElements.join(',\n')},
};

pub static GLOBAL_ATTRIBUTES: &[A] = &[
    ${globalAttributes.map(attributeRust).join(',\n    ')},
];

pub static SVG_PRESENTATION_ATTRIBUTES: &[A] = &[
    ${svgData.svgPresentationAttributes.map(attributeRust).join(',\n    ')},
];

pub static EVENTS: &[(&str, &str)] = &[
    ${[...events.entries()]
      .sort(([a], [b]) => (a < b ? -1 : 1))
      .map(([n, d]) => `(${rustStr(n)}, ${rustStr(d)})`)
      .join(',\n    ')},
];
`;

const target = join(here, '..', 'src', 'generated.rs');
writeFileSync(target, out);
console.log(`wrote ${target}`);
console.log(
  `  ${htmlElements.length} html + ${svgElements.length} svg + ${mathElements.length} mathml elements, ` +
    `${globalAttributes.length} global attributes, ${events.size} events`,
);
