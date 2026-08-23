/**
 * `css` tagged templates go through `vscode-css-languageservice`, never
 * through the Rust engine (decision 0005): validation (`no-invalid-css`),
 * completion, hover and folding, all in the substituted text so offsets map
 * straight back with `+ templateStart`.
 */

// The deep ESM path on purpose: the package's `main` is a UMD build whose
// internal relative requires survive bundling and then fail at runtime.
import {
  getCSSLanguageService,
  type LanguageService,
} from 'vscode-css-languageservice/lib/esm/cssLanguageService.js';
import { TextDocument } from 'vscode-languageserver-textdocument';

import type { ProtocolDiagnostic, VirtualDocumentFact } from './protocol.js';

let service: LanguageService | undefined;

function cssService(): LanguageService {
  service ??= getCSSLanguageService();
  return service;
}

interface ParsedCss {
  textDocument: TextDocument;
  stylesheet: unknown;
}

const parsedCache = new WeakMap<VirtualDocumentFact, ParsedCss>();

function parsed(doc: VirtualDocumentFact): ParsedCss {
  let entry = parsedCache.get(doc);
  if (!entry) {
    const textDocument = TextDocument.create('untitled://fast-template.css', 'css', 0, doc.text);
    entry = { textDocument, stylesheet: cssService().parseStylesheet(textDocument) };
    parsedCache.set(doc, entry);
  }
  return entry;
}

/** A stylesheet that is placeholders and whitespace — `css\`\${sheet}\``,
 * the typst-ultra shape — has no CSS of its own to validate. */
function hasLiteralCss(doc: VirtualDocumentFact): boolean {
  let literal = '';
  let cursor = 0;
  for (const p of [...doc.placeholders].sort((a, b) => a.start - b.start)) {
    literal += doc.text.slice(cursor, p.start);
    cursor = p.end;
  }
  literal += doc.text.slice(cursor);
  return literal.trim().length > 0;
}

function overlapsPlaceholder(doc: VirtualDocumentFact, start: number, end: number): boolean {
  // Touching counts: a zero-length "syntax expected" diagnostic right at a
  // placeholder's edge is about the substitution, not the user's CSS.
  return doc.placeholders.some((p) => start <= p.end && p.start <= end);
}

/** `no-invalid-css` over one css document; doc-relative spans. */
export function cssDiagnostics(
  doc: VirtualDocumentFact,
  severity: 'warning' | 'error' | 'suggestion' | undefined,
  lightDomComponent: string | null,
): ProtocolDiagnostic[] {
  const out: ProtocolDiagnostic[] = [];
  if (severity && hasLiteralCss(doc)) {
    const { textDocument, stylesheet } = parsed(doc);
    for (const diagnostic of cssService().doValidation(textDocument, stylesheet as never)) {
      const start = textDocument.offsetAt(diagnostic.range.start);
      const end = textDocument.offsetAt(diagnostic.range.end);
      // The substitution's underscore runs are not CSS; anything the parser
      // says about them is about the substitution, not the user's code.
      if (overlapsPlaceholder(doc, start, end)) continue;
      out.push({
        ruleId: 'no-invalid-css',
        severity,
        message: diagnostic.message,
        start,
        end,
      });
    }
  }
  // F3's CSS half: ::part is shadow-DOM plumbing.
  if (lightDomComponent) {
    const pattern = /::part\(/g;
    let match: RegExpExecArray | null;
    while ((match = pattern.exec(doc.text)) !== null) {
      out.push({
        ruleId: 'no-slot-without-shadow-root',
        severity: 'warning',
        message: `::part does nothing here: <${lightDomComponent}> renders into the light DOM (shadowOptions: null), so it exposes no parts.`,
        start: match.index,
        end: match.index + 6,
      });
    }
  }
  return out;
}

export interface CssCompletionEntry {
  name: string;
  insertText?: string;
  documentation?: string;
  kind: 'value' | 'attribute';
  replaceStart?: number;
  replaceEnd?: number;
}

export function cssCompletions(doc: VirtualDocumentFact, offset: number): CssCompletionEntry[] {
  const { textDocument, stylesheet } = parsed(doc);
  const list = cssService().doComplete(
    textDocument,
    textDocument.positionAt(offset),
    stylesheet as never,
  );
  return list.items.slice(0, 300).map((item) => {
    const textEdit = item.textEdit && 'range' in item.textEdit ? item.textEdit : undefined;
    return {
      name: item.label,
      insertText: textEdit?.newText ?? item.insertText,
      documentation:
        typeof item.documentation === 'string'
          ? item.documentation
          : item.documentation?.value,
      kind: 'value',
      replaceStart: textEdit ? textDocument.offsetAt(textEdit.range.start) : undefined,
      replaceEnd: textEdit ? textDocument.offsetAt(textEdit.range.end) : undefined,
    };
  });
}

export function cssHover(
  doc: VirtualDocumentFact,
  offset: number,
): { contents: string; start: number; end: number } | undefined {
  const { textDocument, stylesheet } = parsed(doc);
  const hover = cssService().doHover(textDocument, textDocument.positionAt(offset), stylesheet as never);
  if (!hover) return undefined;
  const contents =
    typeof hover.contents === 'string'
      ? hover.contents
      : 'value' in hover.contents
        ? hover.contents.value
        : hover.contents
            .map((c) => (typeof c === 'string' ? c : c.value))
            .join('\n\n');
  const start = hover.range ? textDocument.offsetAt(hover.range.start) : offset;
  const end = hover.range ? textDocument.offsetAt(hover.range.end) : offset;
  return { contents, start, end };
}

export function cssFoldingRanges(doc: VirtualDocumentFact): Array<{ start: number; end: number }> {
  const { textDocument } = parsed(doc);
  return cssService()
    .getFoldingRanges(textDocument)
    .map((range) => ({
      start: textDocument.offsetAt({ line: range.startLine, character: range.startCharacter ?? 0 }),
      end: textDocument.offsetAt({
        line: range.endLine,
        character: range.endCharacter ?? Number.MAX_SAFE_INTEGER,
      }),
    }));
}
