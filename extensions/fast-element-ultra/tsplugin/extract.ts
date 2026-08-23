/**
 * Discovery: what the TypeScript side extracts from a source file and hands
 * to the engine — components with their members (design/component-model.md),
 * virtual documents with per-expression metadata (design/architecture.md §3),
 * and the file's resolved imports.
 *
 * Everything here reads the checker, never the AST shape alone: the tag name
 * of `@customElement({ name: CSV_GRID_TAG })` comes from the *type* of the
 * name expression, which is what makes a `const`, an imported `const` and an
 * `as const` member access all work — the bug that motivated RFC 011.
 */

import type * as tslib from 'typescript';

import type {
  ComponentFact,
  DirectiveInfo,
  EventFact,
  ExprInfo,
  FileDiagnostic,
  FileSpan,
  MemberFact,
  NamedFact,
  PlaceholderFact,
  SourceMember,
  UpsertFile,
  VirtualDocumentFact,
} from './protocol.js';

type Ts = typeof tslib;

const FAST_DIRECTIVES = new Set(['when', 'repeat', 'render', 'ref', 'slotted', 'children']);

/** Caps that keep per-file payloads bounded on pathological inputs. */
const MAX_SOURCE_MEMBERS = 512;
const MAX_TYPE_TEXT = 120;

export interface TemplateInfo {
  documentId: string;
  node: tslib.TaggedTemplateExpression;
  /** Source offset of the first character after the opening backtick. */
  templateStart: number;
  templateEnd: number;
  kind: 'html' | 'css';
  expressions: readonly tslib.Expression[];
}

export interface Extraction {
  upsert: UpsertFile;
  templates: TemplateInfo[];
  /** R10/R12/R19/R20 — class-file rules, absolute spans, unfiltered by
   * severity: the service applies the config. */
  discoveryDiagnostics: FileDiagnostic[];
}

/** Node/type identity across the boundary: ids the engine hands back. */
export class Interner {
  private nodeIds = new Map<tslib.Node, number>();
  private nodes: tslib.Node[] = [];
  private typeIds = new Map<tslib.Type, number>();
  private nextType = 1;

  idOfNode(node: tslib.Node): number {
    let id = this.nodeIds.get(node);
    if (id === undefined) {
      id = this.nodes.length + 1;
      this.nodeIds.set(node, id);
      this.nodes.push(node);
    }
    return id;
  }

  nodeOf(id: number): tslib.Node | undefined {
    return this.nodes[id - 1];
  }

  idOfType(type: tslib.Type): number {
    let id = this.typeIds.get(type);
    if (id === undefined) {
      id = this.nextType++;
      this.typeIds.set(type, id);
    }
    return id;
  }

  clear(): void {
    this.nodeIds.clear();
    this.nodes = [];
    this.typeIds.clear();
    this.nextType = 1;
  }
}

export interface ExtractOptions {
  ts: Ts;
  checker: tslib.TypeChecker;
  sourceFile: tslib.SourceFile;
  htmlTemplateTags: string[];
  cssTemplateTags: string[];
  interner: Interner;
  resolveModule(
    specifier: string,
    fromFile: string,
  ): { resolvedFileName: string; isExternal: boolean } | undefined;
}

export function extractFile(options: ExtractOptions): Extraction {
  return new Extractor(options).run();
}

class Extractor {
  private readonly ts: Ts;
  private readonly checker: tslib.TypeChecker;
  private readonly sf: tslib.SourceFile;
  private readonly interner: Interner;

  private templates: TemplateInfo[] = [];
  private documents: VirtualDocumentFact[] = [];
  private components: ComponentFact[] = [];
  private componentClasses = new Set<tslib.ClassDeclaration>();
  private dependencies: string[] = [];
  private nodeModuleDependencies: string[] = [];
  private discovery: FileDiagnostic[] = [];

  constructor(private readonly options: ExtractOptions) {
    this.ts = options.ts;
    this.checker = options.checker;
    this.sf = options.sourceFile;
    this.interner = options.interner;
  }

  run(): Extraction {
    this.collectImports();
    const visit = (node: tslib.Node): void => {
      if (this.ts.isTaggedTemplateExpression(node)) {
        this.maybeTemplate(node);
      } else if (this.ts.isClassDeclaration(node)) {
        this.maybeDecoratedComponent(node);
      } else if (this.ts.isCallExpression(node)) {
        this.maybeDefineCall(node);
      }
      this.ts.forEachChild(node, visit);
    };
    visit(this.sf);
    this.linkComponentsToDocuments();
    return {
      upsert: {
        fileName: this.sf.fileName,
        dependencies: this.dependencies,
        nodeModuleDependencies: this.nodeModuleDependencies,
        components: this.components,
        documents: this.documents,
      },
      templates: this.templates,
      discoveryDiagnostics: this.discovery,
    };
  }

  // -------------------------------------------------------------- utilities

  /** The symbol behind a node, aliases resolved. In `{ template }` the
   * shorthand name is a property symbol; the *value* symbol is wanted. */
  private resolvedSymbol(node: tslib.Node): tslib.Symbol | undefined {
    try {
      let symbol: tslib.Symbol | undefined;
      if (
        this.ts.isIdentifier(node) &&
        this.ts.isShorthandPropertyAssignment(node.parent) &&
        node.parent.name === node
      ) {
        symbol = this.checker.getShorthandAssignmentValueSymbol(node.parent);
      } else {
        symbol = this.checker.getSymbolAtLocation(node);
      }
      if (symbol && symbol.flags & this.ts.SymbolFlags.Alias) {
        symbol = this.checker.getAliasedSymbol(symbol);
      }
      return symbol;
    } catch {
      return undefined;
    }
  }

  private isFastElementSymbol(symbol: tslib.Symbol | undefined): boolean {
    if (!symbol) return false;
    return (symbol.getDeclarations() ?? []).some((d) =>
      isFastElementPath(d.getSourceFile().fileName),
    );
  }

  /** The fast-element export name a node resolves to, or undefined. */
  private fastExportName(node: tslib.Node): string | undefined {
    const symbol = this.resolvedSymbol(node);
    if (!this.isFastElementSymbol(symbol)) return undefined;
    return symbol?.getName();
  }

  private span(node: tslib.Node): FileSpan {
    return {
      fileName: node.getSourceFile().fileName,
      start: node.getStart(),
      end: node.getEnd(),
    };
  }

  /** The contents of a string literal, without the quotes. */
  private literalContentsSpan(node: tslib.StringLiteralLike): FileSpan {
    return {
      fileName: node.getSourceFile().fileName,
      start: node.getStart() + 1,
      end: node.getEnd() - 1,
    };
  }

  private typeText(type: tslib.Type): string {
    const text = this.checker.typeToString(type);
    return text.length > MAX_TYPE_TEXT ? `${text.slice(0, MAX_TYPE_TEXT)}…` : text;
  }

  private documentationOf(symbol: tslib.Symbol | undefined): string | null {
    if (!symbol) return null;
    try {
      const text = this.ts.displayPartsToString(symbol.getDocumentationComment(this.checker));
      return text.length > 0 ? text : null;
    } catch {
      return null;
    }
  }

  // ---------------------------------------------------------------- imports

  private collectImports(): void {
    for (const statement of this.sf.statements) {
      let specifier: tslib.Expression | undefined;
      if (this.ts.isImportDeclaration(statement)) specifier = statement.moduleSpecifier;
      else if (this.ts.isExportDeclaration(statement)) specifier = statement.moduleSpecifier;
      if (!specifier || !this.ts.isStringLiteralLike(specifier)) continue;
      const resolved = this.options.resolveModule(specifier.text, this.sf.fileName);
      if (!resolved) continue;
      if (resolved.isExternal) this.nodeModuleDependencies.push(resolved.resolvedFileName);
      else this.dependencies.push(resolved.resolvedFileName);
    }
  }

  // -------------------------------------------------------------- templates

  private maybeTemplate(node: tslib.TaggedTemplateExpression): void {
    if (!this.ts.isIdentifier(node.tag)) return;
    const spelled = node.tag.text;
    const isHtmlSpelling = this.options.htmlTemplateTags.includes(spelled);
    const isCssSpelling = this.options.cssTemplateTags.includes(spelled);
    if (!isHtmlSpelling && !isCssSpelling) return;
    const exportName = this.fastExportName(node.tag);
    // A template is FAST's when its tag resolves to fast-element (0009); the
    // configured spellings only widen which local names are considered.
    if (exportName !== 'html' && exportName !== 'css') return;
    const kind: 'html' | 'css' = exportName;

    const literal = node.template;
    const templateStart = literal.getStart() + 1;
    const templateEnd = literal.getEnd() - 1;
    if (templateEnd <= templateStart && !this.ts.isTemplateExpression(literal)) {
      // Empty template; still record it so features know it exists.
    }
    const text = this.sf.text.slice(templateStart, templateEnd);

    const expressions: tslib.Expression[] = [];
    const placeholders: PlaceholderFact[] = [];
    if (this.ts.isTemplateExpression(literal)) {
      let prevEnd = literal.head.getEnd(); // just past `${`
      literal.templateSpans.forEach((span, index) => {
        const regionStart = prevEnd - 2; // the `${`
        const regionEnd = span.literal.getStart() + 1; // through the `}`
        expressions.push(span.expression);
        placeholders.push({
          index,
          start: regionStart - templateStart,
          end: regionEnd - templateStart,
          expr: this.classifyExpression(span.expression, templateStart),
        });
        prevEnd = span.literal.getEnd();
      });
    }

    const substituted = substitute(text, placeholders);
    const documentId = `${this.sf.fileName}#${templateStart}`;

    const info: TemplateInfo = {
      documentId,
      node,
      templateStart,
      templateEnd,
      kind,
      expressions,
    };
    this.templates.push(info);

    let sourceTypeId: number | null = null;
    let parentTypeId: number | null = null;
    let sourceTypeName: string | null = null;
    let sourceMembers: SourceMember[] | null = null;
    let typeArgInsertOffset: number | null = null;

    if (kind === 'html') {
      let sourceTypeNode: tslib.TypeNode | undefined = node.typeArguments?.[0];
      const parentTypeNode = node.typeArguments?.[1];
      let inheritedFrom: TemplateInfo | undefined;
      if (!sourceTypeNode) {
        inheritedFrom = this.enclosingHtmlTemplate(node);
        typeArgInsertOffset = node.tag.getEnd();
      }
      if (sourceTypeNode) {
        try {
          const type = this.checker.getTypeFromTypeNode(sourceTypeNode);
          sourceTypeId = this.interner.idOfType(type);
          sourceTypeName = this.typeText(type);
          sourceMembers = this.membersOfType(type, node);
        } catch {
          // Unresolvable type argument: treat as untyped.
        }
      } else if (inheritedFrom) {
        const inherited = this.documents.find((d) => d.id === inheritedFrom.documentId);
        if (inherited) {
          sourceTypeId = inherited.sourceTypeId ?? null;
          parentTypeId = inherited.parentTypeId ?? null;
          sourceTypeName = inherited.sourceTypeName ?? null;
          sourceMembers = inherited.sourceMembers ?? null;
          typeArgInsertOffset = null;
        }
      }
      if (parentTypeNode) {
        try {
          parentTypeId = this.interner.idOfType(this.checker.getTypeFromTypeNode(parentTypeNode));
        } catch {
          // Leave unset.
        }
      }
    }

    this.documents.push({
      id: documentId,
      fileName: this.sf.fileName,
      templateStart,
      kind,
      text: substituted,
      placeholders,
      sourceTypeId,
      parentTypeId,
      sourceTypeName,
      sourceMembers,
      componentTag: null,
      typeArgInsertOffset,
    });
  }

  /**
   * The nearest enclosing FAST html template a bare inner template inherits
   * its source type from — `when(…, html`…`)`. A `repeat` item template is
   * excluded: its source is the item type, which only an explicit type
   * argument can state.
   */
  private enclosingHtmlTemplate(
    node: tslib.TaggedTemplateExpression,
  ): TemplateInfo | undefined {
    const call = node.parent;
    if (this.ts.isCallExpression(call)) {
      const index = call.arguments.indexOf(node);
      if (index >= 1 && this.fastExportName(call.expression) === 'repeat') {
        return undefined;
      }
    }
    let current: tslib.Node | undefined = node.parent;
    while (current) {
      if (this.ts.isTaggedTemplateExpression(current)) {
        const found = this.templates.find((t) => t.node === current);
        if (found && found.kind === 'html') return found;
      }
      current = current.parent;
    }
    return undefined;
  }

  private membersOfType(type: tslib.Type, location: tslib.Node): SourceMember[] {
    const out: SourceMember[] = [];
    let properties: tslib.Symbol[];
    try {
      properties = type.getProperties();
    } catch {
      return out;
    }
    for (const property of properties.slice(0, MAX_SOURCE_MEMBERS)) {
      const name = property.getName();
      if (name.startsWith('__')) continue;
      const declarations = property.getDeclarations() ?? [];
      const fromLib = declarations.every((d) => isDefaultLibrary(d.getSourceFile().fileName));
      if (fromLib) {
        // Present for membership checks (`ref('style')` is legal), but not
        // worth a type computation each.
        out.push({ name, isFunction: false });
        continue;
      }
      let typeText: string | null = null;
      let isFunction = false;
      try {
        const memberType = this.checker.getTypeOfSymbolAtLocation(property, location);
        typeText = this.typeText(memberType);
        isFunction = memberType.getCallSignatures().length > 0;
      } catch {
        // Keep the name.
      }
      const nameNode = declarations[0] && this.ts.getNameOfDeclaration(declarations[0]);
      out.push({
        name,
        typeText,
        documentation: this.documentationOf(property),
        declSpan: nameNode ? this.span(nameNode) : null,
        isFunction,
      });
    }
    return out;
  }

  // ----------------------------------------------------- expression classes

  private classifyExpression(rawExpr: tslib.Expression, templateStart: number): ExprInfo {
    const ts = this.ts;
    let expr = rawExpr;
    while (ts.isParenthesizedExpression(expr) || ts.isAsExpression(expr)) {
      expr = expr.expression;
    }

    const info: ExprInfo = { kind: 'other' };

    if (ts.isArrowFunction(expr)) info.kind = 'arrow';
    else if (ts.isFunctionExpression(expr)) info.kind = 'function';
    else if (ts.isCallExpression(expr)) info.kind = 'call';
    else if (ts.isIdentifier(expr)) info.kind = 'identifier';
    else if (ts.isPropertyAccessExpression(expr) || ts.isElementAccessExpression(expr)) {
      info.kind = 'propertyAccess';
    } else if (
      ts.isStringLiteralLike(expr) ||
      ts.isNumericLiteral(expr) ||
      expr.kind === ts.SyntaxKind.TrueKeyword ||
      expr.kind === ts.SyntaxKind.FalseKeyword ||
      expr.kind === ts.SyntaxKind.NullKeyword
    ) {
      info.kind = 'literal';
      info.isConstant = true;
    } else if (ts.isTaggedTemplateExpression(expr)) {
      info.kind = 'template';
      info.isDirectiveValue = true;
    }

    if (ts.isCallExpression(expr)) {
      // `html.partial(…)` defeats analysis by construction.
      if (
        ts.isPropertyAccessExpression(expr.expression) &&
        expr.expression.name.text === 'partial' &&
        this.fastExportName(expr.expression.expression) === 'html'
      ) {
        info.isPartial = true;
        return info;
      }
      const directiveName = this.fastExportName(expr.expression);
      if (directiveName && FAST_DIRECTIVES.has(directiveName)) {
        const directive: DirectiveInfo = { name: directiveName };
        const arg = expr.arguments[0];
        if (arg && ts.isStringLiteralLike(arg)) {
          directive.argString = arg.text;
          directive.argStart = arg.getStart() + 1 - templateStart;
          directive.argEnd = arg.getEnd() - 1 - templateStart;
        }
        info.directive = directive;
        info.isDirectiveValue = true;
      }
    }

    try {
      const type = this.checker.getTypeAtLocation(expr);
      const apparent = this.checker.getApparentType(type);
      info.isFunctionType = apparent.getCallSignatures().length > 0;
      if (info.isConstant === undefined) {
        info.isConstant = this.isConstantExpression(expr, type);
      }
      if (!info.isDirectiveValue && this.typeIsFromFastElement(type)) {
        info.isDirectiveValue = true;
      }
    } catch {
      info.isFunctionType = null;
      info.isConstant = info.isConstant ?? null;
    }

    return info;
  }

  private isConstantExpression(expr: tslib.Expression, type: tslib.Type): boolean {
    if (type.isLiteral()) return true;
    const flags = type.getFlags();
    if (flags & this.ts.TypeFlags.BooleanLiteral || flags & this.ts.TypeFlags.EnumLiteral) {
      return true;
    }
    const symbol = this.checker.getSymbolAtLocation(expr);
    const declaration = symbol?.getDeclarations()?.[0];
    if (!declaration) return false;
    if (this.ts.isEnumMember(declaration)) return true;
    if (
      this.ts.isVariableDeclaration(declaration) &&
      this.ts.isVariableDeclarationList(declaration.parent) &&
      (declaration.parent.flags & this.ts.NodeFlags.Const) !== 0
    ) {
      // A module-level `const` read is a deliberate one-time binding even
      // when its type is not a literal.
      return true;
    }
    return false;
  }

  private typeIsFromFastElement(type: tslib.Type): boolean {
    const check = (t: tslib.Type): boolean => {
      const symbol = t.aliasSymbol ?? t.getSymbol();
      if (this.isFastElementSymbol(symbol)) return true;
      if (t.isUnionOrIntersection()) return t.types.some(check);
      const bases = (t as tslib.InterfaceType).getBaseTypes?.();
      return bases?.some(check) ?? false;
    };
    try {
      return check(type);
    } catch {
      return false;
    }
  }

  // ------------------------------------------------------------- components

  private maybeDecoratedComponent(cls: tslib.ClassDeclaration): void {
    if (!this.ts.canHaveDecorators(cls)) return;
    for (const decorator of this.ts.getDecorators(cls) ?? []) {
      if (!this.ts.isCallExpression(decorator.expression)) continue;
      if (this.fastExportName(decorator.expression.expression) !== 'customElement') continue;
      const nameOrDef = decorator.expression.arguments[0];
      if (nameOrDef) this.buildComponent(cls, nameOrDef, 'decorator');
      return;
    }
  }

  private maybeDefineCall(call: tslib.CallExpression): void {
    const callee = call.expression;
    if (!this.ts.isPropertyAccessExpression(callee) || callee.name.text !== 'define') return;
    const defineSymbol = this.resolvedSymbol(callee.name);
    if (!this.isFastElementSymbol(defineSymbol)) return;

    const targetSymbol = this.resolvedSymbol(callee.expression);
    const targetName = targetSymbol?.getName();
    let cls: tslib.ClassDeclaration | undefined;
    let nameOrDef: tslib.Expression | undefined;
    if (targetName === 'FASTElement') {
      // FASTElement.define(MyClass, nameOrDef?)
      const classArg = call.arguments[0];
      const classSymbol = classArg && this.resolvedSymbol(classArg);
      cls = classSymbol
        ?.getDeclarations()
        ?.find((d): d is tslib.ClassDeclaration => this.ts.isClassDeclaration(d));
      nameOrDef = call.arguments[1];
    } else {
      // MyClass.define(nameOrDef)
      cls = targetSymbol
        ?.getDeclarations()
        ?.find((d): d is tslib.ClassDeclaration => this.ts.isClassDeclaration(d));
      nameOrDef = call.arguments[0];
    }
    if (!cls || !nameOrDef) return;
    if (cls.getSourceFile() !== this.sf) return;
    this.buildComponent(cls, nameOrDef, 'define');
  }

  private buildComponent(
    cls: tslib.ClassDeclaration,
    nameOrDef: tslib.Expression,
    origin: 'decorator' | 'define',
  ): void {
    if (this.componentClasses.has(cls)) return;
    this.componentClasses.add(cls);

    const className = cls.name?.text ?? '(anonymous)';
    const classSymbol = cls.name ? this.checker.getSymbolAtLocation(cls.name) : undefined;
    let sourceTypeId: number | null = null;
    try {
      if (classSymbol) {
        sourceTypeId = this.interner.idOfType(this.checker.getDeclaredTypeOfSymbol(classSymbol));
      }
    } catch {
      // Keep null.
    }

    // The definition: either the name expression directly, or the object.
    let nameExpr: tslib.Expression | undefined;
    let defObject: tslib.ObjectLiteralExpression | undefined;
    if (this.ts.isObjectLiteralExpression(nameOrDef)) {
      defObject = nameOrDef;
      const nameProp = findProperty(this.ts, nameOrDef, 'name');
      nameExpr = nameProp?.initializer;
    } else {
      nameExpr = nameOrDef;
    }

    let tagName: string | null = null;
    let tagNameSpan: FileSpan | null = null;
    if (nameExpr) {
      try {
        const type = this.checker.getTypeAtLocation(nameExpr);
        if (type.isStringLiteral()) tagName = type.value;
      } catch {
        // Not resolvable: register with tagName null.
      }
      tagNameSpan = this.tagNameStringSpan(nameExpr);
    }

    let hasShadowRoot = true;
    if (defObject) {
      const shadow = findProperty(this.ts, defObject, 'shadowOptions');
      if (shadow && shadow.initializer.kind === this.ts.SyntaxKind.NullKeyword) {
        hasShadowRoot = false;
      }
    }

    const attributes: MemberFact[] = [];
    const properties: MemberFact[] = [];
    const events: EventFact[] = [];
    const slots: NamedFact[] = [];
    const cssParts: NamedFact[] = [];
    const cssProperties: NamedFact[] = [];

    this.collectClassChain(cls, tagName, attributes, properties, events);
    if (defObject) {
      this.collectDefinitionAttributes(cls, defObject, attributes);
    }
    this.collectJsDocFacts(cls, attributes, properties, events, slots, cssParts, cssProperties);

    const templateDocumentId = defObject
      ? this.resolveTemplateReference(findProperty(this.ts, defObject, 'template')?.initializer, 'html')
      : null;
    const styleDocumentIds: string[] = [];
    if (defObject) {
      const styles = findProperty(this.ts, defObject, 'styles')?.initializer;
      for (const target of this.styleExpressions(styles)) {
        const id = this.resolveTemplateReference(target, 'css');
        if (id) styleDocumentIds.push(id);
      }
    }

    const inTagNameMap = tagName !== null && this.isInTagNameMap(tagName);
    if (!inTagNameMap && tagName && cls.name) {
      this.discovery.push({
        ruleId: 'no-missing-element-type-definition',
        severity: 'warning',
        message: `'${tagName}' is not on HTMLElementTagNameMap, so document.createElement('${tagName}') and querySelector results are typed as plain HTMLElement.`,
        ...this.span(cls.name),
      });
    }

    this.components.push({
      tagName,
      className,
      tagNameSpan,
      declSpan: cls.name ? this.span(cls.name) : this.span(cls),
      declarationId: this.interner.idOfNode(cls),
      sourceTypeId,
      attributes,
      properties,
      events,
      slots,
      cssParts,
      cssProperties,
      hasShadowRoot,
      templateDocumentId,
      styleDocumentIds,
      documentation: this.documentationOf(classSymbol),
      inTagNameMap,
      origin,
    });
  }

  /** Where the tag-name *string* lives, following one `const` hop. */
  private tagNameStringSpan(nameExpr: tslib.Expression): FileSpan | null {
    if (this.ts.isStringLiteralLike(nameExpr)) {
      return this.literalContentsSpan(nameExpr);
    }
    const symbol = this.resolvedSymbol(nameExpr);
    const declaration = symbol?.getDeclarations()?.[0];
    if (
      declaration &&
      this.ts.isVariableDeclaration(declaration) &&
      declaration.initializer &&
      this.ts.isStringLiteralLike(declaration.initializer)
    ) {
      return this.literalContentsSpan(declaration.initializer);
    }
    return null;
  }

  private isInTagNameMap(tagName: string): boolean {
    try {
      const resolveName = (
        this.checker as unknown as {
          resolveName?(
            name: string,
            location: tslib.Node | undefined,
            meaning: number,
            excludeGlobals: boolean,
          ): tslib.Symbol | undefined;
        }
      ).resolveName;
      if (!resolveName) return true; // cannot check → do not report
      const symbol = resolveName.call(
        this.checker,
        'HTMLElementTagNameMap',
        undefined,
        this.ts.SymbolFlags.Type,
        false,
      );
      if (!symbol) return true;
      const type = this.checker.getDeclaredTypeOfSymbol(symbol);
      return type.getProperty(tagName) !== undefined;
    } catch {
      return true;
    }
  }

  // ----------------------------------------------------------- class chain

  /** Own members, then the inheritance chain up to FASTElement (P2-06). */
  private collectClassChain(
    cls: tslib.ClassDeclaration,
    tagName: string | null,
    attributes: MemberFact[],
    properties: MemberFact[],
    events: EventFact[],
  ): void {
    let current: tslib.ClassDeclaration | undefined = cls;
    let depth = 0;
    const seen = new Set<string>();
    while (current && depth < 16) {
      const origin = depth === 0 ? 'decorator' : 'inherited';
      this.collectClassMembers(current, tagName, origin, seen, attributes, properties);
      this.collectEmitCalls(current, events);
      current = this.baseClassOf(current);
      depth += 1;
    }
  }

  private baseClassOf(cls: tslib.ClassDeclaration): tslib.ClassDeclaration | undefined {
    const extendsClause = cls.heritageClauses?.find(
      (h) => h.token === this.ts.SyntaxKind.ExtendsKeyword,
    );
    const target = extendsClause?.types[0]?.expression;
    if (!target) return undefined;
    // `extends SomeMixin(FASTElement)`: take the call's return type's class
    // declaration where the checker offers one; give up quietly otherwise.
    const node = this.ts.isCallExpression(target) ? target : target;
    const symbol = this.ts.isCallExpression(node)
      ? this.checker.getTypeAtLocation(node)?.getSymbol()
      : this.resolvedSymbol(node);
    if (this.isFastElementSymbol(symbol ?? undefined)) return undefined;
    const declaration = symbol
      ?.getDeclarations()
      ?.find((d): d is tslib.ClassDeclaration => this.ts.isClassDeclaration(d));
    return declaration;
  }

  private collectClassMembers(
    cls: tslib.ClassDeclaration,
    tagName: string | null,
    origin: string,
    seen: Set<string>,
    attributes: MemberFact[],
    properties: MemberFact[],
  ): void {
    for (const member of cls.members) {
      const isProperty = this.ts.isPropertyDeclaration(member);
      const isAccessor = this.ts.isGetAccessor(member) || this.ts.isSetAccessor(member);
      if (!isProperty && !isAccessor) continue;
      if (!member.name || !this.ts.isIdentifier(member.name)) continue;
      const propertyName = member.name.text;
      if (seen.has(propertyName)) continue; // shadowed by a subclass
      if (!this.ts.canHaveDecorators(member)) continue;

      for (const decorator of this.ts.getDecorators(member) ?? []) {
        const callee = this.ts.isCallExpression(decorator.expression)
          ? decorator.expression.expression
          : decorator.expression;
        const name = this.fastExportName(callee);
        if (name !== 'attr' && name !== 'observable' && name !== 'volatile') continue;
        seen.add(propertyName);

        const memberType = this.tryTypeOf(member.name);
        const base: MemberFact = {
          name: propertyName,
          typeText: memberType ? this.typeText(memberType) : null,
          typeId: memberType ? this.interner.idOfType(memberType) : null,
          declarationId: this.interner.idOfNode(member),
          declSpan: this.span(member.name),
          documentation: this.documentationOf(this.checker.getSymbolAtLocation(member.name)),
          origin,
          visibility: visibilityOf(this.ts, member),
          values: memberType ? stringLiteralUnion(memberType) : [],
        };

        if (name === 'observable' || name === 'volatile') {
          properties.push(base);
          break;
        }

        // @attr — bare or with a config object.
        let attributeName = propertyName.toLowerCase();
        let mode = 'reflect';
        let hasConverter = false;
        if (this.ts.isCallExpression(decorator.expression)) {
          const config = decorator.expression.arguments[0];
          if (config && this.ts.isObjectLiteralExpression(config)) {
            const attrProp = findProperty(this.ts, config, 'attribute');
            if (attrProp && this.ts.isStringLiteralLike(attrProp.initializer)) {
              attributeName = attrProp.initializer.text;
              this.checkAttributeName(attrProp.initializer);
            }
            const modeProp = findProperty(this.ts, config, 'mode');
            if (modeProp && this.ts.isStringLiteralLike(modeProp.initializer)) {
              mode = modeProp.initializer.text;
            }
            hasConverter = findProperty(this.ts, config, 'converter') !== undefined;
          }
        }
        this.checkAttrConfig(member, memberType, mode, hasConverter, tagName);
        this.checkAttrVisibility(member, base.visibility ?? null);
        attributes.push({
          ...base,
          name: attributeName,
          propertyName,
          mode,
        });
        break;
      }
    }
  }

  private tryTypeOf(node: tslib.Node): tslib.Type | undefined {
    try {
      return this.checker.getTypeAtLocation(node);
    } catch {
      return undefined;
    }
  }

  /** R10: the configured attribute name must be legal. */
  private checkAttributeName(literal: tslib.StringLiteralLike): void {
    const name = literal.text;
    if (/^[a-zA-Z][^\s"'<>/=]*$/.test(name)) return;
    this.discovery.push({
      ruleId: 'no-invalid-attribute-name',
      severity: 'error',
      message: `'${name}' is not a valid attribute name.`,
      ...this.literalContentsSpan(literal),
    });
  }

  /** R19: `@attr({ mode })` must agree with the declared type. */
  private checkAttrConfig(
    member: tslib.ClassElement,
    memberType: tslib.Type | undefined,
    mode: string,
    hasConverter: boolean,
    _tagName: string | null,
  ): void {
    if (!memberType || !member.name) return;
    const primitiveMask =
      this.ts.TypeFlags.StringLike |
      this.ts.TypeFlags.NumberLike |
      this.ts.TypeFlags.BigIntLike |
      this.ts.TypeFlags.BooleanLike |
      this.ts.TypeFlags.EnumLike |
      this.ts.TypeFlags.Any |
      this.ts.TypeFlags.Unknown |
      this.ts.TypeFlags.Undefined |
      this.ts.TypeFlags.Null |
      this.ts.TypeFlags.Never;
    const parts = memberType.isUnion() ? memberType.types : [memberType];
    const allPrimitive = parts.every((t) => (t.getFlags() & primitiveMask) !== 0);
    const booleanish = parts.every(
      (t) =>
        (t.getFlags() &
          (this.ts.TypeFlags.BooleanLike |
            this.ts.TypeFlags.Any |
            this.ts.TypeFlags.Unknown |
            this.ts.TypeFlags.Undefined |
            this.ts.TypeFlags.Null)) !== 0,
    );
    if (mode === 'boolean' && !booleanish) {
      this.discovery.push({
        ruleId: 'no-incompatible-attr-config',
        severity: 'warning',
        message: `mode: "boolean" reflects presence/absence, but this property is typed '${this.typeText(memberType)}' — a boolean attribute wants a boolean property.`,
        ...this.span(member.name),
      });
      return;
    }
    if (mode !== 'boolean' && !hasConverter && !allPrimitive) {
      const callable =
        this.checker.getApparentType(memberType).getCallSignatures().length > 0;
      // Dates and arrays have a passable string form; keep the rule to plain
      // object shapes to stay quiet on intentional code.
      if (
        !callable &&
        !isArrayLike(this.ts, this.checker, memberType) &&
        this.typeText(memberType) !== 'Date'
      ) {
        this.discovery.push({
          ruleId: 'no-incompatible-attr-config',
          severity: 'warning',
          message: `'${this.typeText(memberType)}' has no useful attribute string form — @attr here needs a converter, or @observable was meant.`,
          ...this.span(member.name),
        });
      }
    }
  }

  /** R20: an attribute is part of the public DOM contract. */
  private checkAttrVisibility(member: tslib.ClassElement, visibility: string | null): void {
    if (visibility !== 'private' && visibility !== 'protected') return;
    if (!member.name) return;
    this.discovery.push({
      ruleId: 'no-attr-visibility-mismatch',
      severity: 'warning',
      message: `An @attr is part of the element's public DOM contract — a ${visibility} member cannot be one. Use @observable for internal state.`,
      ...this.span(member.name),
    });
  }

  /** `attributes: [...]` in the definition — no decorator involved (P2-05). */
  private collectDefinitionAttributes(
    _cls: tslib.ClassDeclaration,
    defObject: tslib.ObjectLiteralExpression,
    attributes: MemberFact[],
  ): void {
    const attrsProp = findProperty(this.ts, defObject, 'attributes');
    if (!attrsProp || !this.ts.isArrayLiteralExpression(attrsProp.initializer)) return;
    for (const element of attrsProp.initializer.elements) {
      if (this.ts.isStringLiteralLike(element)) {
        attributes.push({
          name: element.text.toLowerCase(),
          propertyName: element.text,
          mode: 'reflect',
          declSpan: this.literalContentsSpan(element),
          origin: 'definition',
        });
      } else if (this.ts.isObjectLiteralExpression(element)) {
        const property = findProperty(this.ts, element, 'property');
        if (!property || !this.ts.isStringLiteralLike(property.initializer)) continue;
        const propertyName = property.initializer.text;
        const attrProp = findProperty(this.ts, element, 'attribute');
        const modeProp = findProperty(this.ts, element, 'mode');
        attributes.push({
          name:
            attrProp && this.ts.isStringLiteralLike(attrProp.initializer)
              ? attrProp.initializer.text
              : propertyName.toLowerCase(),
          propertyName,
          mode:
            modeProp && this.ts.isStringLiteralLike(modeProp.initializer)
              ? modeProp.initializer.text
              : 'reflect',
          declSpan: this.literalContentsSpan(property.initializer),
          origin: 'definition',
        });
      }
    }
  }

  /** `this.$emit("name", detail)` → an event (P2-07). */
  private collectEmitCalls(cls: tslib.ClassDeclaration, events: EventFact[]): void {
    const visit = (node: tslib.Node): void => {
      if (
        this.ts.isCallExpression(node) &&
        this.ts.isPropertyAccessExpression(node.expression) &&
        node.expression.name.text === '$emit' &&
        node.expression.expression.kind === this.ts.SyntaxKind.ThisKeyword
      ) {
        const nameArg = node.arguments[0];
        let name: string | undefined;
        let declSpan: FileSpan | null = null;
        if (nameArg && this.ts.isStringLiteralLike(nameArg)) {
          name = nameArg.text;
          declSpan = this.literalContentsSpan(nameArg);
        } else if (nameArg) {
          const type = this.tryTypeOf(nameArg);
          if (type?.isStringLiteral()) {
            name = type.value;
            declSpan = this.span(nameArg);
          }
        }
        if (name && !events.some((e) => e.name === name)) {
          const detail = node.arguments[1];
          const detailType = detail && this.tryTypeOf(detail);
          events.push({
            name,
            typeText: detailType ? this.typeText(detailType) : null,
            declSpan,
            documentation: null,
          });
        }
      }
      this.ts.forEachChild(node, visit);
    };
    visit(cls);
  }

  /** JSDoc `@slot` / `@fires` / `@csspart` / `@cssprop` / `@attr` / `@prop`. */
  private collectJsDocFacts(
    cls: tslib.ClassDeclaration,
    attributes: MemberFact[],
    properties: MemberFact[],
    events: EventFact[],
    slots: NamedFact[],
    cssParts: NamedFact[],
    cssProperties: NamedFact[],
  ): void {
    for (const tag of this.ts.getJSDocTags(cls)) {
      const tagName = tag.tagName.text;
      const comment = typeof tag.comment === 'string'
        ? tag.comment
        : (tag.comment?.map((c) => c.text).join('') ?? '');
      const { name, description } = parseJsDocNameComment(comment);
      const declSpan: FileSpan = {
        fileName: this.sf.fileName,
        start: tag.getStart(),
        end: tag.getEnd(),
      };
      const named: NamedFact = { name, documentation: description || null, declSpan };
      switch (tagName) {
        case 'slot':
          slots.push(named);
          break;
        case 'csspart':
          cssParts.push(named);
          break;
        case 'cssprop':
        case 'cssproperty':
          cssProperties.push(named);
          break;
        case 'fires':
        case 'event':
          if (name && !events.some((e) => e.name === name)) {
            events.push({ name, typeText: null, declSpan, documentation: description || null });
          }
          break;
        case 'attr':
        case 'attribute':
          if (name && !attributes.some((a) => a.name === name)) {
            attributes.push({
              name,
              propertyName: null,
              mode: 'reflect',
              declSpan,
              documentation: description || null,
              origin: 'jsdoc',
            });
          }
          break;
        case 'prop':
        case 'property':
          if (name && !properties.some((p) => p.name === name)) {
            properties.push({
              name,
              declSpan,
              documentation: description || null,
              origin: 'jsdoc',
            });
          }
          break;
        default:
          break;
      }
    }
  }

  // ------------------------------------------------------ template linking

  /** Follow `template:` / `styles:` initializers to the tagged template they
   * name — possibly in another file — and return its document id. */
  private resolveTemplateReference(
    expr: tslib.Expression | undefined,
    kind: 'html' | 'css',
  ): string | null {
    if (!expr) return null;
    let target: tslib.Node = expr;
    if (this.ts.isIdentifier(expr) || this.ts.isPropertyAccessExpression(expr)) {
      const declaration = this.resolvedSymbol(
        this.ts.isPropertyAccessExpression(expr) ? expr.name : expr,
      )?.getDeclarations()?.[0];
      if (
        declaration &&
        this.ts.isVariableDeclaration(declaration) &&
        declaration.initializer
      ) {
        target = declaration.initializer;
      }
    }
    if (this.ts.isTaggedTemplateExpression(target)) {
      const tagKind = this.fastExportName(target.tag);
      if (tagKind === kind) {
        const file = target.getSourceFile();
        return `${file.fileName}#${target.template.getStart() + 1}`;
      }
    }
    return null;
  }

  private styleExpressions(expr: tslib.Expression | undefined): tslib.Expression[] {
    if (!expr) return [];
    if (this.ts.isArrayLiteralExpression(expr)) return [...expr.elements];
    return [expr];
  }

  private linkComponentsToDocuments(): void {
    for (const component of this.components) {
      if (!component.tagName) continue;
      for (const document of this.documents) {
        if (
          document.id === component.templateDocumentId ||
          component.styleDocumentIds?.includes(document.id)
        ) {
          document.componentTag = component.tagName;
        }
      }
    }
  }
}

// ------------------------------------------------------------------ helpers

export function isFastElementPath(fileName: string): boolean {
  return (
    fileName.includes('/@microsoft/fast-element/') ||
    fileName.includes('\\@microsoft\\fast-element\\')
  );
}

function isDefaultLibrary(fileName: string): boolean {
  return /\/typescript\/lib\/lib\.|\/lib\.(dom|es|webworker|decorators|scripthost)[^/]*\.d\.ts$/.test(
    fileName,
  );
}

function findProperty(
  ts: Ts,
  object: tslib.ObjectLiteralExpression,
  name: string,
): { initializer: tslib.Expression } | undefined {
  for (const property of object.properties) {
    if (
      ts.isPropertyAssignment(property) &&
      (ts.isIdentifier(property.name) || ts.isStringLiteralLike(property.name)) &&
      property.name.text === name
    ) {
      return { initializer: property.initializer };
    }
    // `{ template }` — the identifier doubles as the value.
    if (ts.isShorthandPropertyAssignment(property) && property.name.text === name) {
      return { initializer: property.name };
    }
  }
  return undefined;
}

function visibilityOf(ts: Ts, member: tslib.ClassElement): string {
  const modifiers = ts.canHaveModifiers(member) ? (ts.getModifiers(member) ?? []) : [];
  for (const modifier of modifiers) {
    if (modifier.kind === ts.SyntaxKind.PrivateKeyword) return 'private';
    if (modifier.kind === ts.SyntaxKind.ProtectedKeyword) return 'protected';
  }
  if (ts.isPropertyDeclaration(member) && ts.isPrivateIdentifier(member.name)) return 'private';
  return 'public';
}

function stringLiteralUnion(type: tslib.Type): string[] {
  if (!type.isUnion()) return [];
  const values: string[] = [];
  for (const part of type.types) {
    if (part.isStringLiteral()) values.push(part.value);
    else return [];
  }
  return values;
}

function isArrayLike(ts: Ts, checker: tslib.TypeChecker, type: tslib.Type): boolean {
  void ts;
  try {
    return (
      (checker as unknown as { isArrayLikeType?(t: tslib.Type): boolean }).isArrayLikeType?.(
        type,
      ) ?? false
    );
  } catch {
    return false;
  }
}

/** `@slot name - description`, `@slot - description` (default slot). */
function parseJsDocNameComment(comment: string): { name: string; description: string } {
  const trimmed = comment.trim();
  if (!trimmed) return { name: '', description: '' };
  if (/^-(\s|$)/.test(trimmed)) {
    return { name: '', description: trimmed.slice(1).trim() };
  }
  const match = /^(\S+)\s*(?:-\s*)?(.*)$/s.exec(trimmed);
  if (!match) return { name: '', description: trimmed };
  return { name: match[1], description: match[2].trim() };
}

/** The virtual-document substitution: length preserved, index recoverable. */
export function substitute(text: string, placeholders: PlaceholderFact[]): string {
  let out = '';
  let cursor = 0;
  for (const placeholder of placeholders) {
    out += text.slice(cursor, placeholder.start);
    const length = placeholder.end - placeholder.start;
    const index36 = placeholder.index.toString(36);
    if (length >= index36.length + 3) {
      out += `__${index36}${'_'.repeat(length - 2 - index36.length)}`;
    } else {
      out += '_'.repeat(length);
    }
    cursor = placeholder.end;
  }
  out += text.slice(cursor);
  return out;
}
