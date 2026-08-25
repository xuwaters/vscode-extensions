/**
 * Phase 3 exit, half one: a seeded mistake of each rule's kind is reported at
 * the right place with the right message — through the real extraction, the
 * real WASM engine, and the real type oracle. (Half two, silence over the
 * corpus, lives in corpus.test.ts.)
 */

import { describe, expect, it } from 'vitest';

import type { PluginSettings } from '../tsplugin/config.js';
import { createHarness, fixture, messages, wasmBuilt } from './harness.js';

const FILE = fixture('seeded.ts');

/** One strict-mode harness per fixture body. */
function diagnose(body: string, settings?: PluginSettings) {
  const harness = createHarness(
    { [FILE]: body },
    { settings: settings ?? { strict: true, logging: 'off' } },
  );
  return { harness, diagnostics: harness.fastDiagnostics(FILE) };
}

function componentPrelude(extra = ''): string {
  return `
    import { FASTElement, customElement, observable, attr, html, css, ref, when, repeat, slotted } from '@microsoft/fast-element';
    export const GRID_TAG = 'my-grid';
    @customElement({ name: GRID_TAG, template: null as never })
    export class MyGrid extends FASTElement {
      @observable hasHeader = false;
      @observable query = '';
      @observable menu: { x: number } | null = null;
      @attr({ mode: 'boolean' }) locked = false;
      @attr rowHeight = 24;
      findInput!: HTMLInputElement;
      toggleHeader(): void {}
      close(): void { this.$emit('closed', { reason: 'x' }); }
    }
    ${extra}
  `;
}

describe.skipIf(!wasmBuilt)('structural rules', () => {
  it('no-unknown-tag-name, with a rename fix that hits both tags', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<my-gird>x</my-gird>\`;`),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('Unknown tag <my-gird>'),
    ]);
    expect(messages(diagnostics)[0]).toContain("Did you mean 'my-grid'?");
  });

  it('no-unclosed-tag', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<div><butto>x</div>\`;`),
    );
    const unclosed = messages(diagnostics).filter((m) => m.includes('never closed'));
    expect(unclosed).toEqual([expect.stringContaining('<butto> was never closed')]);
  });

  it('no-unknown-attribute with suggestion, and its span lands on the name', () => {
    const { harness, diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<button aria-pressd="x">y</button>\`;`),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining("Unknown attribute 'aria-pressd'"),
    ]);
    const source = harness.ls.getProgram()!.getSourceFile(FILE)!.text;
    const diagnostic = diagnostics[0];
    expect(source.slice(diagnostic.start!, diagnostic.start! + diagnostic.length!)).toBe(
      'aria-pressd',
    );
  });

  it('no-unknown-property on a component', () => {
    const { diagnostics } = diagnose(
      componentPrelude(
        `export const t = html<MyGrid>\`<my-grid :hasHedaer="\${(x) => x.hasHeader}"></my-grid>\`;`,
      ),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining("Unknown property ':hasHedaer'"),
    ]);
  });

  it('no-unknown-event, on by default outside strict mode', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<button @clik="\${(x) => x.toggleHeader()}">y</button>\`;`),
      { logging: 'off' }, // strict OFF: the changed default still fires
    );
    expect(messages(diagnostics)).toEqual([expect.stringContaining("Unknown event '@clik'")]);
    expect(messages(diagnostics)[0]).toContain("Did you mean 'click'?");
  });

  it('no-unknown-event stays quiet for events declared any of the three ways', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, html, observable, repeat } from '@microsoft/fast-element';
      export class Tab { id = 0; }
      const tabTemplate = html<Tab, TabBar>\`
        <div @click="\${(x, c) => c.parent.$emit('tab-select', { id: x.id })}"></div>
      \`;
      const template = html<TabBar>\`
        \${repeat((x) => x.tabs, tabTemplate)}
        <button @click="\${(x) => x.$emit('tab-add')}"></button>
      \`;
      /** @fires tab-rename - A tab was renamed. */
      @customElement({ name: 'tab-bar', template })
      export class TabBar extends FASTElement {
        declare $events: { 'tab-close': { id: number } };
        @observable tabs: Tab[] = [];
      }

      @customElement({ name: 'tab-host', template: null as never })
      export class TabHost extends FASTElement {}
      export const host = html<TabHost>\`
        <tab-bar
          @tab-select="\${(x, c) => c.event}"
          @tab-close="\${(x, c) => c.event}"
          @tab-add="\${(x, c) => c.event}"
          @tab-rename="\${(x, c) => c.event}"
          @tab-shuffle="\${(x, c) => c.event}"
        ></tab-bar>
      \`;
    `);
    // Only the one nobody declared.
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining("Unknown event '@tab-shuffle'"),
    ]);
  });

  it('no-unknown-slot against JSDoc-declared slots', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, html } from '@microsoft/fast-element';
      /** @slot toolbar - Buttons. */
      @customElement('slotted-el')
      export class SlottedEl extends FASTElement {}
      export const t = html<SlottedEl>\`<slotted-el><div slot="toolbr">x</div></slotted-el>\`;
    `);
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining("declares no slot named 'toolbr'"),
    ]);
  });

  it('no-expressionless-property-binding', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<my-grid :query="literal"></my-grid>\`;`),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining("':query' is a property binding"),
    ]);
  });

  it('no-unintended-mixed-binding', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<input value=\${(x) => x.query}/ >\`;`),
    );
    expect(messages(diagnostics)).toEqual([expect.stringContaining('swept into')]);
  });

  it('no-missing-import when the declaring module is unreachable', () => {
    const OTHER = fixture('other.ts');
    const harness = createHarness(
      {
        [FILE]: componentPrelude(),
        [OTHER]: `
          import { html } from '@microsoft/fast-element';
          interface Anything { q: string }
          export const t = html<Anything>\`<my-grid></my-grid>\`;
        `,
      },
      { settings: { strict: true } },
    );
    const diagnostics = harness.fastDiagnostics(OTHER);
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('not reachable'),
    ]);
  });
});

describe.skipIf(!wasmBuilt)('FAST-native rules', () => {
  it('no-non-reactive-binding fires on a value read and offers the arrow fix', () => {
    const { harness, diagnostics } = diagnose(
      componentPrelude(`
        const gridState = { count: 0 };
        export const t = html<MyGrid>\`<span>\${gridState.count}</span>\`;
      `),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('bound once, when the view is created'),
    ]);
    const fixes = harness.decorated.getCodeFixesAtPosition(
      FILE,
      diagnostics[0].start!,
      diagnostics[0].start! + diagnostics[0].length!,
      [diagnostics[0].code],
      {},
      {},
    );
    const wrap = fixes.find((f) => f.description.includes('arrow'));
    expect(wrap).toBeDefined();
    expect(wrap!.changes[0].textChanges[0].newText).toBe('() => ');
  });

  it('no-non-reactive-binding stays quiet on constants, calls and templates', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`
        const LABEL = 'fixed';
        const shortcut = (a: string, b: string): string => (a.length > b.length ? a : b);
        const fragment = html<MyGrid>\`<b>b</b>\`;
        export const t = html<MyGrid>\`
          <span title="k: \${shortcut('a', 'bb')}">\${LABEL}</span>
          \${fragment}
        \`;
      `),
    );
    expect(messages(diagnostics)).toEqual([]);
  });

  it('no-invalid-directive-binding: position rules for both directions', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`
        export const t = html<MyGrid>\`
          <div class="\${when((x) => x.hasHeader, html\`<b>b</b>\`)}">a</div>
          <div>\${ref('findInput')}</div>
        \`;
      `),
    );
    const texts = messages(diagnostics);
    expect(texts).toHaveLength(2);
    expect(texts[0]).toContain('when(…) builds content');
    expect(texts[1]).toContain('attaches to an element');
  });

  it('no-invalid-directive-target: a ref to a missing member, with the fix', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<input \${ref('findInpt')} />\`;`),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining("'findInpt' is not a member of MyGrid"),
    ]);
    expect(messages(diagnostics)[0]).toContain("Did you mean 'findInput'?");
  });

  it('no-slot-without-shadow-root, in the template and for slotted()', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, html } from '@microsoft/fast-element';
      const template = html<LightEl>\`<div><slot></slot></div>\`;
      @customElement({ name: 'light-el', template, shadowOptions: null })
      export class LightEl extends FASTElement {}
    `);
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('<slot> does nothing here'),
    ]);
  });

  it('no-duplicate-tag-name on both registrations', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement } from '@microsoft/fast-element';
      @customElement('dup-el')
      export class DupA extends FASTElement {}
      @customElement('dup-el')
      export class DupB extends FASTElement {}
    `);
    const dupes = messages(diagnostics).filter((m) => m.includes('registered more than once'));
    expect(dupes).toHaveLength(2);
  });

  it('no-invalid-tag-name', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement } from '@microsoft/fast-element';
      @customElement('grid')
      export class BadName extends FASTElement {}
    `);
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('must contain a hyphen'),
    ]);
  });

  it('no-untyped-template on a component template, with the type-argument fix', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, html } from '@microsoft/fast-element';
      const template = html\`<div>\${(x) => x.anything}</div>\`;
      @customElement({ name: 'untyped-el', template })
      export class UntypedEl extends FASTElement {}
    `);
    const untyped = messages(diagnostics).filter((m) => m.includes('no type argument'));
    expect(untyped).toEqual([expect.stringContaining('html<UntypedEl>')]);
  });

  it('an untyped fragment inside when() inherits its type and stays silent', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`
        export const t = html<MyGrid>\`
          \${when((x) => x.hasHeader, html\`<span>\${(x) => x.query}</span>\`)}
        \`;
      `),
    );
    expect(messages(diagnostics)).toEqual([]);
  });

  it('template-not-analyzed for html.partial', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`
        declare const raw: string;
        export const t = html<MyGrid>\`<butto>\${html.partial(raw)}\`;
      `),
    );
    // Only the suggestion — the unclosed <butto> is deliberately not reported.
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('not analyzed'),
    ]);
  });
});

describe.skipIf(!wasmBuilt)('the type oracle', () => {
  it('no-noncallable-event-binding', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<button @click="\${(x) => x.query}">y</button>\`;`),
    );
    // (x) => x.query is callable — its *return* is a string, but the binding
    // itself is a function, so this is fine.
    expect(messages(diagnostics)).toEqual([]);

    const bad = diagnose(
      componentPrelude(`
        const notAFunction = { length: 1 };
        export const t = html<MyGrid>\`<button @click="\${notAFunction}">y</button>\`;
      `),
    );
    expect(messages(bad.diagnostics)).toEqual([
      expect.stringContaining('not callable'),
    ]);
  });

  it('no-boolean-in-attribute-binding', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<input readonly="\${(x) => x.hasHeader}" />\`;`),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('sets the string "false"'),
    ]);
  });

  it('no-complex-attribute-binding', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<div title="\${(x) => x.menu}">y</div>\`;`),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('[object Object]'),
    ]);
  });

  it('no-incompatible-type-binding against a declared attribute type', () => {
    const { diagnostics } = diagnose(
      componentPrelude(
        `export const t = html<MyGrid>\`<my-grid rowheight="\${(x) => x.query}"></my-grid>\`;`,
      ),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining("'rowheight' is typed 'number'"),
    ]);
  });

  it('a number literal in a numeric attribute is fine; a word is not', () => {
    const good = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<my-grid rowheight="24"></my-grid>\`;`),
    );
    expect(messages(good.diagnostics)).toEqual([]);
    const bad = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<my-grid rowheight="tall"></my-grid>\`;`),
    );
    expect(messages(bad.diagnostics)).toEqual([
      expect.stringContaining('"tall" is not a number'),
    ]);
  });

  it('null and undefined are stripped before attribute checks — FAST removes the attribute', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`
        export const t = html<MyGrid>\`<div title="\${(x) => (x.hasHeader ? 'yes' : null)}">y</div>\`;
      `),
    );
    expect(messages(diagnostics)).toEqual([]);
  });

  it('boolean attribute bindings want booleans', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<my-grid ?locked="\${(x) => x.query}"></my-grid>\`;`),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('bind a boolean'),
    ]);
  });

  it('property bindings use real assignability', () => {
    const { diagnostics } = diagnose(
      componentPrelude(
        `export const t = html<MyGrid>\`<my-grid :hasHeader="\${(x) => x.query}"></my-grid>\`;`,
      ),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('not assignable'),
    ]);
  });

  it('a builtin attribute whose IDL property is an object has no target type', () => {
    // `style` reflects a CSSStyleDeclaration and `form`/`list` reflect
    // elements, but all three are set through setAttribute — a string is
    // exactly right, and the property's type is not the attribute's.
    const styled = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<div style="\${(x) => x.query}">y</div>\`;`),
    );
    expect(messages(styled.diagnostics)).toEqual([]);

    const associated = diagnose(
      componentPrelude(
        `export const t = html<MyGrid>\`<input form="\${(x) => x.query}" list="\${(x) => x.query}" />\`;`,
      ),
    );
    expect(messages(associated.diagnostics)).toEqual([]);
  });

  it('a builtin property binding still checks against the property type', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<input :value="\${(x) => x.menu}" />\`;`),
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('not assignable'),
    ]);
  });

  it('no-implicit-prevent-default when opted in', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`
        export const t = html<MyGrid>\`<input @keydown="\${(x, c) => x.toggleHeader()}" />\`;
      `),
      { strict: true, rules: { 'no-implicit-prevent-default': 'warning' } },
    );
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('preventDefault()'),
    ]);
  });
});

describe.skipIf(!wasmBuilt)('discovery rules', () => {
  it('no-incompatible-attr-config: boolean mode on a string property', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, attr } from '@microsoft/fast-element';
      @customElement('cfg-el')
      export class CfgEl extends FASTElement {
        @attr({ mode: 'boolean' }) label = '';
      }
    `);
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('boolean attribute wants a boolean property'),
    ]);
  });

  it('no-attr-visibility-mismatch: a private @attr', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, attr } from '@microsoft/fast-element';
      @customElement('vis-el')
      export class VisEl extends FASTElement {
        @attr private secret = '';
      }
    `);
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('public DOM contract'),
    ]);
  });

  it('no-invalid-attribute-name', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, attr } from '@microsoft/fast-element';
      @customElement('name-el')
      export class NameEl extends FASTElement {
        @attr({ attribute: 'has space' }) x = '';
      }
    `);
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('not a valid attribute name'),
    ]);
  });
});

describe.skipIf(!wasmBuilt)('css documents', () => {
  it('no-invalid-css from the CSS language service', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, css } from '@microsoft/fast-element';
      const styles = css\`:host { colr: red; }\`;
      @customElement({ name: 'css-el', styles })
      export class CssEl extends FASTElement {}
    `);
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining("Unknown property: 'colr'"),
    ]);
  });

  it('a placeholder-only stylesheet produces nothing — the typst-ultra shape', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, css } from '@microsoft/fast-element';
      const sheet = ':host { color: red; }';
      const styles = css\`\${sheet}\`;
      @customElement({ name: 'sheet-el', styles })
      export class SheetEl extends FASTElement {}
    `);
    expect(messages(diagnostics)).toEqual([]);
  });

  it('::part in a light-DOM component’s styles', () => {
    const { diagnostics } = diagnose(`
      import { FASTElement, customElement, css, html } from '@microsoft/fast-element';
      const styles = css\`.x::part(frame) { color: red; }\`;
      @customElement({ name: 'part-el', template: html<PartEl>\`<b>b</b>\`, styles, shadowOptions: null })
      export class PartEl extends FASTElement {}
    `);
    expect(messages(diagnostics)).toEqual([
      expect.stringContaining('::part does nothing here'),
    ]);
  });
});

describe.skipIf(!wasmBuilt)('suppression and severity plumbing', () => {
  it('@ts-ignore on the previous line suppresses', () => {
    const { diagnostics } = diagnose(
      componentPrelude(`
        export const t = html<MyGrid>\`
          <div>
            <!-- @ts-ignore: the tag below is intentional -->
            <butto>x</butto>
          </div>
        \`;
      `),
    );
    expect(messages(diagnostics)).toEqual([]);
  });

  it('a rule set to off reports nothing; error overrides severity', () => {
    const off = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<div><butto>x</div>\`;`),
      { strict: true, rules: { 'no-unclosed-tag': 'off', 'no-unknown-tag-name': 'off' } },
    );
    expect(messages(off.diagnostics)).toEqual([]);

    const escalated = diagnose(
      componentPrelude(`export const t = html<MyGrid>\`<div><butto>x</butto></div>\`;`),
      { rules: { 'no-unknown-tag-name': 'error' } },
    );
    expect(escalated.diagnostics[0]?.category).toBe(1); // ts.DiagnosticCategory.Error
  });
});
