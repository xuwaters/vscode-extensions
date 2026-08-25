/**
 * Incremental sync: what a change invalidates, and what it leaves alone.
 *
 * `sync()` re-extracts the files whose text moved and then whatever imports
 * them, transitively, instead of the whole FAST slice. These are the cases
 * that arrangement has to get right — a fact that lives in one file and is
 * read from another must still update when the file it lives in changes,
 * even though the reading file was never touched.
 */

import { describe, expect, it } from 'vitest';

import { createHarness, fixture, messages, wasmBuilt } from './harness.js';

const TAGS = fixture('tags.ts');
const BASE = fixture('base.ts');
const ELEMENT = fixture('element.ts');
const CONSUMER = fixture('consumer.ts');
const UNRELATED = fixture('unrelated.ts');

describe.skipIf(!wasmBuilt)('incremental sync', () => {
  it('re-reads a tag name const from the file that owns it', () => {
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
    expect(harness.service.fileExtraction(ELEMENT)!.upsert.components[0].tagName).toBe('x-grid');

    // tags.ts moves; element.ts does not. Its component must still follow.
    harness.updateFile(TAGS, `export const GRID_TAG = 'x-table';`);
    harness.service.sync();
    expect(harness.service.fileExtraction(ELEMENT)!.upsert.components[0].tagName).toBe('x-table');
  });

  it('re-reads an inherited member from the base class file', () => {
    const harness = createHarness({
      [BASE]: `
        import { FASTElement, attr } from '@microsoft/fast-element';
        export class BaseCard extends FASTElement {
          @attr heading = '';
        }
      `,
      [ELEMENT]: `
        import { customElement } from '@microsoft/fast-element';
        import { BaseCard } from './base.js';
        @customElement({ name: 'x-card' })
        export class XCard extends BaseCard {}
      `,
    });
    harness.service.sync();
    const attrsOf = (): string[] =>
      harness
        .service.fileExtraction(ELEMENT)!
        .upsert.components[0].attributes.map((a) => a.name)
        .sort();
    expect(attrsOf()).toEqual(['heading']);

    harness.updateFile(
      BASE,
      `
        import { FASTElement, attr } from '@microsoft/fast-element';
        export class BaseCard extends FASTElement {
          @attr heading = '';
          @attr subtitle = '';
        }
      `,
    );
    harness.service.sync();
    expect(attrsOf()).toEqual(['heading', 'subtitle']);
  });

  it('updates a consumer template when the component it uses gains an attribute', () => {
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement, attr } from '@microsoft/fast-element';
        @customElement({ name: 'x-badge' })
        export class XBadge extends FASTElement {
          @attr label = '';
        }
        declare global {
          interface HTMLElementTagNameMap { 'x-badge': XBadge }
        }
      `,
      [CONSUMER]: `
        import { FASTElement, customElement, html } from '@microsoft/fast-element';
        import { XBadge } from './element.js';
        void XBadge;
        const template = html<XPanel>\`<x-badge tone="warn"></x-badge>\`;
        @customElement({ name: 'x-panel', template })
        export class XPanel extends FASTElement {}
      `,
    });
    // `tone` is not declared: the consumer is told so.
    expect(messages(harness.fastDiagnostics(CONSUMER)).join(' ')).toContain('tone');

    // Declaring it on the component clears the consumer's diagnostic, though
    // nothing in consumer.ts changed.
    harness.updateFile(
      ELEMENT,
      `
        import { FASTElement, customElement, attr } from '@microsoft/fast-element';
        @customElement({ name: 'x-badge' })
        export class XBadge extends FASTElement {
          @attr label = '';
          @attr tone = '';
        }
        declare global {
          interface HTMLElementTagNameMap { 'x-badge': XBadge }
        }
      `,
    );
    expect(messages(harness.fastDiagnostics(CONSUMER)).join(' ')).not.toContain('tone');
  });

  it('leaves a file nothing imports out of the re-extraction', () => {
    const harness = createHarness({
      [UNRELATED]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        @customElement({ name: 'x-alone' })
        export class XAlone extends FASTElement {}
      `,
      [ELEMENT]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        @customElement({ name: 'x-other' })
        export class XOther extends FASTElement {}
      `,
    });
    harness.service.sync();
    const before = harness.service.fileExtraction(UNRELATED);

    harness.updateFile(
      ELEMENT,
      `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        @customElement({ name: 'x-other-renamed' })
        export class XOther extends FASTElement {}
      `,
    );
    harness.service.sync();

    // The edited file is re-extracted; the one that imports nothing from it
    // keeps the extraction object it already had.
    expect(harness.service.fileExtraction(ELEMENT)!.upsert.components[0].tagName).toBe(
      'x-other-renamed',
    );
    expect(harness.service.fileExtraction(UNRELATED)).toBe(before);
  });

  it('still finds a duplicate tag name after the second file is edited into one', () => {
    // A cross-file rule the engine — not the extraction — owns, so it has to
    // survive one file being re-extracted without the other.
    const harness = createHarness({
      [ELEMENT]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        @customElement({ name: 'x-dup' })
        export class XOne extends FASTElement {}
      `,
      [UNRELATED]: `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        @customElement({ name: 'x-other' })
        export class XTwo extends FASTElement {}
      `,
    });
    harness.service.sync();
    expect(messages(harness.fastDiagnostics(UNRELATED)).join(' ')).not.toContain('x-dup');

    harness.updateFile(
      UNRELATED,
      `
        import { FASTElement, customElement } from '@microsoft/fast-element';
        @customElement({ name: 'x-dup' })
        export class XTwo extends FASTElement {}
      `,
    );
    expect(messages(harness.fastDiagnostics(UNRELATED)).join(' ')).toContain('x-dup');
  });
});
