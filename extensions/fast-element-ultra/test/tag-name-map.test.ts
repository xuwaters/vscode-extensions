/**
 * Discovery through `HTMLElementTagNameMap`: the components no file
 * extraction can see, because the library registers them behind its own
 * `define*` wrapper — the tag is assembled at runtime from a prefix and a
 * base name — or ships as a built package the plugin only reads as `.d.ts`.
 */

import { describe, expect, it } from 'vitest';

import { createHarness, fixture, messages, wasmBuilt } from './harness.js';

const DESIGN_SYSTEM = fixture('design-system.ts');
const TOAST = fixture('toast.ts');
const APP = fixture('app.ts');

/** A miniature of @mars-garden/fast-ui: blueprints plus a define wrapper. */
const LIBRARY = {
  [DESIGN_SYSTEM]: `
    import { FASTElement, type PartialFASTElementDefinition } from '@microsoft/fast-element';
    export const DEFAULT_PREFIX = 'fui';
    export interface ComponentBlueprint<T extends typeof FASTElement = typeof FASTElement> {
      baseName: string;
      type: T;
      template?: any;
    }
    export interface DefineComponentOptions { prefix?: string; name?: string }
    export function tagFor(baseName: string, prefix: string = DEFAULT_PREFIX): string {
      return \`\${prefix}-\${baseName}\`;
    }
    export async function defineComponent<T extends typeof FASTElement>(
      blueprint: ComponentBlueprint<T>,
      options: DefineComponentOptions = {},
    ): Promise<T> {
      const name = options.name ?? tagFor(blueprint.baseName, options.prefix);
      const definition: PartialFASTElementDefinition = { name, template: blueprint.template };
      await blueprint.type.define(definition);
      return blueprint.type;
    }
  `,
  [TOAST]: `
    import { FASTElement, attr, observable, html } from '@microsoft/fast-element';
    import { defineComponent, type ComponentBlueprint, type DefineComponentOptions } from './design-system.js';

    /** Toasts, stacked in a corner. */
    export class Toaster extends FASTElement {
      @attr declare position: 'top-right' | 'bottom-right';
      @observable declare toasts: string[];
    }

    export const toasterTemplate = html<Toaster>\`<div class="toaster"></div>\`;

    export const toasterBlueprint: ComponentBlueprint<typeof Toaster> = {
      baseName: 'toaster',
      type: Toaster,
      template: toasterTemplate,
    };

    export function defineToaster(options?: DefineComponentOptions): Promise<typeof Toaster> {
      return defineComponent(toasterBlueprint, options);
    }

    declare global {
      interface HTMLElementTagNameMap {
        'fui-toaster': Toaster;
      }
    }
  `,
};

const APP_SOURCE = `
    import { FASTElement, customElement, html } from '@microsoft/fast-element';
    import { defineToaster } from './toast.js';
    void defineToaster();

    const template = html<App>\`<fui-toaster position="top-right"></fui-toaster>\`;

    @customElement({ name: 'dv-app', template })
    export class App extends FASTElement {}
  `;

describe.skipIf(!wasmBuilt)('tag-name-map discovery', () => {
  it('finds a component its own file never names', () => {
    const harness = createHarness({ ...LIBRARY, [APP]: APP_SOURCE });
    harness.service.sync();
    // Nothing in toast.ts declares the tag: the wrapper builds it at runtime.
    expect(harness.service.fileExtraction(TOAST)!.upsert.components).toEqual([]);
    expect(messages(harness.fastDiagnostics(APP))).toEqual([]);
  });

  it('goes to the class behind the tag, and to the member behind an attribute', () => {
    const harness = createHarness({ ...LIBRARY, [APP]: APP_SOURCE });
    harness.service.sync();

    const tagAt = APP_SOURCE.indexOf('<fui-toaster') + 3;
    const tag = harness.decorated.getDefinitionAndBoundSpan(APP, tagAt);
    expect(tag?.definitions?.[0]?.fileName).toBe(TOAST);
    expect(LIBRARY[TOAST].slice(
      tag!.definitions![0].textSpan.start,
      tag!.definitions![0].textSpan.start + tag!.definitions![0].textSpan.length,
    )).toBe('Toaster');

    const attributeAt = APP_SOURCE.indexOf('position="top-right"') + 2;
    const attribute = harness.decorated.getDefinitionAndBoundSpan(APP, attributeAt);
    expect(attribute?.definitions?.[0]?.fileName).toBe(TOAST);
  });

  it('completes the attributes of an ambient component', () => {
    const source = APP_SOURCE.replace('position="top-right"', '');
    const harness = createHarness({ ...LIBRARY, [APP]: source });
    harness.service.sync();
    const at = source.indexOf('<fui-toaster') + '<fui-toaster '.length;
    const completions = harness.decorated.getCompletionsAtPosition(APP, at, undefined);
    const names = completions?.entries.map((e) => e.name) ?? [];
    expect(names).toContain('position');
  });

  it('reads a component out of a published package that only ships .d.ts', () => {
    const DECLARATION = fixture('vendor.d.ts');
    const app = `
      import { FASTElement, customElement, html } from '@microsoft/fast-element';
      import { defineBadge } from './vendor.js';
      void defineBadge();
      const template = html<App>\`<vendor-badge label="new"></vendor-badge>\`;
      @customElement({ name: 'dv-app', template })
      export class App extends FASTElement {}
    `;
    const harness = createHarness({
      [DECLARATION]: `
        import { FASTElement } from '@microsoft/fast-element';
        export declare class Badge extends FASTElement {
          label: string;
          items: string[];
          private hidden;
        }
        export declare function defineBadge(): Promise<typeof Badge>;
        declare global {
          interface HTMLElementTagNameMap { 'vendor-badge': Badge }
        }
      `,
      [APP]: app,
    });
    harness.service.sync();
    // A declaration file is never extracted, so the tag can only have come
    // from the map — with its decorator-less members recovered.
    expect(harness.service.fileExtraction(DECLARATION)).toBeUndefined();
    expect(messages(harness.fastDiagnostics(APP))).toEqual([]);
    const at = app.indexOf('<vendor-badge') + 3;
    expect(harness.decorated.getDefinitionAndBoundSpan(APP, at)?.definitions?.[0]?.fileName).toBe(
      DECLARATION,
    );
  });

  it('knows a container tag whose entry names no class of its own', () => {
    const app = `
      import { FASTElement, customElement, html } from '@microsoft/fast-element';
      export const CardHeader = (() => class extends FASTElement {})();
      declare global {
        interface HTMLElementTagNameMap { 'fui-card-header': FASTElement }
      }
      const template = html<App>\`<fui-card-header class="x"></fui-card-header>\`;
      @customElement({ name: 'dv-app', template })
      export class App extends FASTElement {}
    `;
    const harness = createHarness({ [APP]: app });
    harness.service.sync();
    // Known tag, global attributes fine — and no members invented for it.
    expect(messages(harness.fastDiagnostics(APP))).toEqual([]);
    const withAttribute = app.replace('class="x"', 'nope="x"');
    harness.updateFile(APP, withAttribute);
    expect(messages(harness.fastDiagnostics(APP))).toEqual([
      "Unknown attribute 'nope' on <fui-card-header>.",
    ]);
  });

  it('leaves a non-FAST entry in the map alone', () => {
    const app = `
      import { FASTElement, customElement, html } from '@microsoft/fast-element';
      declare global {
        interface HTMLElementTagNameMap { 'plain-el': HTMLElement }
      }
      const template = html<App>\`<plain-el></plain-el>\`;
      @customElement({ name: 'dv-app', template })
      export class App extends FASTElement {}
    `;
    const harness = createHarness({ [APP]: app });
    harness.service.sync();
    expect(messages(harness.fastDiagnostics(APP))).toEqual(['Unknown tag <plain-el>.']);
  });

  it('a source declaration wins over the map entry for the same tag', () => {
    const app = `
      import { FASTElement, customElement, html } from '@microsoft/fast-element';
      const template = html<App>\`<dv-panel wrong-attr=""></dv-panel>\`;
      @customElement({ name: 'dv-app', template })
      export class App extends FASTElement {}

      @customElement('dv-panel')
      export class Panel extends FASTElement {}

      declare global {
        interface HTMLElementTagNameMap { 'dv-panel': Panel }
      }
    `;
    const harness = createHarness({ [APP]: app });
    harness.service.sync();
    // One component, from the decorator — not two, and not the map's.
    const components = harness.service
      .fileExtraction(APP)!
      .upsert.components.filter((c) => c.tagName === 'dv-panel');
    expect(components).toHaveLength(1);
    expect(components[0].origin).toBe('decorator');
    // The decorated class knows its attributes exactly, so this still fires.
    expect(messages(harness.fastDiagnostics(APP))).toEqual([
      "Unknown attribute 'wrong-attr' on <dv-panel>.",
    ]);
  });
});
