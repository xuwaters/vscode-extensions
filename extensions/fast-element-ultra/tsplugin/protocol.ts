/**
 * The boundary protocol, mirroring `crates/fast/fast-analyzer-core/src/protocol.rs`.
 * The two are kept in step by hand; the integration tests drive the real WASM
 * through these types, so a drift fails loudly.
 *
 * Every `start`/`end` is UTF-16 code units — JavaScript string offsets.
 * Spans without a `fileName` are relative to their virtual document; the
 * plugin adds `templateStart` when mapping into a source file.
 */

export interface FileSpan {
  fileName: string;
  start: number;
  end: number;
}

export type RuleSetting = 'default' | 'off' | 'warning' | 'error';

export interface EngineConfig {
  strict: boolean;
  rules: Record<string, RuleSetting>;
  globalTags: string[];
  globalAttributes: string[];
  globalEvents: string[];
  dontShowSuggestions: boolean;
  customHtmlData: unknown[];
  maxProjectImportDepth: number;
  maxNodeModuleImportDepth: number;
}

export interface MemberFact {
  name: string;
  propertyName?: string | null;
  mode?: string | null;
  typeText?: string | null;
  typeId?: number | null;
  declarationId?: number | null;
  declSpan?: FileSpan | null;
  documentation?: string | null;
  origin: string;
  visibility?: string | null;
  values?: string[];
}

export interface EventFact {
  name: string;
  typeText?: string | null;
  declSpan?: FileSpan | null;
  documentation?: string | null;
}

export interface NamedFact {
  name: string;
  documentation?: string | null;
  declSpan?: FileSpan | null;
}

export interface ComponentFact {
  tagName: string | null;
  className: string;
  tagNameSpan?: FileSpan | null;
  declSpan?: FileSpan | null;
  declarationId?: number | null;
  sourceTypeId?: number | null;
  attributes: MemberFact[];
  properties: MemberFact[];
  events: EventFact[];
  slots: NamedFact[];
  cssParts: NamedFact[];
  cssProperties: NamedFact[];
  hasShadowRoot: boolean;
  templateDocumentId?: string | null;
  styleDocumentIds?: string[];
  documentation?: string | null;
  inTagNameMap?: boolean;
  origin: 'decorator' | 'define';
}

export interface SourceMember {
  name: string;
  typeText?: string | null;
  documentation?: string | null;
  declSpan?: FileSpan | null;
  isFunction: boolean;
}

export interface DirectiveInfo {
  name: string;
  argString?: string | null;
  argStart?: number | null;
  argEnd?: number | null;
}

export interface ExprInfo {
  kind:
    | 'arrow'
    | 'function'
    | 'call'
    | 'identifier'
    | 'propertyAccess'
    | 'literal'
    | 'template'
    | 'other';
  isFunctionType?: boolean | null;
  isConstant?: boolean | null;
  isDirectiveValue?: boolean;
  directive?: DirectiveInfo | null;
  isPartial?: boolean;
}

export interface PlaceholderFact {
  index: number;
  start: number;
  end: number;
  expr?: ExprInfo | null;
}

export interface VirtualDocumentFact {
  id: string;
  fileName: string;
  templateStart: number;
  kind: 'html' | 'css';
  text: string;
  placeholders: PlaceholderFact[];
  sourceTypeId?: number | null;
  parentTypeId?: number | null;
  sourceTypeName?: string | null;
  sourceMembers?: SourceMember[] | null;
  componentTag?: string | null;
  typeArgInsertOffset?: number | null;
}

export interface UpsertFile {
  fileName: string;
  dependencies: string[];
  nodeModuleDependencies?: string[];
  components: ComponentFact[];
  documents: VirtualDocumentFact[];
}

export type DiagnosticSeverity = 'warning' | 'error' | 'suggestion';

export interface ProtocolEdit {
  fileName: string | null;
  start: number;
  end: number;
  newText: string;
}

export interface FixCommand {
  kind: 'addImport';
  targetFile?: string | null;
}

export interface ProtocolFix {
  label: string;
  edits: ProtocolEdit[];
  command?: FixCommand | null;
}

export interface ProtocolDiagnostic {
  ruleId: string;
  severity: DiagnosticSeverity;
  message: string;
  start: number;
  end: number;
  origin?: string | null;
  fixes?: ProtocolFix[];
}

export interface BindingFact {
  kind: 'attribute' | 'booleanAttribute' | 'property' | 'event';
  start: number;
  end: number;
  tagName: string;
  memberName?: string | null;
  targetDeclarationId?: number | null;
  targetKind?: string | null;
  targetBuiltin?: boolean;
  expressionIndex?: number | null;
  literal?: string | null;
  mode?: string | null;
}

export interface AnalyzeResult {
  diagnostics: ProtocolDiagnostic[];
  facts: BindingFact[];
}

export interface EngineCompletionItem {
  name: string;
  kind: string;
  insertText?: string;
  sortText?: string;
  documentation?: string;
  typeText?: string;
  importFrom?: string;
  isSnippet: boolean;
}

export interface EngineCompletions {
  items: EngineCompletionItem[];
  replaceStart?: number | null;
  replaceEnd?: number | null;
}

export interface EngineQuickInfo {
  contents: string;
  start: number;
  end: number;
}

export interface EngineDefinition {
  targets: FileSpan[];
  originStart: number;
  originEnd: number;
  name: string;
}

export interface EngineRenameInfo {
  canRename: boolean;
  displayName: string;
  triggerStart: number;
  triggerEnd: number;
  kind: 'tag' | 'member';
  tag?: string | null;
  member?: string | null;
  sourceTypeId?: number | null;
}

export interface EngineDocumentInfo {
  inPlaceholder?: number | null;
  directiveName?: string | null;
  directiveArgStart?: number | null;
  directiveArgEnd?: number | null;
  inTemplate: boolean;
}

export interface FileDiagnostic {
  ruleId: string;
  severity: DiagnosticSeverity;
  message: string;
  fileName: string;
  start: number;
  end: number;
}

/** Stable numeric diagnostic codes, one per rule, in the 61xxx range. */
export const RULE_CODES: Record<string, number> = {
  'no-unknown-tag-name': 61001,
  'no-missing-import': 61002,
  'no-unclosed-tag': 61003,
  'no-unknown-attribute': 61004,
  'no-unknown-property': 61005,
  'no-unknown-event': 61006,
  'no-unknown-slot': 61007,
  'no-unintended-mixed-binding': 61008,
  'no-expressionless-property-binding': 61009,
  'no-invalid-attribute-name': 61010,
  'no-invalid-tag-name': 61011,
  'no-missing-element-type-definition': 61012,
  'no-invalid-css': 61013,
  'no-noncallable-event-binding': 61014,
  'no-boolean-in-attribute-binding': 61015,
  'no-complex-attribute-binding': 61016,
  'no-incompatible-type-binding': 61017,
  'no-invalid-directive-binding': 61018,
  'no-incompatible-attr-config': 61019,
  'no-attr-visibility-mismatch': 61020,
  'no-non-reactive-binding': 61021,
  'no-invalid-directive-target': 61022,
  'no-slot-without-shadow-root': 61023,
  'no-duplicate-tag-name': 61024,
  'no-untyped-template': 61025,
  'no-implicit-prevent-default': 61026,
  'template-not-analyzed': 61099,
};

export const DIAGNOSTIC_SOURCE = 'fast-element-ultra';

export function ruleCode(ruleId: string): number {
  return RULE_CODES[ruleId] ?? 61000;
}

export function ruleIdOfCode(code: number): string | undefined {
  for (const [ruleId, ruleCodeValue] of Object.entries(RULE_CODES)) {
    if (ruleCodeValue === code) return ruleId;
  }
  return undefined;
}
