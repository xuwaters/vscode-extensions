import * as vscode from 'vscode';
import { ASKAMA_LANGUAGES } from './languages';

const KEYWORD_RE = /\b(if|else|elif|endif|for|endfor|in|match|when|endwhen|endmatch|block|endblock|extends|include|import|macro|endmacro|call|endcall|filter|endfilter|let|set|mut|decl|declare|endlet|continue|break|raw|endraw)\b/g;
const FILTER_CALL_RE = /\|\s*\w+/g;
const FUNC_CALL_RE = /\b\w+(?:\.\w+)*(?=\s*\()/g;
const NAMED_ARG_RE = /(\w+)=(?!=)/g;

const STATEMENT_RE = /(\{%[-+~]?)([\s\S]*?)([-+~]?%\})/g;
const EXPRESSION_RE = /(\{\{[-+~]?)([\s\S]*?)([-+~]?\}\})/g;
const COMMENT_RE = /(\{#[-+~]?)([\s\S]*?)([-+~]?#\})/g;
const RAW_BLOCK_RE = /(\{%[-+~]?\s*raw\s*[-+~]?%\})([\s\S]*?)(\{%[-+~]?\s*endraw\s*[-+~]?%\})/g;

interface Span {
  start: number;
  end: number;
}

let delimiterDeco: vscode.TextEditorDecorationType;
let keywordDeco: vscode.TextEditorDecorationType;
let contentDeco: vscode.TextEditorDecorationType;
let commentDeco: vscode.TextEditorDecorationType;
let filterDeco: vscode.TextEditorDecorationType;
let functionDeco: vscode.TextEditorDecorationType;
let namedArgDeco: vscode.TextEditorDecorationType;

export function activate(context: vscode.ExtensionContext): void {
  delimiterDeco = vscode.window.createTextEditorDecorationType({
    color: new vscode.ThemeColor('askamaTemplates.delimiterForeground'),
    fontWeight: 'bold',
  });
  keywordDeco = vscode.window.createTextEditorDecorationType({
    color: new vscode.ThemeColor('askamaTemplates.keywordForeground'),
    fontWeight: 'bold',
  });
  contentDeco = vscode.window.createTextEditorDecorationType({
    color: new vscode.ThemeColor('askamaTemplates.contentForeground'),
  });
  commentDeco = vscode.window.createTextEditorDecorationType({
    color: new vscode.ThemeColor('askamaTemplates.commentForeground'),
    fontStyle: 'italic',
  });
  filterDeco = vscode.window.createTextEditorDecorationType({
    color: new vscode.ThemeColor('askamaTemplates.filterForeground'),
  });
  functionDeco = vscode.window.createTextEditorDecorationType({
    color: new vscode.ThemeColor('askamaTemplates.functionForeground'),
  });
  namedArgDeco = vscode.window.createTextEditorDecorationType({
    color: new vscode.ThemeColor('askamaTemplates.namedArgForeground'),
  });

  context.subscriptions.push(delimiterDeco, keywordDeco, contentDeco, commentDeco, filterDeco, functionDeco, namedArgDeco);

  let timeout: ReturnType<typeof setTimeout> | undefined;

  function scheduleUpdate(editor: vscode.TextEditor): void {
    if (timeout) clearTimeout(timeout);
    timeout = setTimeout(() => updateDecorations(editor), 50);
  }

  if (vscode.window.activeTextEditor) {
    scheduleUpdate(vscode.window.activeTextEditor);
  }

  context.subscriptions.push(
    vscode.window.onDidChangeActiveTextEditor(editor => {
      if (editor) scheduleUpdate(editor);
    }),
    vscode.workspace.onDidChangeTextDocument(event => {
      const editor = vscode.window.activeTextEditor;
      if (editor && editor.document === event.document) {
        scheduleUpdate(editor);
      }
    }),
  );
}

export function deactivate(): void {}

function updateDecorations(editor: vscode.TextEditor): void {
  if (!ASKAMA_LANGUAGES.has(editor.document.languageId)) {
    editor.setDecorations(delimiterDeco, []);
    editor.setDecorations(keywordDeco, []);
    editor.setDecorations(contentDeco, []);
    editor.setDecorations(commentDeco, []);
    editor.setDecorations(filterDeco, []);
    editor.setDecorations(functionDeco, []);
    editor.setDecorations(namedArgDeco, []);
    return;
  }

  const doc = editor.document;
  const text = doc.getText();

  const delimiters: vscode.Range[] = [];
  const keywords: vscode.Range[] = [];
  const contents: vscode.Range[] = [];
  const comments: vscode.Range[] = [];
  const filters: vscode.Range[] = [];
  const functions: vscode.Range[] = [];
  const namedArgs: vscode.Range[] = [];

  // Find raw block content spans (to exclude from decoration)
  const rawSpans = findRawBlockContentSpans(text);

  // Scan comments (excluded from statement/expression scanning)
  const commentSpans: Span[] = [];
  scanComments(text, doc, comments, commentSpans);

  // Scan statements (excluded from expression scanning)
  const excludeFromStatements = [...commentSpans, ...rawSpans];
  const statementSpans: Span[] = [];
  scanBlocks(STATEMENT_RE, text, doc, delimiters, keywords, contents, filters, functions, namedArgs, excludeFromStatements, true, statementSpans);

  // Scan expressions (excluded by comments, raw blocks, and statements)
  const excludeFromExpressions = [...excludeFromStatements, ...statementSpans];
  scanBlocks(EXPRESSION_RE, text, doc, delimiters, keywords, contents, filters, functions, namedArgs, excludeFromExpressions, false);

  editor.setDecorations(delimiterDeco, delimiters);
  editor.setDecorations(keywordDeco, keywords);
  editor.setDecorations(contentDeco, contents);
  editor.setDecorations(commentDeco, comments);
  editor.setDecorations(filterDeco, filters);
  editor.setDecorations(functionDeco, functions);
  editor.setDecorations(namedArgDeco, namedArgs);
}

function findRawBlockContentSpans(text: string): Span[] {
  const spans: Span[] = [];
  RAW_BLOCK_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = RAW_BLOCK_RE.exec(text)) !== null) {
    const openTagEnd = m.index + m[1].length;
    const contentEnd = openTagEnd + m[2].length;
    spans.push({ start: openTagEnd, end: contentEnd });
  }
  return spans;
}

function scanComments(
  text: string,
  doc: vscode.TextDocument,
  comments: vscode.Range[],
  spans: Span[],
): void {
  COMMENT_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = COMMENT_RE.exec(text)) !== null) {
    const start = m.index;
    const openLen = m[1].length;
    const innerLen = m[2].length;
    const closeLen = m[3].length;

    const openEnd = start + openLen;
    const contentEnd = openEnd + innerLen;
    const fullEnd = contentEnd + closeLen;

    spans.push({ start, end: fullEnd });
    comments.push(toRange(doc, start, openEnd));
    comments.push(toRange(doc, contentEnd, fullEnd));
    if (innerLen > 0) {
      comments.push(toRange(doc, openEnd, contentEnd));
    }
  }
}

function scanBlocks(
  pattern: RegExp,
  text: string,
  doc: vscode.TextDocument,
  delimiters: vscode.Range[],
  keywords: vscode.Range[],
  contents: vscode.Range[],
  filters: vscode.Range[],
  functions: vscode.Range[],
  namedArgs: vscode.Range[],
  excludeSpans: Span[],
  findKeywords: boolean,
  outSpans?: Span[],
): void {
  pattern.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = pattern.exec(text)) !== null) {
    const start = m.index;
    const openLen = m[1].length;
    const inner = m[2];
    const closeLen = m[3].length;

    const openEnd = start + openLen;
    const contentStart = openEnd;
    const contentEnd = contentStart + inner.length;
    const fullEnd = contentEnd + closeLen;

    if (isInsideSpan(start, fullEnd, excludeSpans)) continue;

    outSpans?.push({ start, end: fullEnd });

    delimiters.push(toRange(doc, start, openEnd));
    delimiters.push(toRange(doc, contentEnd, fullEnd));

    if (findKeywords) {
      const kwSpans: Span[] = [];
      KEYWORD_RE.lastIndex = 0;
      let kw: RegExpExecArray | null;
      while ((kw = KEYWORD_RE.exec(inner)) !== null) {
        const kwStart = contentStart + kw.index;
        const kwEnd = kwStart + kw[0].length;
        keywords.push(toRange(doc, kwStart, kwEnd));
        kwSpans.push({ start: kwStart, end: kwEnd });
      }
      addRemainder(contentStart, contentEnd, kwSpans, text, doc, contents);
    } else if (inner.trim().length > 0) {
      scanExpressionContent(contentStart, contentEnd, text, doc, contents, filters, functions, namedArgs);
    }
  }
}

function scanExpressionContent(
  start: number,
  end: number,
  text: string,
  doc: vscode.TextDocument,
  contents: vscode.Range[],
  filters: vscode.Range[],
  functions: vscode.Range[],
  namedArgs: vscode.Range[],
): void {
  const slice = text.slice(start, end);
  const exclusions: Span[] = [];

  // 1. Find filters: | filter_name
  FILTER_CALL_RE.lastIndex = 0;
  let m: RegExpExecArray | null;
  while ((m = FILTER_CALL_RE.exec(slice)) !== null) {
    const fStart = start + m.index;
    const fEnd = fStart + m[0].length;
    filters.push(toRange(doc, fStart, fEnd));
    exclusions.push({ start: fStart, end: fEnd });
  }

  // 2. Find function calls: name( or name.name(
  FUNC_CALL_RE.lastIndex = 0;
  while ((m = FUNC_CALL_RE.exec(slice)) !== null) {
    const fStart = start + m.index;
    const fEnd = fStart + m[0].length;
    if (!isInsideSpan(fStart, fEnd, exclusions)) {
      functions.push(toRange(doc, fStart, fEnd));
      exclusions.push({ start: fStart, end: fEnd });
    }
  }

  // 3. Find named arg keys: key= (but not ==)
  NAMED_ARG_RE.lastIndex = 0;
  while ((m = NAMED_ARG_RE.exec(slice)) !== null) {
    const keyStart = start + m.index;
    const keyEnd = keyStart + m[1].length;
    if (!isInsideSpan(keyStart, keyEnd, exclusions)) {
      namedArgs.push(toRange(doc, keyStart, keyEnd));
      exclusions.push({ start: keyStart, end: keyEnd });
    }
  }

  // 4. Remainder as content
  addRemainder(start, end, exclusions, text, doc, contents);
}

function addRemainder(
  start: number,
  end: number,
  exclusions: Span[],
  text: string,
  doc: vscode.TextDocument,
  out: vscode.Range[],
): void {
  const sorted = exclusions.sort((a, b) => a.start - b.start);
  let cursor = start;
  for (const ex of sorted) {
    if (ex.start > cursor && text.slice(cursor, ex.start).trim().length > 0) {
      out.push(toRange(doc, cursor, ex.start));
    }
    cursor = Math.max(cursor, ex.end);
  }
  if (cursor < end && text.slice(cursor, end).trim().length > 0) {
    out.push(toRange(doc, cursor, end));
  }
}

function isInsideSpan(start: number, end: number, spans: Span[]): boolean {
  return spans.some(s => start >= s.start && end <= s.end);
}

function toRange(doc: vscode.TextDocument, start: number, end: number): vscode.Range {
  return new vscode.Range(doc.positionAt(start), doc.positionAt(end));
}
