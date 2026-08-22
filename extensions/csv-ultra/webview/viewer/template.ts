import { html, ref, repeat, when } from '@microsoft/fast-element';
import type { CsvGrid, MenuItem } from './element.js';

/**
 * The table's chrome.
 *
 * Everything reactive lives here and nothing else does: the cells are built and
 * positioned imperatively by `Sheet` — a few hundred recycled boxes over a file
 * of any size — so this template only stakes out the four regions the sheet
 * draws into and the controls around them.
 *
 * Glyphs are inline SVG rather than characters, because the obvious ones live in
 * Unicode blocks the system UI fonts do not reliably cover and a button that
 * renders as tofu is worse than no button. Inline markup is document, not a
 * fetch, so the CSP does not apply to it.
 */

/** A shortcut written out for the eye, in the notation of the platform. */
const apple = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.userAgent);
const shortcut = (windows: string, mac: string): string => (apple ? mac : windows);

const chrome = html<CsvGrid>`
  <div class="chrome" role="toolbar" aria-label="Table tools">
    <button
      class="btn"
      title="Open in the text editor"
      aria-label="Open in the text editor"
      @click="${(x) => x.runHost('openInTextEditor')}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M6 3L2 8l4 5M10 3l4 5-4 5"
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
      class="btn ${(x) => (x.hasHeader ? 'on' : '')}"
      title="First row is a header  ·  ${shortcut('Ctrl+K Ctrl+H', '⌘K ⌘H')}"
      aria-label="Toggle header row"
      aria-pressed="${(x) => String(x.hasHeader)}"
      @click="${(x) => x.toggleHeader()}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path d="M2 3h12v3H2z" fill="currentColor" opacity="0.8" />
        <path
          d="M2 3h12v10H2zM2 6h12M6 6v7M10 6v7"
          fill="none"
          stroke="currentColor"
          stroke-width="1.2"
        />
      </svg>
    </button>

    <button
      class="btn ${(x) => (x.wrap ? 'on' : '')}"
      title="Wrap cell text  ·  ${shortcut('Ctrl+K Ctrl+W', '⌘K ⌘W')}"
      aria-label="Toggle cell wrapping"
      aria-pressed="${(x) => String(x.wrap)}"
      @click="${(x) => x.toggleWrap()}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M2 4h12M2 8h9a2.5 2.5 0 010 5H8m0 0l2-2m-2 2l2 2M2 12h3"
          fill="none"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>

    <button
      class="btn"
      title="Fit the columns to their content  ·  ${shortcut('Ctrl+K Ctrl+A', '⌘K ⌘A')}"
      aria-label="Fit columns"
      @click="${(x) => x.autoFitColumns()}"
    >
      <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
        <path
          d="M1 3v10M15 3v10M4 8h8M4 8l2.5-2.5M4 8l2.5 2.5M12 8l-2.5-2.5M12 8l-2.5 2.5"
          fill="none"
          stroke="currentColor"
          stroke-width="1.3"
          stroke-linecap="round"
          stroke-linejoin="round"
        />
      </svg>
    </button>

    ${when(
      (x) => x.sort !== null,
      html<CsvGrid>`
        <span class="separator"></span>
        <span class="chip sorted">
          <span class="chip-text"
            >Sorted by ${(x) => x.sortLabel}
            ${(x) => (x.sort?.direction === 'asc' ? '↑' : '↓')}</span
          >
          <button
            class="chip-btn"
            title="Write this order into the file"
            aria-label="Write the sorted order into the file"
            @click="${(x) => x.applySort()}"
          >
            Write
          </button>
          <button
            class="chip-btn"
            title="Back to the file's own order"
            aria-label="Clear the sort"
            @click="${(x) => x.clearSort()}"
          >
            ✕
          </button>
        </span>
      `,
    )}

    <span class="spacer"></span>

    ${when(
      (x) => x.findOpen,
      html<CsvGrid>`
        <div class="find">
          <input
            class="find-input"
            type="text"
            placeholder="Find in table"
            aria-label="Find in table"
            ${ref('findInput')}
            :value="${(x) => x.findQuery}"
            @input="${(x, c) => x.onFindInput(c.event)}"
            @keydown="${(x, c) => x.onFindKeydown(c.event as KeyboardEvent)}"
          />
          <button
            class="mini ${(x) => (x.matchCase ? 'on' : '')}"
            title="Match case"
            aria-label="Match case"
            aria-pressed="${(x) => String(x.matchCase)}"
            @click="${(x) => x.toggleMatchCase()}"
          >
            Aa
          </button>
          <button
            class="mini ${(x) => (x.wholeCell ? 'on' : '')}"
            title="Match the whole cell"
            aria-label="Match the whole cell"
            aria-pressed="${(x) => String(x.wholeCell)}"
            @click="${(x) => x.toggleWholeCell()}"
          >
            ⌷
          </button>
          <span class="find-count">${(x) => x.findCount}</span>
          <button
            class="mini"
            title="Previous match  ·  ${shortcut('Shift+F3', '⇧⌘F3')}"
            aria-label="Previous match"
            @click="${(x) => x.step(false)}"
          >
            ↑
          </button>
          <button
            class="mini"
            title="Next match  ·  ${shortcut('F3', '⇧⌘G')}"
            aria-label="Next match"
            @click="${(x) => x.step(true)}"
          >
            ↓
          </button>
          <button class="mini" title="Close" aria-label="Close find" @click="${(x) => x.closeFind()}">
            ✕
          </button>
        </div>
      `,
      html<CsvGrid>`
        <button
          class="btn"
          title="Find in table  ·  ${shortcut('Ctrl+F', '⌘F')}"
          aria-label="Find in table"
          @click="${(x) => x.openFind()}"
        >
          <svg class="icon" viewBox="0 0 16 16" aria-hidden="true" focusable="false">
            <circle cx="7" cy="7" r="4.2" fill="none" stroke="currentColor" stroke-width="1.4" />
            <path d="M10.2 10.2L14 14" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
          </svg>
        </button>
      `,
    )}

    <span class="separator"></span>

    <input
      class="row-input"
      type="text"
      inputmode="numeric"
      title="Go to row  ·  ${shortcut('Ctrl+G', '⌘G')}"
      aria-label="Row number"
      ${ref('rowInput')}
      :value="${(x) => x.rowField}"
      @change="${(x, c) => x.onRowEntered(c.event)}"
      @focus="${(_, c) => (c.event.target as HTMLInputElement).select()}"
      @blur="${(x) => x.showRow()}"
      @keydown="${(x, c) => x.onRowKeydown(c.event as KeyboardEvent)}"
    />
    <span class="count">/ ${(x) => x.rowsLabel}</span>

    <button
      class="chip"
      title="Read this file with a different delimiter"
      aria-label="Delimiter"
      @click="${(x) => x.runHost('setDelimiter')}"
    >
      <span class="chip-text">${(x) => x.delimiterLabel}</span>
    </button>
  </div>
`;

const table = html<CsvGrid>`
  <div class="table" ${ref('tableEl')} tabindex="0" @keydown="${(x, c) => x.onKeydown(c.event as KeyboardEvent)}">
    <!--
      Purely visual: the corner is hit-tested by geometry like every other
      region of the table, so it needs no ref and no listener of its own.
    -->
    <div class="corner" aria-hidden="true" title="Select all"></div>
    <div class="colhead" ${ref('columnHeadEl')}><div class="colhead-inner"></div></div>
    <div class="rowhead" ${ref('rowHeadEl')}><div class="rowhead-inner"></div></div>
    <div class="viewport" ${ref('viewportEl')}>
      <div class="canvas" ${ref('canvasEl')}></div>
    </div>
    <span class="measure" ${ref('measureEl')}></span>
  </div>
`;

const footer = html<CsvGrid>`
  <div class="footer">
    <span class="footer-cell">${(x) => x.cellLabel}</span>
    <span class="spacer"></span>
    <span class="footer-stats">${(x) => x.summary}</span>
    ${when(
      (x) => x.notice !== '',
      html<CsvGrid>`<span class="notice">${(x) => x.notice}</span>`,
    )}
  </div>
`;

/**
 * The right-click menu.
 *
 * In the page rather than contributed to VSCode's own context menu, because
 * VSCode has no idea what is under a pointer inside a webview — it cannot know
 * whether the reader right-clicked a row number, a column head or a cell, which
 * is exactly what decides whether the menu should offer to delete a row or a
 * column.
 */
const menu = html<CsvGrid>`
  <div
    class="menu"
    role="menu"
    style="left: ${(x) => x.menu?.x ?? 0}px; top: ${(x) => x.menu?.y ?? 0}px"
  >
    ${repeat(
      (x) => x.menu?.items ?? [],
      html<MenuItem, CsvGrid>`
        <button class="menu-item" role="menuitem" @click="${(item, c) => c.parent.pick(item)}">
          ${(item) => item.label}
        </button>
      `,
      { positioning: false },
    )}
  </div>
`;

const card = html<CsvGrid>`
  <div class="card">
    <div class="card-name">${(x) => x.name}</div>
    <div class="card-body">${(x) => x.cardMessage}</div>
    ${when(
      (x) => x.state === 'refused',
      html<CsvGrid>`
        <button class="card-action" @click="${(x) => x.runHost('openInTextEditor')}">
          Open in the text editor
        </button>
      `,
    )}
  </div>
`;

/**
 * The whole viewer.
 *
 * The table's markup is always in the document, hidden rather than removed while
 * the tab has nothing to show. `Sheet` holds direct references to those four
 * boxes, and a `when` around them would tear the boxes out from under it on
 * every state change — so the state is a class, and the card sits over the top.
 */
export const template = html<CsvGrid>`
  <div class="shell ${(x) => (x.state === 'ready' ? '' : 'blank')}">
    ${chrome} ${table} ${footer} ${when((x) => x.state !== 'ready', card)}
    ${when((x) => x.menu !== null, menu)}
  </div>
`;
