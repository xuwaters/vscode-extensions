/**
 * Preview zoom, the same way `pdf-ultra` and `typst-ultra` do it: the keys step
 * through a ladder of levels, and `ctrl`/`alt`+wheel or a pinch zoom smoothly
 * about the pointer.
 *
 * The wheel is answered here: VSCode has no keybindings for it, and taking it
 * from the browser is what stops it zooming the webview's own page. The keys are
 * not. VSCode forwards every keystroke a webview sees to its own keybinding
 * resolver whatever the page does with it, so a shortcut answered in both places
 * is answered twice — they arrive as a host message instead, through `run`,
 * which is also what makes them rebindable.
 */
import type { ZoomAction } from '../../src/messages';

/**
 * Narrower than the PDF range: this text reflows, so past these ends it is one
 * word to a line, or a column too small to read.
 */
export const MIN_ZOOM = 0.25;
export const MAX_ZOOM = 5;

/** The levels the keys walk through. */
export const ZOOM_STEPS: readonly number[] = [
  0.25, 0.33, 0.5, 0.67, 0.8, 1, 1.25, 1.5, 2, 3, 4, 5,
];

/**
 * What a wheel delta of one pixel is worth, as an exponent. A hundredth, because
 * Chromium hands a pinch to the page as `ctrl`+wheel events whose `deltaY` is a
 * hundred times the log of the scale the fingers made — this is what makes a
 * pinch track them exactly.
 */
const WHEEL_ZOOM = 0.01;
/** The most one wheel event may scale by: a mouse notch moves about one step. */
const WHEEL_ZOOM_CAP = 1.25;
const WHEEL_LINE_PX = 16;
const WHEEL_PAGE_PX = 400;

/** Reporting to the host waits for the wheel to stop: it writes to storage. */
const SETTLE_MS = 200;

export function clampZoom(zoom: number): number {
  if (!Number.isFinite(zoom) || zoom <= 0) return 1;
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, zoom));
}

/** The next level in `direction`, or the end of the range. */
export function stepZoom(zoom: number, direction: 1 | -1): number {
  const next =
    direction > 0
      ? ZOOM_STEPS.find((step) => step > zoom + 0.001)
      : [...ZOOM_STEPS].reverse().find((step) => step < zoom - 0.001);
  return next ?? (direction > 0 ? MAX_ZOOM : MIN_ZOOM);
}

/** The zoom after one wheel event of `pixels`. */
export function wheelZoom(zoom: number, pixels: number): number {
  const cap = Math.log(WHEEL_ZOOM_CAP);
  const exponent = Math.min(cap, Math.max(-cap, -pixels * WHEEL_ZOOM));
  return clampZoom(zoom * Math.exp(exponent));
}

function wheelPixels(event: WheelEvent): number {
  if (event.deltaMode === 1) return event.deltaY * WHEEL_LINE_PX;
  if (event.deltaMode === 2) return event.deltaY * WHEEL_PAGE_PX;
  return event.deltaY;
}

interface Point {
  x: number;
  y: number;
}

/** A spot in the document, as a place in a block that survives a reflow. */
interface Anchor {
  block: HTMLElement;
  /** How far down the block, as a fraction of its height. */
  ratio: number;
  /** How far the point itself sits off the block, for a point in a margin. */
  gap: number;
}

function blockAt(content: HTMLElement, y: number): HTMLElement | null {
  let last: HTMLElement | null = null;
  for (const child of content.children) {
    last = child as HTMLElement;
    if (last.getBoundingClientRect().bottom >= y) break;
  }
  return last;
}

function takeAnchor(content: HTMLElement, at: Point): Anchor | null {
  const hit = document
    .elementFromPoint(at.x, at.y)
    ?.closest<HTMLElement>('[data-sourcepos]');
  // Over a margin or the toolbar there is no block to hit, only one to go by.
  const block = hit && content.contains(hit) ? hit : blockAt(content, at.y);
  if (!block) return null;
  const { top, height } = block.getBoundingClientRect();
  const ratio =
    height > 0 ? Math.min(1, Math.max(0, (at.y - top) / height)) : 0;
  return { block, ratio, gap: at.y - (top + ratio * height) };
}

/** Scroll so the anchored spot is back at `y`, `factor` times the size it was. */
function holdAnchor(anchor: Anchor, y: number, factor: number): void {
  const { top, height } = anchor.block.getBoundingClientRect();
  const now = top + anchor.ratio * height + anchor.gap * factor;
  window.scrollBy({ top: now - y, behavior: 'instant' });
}

export interface ZoomOptions {
  /** Everything that scales: the document, and the frontmatter card above it. */
  scaled: HTMLElement[];
  /** The rendered blocks, for keeping the reader's place across a reflow. */
  content: HTMLElement;
  initial: number;
  /** While true the page keeps out of the wheel: the image viewer is up. */
  suspended: () => boolean;
  /** The layout moved; called on every change. */
  onChange: () => void;
  /** The zoom has come to rest; called once a gesture is over. */
  onSettle: (zoom: number) => void;
}

export interface PageZoom {
  run(action: ZoomAction): void;
}

export function installZoom(options: ZoomOptions): PageZoom {
  const { scaled, content, suspended, onChange, onSettle } = options;
  let zoom = clampZoom(options.initial);
  let settle: ReturnType<typeof setTimeout> | undefined;

  function apply(): void {
    // Chromium supports non-standard `zoom`, which reflows (unlike transform).
    for (const el of scaled) el.style.setProperty('zoom', String(zoom));
  }
  apply();

  /** Change the zoom, holding the document still at `at` if there is one. */
  function setZoom(next: number, at: Point | null): void {
    if (next === zoom) return;
    const anchor = at && takeAnchor(content, at);
    const factor = next / zoom;
    zoom = next;
    apply();
    onChange();
    if (anchor && at) holdAnchor(anchor, at.y, factor);
    clearTimeout(settle);
    settle = setTimeout(() => onSettle(zoom), SETTLE_MS);
  }

  // Not passive: the whole point is to take the scroll away and zoom instead.
  window.addEventListener(
    'wheel',
    (event) => {
      if (!event.ctrlKey && !event.altKey) return;
      if (suspended()) return;
      event.preventDefault();
      const pixels = wheelPixels(event);
      if (pixels === 0) return;
      setZoom(wheelZoom(zoom, pixels), {
        x: event.clientX,
        y: event.clientY,
      });
    },
    { passive: false },
  );

  return {
    run(action) {
      const next =
        action === 'reset' ? 1 : stepZoom(zoom, action === 'in' ? 1 : -1);
      // The keys have no pointer to zoom about, so they hold what is at the top
      // of the view — and a page at its very top stays there.
      const top =
        window.scrollY > 0 ? { x: window.innerWidth / 2, y: 0 } : null;
      setZoom(next, top);
    },
  };
}
