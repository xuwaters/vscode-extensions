import { html, ref, when } from '@microsoft/fast-element';
import type { TypstPreview } from './element.js';

/**
 * The preview's chrome.
 *
 * Everything reactive lives here and nothing else does: the page column is
 * built and sized imperatively by `PageColumn` — one box per page, filled with
 * SVG as it scrolls into view and emptied again behind the reader — so this
 * template only stakes out the scroller it lives in. A binding per page would
 * be a binding per SVG, and the whole point of the column is that most pages
 * have none.
 *
 * The fit buttons carry both `aria-pressed` and an `on` class, because they are
 * toggles rather than actions: a fit stays switched on and re-resolves itself
 * every time the panel changes size, so the toolbar has to say which one is in
 * force. Typing a zoom, or stepping it, is what switches them off.
 */
export const template = html<TypstPreview>`
  <div class="shell" data-background="${(x) => x.settings.background}">
    <div class="chrome" role="toolbar" aria-label="Preview tools">
      <button class="btn" title="Zoom out" aria-label="Zoom out" @click="${(x) => x.zoomBy(-1)}">
        −
      </button>
      <input
        class="field-input wide"
        type="text"
        inputmode="decimal"
        title="Zoom — type a percentage and press Enter"
        aria-label="Zoom percentage"
        ${ref('zoomInput')}
        :value="${(x) => x.zoomField}"
        @change="${(x, c) => x.onZoomEntered(c.event)}"
        @focus="${(_, c) => (c.event.target as HTMLInputElement).select()}"
        @blur="${(x) => x.showZoom()}"
        @keydown="${(x, c) => x.onZoomKeydown(c.event as KeyboardEvent)}"
      />
      <button class="btn" title="Zoom in" aria-label="Zoom in" @click="${(x) => x.zoomBy(1)}">
        +
      </button>
      <button
        class="btn ${(x) => (x.fit === 'width' ? 'on' : '')}"
        title="Fit width — stays on until the zoom is set by hand"
        aria-label="Fit width"
        aria-pressed="${(x) => String(x.fit === 'width')}"
        @click="${(x) => x.applyFit('width')}"
      >
        ↔
      </button>
      <button
        class="btn ${(x) => (x.fit === 'page' ? 'on' : '')}"
        title="Fit page — stays on until the zoom is set by hand"
        aria-label="Fit page"
        aria-pressed="${(x) => String(x.fit === 'page')}"
        @click="${(x) => x.applyFit('page')}"
      >
        ⤢
      </button>
      <button
        class="btn ${(x) => (x.inverted ? 'on' : '')}"
        title="Invert colors"
        aria-label="Invert colors"
        aria-pressed="${(x) => String(x.inverted)}"
        @click="${(x) => x.toggleInvert()}"
      >
        ◐
      </button>

      <span class="separator"></span>

      <!-- Leaving the page: the host decides what each of these means for the
           surface it is showing — a panel hands focus to the editor beside it,
           a full-tab preview hands the tab itself back. -->
      <button
        class="btn"
        title="Edit — show the source in a text editor"
        aria-label="Edit source"
        @click="${(x) => x.openSource()}"
      >
        ✎
      </button>
      <button
        class="btn"
        title="Export… — PDF, SVG, PNG, or HTML"
        aria-label="Export"
        @click="${(x) => x.exportDocument()}"
      >
        <!-- Inline SVG, not a glyph: the download arrows live in Miscellaneous
             Symbols and Arrows, which the system UI fonts do not cover, so the
             button rendered as tofu. Inline markup is document, not a fetch, so
             the CSP does not apply to it. -->
        <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
          <path
            d="M8 2v7.5M5 6.6 8 9.7l3-3.1M3 11.4v2.1h10v-2.1"
            fill="none"
            stroke="currentColor"
            stroke-width="1.3"
            stroke-linecap="round"
            stroke-linejoin="round"
          />
        </svg>
      </button>

      <span class="spacer"></span>

      <!-- Labelled, because a bare number box next to "/ 12" reads as decoration
           rather than as somewhere to type. -->
      <span class="field-label">Page</span>
      <input
        class="field-input"
        type="text"
        inputmode="numeric"
        title="Go to page"
        aria-label="Page number"
        ${ref('pageInput')}
        :value="${(x) => x.pageField}"
        @change="${(x, c) => x.onPageEntered(c.event)}"
        @focus="${(_, c) => (c.event.target as HTMLInputElement).select()}"
        @blur="${(x) => x.showPage()}"
        @keydown="${(x, c) => x.onPageKeydown(c.event as KeyboardEvent)}"
      />
      <span class="count">/ ${(x) => x.pageCount}</span>
    </div>

    ${when(
      (x) => x.status.state !== 'ok',
      html<TypstPreview>`
        <div class="status status-${(x) => x.status.state}" role="status">
          ${(x) => x.statusText}
        </div>
      `,
    )}

    <div
      class="pages ${(x) => x.columnClass}"
      tabindex="0"
      ${ref('scrollEl')}
      @scroll="${(x) => x.onScroll()}"
      @click="${(x, c) => x.onColumnClick(c.event)}"
    ></div>
  </div>
`;
