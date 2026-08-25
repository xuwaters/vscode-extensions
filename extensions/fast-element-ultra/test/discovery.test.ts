/**
 * Component discovery (Phase 2): every registration form, every member
 * declaration form, inheritance, `$emit`, JSDoc facts — driven through the
 * real checker and the real @microsoft/fast-element package.
 */

import { describe, expect, it } from 'vitest';

import { createHarness, fixture, wasmBuilt } from './harness.js';

const ELEMENT = fixture('element.ts');

describe.skipIf(!wasmBuilt)('component discovery', () => {
  it('resolves a tag name that is a const — the case that motivated the RFC', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement, observable, html } from '@microsoft/fast-element';
        export const MY_TAG = 'my-grid';
        const template = html<MyGrid>\`<div></div>\`;
        @customElement({ name: MY_TAG, template })
        export class MyGrid extends FASTElement {
          @observable hasHeader = false;
          @observable query = '';
        }
      `,
    });
    harness.service.sync();
    const extraction = harness.service.fileExtraction(ELEMENT);
    expect(extraction).toBeDefined();
    const [component] = extraction!.upsert.components;
    expect(component.tagName).toBe('my-grid');
    expect(component.className).toBe('MyGrid');
    expect(component.properties.map((p) => p.name).sort()).toEqual(['hasHeader', 'query']);
    expect(component.hasShadowRoot).toBe(true);
    expect(component.templateDocumentId).toBe(extraction!.upsert.documents[0].id);
    // The tag string's span points into the const's initializer for rename.
    expect(component.tagNameSpan?.fileName).toBe(ELEMENT);
  });

  it('resolves an imported const tag name through the checker', () => {
    const TAGS = fixture('tags.ts');
    const harness = createHarness({
      [TAGS]: `export const GRID_TAG = 'x-grid';`,
      [ELEMENT]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        import { GRID_TAG } from './tags.js';
        @customElement({ name: GRID_TAG })
        export class XGrid extends FASTElement {}
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    expect(component.tagName).toBe('x-grid');
    expect(component.tagNameSpan?.fileName).toBe(TAGS);
  });

  it('handles the string form and both define forms', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        @customElement('a-el')
        export class AEl extends FASTElement {}

        export class BEl extends FASTElement {}
        BEl.define({ name: 'b-el' });

        export class CEl extends FASTElement {}
        FASTElement.define(CEl, 'c-el');
      `,
    });
    harness.service.sync();
    const components = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    const byTag = new Map(components.map((c) => [c.tagName, c]));
    expect([...byTag.keys()].sort()).toEqual(['a-el', 'b-el', 'c-el']);
    expect(byTag.get('a-el')?.origin).toBe('decorator');
    expect(byTag.get('b-el')?.origin).toBe('define');
    expect(byTag.get('c-el')?.origin).toBe('define');
  });

  it('a renamed import of customElement still counts; a local one does not', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement as element } from '@microsoft/fast-element';
        @element('renamed-el')
        export class RenamedEl extends FASTElement {}

        function customElement(_name: string) {
          return (_target: unknown) => {};
        }
        @customElement('not-fast')
        export class NotFast {}
      `,
    });
    harness.service.sync();
    const components = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    expect(components.map((c) => c.tagName)).toEqual(['renamed-el']);
  });

  it('collects @attr in all its forms, on properties and accessors', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement, attr, observable, volatile } from '@microsoft/fast-element';
        @customElement('attr-el')
        export class AttrEl extends FASTElement {
          @attr myAttr = '';
          @attr({ attribute: 'renamed-attr' }) renamed = '';
          @attr({ mode: 'boolean' }) disabled = false;
          @attr({ mode: 'fromView' }) value = '';
          @observable internal = 0;
          @volatile get computed(): number { return this.internal * 2; }
          private view = 0;
          @attr get accessorAttr(): string { return String(this.view); }
        }
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    const attrs = new Map(component.attributes.map((a) => [a.name, a]));
    // The default attribute name is the property name lowercased, as
    // fast-element's AttributeDefinition constructor does.
    expect(attrs.get('myattr')?.propertyName).toBe('myAttr');
    expect(attrs.get('renamed-attr')?.propertyName).toBe('renamed');
    expect(attrs.get('disabled')?.mode).toBe('boolean');
    expect(attrs.get('value')?.mode).toBe('fromView');
    expect(attrs.get('accessorattr')?.propertyName).toBe('accessorAttr');
    expect(component.properties.map((p) => p.name).sort()).toEqual(['computed', 'internal']);
  });

  it('collects attributes declared in the definition, with no decorator', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        @customElement({ name: 'def-el', attributes: ['plain', { property: 'fancy', attribute: 'data-fancy', mode: 'boolean' }] })
        export class DefEl extends FASTElement {
          plain = '';
          fancy = false;
        }
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    const names = component.attributes.map((a) => a.name).sort();
    expect(names).toEqual(['data-fancy', 'plain']);
    expect(component.attributes.find((a) => a.name === 'data-fancy')?.mode).toBe('boolean');
  });

  it('walks the inheritance chain up to FASTElement', () => {
    const BASE = fixture('base.ts');
    const harness = createHarness({
      [BASE]: `
        import { FASTElement, observable } from '@microsoft/fast-element';
        export class BaseEl extends FASTElement {
          @observable baseProp = 1;
          @observable shadowed = 'base';
        }
      `,
      [ELEMENT]: `
        import { customElement, observable } from '@microsoft/fast-element';
        import { BaseEl } from './base.js';
        @customElement('sub-el')
        export class SubEl extends BaseEl {
          @observable ownProp = 2;
          @observable shadowed = 'sub';
        }
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    const properties = new Map(component.properties.map((p) => [p.name, p]));
    expect([...properties.keys()].sort()).toEqual(['baseProp', 'ownProp', 'shadowed']);
    expect(properties.get('baseProp')?.origin).toBe('inherited');
    // The shadowing declaration wins.
    expect(properties.get('shadowed')?.origin).toBe('decorator');
    expect(properties.get('shadowed')?.declSpan?.fileName).toBe(ELEMENT);
  });

  it('finds events from $emit calls and JSDoc, and slot/part facts', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        /**
         * A thing.
         * @slot - Default content.
         * @slot toolbar - Buttons.
         * @csspart frame - The frame.
         * @cssprop --thing-gap - Spacing.
         * @fires opened - Fired on open.
         */
        @customElement('emit-el')
        export class EmitEl extends FASTElement {
          close(): void {
            this.$emit('closed', { reason: 'user' });
          }
        }
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    expect(component.events.map((e) => e.name).sort()).toEqual(['closed', 'opened']);
    expect(component.events.find((e) => e.name === 'closed')?.typeText).toContain('reason');
    expect(component.slots.map((s) => s.name)).toEqual(['', 'toolbar']);
    expect(component.cssParts.map((p) => p.name)).toEqual(['frame']);
    expect(component.cssProperties.map((p) => p.name)).toEqual(['--thing-gap']);
  });

  it('finds events emitted from the template, host and repeat-item alike', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement, html, observable, repeat } from '@microsoft/fast-element';
        export class Tab { id = 0; }
        const tabTemplate = html<Tab, TabBar>\`
          <div @click="\${(x, c) => c.parent.$emit('tab-select', { id: x.id })}"></div>
        \`;
        const template = html<TabBar>\`
          \${repeat((x) => x.tabs, tabTemplate)}
          <button @click="\${(x) => x.$emit('tab-add')}"></button>
        \`;
        @customElement({ name: 'tab-bar', template })
        export class TabBar extends FASTElement {
          @observable tabs: Tab[] = [];
        }
      `,
    });
    harness.service.sync();
    const [component] = harness.service
      .fileExtraction(ELEMENT)!
      .upsert.components.filter((c) => c.tagName === 'tab-bar');
    expect(component.events.map((e) => e.name).sort()).toEqual(['tab-add', 'tab-select']);
    // `c.parent` types to the host, so the detail comes along with it.
    expect(component.events.find((e) => e.name === 'tab-select')?.typeText).toContain('id');
  });

  it('attributes an emit to the class the receiver names, not the nearest one', () => {
    const OTHER = fixture('other.ts');
    const harness = createHarness({
      [OTHER]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        @customElement('other-el')
        export class OtherEl extends FASTElement {}
      `,
      [ELEMENT]: `
        import { FASTElement, customElement, html } from '@microsoft/fast-element';
        import { OtherEl } from './other.js';
        const template = html<HostEl>\`
          <button @click="\${(x) => x.other.$emit('not-mine')}"></button>
          <button @click="\${(x) => x.$emit('mine')}"></button>
        \`;
        @customElement({ name: 'host-el', template })
        export class HostEl extends FASTElement {
          other!: OtherEl;
        }
      `,
    });
    harness.service.sync();
    const [component] = harness.service
      .fileExtraction(ELEMENT)!
      .upsert.components.filter((c) => c.tagName === 'host-el');
    expect(component.events.map((e) => e.name)).toEqual(['mine']);
  });

  it('reads a declared $events map, through an interface and with docs', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        export interface BarEvents {
          /** A tab was chosen. */
          'tab-select': { id: number };
          'tab-add': void;
        }
        @customElement('map-el')
        export class MapEl extends FASTElement {
          declare $events: BarEvents;
        }
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    const events = new Map(component.events.map((e) => [e.name, e]));
    expect([...events.keys()].sort()).toEqual(['tab-add', 'tab-select']);
    expect(events.get('tab-select')?.typeText).toContain('id');
    expect(events.get('tab-select')?.documentation).toBe('A tab was chosen.');
    // `void` is "no detail", not a detail type worth showing.
    expect(events.get('tab-add')?.typeText).toBeNull();
    // The map itself is a contract, not a bindable property.
    expect(component.properties.map((p) => p.name)).not.toContain('$events');
  });

  it('a declared $events entry types an event the class also emits', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        /** @fires closed - The panel went away. */
        @customElement('both-el')
        export class BothEl extends FASTElement {
          declare $events: { closed: { reason: 'user' | 'timeout' } };
          close(): void { this.$emit('closed', { reason: 'user' } as { reason: 'user' | 'timeout' }); }
        }
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    expect(component.events).toHaveLength(1);
    // Declared type, JSDoc prose, and the map's own span — merged, not
    // dropped because the name was already taken.
    expect(component.events[0].typeText).toContain('reason');
    expect(component.events[0].documentation).toBe('The panel went away.');
  });

  it('takes the detail type from a typed @fires tag', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        /**
         * @fires {{ id: number }} tab-select - A tab was chosen.
         * @fires {CustomEvent<string>} named - Documented elsewhere.
         * @attr {number} row-height - How tall a row is.
         */
        @customElement('doc-el')
        export class DocEl extends FASTElement {}
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    const events = new Map(component.events.map((e) => [e.name, e]));
    expect([...events.keys()].sort()).toEqual(['named', 'tab-select']);
    expect(events.get('tab-select')?.typeText).toBe('{ id: number }');
    expect(events.get('tab-select')?.documentation).toBe('A tab was chosen.');
    expect(events.get('named')?.typeText).toBe('CustomEvent<string>');
    const attribute = component.attributes.find((a) => a.name === 'row-height');
    expect(attribute?.typeText).toBe('number');
    expect(attribute?.documentation).toBe('How tall a row is.');
  });

  it('records shadowOptions: null as light DOM', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement, html } from '@microsoft/fast-element';
        const template = html<LightEl>\`<div></div>\`;
        @customElement({ name: 'light-el', template, shadowOptions: null })
        export class LightEl extends FASTElement {}
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    expect(component.hasShadowRoot).toBe(false);
  });

  it('an unresolvable tag name registers the component without a tag', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement, observable } from '@microsoft/fast-element';
        declare function computeTag(): string;
        @customElement({ name: computeTag() })
        export class DynamicEl extends FASTElement {
          @observable still = 'discovered';
        }
      `,
    });
    harness.service.sync();
    const [component] = harness.service.fileExtraction(ELEMENT)!.upsert.components;
    expect(component.tagName).toBeNull();
    expect(component.properties.map((p) => p.name)).toEqual(['still']);
  });
});

describe.skipIf(!wasmBuilt)('template discovery', () => {
  it('extracts typed templates with source members and placeholder metadata', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement, observable, html, ref } from '@microsoft/fast-element';
        @customElement('t-el')
        export class TEl extends FASTElement {
          @observable count = 0;
          input!: HTMLInputElement;
        }
        export const template = html<TEl>\`
          <input \${ref('input')} />
          <span>\${(x) => x.count}</span>
        \`;
      `,
    });
    harness.service.sync();
    const documents = harness.service.fileExtraction(ELEMENT)!.upsert.documents;
    expect(documents).toHaveLength(1);
    const [doc] = documents;
    expect(doc.kind).toBe('html');
    expect(doc.sourceTypeName).toBe('TEl');
    const memberNames = doc.sourceMembers!.map((m) => m.name);
    expect(memberNames).toContain('count');
    expect(memberNames).toContain('input');
    // Placeholder 0 is the ref directive with its argument span.
    const [refPh, arrowPh] = doc.placeholders;
    expect(refPh.expr?.directive?.name).toBe('ref');
    expect(refPh.expr?.directive?.argString).toBe('input');
    expect(arrowPh.expr?.kind).toBe('arrow');
    expect(arrowPh.expr?.isFunctionType).toBe(true);
    // The substitution preserved length exactly.
    const source = harness.ls.getProgram()!.getSourceFile(ELEMENT)!.text;
    expect(doc.text.length).toBe(
      source.indexOf('`;', source.indexOf('html<TEl>')) - (source.indexOf('html<TEl>`') + 'html<TEl>`'.length),
    );
  });

  it('nested when-templates inherit the source type; repeat items do not', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement, observable, html, when, repeat } from '@microsoft/fast-element';
        interface Item { label: string }
        @customElement('n-el')
        export class NEl extends FASTElement {
          @observable busy = false;
          @observable items: Item[] = [];
        }
        export const template = html<NEl>\`
          \${when((x) => x.busy, html\`<span>\${(x) => x.busy}</span>\`)}
          \${repeat((x) => x.items, html<Item, NEl>\`<li>\${(i) => i.label}</li>\`)}
        \`;
      `,
    });
    harness.service.sync();
    const documents = harness.service.fileExtraction(ELEMENT)!.upsert.documents;
    expect(documents).toHaveLength(3);
    const inner = documents.filter((d) => d.id !== documents.find((x) => x.sourceTypeName === 'NEl' && x.text.includes('\n'))?.id);
    const whenDoc = documents.find((d) => d.text.includes('x.busy') === false && d.text.includes('span'));
    expect(whenDoc?.sourceTypeName).toBe('NEl');
    const repeatDoc = documents.find((d) => d.text.includes('li'));
    expect(repeatDoc?.sourceTypeName).toBe('Item');
    void inner;
  });

  it('css templates become css documents and never reach the html parser', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement, css, html } from '@microsoft/fast-element';
        const styles = css\`:host { color: red; }\`;
        @customElement({ name: 'c-el', template: html<CEl>\`<p>x</p>\`, styles })
        export class CEl extends FASTElement {}
      `,
    });
    harness.service.sync();
    const documents = harness.service.fileExtraction(ELEMENT)!.upsert.documents;
    const cssDoc = documents.find((d) => d.kind === 'css');
    expect(cssDoc).toBeDefined();
    expect(cssDoc!.componentTag).toBe('c-el');
  });
});
