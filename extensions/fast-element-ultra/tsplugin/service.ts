/**
 * The decorated language service: sync source files into the engine, ask it
 * questions, answer its binding facts, and merge everything with TypeScript's
 * own results. Every decorated method falls back to the undecorated one on
 * any failure — a broken analyzer degrades to plain TypeScript, never to a
 * broken editor (architecture.md §1.1).
 */

import * as path from 'path';

import type * as tslib from 'typescript';

import { resolveConfig, type PluginSettings, type ResolvedConfig } from './config.js';
import {
  cssCompletions,
  cssDiagnostics,
  cssFoldingRanges,
  cssHover,
} from './css.js';
import type { SafeEngine } from './engine.js';
import {
  extractAmbientComponents,
  extractFile,
  Interner,
  type Extraction,
  type TemplateInfo,
} from './extract.js';
import type { Logger } from './logger.js';
import { answerFacts } from './oracle.js';
import {
  DIAGNOSTIC_SOURCE,
  ruleCode,
  ruleIdOfCode,
  type ComponentFact,
  type EngineCompletionItem,
  type EngineCompletions,
  type EngineDefinition,
  type EngineDocumentInfo,
  type EngineQuickInfo,
  type EngineRenameInfo,
  type FileSpan,
  type MemberFact,
  type ProtocolDiagnostic,
  type ProtocolFix,
  type VirtualDocumentFact,
} from './protocol.js';

type Ts = typeof tslib;

export interface PluginContext {
  ts: Ts;
  languageService: tslib.LanguageService;
  languageServiceHost: tslib.LanguageServiceHost;
  engine: SafeEngine;
  logger: Logger;
  getSettings(): PluginSettings | undefined;
  projectRoot: string;
}

interface CachedFile {
  version: string;
  extraction: Extraction;
}

/** Not a path: the registry key the ambient (tag-name-map) components live
 * under. No file of the project can collide with it. */
const AMBIENT_FILE = 'fast-element-ultra:tag-name-map';

export class FastService {
  private readonly ts: Ts;
  private readonly interner = new Interner();
  private readonly files = new Map<string, CachedFile>();
  private lastProgram: tslib.Program | undefined;
  private ambientProgram: tslib.Program | undefined;
  private ambientComponents: ComponentFact[] = [];
  private config: ResolvedConfig;
  private configPushed = false;
  private severities: Record<string, string> = {};
  private lastCompletionItems = new Map<string, EngineCompletionItem>();

  constructor(private readonly context: PluginContext) {
    this.ts = context.ts;
    this.config = resolveConfig(context.getSettings(), context.projectRoot);
  }

  onSettingsChanged(): void {
    this.config = resolveConfig(this.context.getSettings(), this.context.projectRoot);
    this.context.logger.level = this.config.logging;
    this.configPushed = false;
    // Force re-extraction: template-tag settings change what a template is.
    this.files.clear();
    this.lastProgram = undefined;
    this.ambientProgram = undefined;
  }

  get enabled(): boolean {
    return !this.config.disable && this.context.engine.available;
  }

  engineState(): string {
    if (this.config.disable) return 'disabled';
    return this.context.engine.state;
  }

  // ------------------------------------------------------------------ sync

  private program(): tslib.Program | undefined {
    return this.context.languageService.getProgram();
  }

  private checker(): tslib.TypeChecker | undefined {
    return this.program()?.getTypeChecker();
  }

  /** Bring the engine's picture of the project up to date. */
  sync(): void {
    if (!this.enabled) return;
    const program = this.program();
    if (!program) return;

    if (!this.configPushed) {
      this.context.engine.setConfig(this.config.engine);
      this.severities = this.context.engine.severities();
      this.configPushed = true;
    }

    const checker = program.getTypeChecker();
    const present = new Set<string>();
    const changed: string[] = [];
    for (const sourceFile of program.getSourceFiles()) {
      if (sourceFile.isDeclarationFile) continue;
      if (sourceFile.fileName.includes('/node_modules/')) continue;
      if (!sourceFile.text.includes('fast-element') && !this.files.has(sourceFile.fileName)) {
        continue;
      }
      present.add(sourceFile.fileName);
      const version = this.context.languageServiceHost.getScriptVersion(sourceFile.fileName);
      const cached = this.files.get(sourceFile.fileName);
      if (cached && cached.version === version && this.lastProgram === program) continue;
      changed.push(sourceFile.fileName);
    }

    for (const fileName of [...this.files.keys()]) {
      if (!present.has(fileName)) {
        this.files.delete(fileName);
        this.context.engine.removeFile(fileName);
      }
    }

    if (changed.length === 0) {
      this.syncAmbient(program, checker);
      this.lastProgram = program;
      return;
    }

    // Types in one file feed extractions of another (source members, tag
    // consts), so any change re-extracts the whole FAST slice of the
    // program. The slice is small by construction — files that mention
    // fast-element — and extraction is a walk, not a check of the world.
    for (const fileName of present) {
      const sourceFile = program.getSourceFile(fileName);
      if (!sourceFile) continue;
      const version = this.context.languageServiceHost.getScriptVersion(fileName);
      try {
        const extraction = extractFile({
          ts: this.ts,
          checker,
          sourceFile,
          htmlTemplateTags: this.config.htmlTemplateTags,
          cssTemplateTags: this.config.cssTemplateTags,
          interner: this.interner,
          resolveModule: (specifier, fromFile) => this.resolveModule(specifier, fromFile),
        });
        this.files.set(fileName, { version, extraction });
        this.context.engine.upsertFile(extraction.upsert);
      } catch (error) {
        this.context.logger.error(
          `extraction failed for ${fileName}: ${error instanceof Error ? error.message : error}`,
        );
      }
    }
    this.syncAmbient(program, checker);
    this.lastProgram = program;
  }

  /**
   * Tags only `HTMLElementTagNameMap` knows about — a library that registers
   * its elements behind its own `define*` wrapper, or one consumed as a built
   * package, declares nothing a file extraction can see. They land in one
   * synthetic file so that removing them is a single call and so that the
   * import-reachability rules can tell them apart (they are ambient: no
   * import of ours makes them more or less defined).
   */
  private syncAmbient(program: tslib.Program, checker: tslib.TypeChecker): void {
    if (this.ambientProgram === program) return;
    this.ambientProgram = program;
    const location = program.getSourceFiles().find((f) => this.files.has(f.fileName));
    if (!location) {
      if (this.ambientComponents.length > 0) {
        this.ambientComponents = [];
        this.context.engine.removeFile(AMBIENT_FILE);
      }
      return;
    }
    const known = new Set<string>();
    for (const { extraction } of this.files.values()) {
      for (const component of extraction.upsert.components) {
        if (component.tagName) known.add(component.tagName);
      }
    }
    try {
      this.ambientComponents = extractAmbientComponents({
        ts: this.ts,
        checker,
        location,
        interner: this.interner,
        known,
      });
    } catch (error) {
      this.ambientComponents = [];
      this.context.logger.error(
        `tag-name-map discovery failed: ${error instanceof Error ? error.message : error}`,
      );
    }
    this.context.engine.upsertFile({
      fileName: AMBIENT_FILE,
      dependencies: [],
      nodeModuleDependencies: [],
      components: this.ambientComponents,
      documents: [],
    });
  }

  private resolveModule(
    specifier: string,
    fromFile: string,
  ): { resolvedFileName: string; isExternal: boolean } | undefined {
    try {
      const options = this.program()?.getCompilerOptions() ?? {};
      const result = this.ts.resolveModuleName(
        specifier,
        fromFile,
        options,
        this.context.languageServiceHost,
      );
      const resolved = result.resolvedModule;
      if (!resolved) return undefined;
      return {
        resolvedFileName: resolved.resolvedFileName,
        isExternal: resolved.isExternalLibraryImport === true,
      };
    } catch {
      return undefined;
    }
  }

  // ------------------------------------------------------------ navigation

  fileExtraction(fileName: string): Extraction | undefined {
    return this.files.get(fileName)?.extraction;
  }

  private templateAt(
    fileName: string,
    position: number,
  ): { template: TemplateInfo; doc: VirtualDocumentFact } | undefined {
    const extraction = this.files.get(fileName)?.extraction;
    if (!extraction) return undefined;
    // Innermost wins: `when(…, html\`…\`)` nests whole templates inside an
    // outer template's expression, and the position belongs to the inner one.
    let best: { template: TemplateInfo; doc: VirtualDocumentFact } | undefined;
    for (const template of extraction.templates) {
      if (position < template.templateStart || position > template.templateEnd) continue;
      const doc = extraction.upsert.documents.find((d) => d.id === template.documentId);
      if (!doc) continue;
      if (!best || template.templateStart > best.template.templateStart) {
        best = { template, doc };
      }
    }
    return best;
  }

  private docInfo(doc: VirtualDocumentFact, offset: number): EngineDocumentInfo | undefined {
    return this.context.engine.query<EngineDocumentInfo>({
      type: 'documentInfoAt',
      documentId: doc.id,
      offset,
    });
  }

  private severityOf(ruleId: string): 'warning' | 'error' | 'suggestion' | undefined {
    const value = this.severities[ruleId];
    if (value === 'warning' || value === 'error' || value === 'suggestion') return value;
    return undefined;
  }

  private componentsOf(tag: string): ComponentFact[] {
    const out: ComponentFact[] = [];
    for (const { extraction } of this.files.values()) {
      for (const component of extraction.upsert.components) {
        if (component.tagName === tag) out.push(component);
      }
    }
    for (const component of this.ambientComponents) {
      if (component.tagName === tag) out.push(component);
    }
    return out;
  }

  /** The light-DOM component whose styles this css document is, if any. */
  private lightDomComponentFor(doc: VirtualDocumentFact): string | null {
    for (const { extraction } of this.files.values()) {
      for (const component of extraction.upsert.components) {
        if (!component.tagName || component.hasShadowRoot) continue;
        if (
          component.styleDocumentIds?.includes(doc.id) ||
          (doc.componentTag && doc.componentTag === component.tagName)
        ) {
          return component.tagName;
        }
      }
    }
    return null;
  }

  // ----------------------------------------------------------- diagnostics

  semanticDiagnostics(fileName: string, prior: tslib.Diagnostic[]): tslib.Diagnostic[] {
    if (!this.enabled) return prior;
    this.sync();
    const cached = this.files.get(fileName);
    const program = this.program();
    const sourceFile = program?.getSourceFile(fileName);
    if (!cached || !sourceFile) return prior;

    const out = [...prior];
    const checker = program?.getTypeChecker();

    for (const template of cached.extraction.templates) {
      const doc = cached.extraction.upsert.documents.find((d) => d.id === template.documentId);
      if (!doc) continue;
      if (doc.kind === 'css') {
        const cssDiags = cssDiagnostics(
          doc,
          this.severityOf('no-invalid-css'),
          this.lightDomComponentFor(doc),
        );
        for (const diagnostic of cssDiags) {
          out.push(this.toTsDiagnostic(sourceFile, template, diagnostic));
        }
        continue;
      }
      const result = this.context.engine.analyze(doc.id);
      if (!result) continue;
      for (const diagnostic of result.diagnostics) {
        out.push(this.toTsDiagnostic(sourceFile, template, diagnostic));
      }
      if (checker && result.facts.length > 0) {
        const answered = answerFacts(result.facts, {
          ts: this.ts,
          checker,
          expressions: template.expressions,
          nodeOf: (id) => this.interner.nodeOf(id),
          severity: (ruleId) => this.severityOf(ruleId),
        });
        for (const diagnostic of answered) {
          out.push(this.toTsDiagnostic(sourceFile, template, diagnostic));
        }
      }
    }

    // Registry-level rules: duplicate tags, invalid tag names.
    for (const diagnostic of this.context.engine.fileDiagnostics(fileName)) {
      if (diagnostic.fileName !== fileName) continue;
      out.push({
        file: sourceFile,
        start: diagnostic.start,
        length: Math.max(1, diagnostic.end - diagnostic.start),
        messageText: diagnostic.message,
        category: this.category(diagnostic.severity),
        code: ruleCode(diagnostic.ruleId),
        source: DIAGNOSTIC_SOURCE,
      });
    }

    // Discovery rules (R10/R12/R19/R20): emitted unfiltered, gated here.
    for (const diagnostic of cached.extraction.discoveryDiagnostics) {
      const severity = this.severityOf(diagnostic.ruleId);
      if (!severity) continue;
      out.push({
        file: sourceFile,
        start: diagnostic.start,
        length: Math.max(1, diagnostic.end - diagnostic.start),
        messageText: diagnostic.message,
        category: this.category(severity),
        code: ruleCode(diagnostic.ruleId),
        source: DIAGNOSTIC_SOURCE,
      });
    }

    return this.dropSuppressed(sourceFile, out);
  }

  private toTsDiagnostic(
    sourceFile: tslib.SourceFile,
    template: TemplateInfo,
    diagnostic: ProtocolDiagnostic,
  ): tslib.Diagnostic {
    let start = template.templateStart + diagnostic.start;
    let length = Math.max(1, diagnostic.end - diagnostic.start);
    if (
      diagnostic.ruleId === 'no-untyped-template' &&
      diagnostic.start === 0 &&
      diagnostic.end === 0
    ) {
      // The engine reports the document; the tag identifier is the span a
      // reader wants.
      start = template.node.tag.getStart();
      length = template.node.tag.getEnd() - start;
    }
    let message = diagnostic.message;
    if (diagnostic.origin && diagnostic.origin !== 'declaration' && diagnostic.origin !== 'builtin') {
      message += ` (tag known from ${diagnostic.origin})`;
    }
    return {
      file: sourceFile,
      start,
      length,
      messageText: message,
      category: this.category(diagnostic.severity),
      code: ruleCode(diagnostic.ruleId),
      source: DIAGNOSTIC_SOURCE,
    };
  }

  private category(severity: 'warning' | 'error' | 'suggestion'): tslib.DiagnosticCategory {
    switch (severity) {
      case 'error':
        return this.ts.DiagnosticCategory.Error;
      case 'warning':
        return this.ts.DiagnosticCategory.Warning;
      case 'suggestion':
        return this.ts.DiagnosticCategory.Suggestion;
    }
  }

  /** `// @ts-ignore` (or `@fast-ignore`) on the previous line suppresses. */
  private dropSuppressed(
    sourceFile: tslib.SourceFile,
    diagnostics: tslib.Diagnostic[],
  ): tslib.Diagnostic[] {
    return diagnostics.filter((diagnostic) => {
      if (diagnostic.source !== DIAGNOSTIC_SOURCE || diagnostic.start === undefined) return true;
      const { line } = sourceFile.getLineAndCharacterOfPosition(diagnostic.start);
      if (line === 0) return true;
      const previousStart = sourceFile.getPositionOfLineAndCharacter(line - 1, 0);
      const previousEnd = sourceFile.getPositionOfLineAndCharacter(line, 0);
      const previous = sourceFile.text.slice(previousStart, previousEnd);
      return !/@(ts|fast)-ignore/.test(previous);
    });
  }

  // ------------------------------------------------------------ completions

  completions(
    fileName: string,
    position: number,
    prior: () => tslib.WithMetadata<tslib.CompletionInfo> | undefined,
  ): tslib.WithMetadata<tslib.CompletionInfo> | undefined {
    if (!this.enabled) return prior();
    this.sync();
    const context = this.templateAt(fileName, position);
    if (!context) return prior();
    const { template, doc } = context;
    const offset = position - template.templateStart;

    if (doc.kind === 'css') {
      return this.cssCompletionInfo(template, doc, offset);
    }

    const info = this.docInfo(doc, offset);
    if (info?.inPlaceholder != null) {
      if (
        info.directiveArgStart != null &&
        info.directiveArgEnd != null &&
        offset >= info.directiveArgStart &&
        offset <= info.directiveArgEnd
      ) {
        return this.memberCompletionInfo(template, doc, info);
      }
      // Inside `${…}`: real TypeScript code, real TypeScript completions.
      return prior();
    }

    const completions = this.context.engine.query<EngineCompletions>({
      type: 'completions',
      documentId: doc.id,
      offset,
    });
    if (!completions || completions.items.length === 0) return prior();

    this.lastCompletionItems.clear();
    const entries: tslib.CompletionEntry[] = completions.items.map((item) => {
      this.lastCompletionItems.set(item.name, item);
      const replacementSpan =
        completions.replaceStart != null && completions.replaceEnd != null
          ? {
              start: template.templateStart + completions.replaceStart,
              length: completions.replaceEnd - completions.replaceStart,
            }
          : undefined;
      return {
        name: item.name,
        kind: this.completionKind(item.kind),
        kindModifiers: '',
        sortText: item.sortText ?? '5',
        insertText: item.insertText,
        isSnippet: item.isSnippet ? true : undefined,
        replacementSpan,
        hasAction: item.importFrom ? true : undefined,
        labelDetails: item.typeText ? { detail: `: ${item.typeText}` } : undefined,
      };
    });

    return {
      isGlobalCompletion: false,
      isMemberCompletion: false,
      isNewIdentifierLocation: true,
      entries,
    };
  }

  private completionKind(kind: string): tslib.ScriptElementKind {
    const kinds = this.ts.ScriptElementKind;
    switch (kind) {
      case 'tag':
        return kinds.classElement;
      case 'attribute':
      case 'booleanAttribute':
        return kinds.memberVariableElement;
      case 'property':
        return kinds.memberVariableElement;
      case 'event':
        return kinds.functionElement;
      case 'value':
      case 'slotName':
      case 'part':
        return kinds.string;
      case 'snippet':
        return kinds.unknown;
      case 'member':
        return kinds.memberVariableElement;
      default:
        return kinds.unknown;
    }
  }

  private memberCompletionInfo(
    template: TemplateInfo,
    doc: VirtualDocumentFact,
    info: EngineDocumentInfo,
  ): tslib.WithMetadata<tslib.CompletionInfo> | undefined {
    const members = doc.sourceMembers;
    if (!members || info.directiveArgStart == null || info.directiveArgEnd == null) {
      return undefined;
    }
    const replacementSpan = {
      start: template.templateStart + info.directiveArgStart,
      length: info.directiveArgEnd - info.directiveArgStart,
    };
    const wantsElements = info.directiveName === 'ref';
    const entries: tslib.CompletionEntry[] = members
      .filter((m) => !m.isFunction || !wantsElements)
      .map((member) => ({
        name: member.name,
        kind: this.ts.ScriptElementKind.memberVariableElement,
        kindModifiers: '',
        sortText: member.typeText ? '0' : '5',
        replacementSpan,
        labelDetails: member.typeText ? { detail: `: ${member.typeText}` } : undefined,
      }));
    return {
      isGlobalCompletion: false,
      isMemberCompletion: true,
      isNewIdentifierLocation: false,
      entries,
    };
  }

  private cssCompletionInfo(
    template: TemplateInfo,
    doc: VirtualDocumentFact,
    offset: number,
  ): tslib.WithMetadata<tslib.CompletionInfo> | undefined {
    const items = cssCompletions(doc, offset);
    if (items.length === 0) return undefined;
    const entries: tslib.CompletionEntry[] = items.map((item) => ({
      name: item.name,
      kind: this.ts.ScriptElementKind.memberVariableElement,
      kindModifiers: '',
      sortText: '5',
      insertText: item.insertText,
      replacementSpan:
        item.replaceStart != null && item.replaceEnd != null
          ? {
              start: template.templateStart + item.replaceStart,
              length: item.replaceEnd - item.replaceStart,
            }
          : undefined,
    }));
    return {
      isGlobalCompletion: false,
      isMemberCompletion: false,
      isNewIdentifierLocation: true,
      entries,
    };
  }

  completionDetails(
    fileName: string,
    entryName: string,
  ): tslib.CompletionEntryDetails | undefined {
    const item = this.lastCompletionItems.get(entryName);
    if (!item) return undefined;
    const parts: tslib.SymbolDisplayPart[] = [{ text: entryName, kind: 'text' }];
    if (item.typeText) parts.push({ text: `: ${item.typeText}`, kind: 'text' });
    const documentation: tslib.SymbolDisplayPart[] = item.documentation
      ? [{ text: item.documentation, kind: 'text' }]
      : [];
    let codeActions: tslib.CodeAction[] | undefined;
    if (item.importFrom) {
      const edit = this.importEdit(fileName, item.importFrom);
      if (edit) {
        codeActions = [
          {
            description: `Import from '${edit.specifier}'`,
            changes: [
              {
                fileName,
                textChanges: [{ span: { start: edit.start, length: 0 }, newText: edit.text }],
              },
            ],
          },
        ];
      }
    }
    return {
      name: entryName,
      kind: this.completionKind(item.kind),
      kindModifiers: '',
      displayParts: parts,
      documentation,
      codeActions,
    };
  }

  /** A side-effect import of the module that defines a component. */
  private importEdit(
    fromFile: string,
    targetFile: string,
  ): { start: number; text: string; specifier: string } | undefined {
    const sourceFile = this.program()?.getSourceFile(fromFile);
    if (!sourceFile) return undefined;
    let specifier = path
      .relative(path.dirname(fromFile), targetFile)
      .replace(/\\/g, '/')
      .replace(/\.tsx?$/, '.js');
    if (!specifier.startsWith('.')) specifier = `./${specifier}`;
    let insertAt = 0;
    for (const statement of sourceFile.statements) {
      if (this.ts.isImportDeclaration(statement)) insertAt = statement.getEnd();
    }
    const text =
      insertAt === 0 ? `import '${specifier}';\n` : `\nimport '${specifier}';`;
    return { start: insertAt, text, specifier };
  }

  // ------------------------------------------------------------- quick info

  quickInfo(
    fileName: string,
    position: number,
    prior: () => tslib.QuickInfo | undefined,
  ): tslib.QuickInfo | undefined {
    if (!this.enabled) return prior();
    this.sync();
    const context = this.templateAt(fileName, position);
    if (!context) return prior();
    const { template, doc } = context;
    const offset = position - template.templateStart;

    if (doc.kind === 'css') {
      const hover = cssHover(doc, offset);
      if (!hover) return prior();
      return this.markdownQuickInfo(template, hover.contents, hover.start, hover.end);
    }

    const info = this.docInfo(doc, offset);
    const inDirectiveArg =
      info?.directiveArgStart != null &&
      info.directiveArgEnd != null &&
      offset >= info.directiveArgStart &&
      offset <= info.directiveArgEnd;
    if (info?.inPlaceholder != null && !inDirectiveArg) return prior();

    const quickInfo = this.context.engine.query<EngineQuickInfo>({
      type: 'quickInfo',
      documentId: doc.id,
      offset,
    });
    if (!quickInfo) return prior();
    return this.markdownQuickInfo(template, quickInfo.contents, quickInfo.start, quickInfo.end);
  }

  private markdownQuickInfo(
    template: TemplateInfo,
    contents: string,
    start: number,
    end: number,
  ): tslib.QuickInfo {
    return {
      kind: this.ts.ScriptElementKind.unknown,
      kindModifiers: '',
      textSpan: {
        start: template.templateStart + start,
        length: Math.max(1, end - start),
      },
      displayParts: [],
      documentation: [{ text: contents, kind: 'text' }],
    };
  }

  // ------------------------------------------------------------- definition

  definition(
    fileName: string,
    position: number,
    prior: () => tslib.DefinitionInfoAndBoundSpan | undefined,
  ): tslib.DefinitionInfoAndBoundSpan | undefined {
    if (!this.enabled) return prior();
    this.sync();
    const context = this.templateAt(fileName, position);
    if (!context || context.doc.kind === 'css') return prior();
    const { template, doc } = context;
    const offset = position - template.templateStart;

    const definition = this.context.engine.query<EngineDefinition>({
      type: 'definition',
      documentId: doc.id,
      offset,
    });
    if (!definition || definition.targets.length === 0) return prior();
    return {
      definitions: definition.targets.map((target) => ({
        fileName: target.fileName,
        textSpan: { start: target.start, length: Math.max(1, target.end - target.start) },
        kind: this.ts.ScriptElementKind.memberVariableElement,
        name: definition.name,
        containerName: '',
        containerKind: this.ts.ScriptElementKind.unknown,
      })),
      textSpan: {
        start: template.templateStart + definition.originStart,
        length: Math.max(1, definition.originEnd - definition.originStart),
      },
    };
  }

  // ------------------------------------------------------------- references

  references(
    fileName: string,
    position: number,
    prior: () => tslib.ReferencedSymbol[] | undefined,
  ): tslib.ReferencedSymbol[] | undefined {
    if (!this.enabled) return prior();
    this.sync();

    let spans: FileSpan[] | undefined;
    let displayName = '';
    const context = this.templateAt(fileName, position);
    if (context && context.doc.kind === 'html') {
      const offset = position - context.template.templateStart;
      spans = this.context.engine.query<FileSpan[]>({
        type: 'references',
        documentId: context.doc.id,
        offset,
      });
      displayName = 'template reference';
    } else {
      const target = this.declarationTargetAt(fileName, position);
      if (target?.kind === 'tag') {
        spans = this.context.engine.query<FileSpan[]>({
          type: 'tagReferences',
          tag: target.tag,
        });
        displayName = target.tag;
      } else if (target?.kind === 'member') {
        spans = this.context.engine.query<FileSpan[]>({
          type: 'memberReferences',
          tag: target.tag,
          sourceTypeId: target.sourceTypeId,
          name: target.member,
        });
        displayName = target.member;
      }
    }

    const priorResult = prior() ?? [];
    if (!spans || spans.length === 0) {
      return priorResult.length > 0 ? priorResult : undefined;
    }

    const first = spans[0];
    const group: tslib.ReferencedSymbol = {
      definition: {
        containerKind: this.ts.ScriptElementKind.unknown,
        containerName: '',
        displayParts: [{ text: displayName, kind: 'text' }],
        fileName: first.fileName,
        kind: this.ts.ScriptElementKind.memberVariableElement,
        name: displayName,
        textSpan: { start: first.start, length: Math.max(1, first.end - first.start) },
      },
      references: spans.map((span) => ({
        fileName: span.fileName,
        textSpan: { start: span.start, length: Math.max(1, span.end - span.start) },
        isWriteAccess: false,
        isDefinition: false,
      })),
    };
    return [...priorResult, group];
  }

  /** What declaration-side thing the cursor is on: a component's tag string,
   * its class name, or one of its members. */
  private declarationTargetAt(
    fileName: string,
    position: number,
  ):
    | { kind: 'tag'; tag: string }
    | { kind: 'member'; tag: string | null; member: string; sourceTypeId: number | null }
    | undefined {
    const extraction = this.files.get(fileName)?.extraction;
    if (!extraction) return undefined;
    for (const component of extraction.upsert.components) {
      const inSpan = (span: FileSpan | null | undefined): boolean =>
        !!span &&
        span.fileName === fileName &&
        position >= span.start &&
        position <= span.end;
      if (component.tagName && (inSpan(component.tagNameSpan) || inSpan(component.declSpan))) {
        return { kind: 'tag', tag: component.tagName };
      }
      const memberOf = (facts: MemberFact[]): MemberFact | undefined =>
        facts.find((f) => inSpan(f.declSpan));
      const attribute = memberOf(component.attributes);
      if (attribute) {
        return {
          kind: 'member',
          tag: component.tagName,
          member: attribute.name,
          sourceTypeId: component.sourceTypeId ?? null,
        };
      }
      const property = memberOf(component.properties);
      if (property) {
        return {
          kind: 'member',
          tag: component.tagName,
          member: property.name,
          sourceTypeId: component.sourceTypeId ?? null,
        };
      }
      const event = component.events.find(
        (e) =>
          e.declSpan &&
          e.declSpan.fileName === fileName &&
          position >= e.declSpan.start &&
          position <= e.declSpan.end,
      );
      if (event) {
        return {
          kind: 'member',
          tag: component.tagName,
          member: event.name,
          sourceTypeId: component.sourceTypeId ?? null,
        };
      }
      // An undecorated member — `findInput!: HTMLInputElement` — is still a
      // ref('…') target; the class body says whether the cursor is on one.
      if (component.declarationId != null) {
        const classNode = this.interner.nodeOf(component.declarationId);
        if (classNode && this.ts.isClassDeclaration(classNode)) {
          for (const member of classNode.members) {
            const name = member.name;
            if (
              name &&
              this.ts.isIdentifier(name) &&
              name.getSourceFile().fileName === fileName &&
              position >= name.getStart() &&
              position <= name.getEnd()
            ) {
              return {
                kind: 'member',
                tag: component.tagName,
                member: name.text,
                sourceTypeId: component.sourceTypeId ?? null,
              };
            }
          }
        }
      }
    }
    return undefined;
  }

  // ----------------------------------------------------------------- rename

  renameInfo(
    fileName: string,
    position: number,
    prior: () => tslib.RenameInfo,
  ): tslib.RenameInfo {
    if (!this.enabled) return prior();
    this.sync();
    const context = this.templateAt(fileName, position);
    if (context && context.doc.kind === 'html') {
      const offset = position - context.template.templateStart;
      const info = this.context.engine.query<EngineRenameInfo>({
        type: 'renameInfo',
        documentId: context.doc.id,
        offset,
      });
      if (info?.canRename) {
        return {
          canRename: true,
          displayName: info.displayName,
          fullDisplayName: info.displayName,
          kind: this.ts.ScriptElementKind.memberVariableElement,
          kindModifiers: '',
          triggerSpan: {
            start: context.template.templateStart + info.triggerStart,
            length: Math.max(1, info.triggerEnd - info.triggerStart),
          },
        };
      }
      return prior();
    }
    const target = this.declarationTargetAt(fileName, position);
    if (target?.kind === 'tag') {
      const extraction = this.files.get(fileName)?.extraction;
      const component = extraction?.upsert.components.find((c) => c.tagName === target.tag);
      const span = component?.tagNameSpan;
      if (span && position >= span.start && position <= span.end) {
        return {
          canRename: true,
          displayName: target.tag,
          fullDisplayName: target.tag,
          kind: this.ts.ScriptElementKind.string,
          kindModifiers: '',
          triggerSpan: { start: span.start, length: span.end - span.start },
        };
      }
    }
    return prior();
  }

  renameLocations(
    fileName: string,
    position: number,
    prior: () => readonly tslib.RenameLocation[] | undefined,
  ): readonly tslib.RenameLocation[] | undefined {
    if (!this.enabled) return prior();
    this.sync();

    const merged = new Map<string, tslib.RenameLocation>();
    const add = (locations: readonly tslib.RenameLocation[] | undefined): void => {
      for (const location of locations ?? []) {
        merged.set(`${location.fileName}:${location.textSpan.start}`, location);
      }
    };
    const addSpans = (spans: FileSpan[] | undefined): void => {
      for (const span of spans ?? []) {
        const location: tslib.RenameLocation = {
          fileName: span.fileName,
          textSpan: { start: span.start, length: span.end - span.start },
        };
        merged.set(`${location.fileName}:${location.textSpan.start}`, location);
      }
    };

    const context = this.templateAt(fileName, position);
    if (context && context.doc.kind === 'html') {
      const offset = position - context.template.templateStart;
      const info = this.context.engine.query<EngineRenameInfo>({
        type: 'renameInfo',
        documentId: context.doc.id,
        offset,
      });
      if (!info?.canRename) return prior();
      addSpans(
        this.context.engine.query<FileSpan[]>({
          type: 'renameLocations',
          documentId: context.doc.id,
          offset,
        }),
      );
      // The declaration and its TypeScript references, renamed by TypeScript.
      if (info.kind === 'member' && info.member) {
        const declaration =
          this.memberDeclaration(info.tag ?? null, info.member) ??
          this.sourceMemberDeclaration(context.doc, info.member);
        if (declaration) {
          add(
            this.context.languageService.findRenameLocations(
              declaration.fileName,
              declaration.start,
              false,
              false,
              {},
            ),
          );
        }
      }
      return [...merged.values()];
    }

    const target = this.declarationTargetAt(fileName, position);
    if (target?.kind === 'member') {
      add(prior());
      addSpans(
        this.context.engine.query<FileSpan[]>({
          type: 'memberRenameLocations',
          tag: target.tag,
          sourceTypeId: target.sourceTypeId,
          name: target.member,
        }),
      );
      return [...merged.values()];
    }
    if (target?.kind === 'tag') {
      addSpans(
        this.context.engine.query<FileSpan[]>({
          type: 'tagRenameLocations',
          tag: target.tag,
        }),
      );
      if (merged.size > 0) return [...merged.values()];
    }
    return prior();
  }

  /** An undecorated member's declaration, from the template's source type. */
  private sourceMemberDeclaration(
    doc: VirtualDocumentFact,
    member: string,
  ): { fileName: string; start: number } | undefined {
    const fact = doc.sourceMembers?.find((m) => m.name === member);
    if (!fact?.declSpan) return undefined;
    return { fileName: fact.declSpan.fileName, start: fact.declSpan.start };
  }

  private memberDeclaration(
    tag: string | null,
    member: string,
  ): { fileName: string; start: number } | undefined {
    if (!tag) return undefined;
    for (const component of this.componentsOf(tag)) {
      for (const fact of [...component.properties, ...component.attributes]) {
        if ((fact.propertyName ?? fact.name) === member || fact.name === member) {
          if (fact.declSpan) {
            return { fileName: fact.declSpan.fileName, start: fact.declSpan.start };
          }
        }
      }
    }
    return undefined;
  }

  // ------------------------------------------------------ workspace analysis

  /** Every FAST diagnostic across the project, as line/character ranges — the
   * `fastElementUltra.analyze` command's answer (design/features.md §12). */
  workspaceDiagnostics(): Array<{
    file: string;
    start: { line: number; character: number };
    end: { line: number; character: number };
    message: string;
    severity: 'error' | 'warning' | 'suggestion';
    ruleId: string;
  }> {
    if (!this.enabled) return [];
    this.sync();
    const program = this.program();
    if (!program) return [];
    const out: ReturnType<FastService['workspaceDiagnostics']> = [];
    for (const fileName of this.files.keys()) {
      const sourceFile = program.getSourceFile(fileName);
      if (!sourceFile) continue;
      for (const diagnostic of this.semanticDiagnostics(fileName, [])) {
        if (diagnostic.start === undefined || diagnostic.length === undefined) continue;
        const start = sourceFile.getLineAndCharacterOfPosition(diagnostic.start);
        const end = sourceFile.getLineAndCharacterOfPosition(
          diagnostic.start + diagnostic.length,
        );
        out.push({
          file: fileName,
          start,
          end,
          message: this.ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n'),
          severity:
            diagnostic.category === this.ts.DiagnosticCategory.Error
              ? 'error'
              : diagnostic.category === this.ts.DiagnosticCategory.Warning
                ? 'warning'
                : 'suggestion',
          ruleId:
            (typeof diagnostic.code === 'number' && ruleIdOfCode(diagnostic.code)) || 'unknown',
        });
      }
    }
    return out;
  }

  // -------------------------------------------------------------- code fixes

  codeFixes(
    fileName: string,
    start: number,
    end: number,
    errorCodes: readonly number[],
    prior: () => readonly tslib.CodeFixAction[],
  ): readonly tslib.CodeFixAction[] {
    const priorResult = prior();
    if (!this.enabled) return priorResult;
    if (!errorCodes.some((code) => code >= 61000 && code <= 61099)) return priorResult;
    this.sync();
    const context = this.templateAt(fileName, start);
    if (!context || context.doc.kind === 'css') return priorResult;
    const { template, doc } = context;
    const relStart = start - template.templateStart;
    const relEnd = end - template.templateStart;

    const fixes: ProtocolFix[] = [];
    const result = this.context.engine.analyze(doc.id);
    const checker = this.checker();
    if (result) {
      const collect = (diagnostics: ProtocolDiagnostic[]): void => {
        for (const diagnostic of diagnostics) {
          if (diagnostic.start <= relEnd && relStart <= diagnostic.end) {
            fixes.push(...(diagnostic.fixes ?? []));
          }
        }
      };
      collect(result.diagnostics);
      if (checker) {
        collect(
          answerFacts(result.facts, {
            ts: this.ts,
            checker,
            expressions: template.expressions,
            nodeOf: (id) => this.interner.nodeOf(id),
            severity: (ruleId) => this.severityOf(ruleId),
          }),
        );
      }
    }

    const actions: tslib.CodeFixAction[] = [];
    for (const fix of fixes) {
      const changes = this.fixChanges(fileName, template, fix);
      if (!changes) continue;
      actions.push({
        fixName: 'fast-element-ultra',
        description: fix.label,
        changes,
      });
    }
    return [...priorResult, ...actions];
  }

  private fixChanges(
    fileName: string,
    template: TemplateInfo,
    fix: ProtocolFix,
  ): tslib.FileTextChanges[] | undefined {
    if (fix.command?.kind === 'addImport') {
      if (!fix.command.targetFile) return undefined;
      const edit = this.importEdit(fileName, fix.command.targetFile);
      if (!edit) return undefined;
      return [
        {
          fileName,
          textChanges: [{ span: { start: edit.start, length: 0 }, newText: edit.text }],
        },
      ];
    }
    const byFile = new Map<string, tslib.TextChange[]>();
    for (const edit of fix.edits) {
      const targetFile = edit.fileName ?? fileName;
      const offset = edit.fileName ? 0 : template.templateStart;
      const changes = byFile.get(targetFile) ?? [];
      changes.push({
        span: { start: offset + edit.start, length: edit.end - edit.start },
        newText: edit.newText,
      });
      byFile.set(targetFile, changes);
    }
    if (byFile.size === 0) return undefined;
    return [...byFile.entries()].map(([file, textChanges]) => ({ fileName: file, textChanges }));
  }

  // ------------------------------------------------- closing tag + outlining

  closingTag(
    fileName: string,
    position: number,
    prior: () => tslib.JsxClosingTagInfo | undefined,
  ): tslib.JsxClosingTagInfo | undefined {
    if (!this.enabled) return prior();
    this.sync();
    const context = this.templateAt(fileName, position);
    if (!context || context.doc.kind === 'css') return prior();
    const result = this.context.engine.query<{ newText: string }>({
      type: 'closingTag',
      documentId: context.doc.id,
      offset: position - context.template.templateStart,
    });
    return result ?? prior();
  }

  outliningSpans(fileName: string, prior: tslib.OutliningSpan[]): tslib.OutliningSpan[] {
    if (!this.enabled) return prior;
    this.sync();
    const cached = this.files.get(fileName);
    if (!cached) return prior;
    const out = [...prior];
    for (const template of cached.extraction.templates) {
      const doc = cached.extraction.upsert.documents.find((d) => d.id === template.documentId);
      if (!doc) continue;
      const push = (start: number, end: number): void => {
        const span: tslib.TextSpan = {
          start: template.templateStart + start,
          length: Math.max(0, end - start),
        };
        out.push({
          textSpan: span,
          hintSpan: span,
          bannerText: '…',
          autoCollapse: false,
          kind: this.ts.OutliningSpanKind.Code,
        });
      };
      // The template itself folds.
      if (/\n/.test(doc.text)) push(0, doc.text.length);
      if (doc.kind === 'css') {
        for (const range of cssFoldingRanges(doc)) push(range.start, range.end);
      } else {
        const ranges = this.context.engine.query<Array<{ start: number; end: number }>>({
          type: 'folding',
          documentId: doc.id,
        });
        for (const range of ranges ?? []) push(range.start, range.end);
      }
    }
    return out;
  }
}

/** Wrap the language service; each override falls back on any throw. */
export function decorateLanguageService(
  service: FastService,
  languageService: tslib.LanguageService,
  logger: Logger,
): tslib.LanguageService {
  const decorated: tslib.LanguageService = Object.create(null);
  for (const key of Object.keys(languageService) as Array<keyof tslib.LanguageService>) {
    const original = languageService[key];
    if (typeof original === 'function') {
      (decorated as unknown as Record<string, unknown>)[key] = (
        original as (...args: unknown[]) => unknown
      ).bind(languageService);
    }
  }

  const safe = <A extends unknown[], R>(
    name: string,
    override: (...args: A) => R,
    fallback: (...args: A) => R,
  ): ((...args: A) => R) => {
    return (...args: A): R => {
      try {
        return override(...args);
      } catch (error) {
        logger.error(
          `${name} failed, falling back to TypeScript: ${error instanceof Error ? error.stack ?? error.message : error}`,
        );
        return fallback(...args);
      }
    };
  };

  decorated.getSemanticDiagnostics = safe(
    'getSemanticDiagnostics',
    (fileName) =>
      service.semanticDiagnostics(fileName, languageService.getSemanticDiagnostics(fileName)),
    (fileName) => languageService.getSemanticDiagnostics(fileName),
  );

  decorated.getCompletionsAtPosition = safe(
    'getCompletionsAtPosition',
    (fileName, position, options, formatting) =>
      service.completions(fileName, position, () =>
        languageService.getCompletionsAtPosition(fileName, position, options, formatting),
      ),
    (fileName, position, options, formatting) =>
      languageService.getCompletionsAtPosition(fileName, position, options, formatting),
  );

  decorated.getCompletionEntryDetails = safe(
    'getCompletionEntryDetails',
    (fileName, position, entryName, formatOptions, source, preferences, data) =>
      service.completionDetails(fileName, entryName) ??
      languageService.getCompletionEntryDetails(
        fileName,
        position,
        entryName,
        formatOptions,
        source,
        preferences,
        data,
      ),
    (fileName, position, entryName, formatOptions, source, preferences, data) =>
      languageService.getCompletionEntryDetails(
        fileName,
        position,
        entryName,
        formatOptions,
        source,
        preferences,
        data,
      ),
  );

  decorated.getQuickInfoAtPosition = safe(
    'getQuickInfoAtPosition',
    (fileName, position) =>
      service.quickInfo(fileName, position, () =>
        languageService.getQuickInfoAtPosition(fileName, position),
      ),
    (fileName, position) => languageService.getQuickInfoAtPosition(fileName, position),
  );

  decorated.getDefinitionAndBoundSpan = safe(
    'getDefinitionAndBoundSpan',
    (fileName, position) =>
      service.definition(fileName, position, () =>
        languageService.getDefinitionAndBoundSpan(fileName, position),
      ),
    (fileName, position) => languageService.getDefinitionAndBoundSpan(fileName, position),
  );

  decorated.findReferences = safe(
    'findReferences',
    (fileName, position) =>
      service.references(fileName, position, () =>
        languageService.findReferences(fileName, position),
      ),
    (fileName, position) => languageService.findReferences(fileName, position),
  );

  decorated.getRenameInfo = safe(
    'getRenameInfo',
    (fileName, position, preferences) =>
      service.renameInfo(fileName, position, () =>
        languageService.getRenameInfo(fileName, position, preferences),
      ),
    (fileName, position, preferences) =>
      languageService.getRenameInfo(fileName, position, preferences),
  );

  decorated.findRenameLocations = safe(
    'findRenameLocations',
    (fileName, position, findInStrings, findInComments, preferences) =>
      service.renameLocations(fileName, position, () =>
        // eslint-disable-next-line @typescript-eslint/no-deprecated
        languageService.findRenameLocations(
          fileName,
          position,
          findInStrings,
          findInComments,
          preferences as never,
        ),
      ),
    (fileName, position, findInStrings, findInComments, preferences) =>
      // eslint-disable-next-line @typescript-eslint/no-deprecated
      languageService.findRenameLocations(
        fileName,
        position,
        findInStrings,
        findInComments,
        preferences as never,
      ),
  ) as tslib.LanguageService['findRenameLocations'];

  decorated.getCodeFixesAtPosition = safe(
    'getCodeFixesAtPosition',
    (fileName, start, end, errorCodes, formatOptions, preferences) =>
      service.codeFixes(fileName, start, end, errorCodes, () =>
        languageService.getCodeFixesAtPosition(
          fileName,
          start,
          end,
          errorCodes,
          formatOptions,
          preferences,
        ),
      ),
    (fileName, start, end, errorCodes, formatOptions, preferences) =>
      languageService.getCodeFixesAtPosition(
        fileName,
        start,
        end,
        errorCodes,
        formatOptions,
        preferences,
      ),
  );

  decorated.getJsxClosingTagAtPosition = safe(
    'getJsxClosingTagAtPosition',
    (fileName, position) =>
      service.closingTag(fileName, position, () =>
        languageService.getJsxClosingTagAtPosition(fileName, position),
      ),
    (fileName, position) => languageService.getJsxClosingTagAtPosition(fileName, position),
  );

  decorated.getOutliningSpans = safe(
    'getOutliningSpans',
    (fileName) => service.outliningSpans(fileName, languageService.getOutliningSpans(fileName)),
    (fileName) => languageService.getOutliningSpans(fileName),
  );

  return decorated;
}
