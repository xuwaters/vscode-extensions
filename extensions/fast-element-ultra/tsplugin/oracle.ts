/**
 * The type oracle: answers the batch of binding facts the engine emitted for
 * one document, in TypeScript, with the checker (decision 0002).
 *
 * Where fast-analyzer used `ts-simple-type`, this uses the checker itself —
 * `checker.isTypeAssignableTo` (internal but stable since TS 4.4) plus
 * `TypeFlags` heuristics for the attribute-coercion rules. `ts-simple-type`
 * has no TypeScript 6 support, and being *differently* right about
 * assignability than the compiler the user builds with is worse than leaning
 * on an internal API (decision 0010).
 *
 * FAST's attribute-removal semantics are modelled here directly: `null` and
 * `undefined` are stripped from the bound value's type before any attribute
 * comparison, because FAST removes the attribute rather than writing "null".
 */

import type * as tslib from 'typescript';

import type { BindingFact, ProtocolDiagnostic, ProtocolFix } from './protocol.js';

type Ts = typeof tslib;

/** Events whose default action a non-`true`-returning handler eats (F6). */
const IMPLICIT_PREVENT_DEFAULT_EVENTS = new Set([
  'keydown', 'keyup', 'keypress', 'input', 'beforeinput', 'paste', 'cut',
  'wheel', 'touchstart', 'touchmove', 'contextmenu',
]);

export interface OracleOptions {
  ts: Ts;
  checker: tslib.TypeChecker;
  /** The template's expressions, by index. */
  expressions: readonly tslib.Expression[];
  nodeOf(id: number): tslib.Node | undefined;
  severity(ruleId: string): 'warning' | 'error' | 'suggestion' | undefined;
}

export function answerFacts(
  facts: BindingFact[],
  options: OracleOptions,
): ProtocolDiagnostic[] {
  const oracle = new Oracle(options);
  const out: ProtocolDiagnostic[] = [];
  for (const fact of facts) {
    try {
      oracle.answer(fact, out);
    } catch {
      // A single unanswerable fact must never take the pass down.
    }
  }
  return out;
}

class Oracle {
  private readonly ts: Ts;
  private readonly checker: tslib.TypeChecker;

  constructor(private readonly options: OracleOptions) {
    this.ts = options.ts;
    this.checker = options.checker;
  }

  answer(fact: BindingFact, out: ProtocolDiagnostic[]): void {
    switch (fact.kind) {
      case 'event':
        this.answerEvent(fact, out);
        break;
      case 'attribute':
        this.answerAttribute(fact, out);
        break;
      case 'booleanAttribute':
        this.answerBooleanAttribute(fact, out);
        break;
      case 'property':
        this.answerProperty(fact, out);
        break;
    }
  }

  // -------------------------------------------------------------- plumbing

  private report(
    out: ProtocolDiagnostic[],
    ruleId: string,
    fact: BindingFact,
    message: string,
    fixes: ProtocolFix[] = [],
  ): void {
    const severity = this.options.severity(ruleId);
    if (!severity) return;
    out.push({ ruleId, severity, message, start: fact.start, end: fact.end, fixes });
  }

  private expressionType(fact: BindingFact): tslib.Type | undefined {
    if (fact.expressionIndex == null) return undefined;
    const expr = this.options.expressions[fact.expressionIndex];
    if (!expr) return undefined;
    return this.checker.getTypeAtLocation(expr);
  }

  /**
   * The value a binding produces: a function expression is the reactive
   * form, so the bound value's type is its return type.
   */
  private boundValueType(fact: BindingFact): tslib.Type | undefined {
    const type = this.expressionType(fact);
    if (!type) return undefined;
    const signatures = this.checker.getApparentType(type).getCallSignatures();
    if (signatures.length > 0) {
      return signatures[0].getReturnType();
    }
    return type;
  }

  private targetType(fact: BindingFact): tslib.Type | undefined {
    if (fact.targetDeclarationId != null) {
      const node = this.options.nodeOf(fact.targetDeclarationId);
      if (node) {
        try {
          return this.checker.getTypeAtLocation(node);
        } catch {
          return undefined;
        }
      }
    }
    if (fact.targetBuiltin && fact.memberName) {
      const type = this.builtinMemberType(fact.tagName, fact.memberName);
      // A builtin's attribute is only *reflected* by the IDL property of the
      // same name, and the reflection is a string: an attribute binding calls
      // setAttribute, never the property setter. Where the property holds
      // something else — `style` is a CSSStyleDeclaration, `form` and `list`
      // are elements — it says nothing about what the attribute accepts, so
      // there is no target to check against. A `:style` property binding does
      // go through the setter, and keeps the property's own type.
      if (
        type &&
        fact.kind !== 'property' &&
        !this.isPrimitiveLike(this.stripNullish(type))
      ) {
        return undefined;
      }
      return type;
    }
    return undefined;
  }

  /** `HTMLElementTagNameMap[tag][member]`, when the DOM lib is resolvable. */
  private builtinMemberType(tag: string, member: string): tslib.Type | undefined {
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
      if (!resolveName) return undefined;
      const mapSymbol = resolveName.call(
        this.checker,
        'HTMLElementTagNameMap',
        undefined,
        this.ts.SymbolFlags.Type,
        false,
      );
      if (!mapSymbol) return undefined;
      const mapType = this.checker.getDeclaredTypeOfSymbol(mapSymbol);
      const tagProp = mapType.getProperty(tag);
      if (!tagProp) return undefined;
      const declaration = tagProp.getDeclarations()?.[0];
      if (!declaration) return undefined;
      const elementType = this.checker.getTypeOfSymbolAtLocation(tagProp, declaration);
      const memberProp = elementType.getProperty(member);
      if (!memberProp) return undefined;
      return this.checker.getTypeOfSymbolAtLocation(memberProp, declaration);
    } catch {
      return undefined;
    }
  }

  private isAssignable(source: tslib.Type, target: tslib.Type): boolean | undefined {
    const impl = (
      this.checker as unknown as {
        isTypeAssignableTo?(source: tslib.Type, target: tslib.Type): boolean;
      }
    ).isTypeAssignableTo;
    if (!impl) return undefined;
    try {
      return impl.call(this.checker, source, target);
    } catch {
      return undefined;
    }
  }

  /** FAST removes the attribute on null/undefined rather than coercing. */
  private stripNullish(type: tslib.Type): tslib.Type {
    try {
      return this.checker.getNonNullableType(type);
    } catch {
      return type;
    }
  }

  private flagsOf(type: tslib.Type): number {
    return this.checker.getApparentType(type).getFlags() | type.getFlags();
  }

  private everyUnionPart(type: tslib.Type, predicate: (t: tslib.Type) => boolean): boolean {
    if (type.isUnion()) return type.types.every(predicate);
    return predicate(type);
  }

  private someUnionPart(type: tslib.Type, predicate: (t: tslib.Type) => boolean): boolean {
    if (type.isUnion()) return type.types.some(predicate);
    return predicate(type);
  }

  private isAnyish(type: tslib.Type): boolean {
    return (type.getFlags() & (this.ts.TypeFlags.Any | this.ts.TypeFlags.Unknown)) !== 0;
  }

  private isBooleanLike(type: tslib.Type): boolean {
    return this.everyUnionPart(type, (t) => (t.getFlags() & this.ts.TypeFlags.BooleanLike) !== 0);
  }

  private isPrimitiveLike(type: tslib.Type): boolean {
    const primitives =
      this.ts.TypeFlags.StringLike |
      this.ts.TypeFlags.NumberLike |
      this.ts.TypeFlags.BigIntLike |
      this.ts.TypeFlags.BooleanLike |
      this.ts.TypeFlags.EnumLike |
      this.ts.TypeFlags.Undefined |
      this.ts.TypeFlags.Null |
      this.ts.TypeFlags.Any |
      this.ts.TypeFlags.Unknown |
      this.ts.TypeFlags.Never;
    return this.everyUnionPart(type, (t) => (t.getFlags() & primitives) !== 0);
  }

  // ---------------------------------------------------------------- events

  private answerEvent(fact: BindingFact, out: ProtocolDiagnostic[]): void {
    const type = this.expressionType(fact);
    if (!type || this.isAnyish(type)) return;

    const callable =
      this.checker.getApparentType(type).getCallSignatures().length > 0 ||
      type.getProperty('handleEvent') !== undefined;
    if (!callable) {
      this.report(
        out,
        'no-noncallable-event-binding',
        fact,
        `'@${fact.memberName}' binds a value of type '${this.checker.typeToString(type)}', which is not callable — an event binding needs a function.`,
      );
      return;
    }

    // F6: on key/input events, a handler that does not return `true` has its
    // default action cancelled by FAST.
    if (
      fact.memberName &&
      IMPLICIT_PREVENT_DEFAULT_EVENTS.has(fact.memberName) &&
      this.options.severity('no-implicit-prevent-default')
    ) {
      const signatures = this.checker.getApparentType(type).getCallSignatures();
      const returnType = signatures[0]?.getReturnType();
      if (returnType && !this.isAnyish(returnType)) {
        const canBeTrue = this.someUnionPart(returnType, (t) => {
          const flags = t.getFlags();
          if ((flags & this.ts.TypeFlags.BooleanLike) !== 0) {
            // `boolean` or the literal `true` can be true; literal `false`
            // cannot.
            return !(t.isLiteral() && String((t as tslib.LiteralType).value) === 'false');
          }
          return false;
        });
        if (!canBeTrue) {
          this.report(
            out,
            'no-implicit-prevent-default',
            fact,
            `This handler returns '${this.checker.typeToString(returnType)}', so FAST calls preventDefault() on every '${fact.memberName}' — which eats the default action. Return true to keep it.`,
          );
        }
      }
    }
  }

  // ------------------------------------------------------------ attributes

  private answerAttribute(fact: BindingFact, out: ProtocolDiagnostic[]): void {
    if (fact.literal != null) {
      this.answerAttributeLiteral(fact, out);
      return;
    }
    const raw = this.boundValueType(fact);
    if (!raw || this.isAnyish(raw)) return;
    const value = this.stripNullish(raw);
    if (this.isAnyish(value)) return;

    // R15: a boolean stringifies to "false", which is truthy as an attribute.
    if (this.isBooleanLike(value)) {
      const fix: ProtocolFix = {
        label: `Use a boolean attribute: ?${fact.memberName}`,
        edits: [{ fileName: null, start: fact.start, end: fact.start, newText: '?' }],
      };
      this.report(
        out,
        'no-boolean-in-attribute-binding',
        fact,
        `'${fact.memberName}' is an attribute binding, and this expression is a boolean — it sets the string "false", which is truthy. Use ?${fact.memberName} instead.`,
        [fix],
      );
      return;
    }

    // R16: an object stringifies to "[object Object]".
    if (!this.isPrimitiveLike(value)) {
      const fix: ProtocolFix = {
        label: `Use a property binding: :${fact.memberName}`,
        edits: [{ fileName: null, start: fact.start, end: fact.start, newText: ':' }],
      };
      this.report(
        out,
        'no-complex-attribute-binding',
        fact,
        `'${fact.memberName}' is an attribute binding, and this expression's type '${this.checker.typeToString(value)}' has no useful string form — it sets "[object Object]". A property binding (:${fact.memberName}) was probably meant.`,
        [fix],
      );
      return;
    }

    // R17: the value must fit the declared attribute type, when there is one.
    const target = this.targetType(fact);
    if (!target || this.isAnyish(target)) return;
    this.checkAssignability(fact, value, target, out);
  }

  private answerAttributeLiteral(fact: BindingFact, out: ProtocolDiagnostic[]): void {
    const target = this.targetType(fact);
    if (!target || this.isAnyish(target) || fact.literal == null) return;
    const targetFlags = this.flagsOf(this.stripNullish(target));
    if ((targetFlags & this.ts.TypeFlags.NumberLike) !== 0) {
      if (fact.literal.trim() !== '' && Number.isNaN(Number(fact.literal))) {
        this.report(
          out,
          'no-incompatible-type-binding',
          fact,
          `'${fact.memberName}' is typed '${this.checker.typeToString(target)}', but "${fact.literal}" is not a number.`,
        );
      }
      return;
    }
    if (target.isUnion()) {
      const literals: string[] = [];
      for (const part of target.types) {
        if (part.isStringLiteral()) literals.push(part.value);
        else return;
      }
      if (!literals.includes(fact.literal)) {
        this.report(
          out,
          'no-incompatible-type-binding',
          fact,
          `'${fact.memberName}' expects one of ${literals.map((v) => `"${v}"`).join(', ')} — not "${fact.literal}".`,
        );
      }
    }
  }

  private answerBooleanAttribute(fact: BindingFact, out: ProtocolDiagnostic[]): void {
    const raw = this.boundValueType(fact);
    if (!raw || this.isAnyish(raw)) return;
    const value = this.stripNullish(raw);
    if (this.isAnyish(value)) return;
    const acceptable =
      this.ts.TypeFlags.BooleanLike |
      this.ts.TypeFlags.NumberLike |
      this.ts.TypeFlags.Undefined |
      this.ts.TypeFlags.Null |
      this.ts.TypeFlags.Never;
    const fits = this.everyUnionPart(value, (t) => (t.getFlags() & acceptable) !== 0);
    if (!fits) {
      this.report(
        out,
        'no-incompatible-type-binding',
        fact,
        `'?${fact.memberName}' is a boolean-attribute binding, but this expression's type is '${this.checker.typeToString(value)}' — bind a boolean.`,
      );
    }
  }

  // ------------------------------------------------------------ properties

  private answerProperty(fact: BindingFact, out: ProtocolDiagnostic[]): void {
    const value = this.boundValueType(fact);
    if (!value || this.isAnyish(value)) return;
    const target = this.targetType(fact);
    if (!target || this.isAnyish(target)) return;
    this.checkAssignability(fact, value, target, out);
  }

  private checkAssignability(
    fact: BindingFact,
    value: tslib.Type,
    target: tslib.Type,
    out: ProtocolDiagnostic[],
  ): void {
    // Attribute bindings coerce primitives through strings: a number bound
    // to a string-typed attribute is fine, and vice versa is not.
    if (fact.kind === 'attribute') {
      const targetFlags = this.flagsOf(this.stripNullish(target));
      if ((targetFlags & this.ts.TypeFlags.StringLike) !== 0 && this.isPrimitiveLike(value)) {
        const targetStripped = this.stripNullish(target);
        if (targetStripped.isUnion() || targetStripped.isStringLiteral()) {
          // A closed set: check for real.
          const ok = this.isAssignable(value, targetStripped);
          if (ok === false && this.everyUnionPart(value, (t) => t.isStringLiteral())) {
            this.report(
              out,
              'no-incompatible-type-binding',
              fact,
              `This expression's type '${this.checker.typeToString(value)}' is not assignable to '${this.checker.typeToString(target)}'.`,
            );
          }
          return;
        }
        return;
      }
      if ((targetFlags & this.ts.TypeFlags.NumberLike) !== 0) {
        const numberish = this.everyUnionPart(value, (t) => {
          const flags = t.getFlags();
          if ((flags & this.ts.TypeFlags.NumberLike) !== 0) return true;
          return t.isStringLiteral() && !Number.isNaN(Number((t as tslib.StringLiteralType).value));
        });
        if (!numberish) {
          this.report(
            out,
            'no-incompatible-type-binding',
            fact,
            `'${fact.memberName}' is typed '${this.checker.typeToString(target)}', but this expression produces '${this.checker.typeToString(value)}'.`,
          );
        }
        return;
      }
      if ((targetFlags & this.ts.TypeFlags.BooleanLike) !== 0) {
        const boolish = this.everyUnionPart(value, (t) => {
          const flags = t.getFlags();
          if ((flags & this.ts.TypeFlags.BooleanLike) !== 0) return true;
          if (!t.isStringLiteral()) return false;
          const text = (t as tslib.StringLiteralType).value;
          return text === 'true' || text === 'false' || text === '';
        });
        if (!boolish) {
          this.report(
            out,
            'no-incompatible-type-binding',
            fact,
            `'${fact.memberName}' is typed '${this.checker.typeToString(target)}', but this expression produces '${this.checker.typeToString(value)}'.`,
          );
        }
        return;
      }
      // Anything else (converter-typed, object): fall through to structural.
    }

    const assignable = this.isAssignable(fact.kind === 'attribute' ? this.stripNullish(value) : value, target);
    if (assignable === false) {
      this.report(
        out,
        'no-incompatible-type-binding',
        fact,
        `This expression's type '${this.checker.typeToString(value)}' is not assignable to '${fact.memberName}', which is '${this.checker.typeToString(target)}'.`,
      );
    }
  }
}
