import { html, ref, repeat, when } from '@microsoft/fast-element';
import type { PdfViewer } from './element.js';
import type { OutlineRow } from '../model/outline.js';

/**
 * The viewer's chrome.
 *
 * Everything reactive lives here and nothing else does: the page column is
 * built and sized imperatively by `PageColumn` — one slot per page, rasterized
 * as it scrolls into view — so this template only stakes out the scroller and
 * its host, the way `mc-pdf-viewer` does. A binding per page would be a binding
 * per canvas, and the whole point of the column is that most pages have neither.
 *
 * Glyphs are inline SVG rather than characters. The obvious ones — the
 * hamburger, the rotation arrow, the half-filled circle — live in Unicode
 * blocks the system UI fonts do not reliably cover, and a button that renders
 * as tofu is worse than no button. Inline markup is document, not a fetch, so
 * the CSP does not apply to it.
 *
 * Which leaves a row of unlabelled pictures, so every control carries a
 * `data-tip` and, where one exists, the `data-keys` shortcut that does the same
 * thing. The toolbar shows them itself — see `PdfViewer.onTipOver` for why not
 * `title` — and `aria-label` stays alongside for the screen reader, which wants
 * the name and not the tooltip's furniture. The `.tip` box below the toolbar is
 * always in the DOM and hidden until it has been measured: it is centred under
 * the control from its own width, which an unrendered box does not have.
 */

/**
 * A shortcut written out for the eye, in the notation of the platform the
 * reader is on. These are the bindings `package.json` contributes, per
 * platform, and the two have to be kept in step by hand.
 */
const apple =
  typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.userAgent);
const shortcut = (windows: string, mac: string): string => (apple ? mac : windows);

const outlineTemplate = html<PdfViewer>`
  <aside
    class="outline"
    style="width: ${(x) => x.outlineWidth}px"
    aria-label="Document outline"
  >
    <div class="outline-tree" role="tree">
      ${when(
        (x) => x.outline.shown.length === 0,
        html<PdfViewer>`<div class="outline-empty">This document has no outline.</div>`,
      )}
      ${repeat(
        (x) => x.outline.shown,
        html<OutlineRow, PdfViewer>`
          <div
            class="outline-row ${(row, c) => (c.parent.outline.current === row.id ? 'current' : '')}"
            style="padding-left: ${(row) => 6 + row.depth * 14}px"
            role="treeitem"
            tabindex="0"
            title="${(row) => row.title}"
            @click="${(row, c) => c.parent.followOutline(row)}"
            @keydown="${(row, c) => c.parent.onOutlineKeydown(row, c.event as KeyboardEvent)}"
          >
            ${when(
              (row) => row.hasChildren,
              html<OutlineRow, PdfViewer>`
                <button
                  class="outline-twisty"
                  tabindex="-1"
                  aria-label="Expand or collapse"
                  @click="${(row, c) => c.parent.toggleOutlineRow(row, c.event)}"
                >
                  <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
                    <path
                      d="${(row, c) => (c.parent.isCollapsed(row) ? 'M6 4l4 4-4 4' : 'M4 6l4 4 4-4')}"
                      fill="none"
                      stroke="currentColor"
                      stroke-width="1.4"
                      stroke-linecap="round"
                      stroke-linejoin="round"
                    />
                  </svg>
                </button>
              `,
              html<OutlineRow>`<span class="outline-twisty-space"></span>`,
            )}
            <span class="outline-title">${(row) => row.title}</span>
          </div>
        `,
        { positioning: false },
      )}
    </div>
    <div
      class="outline-resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label="Resize the outline"
      @pointerdown="${(x, c) => x.onResizeStart(c.event as PointerEvent)}"
      @dblclick="${(x) => x.resetOutlineWidth()}"
    ></div>
  </aside>
`;

const chromeTemplate = html<PdfViewer>`
  <div
    class="chrome"
    role="toolbar"
    aria-label="Document tools"
    @pointerover="${(x, c) => x.onTipOver(c.event)}"
    @pointerleave="${(x) => x.onTipLeave()}"
    @focusin="${(x, c) => x.onTipFocus(c.event)}"
    @focusout="${(x) => x.onTipLeave()}"
  >
    <button
      class="btn ${(x) => (x.outlineVisible ? 'on' : '')}"
      data-tip="Outline"
      data-keys="${shortcut('Ctrl+K Ctrl+O', '⌘K ⌘O')}"
      aria-label="Toggle outline"
      aria-pressed="${(x) => String(x.outlineVisible)}"
      @click="${(x) => x.toggleOutline()}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M2 4h12M2 8h12M2 12h9"
          fill="none"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
        />
      </svg>
    </button>

    <span class="separator"></span>

    <button
      class="btn"
      data-tip="Back to where you jumped from"
      data-keys="${shortcut('Alt+←', '⌘[')}"
      aria-label="Go back"
      ?disabled="${(x) => !x.history.canGoBack}"
      @click="${(x) => x.goBack()}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M13 8H3M7 4L3 8l4 4"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>

    <button
      class="btn"
      data-tip="Previous page"
      data-keys="Page Up"
      aria-label="Previous page"
      ?disabled="${(x) => x.page <= 1}"
      @click="${(x) => x.goToPage(x.page - 1)}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M4 10l4-4 4 4"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>
    <input
      class="field-input"
      type="text"
      inputmode="numeric"
      data-tip="Go to page"
      data-keys="${shortcut('Ctrl+G', '⌘G')}"
      aria-label="Page number"
      ${ref('pageInput')}
      :value="${(x) => x.pageField}"
      @change="${(x, c) => x.onPageEntered(c.event)}"
      @focus="${(_, c) => (c.event.target as HTMLInputElement).select()}"
      @blur="${(x) => x.showPage()}"
      @keydown="${(x, c) => x.onPageKeydown(c.event as KeyboardEvent)}"
    />
    <span class="count">/ ${(x) => x.pageCount}</span>
    <button
      class="btn"
      data-tip="Next page"
      data-keys="Page Down"
      aria-label="Next page"
      ?disabled="${(x) => x.page >= x.pageCount}"
      @click="${(x) => x.goToPage(x.page + 1)}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M4 6l4 4 4-4"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>

    <span class="separator"></span>

    <button
      class="btn"
      data-tip="Zoom out"
      data-keys="${shortcut('Ctrl+-', '⌘-')}"
      aria-label="Zoom out"
      @click="${(x) => x.zoomBy(-1)}"
    >
      −
    </button>
    <input
      class="field-input wide"
      type="text"
      inputmode="decimal"
      data-tip="Zoom — type a percentage and press Enter"
      data-keys="${shortcut('Ctrl+0 for 100%', '⌘0 for 100%')}"
      aria-label="Zoom percentage"
      ${ref('zoomInput')}
      :value="${(x) => x.zoomField}"
      @change="${(x, c) => x.onZoomEntered(c.event)}"
      @focus="${(_, c) => (c.event.target as HTMLInputElement).select()}"
      @blur="${(x) => x.showZoom()}"
      @keydown="${(x, c) => x.onZoomKeydown(c.event as KeyboardEvent)}"
    />
    <button
      class="btn"
      data-tip="Zoom in"
      data-keys="${shortcut('Ctrl+=', '⌘=')}"
      aria-label="Zoom in"
      @click="${(x) => x.zoomBy(1)}"
    >
      +
    </button>
    <button
      class="btn ${(x) => (x.fit === 'fit-width' ? 'on' : '')}"
      data-tip="Fit width"
      data-keys="${shortcut('Ctrl+9', '⌘9')}"
      aria-label="Fit width"
      @click="${(x) => x.applyFit('fit-width')}"
    >
      ↔
    </button>
    <button
      class="btn ${(x) => (x.fit === 'fit-page' ? 'on' : '')}"
      data-tip="Fit page"
      aria-label="Fit page"
      @click="${(x) => x.applyFit('fit-page')}"
    >
      ⤢
    </button>
    <button
      class="btn ${(x) => (x.mode === 'single' ? 'on' : '')}"
      data-tip="One page at a time"
      data-keys="${shortcut('Ctrl+1 for continuous', '⌘1 for continuous')}"
      aria-label="Show one page at a time"
      aria-pressed="${(x) => String(x.mode === 'single')}"
      @click="${(x) => x.togglePageMode()}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <rect
          x="4"
          y="2"
          width="8"
          height="12"
          rx="1"
          fill="none"
          stroke="currentColor"
          stroke-width="1.3"
        />
      </svg>
    </button>
    <button
      class="btn ${(x) => (x.mode === 'dual' ? 'on' : '')}"
      data-tip="Two pages side by side"
      data-keys="${shortcut('Ctrl+2', '⌘2')}"
      aria-label="Show two pages side by side"
      aria-pressed="${(x) => String(x.mode === 'dual')}"
      @click="${(x) => x.toggleDualMode()}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <rect
          x="1"
          y="2"
          width="6"
          height="12"
          rx="1"
          fill="none"
          stroke="currentColor"
          stroke-width="1.3"
        />
        <rect
          x="9"
          y="2"
          width="6"
          height="12"
          rx="1"
          fill="none"
          stroke="currentColor"
          stroke-width="1.3"
        />
      </svg>
    </button>
    <button
      class="btn"
      data-tip="Rotate clockwise"
      data-keys="Shift-click to go the other way"
      aria-label="Rotate clockwise"
      @click="${(x, c) => x.rotateBy((c.event as MouseEvent).shiftKey ? -1 : 1)}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M13 8a5 5 0 1 1-1.6-3.7M13 2v3h-3"
          fill="none"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>
    <button
      class="btn ${(x) => (x.inverted ? 'on' : '')}"
      data-tip="Invert colours"
      data-keys="${shortcut('Ctrl+K Ctrl+I', '⌘K ⌘I')}"
      aria-label="Invert colours"
      aria-pressed="${(x) => String(x.inverted)}"
      @click="${(x) => x.toggleInvert()}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.3" />
        <path d="M8 2a6 6 0 0 1 0 12z" fill="currentColor" />
      </svg>
    </button>

    <span class="spacer"></span>

    <input
      class="find"
      type="search"
      placeholder="Find in document"
      data-tip="Find in document"
      data-keys="${shortcut('Ctrl+F', '⌘F')}"
      aria-label="Find in document"
      ${ref('findInput')}
      :value="${(x) => x.search.query}"
      @input="${(x, c) => x.onFindInput(c.event)}"
      @keydown="${(x, c) => x.onFindKeydown(c.event as KeyboardEvent)}"
    />
    <span class="find-count" aria-live="polite">${(x) => x.search.label}</span>
    <button
      class="btn"
      data-tip="Previous match"
      data-keys="Shift+Enter"
      aria-label="Previous match"
      ?disabled="${(x) => x.search.total === 0}"
      @click="${(x) => x.search.step(-1)}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M4 10l4-4 4 4"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>
    <button
      class="btn"
      data-tip="Next match"
      data-keys="Enter"
      aria-label="Next match"
      ?disabled="${(x) => x.search.total === 0}"
      @click="${(x) => x.search.step(1)}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M4 6l4 4 4-4"
          fill="none"
          stroke="currentColor"
          stroke-width="1.4"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>
  </div>
`;

export const template = html<PdfViewer>`
  ${when(
    (x) => x.state === 'ready',
    html<PdfViewer>`
      <div class="shell" data-background="${(x) => x.settings.background}">
        ${chromeTemplate}
        <div class="tip" role="tooltip" aria-hidden="true" ${ref('tipEl')}>
          <span>${(x) => x.tip?.text ?? ''}</span>
          <span class="tip-keys">${(x) => x.tip?.keys ?? ''}</span>
        </div>
        ${when(
          (x) => x.notice !== '',
          html<PdfViewer>`<div class="notice" role="status">${(x) => x.notice}</div>`,
        )}
        <div class="body">
          ${when((x) => x.outlineVisible, outlineTemplate)}
          <div class="viewer" tabindex="0" ${ref('scrollEl')} @keydown="${(x, c) => x.onViewerKeydown(c.event as KeyboardEvent)}">
            <div class="column ${(x) => (x.inverted ? 'inverted' : '')}" ${ref('columnEl')}></div>
          </div>
        </div>
      </div>
    `,
  )}
  ${when(
    (x) => x.state === 'loading',
    html<PdfViewer>`
      <div class="card">
        <div class="card-name">${(x) => x.name}</div>
        <div class="card-hint">${(x) => x.loadingHint}</div>
        <div class="bar" role="progressbar">
          <div class="bar-fill" style="width: ${(x) => Math.round(x.progress * 100)}%"></div>
        </div>
      </div>
    `,
  )}
  ${when(
    (x) => x.state === 'error',
    html<PdfViewer>`
      <div class="card">
        <div class="card-name">${(x) => x.name}</div>
        <div class="card-hint error">${(x) => x.errorMessage}</div>
        <button class="btn wide" @click="${(x) => x.retry()}">Try again</button>
      </div>
    `,
  )}
`;
