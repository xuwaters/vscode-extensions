/**
 * Image lightbox: click any preview image to open it in a full-window viewer
 * that zooms and scrolls like VS Code's built-in image preview.
 *
 * - The image opens scaled to fit (never upscaled past its natural size).
 * - Click zooms in a step; `alt`+click (`ctrl`+click off macOS) zooms out.
 * - `ctrl`+wheel or a pinch (and `alt`+wheel on macOS) zooms smoothly.
 * - `ctrl`/`cmd` `+` / `-` zoom in / out a step; `0` returns to fit.
 * - Wheel scrolls up/down, `shift`+wheel left/right; scrollbars, arrow/page
 *   keys and dragging the image scroll it too.
 * - `esc`, the close button, or a click beside the image closes it.
 */

/** VS Code's image-preview zoom steps. */
export const ZOOM_LEVELS = [
  0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8, 0.9, 1, 1.5, 2, 3, 5, 7, 10, 15, 20,
];
const MIN_SCALE = 0.1;
const MAX_SCALE = 20;
/** At or past this scale pixels show as crisp squares, not a blur. */
const PIXELATION_THRESHOLD = 3;
/**
 * Zoom per wheel event, whatever its delta — as VS Code does. A pinch or a
 * trackpad scroll sends many small-delta events, so scaling by the delta
 * would crawl.
 */
const WHEEL_ZOOM_STEP = 0.075;
/** A press that travels further than this is a pan, not a click. */
const DRAG_THRESHOLD = 4;

const isMac = /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

/** The next zoom step above `scale`, or the maximum. */
export function zoomInLevel(scale: number): number {
  return ZOOM_LEVELS.find((level) => level > scale) ?? MAX_SCALE;
}

/** The next zoom step below `scale`, or the minimum. */
export function zoomOutLevel(scale: number): number {
  return [...ZOOM_LEVELS].reverse().find((level) => level < scale) ?? MIN_SCALE;
}

function clampScale(scale: number): number {
  return Math.min(MAX_SCALE, Math.max(MIN_SCALE, scale));
}

/** The platform's zoom-out modifier, matching VS Code's image preview. */
function zoomOutModifier(event: MouseEvent | KeyboardEvent): boolean {
  return isMac ? event.altKey : event.ctrlKey;
}

type Scale = number | 'fit';

interface Point {
  x: number;
  y: number;
}

interface Drag {
  pointerId: number;
  start: Point;
  scroll: Point;
  moved: boolean;
}

export function installLightbox(content: HTMLElement): void {
  const overlay = document.createElement('div');
  overlay.id = 'lightbox';

  const scroller = document.createElement('div');
  scroller.className = 'lightbox-scroller';
  scroller.tabIndex = -1;
  const img = document.createElement('img');
  img.draggable = false;
  img.alt = '';
  scroller.append(img);

  const bar = document.createElement('div');
  bar.className = 'lightbox-bar';
  const label = document.createElement('span');
  label.className = 'lightbox-zoom';
  const closeButton = document.createElement('button');
  closeButton.className = 'lightbox-close';
  closeButton.type = 'button';
  closeButton.textContent = '✕';
  closeButton.title = 'Close (Esc)';
  bar.append(label, closeButton);

  overlay.append(scroller, bar);
  document.body.append(overlay);

  let scale: Scale = 'fit';
  let returnFocus: HTMLElement | null = null;
  let drag: Drag | null = null;
  let suppressClick = false;

  const isOpen = () => overlay.classList.contains('visible');

  function open(source: HTMLImageElement): void {
    returnFocus = document.activeElement as HTMLElement | null;
    img.src = source.currentSrc || source.src;
    img.alt = source.alt;
    setScale('fit');
    overlay.classList.add('visible');
    scroller.scrollTo(0, 0);
    scroller.focus({ preventScroll: true });
  }

  function close(): void {
    overlay.classList.remove('visible', 'zoom-out-mode', 'panning');
    img.removeAttribute('src');
    drag = null;
    returnFocus?.focus({ preventScroll: true });
    returnFocus = null;
  }

  let fittedWidth = 0;

  /** The image's unscaled width; an SVG without one counts its fitted width. */
  function naturalWidth(): number {
    if (img.naturalWidth) return img.naturalWidth;
    if (scale === 'fit') fittedWidth = img.clientWidth;
    return fittedWidth;
  }

  /** The scale the image is shown at right now, `fit` resolved. */
  function currentScale(): number {
    if (scale !== 'fit') return scale;
    const natural = naturalWidth();
    return natural ? img.clientWidth / natural : 1;
  }

  function updateLabel(): void {
    label.textContent = `${Math.round(currentScale() * 100)}%`;
    label.title = scale === 'fit' ? 'Scaled to fit' : '';
  }

  /**
   * Shows the image at `next`, keeping the image point under `anchor`
   * (viewport coordinates; default the viewport center) where it was.
   */
  function setScale(next: Scale, anchor?: Point): void {
    if (next === 'fit') {
      scale = 'fit';
      img.classList.add('fit');
      img.classList.remove('pixelated');
      img.style.removeProperty('width');
      updateLabel();
      return;
    }

    const view = scroller.getBoundingClientRect();
    const at = anchor ?? { x: view.left + view.width / 2, y: view.top + view.height / 2 };
    const before = img.getBoundingClientRect();
    const fraction = {
      x: before.width ? clamp01((at.x - before.left) / before.width) : 0.5,
      y: before.height ? clamp01((at.y - before.top) / before.height) : 0.5,
    };

    const natural = naturalWidth();
    scale = clampScale(next);
    img.classList.remove('fit');
    img.classList.toggle('pixelated', scale >= PIXELATION_THRESHOLD);
    // Width only: the height follows the image's own aspect ratio, which an
    // SVG's `viewBox` gives even when its natural height is missing or wrong.
    img.style.width = `${natural * scale}px`;

    const after = img.getBoundingClientRect();
    scroller.scrollLeft += after.left + fraction.x * after.width - at.x;
    scroller.scrollTop += after.top + fraction.y * after.height - at.y;
    updateLabel();
  }

  const zoomIn = (anchor?: Point) => setScale(zoomInLevel(currentScale()), anchor);
  const zoomOut = (anchor?: Point) => setScale(zoomOutLevel(currentScale()), anchor);

  content.addEventListener('click', (event) => {
    const target = event.target as HTMLElement;
    if (!(target instanceof HTMLImageElement)) return;
    // Don't hijack images that act as links.
    if (target.closest('a')) return;
    event.preventDefault();
    open(target);
  });

  img.addEventListener('load', () => {
    if (isOpen()) updateLabel();
  });
  window.addEventListener('resize', () => {
    if (isOpen() && scale === 'fit') updateLabel();
  });

  closeButton.addEventListener('click', (event) => {
    event.stopPropagation();
    close();
  });

  scroller.addEventListener('click', (event) => {
    if (event.button !== 0) return;
    if (suppressClick) {
      suppressClick = false;
      return;
    }
    const at = { x: event.clientX, y: event.clientY };
    if (event.target === img) {
      if (zoomOutModifier(event)) zoomOut(at);
      else zoomIn(at);
    } else if (zoomOutModifier(event)) {
      zoomOut(at);
    } else {
      close();
    }
  });

  scroller.addEventListener(
    'wheel',
    (event) => {
      // A trackpad pinch arrives as a wheel event with `ctrlKey` set.
      if (event.ctrlKey || zoomOutModifier(event)) {
        event.preventDefault();
        const direction = Math.sign(event.deltaY);
        if (!direction) return;
        setScale(currentScale() * (1 - direction * WHEEL_ZOOM_STEP), {
          x: event.clientX,
          y: event.clientY,
        });
        return;
      }
      // `shift`+wheel scrolls sideways. macOS already turns it into a
      // horizontal delta; elsewhere the vertical one is ours to redirect.
      if (event.shiftKey && !event.deltaX && event.deltaY) {
        event.preventDefault();
        scroller.scrollLeft += wheelPixels(event, event.deltaY);
      }
    },
    { passive: false },
  );

  // Drag the image to scroll it.
  img.addEventListener('pointerdown', (event) => {
    suppressClick = false;
    if (event.button !== 0) return;
    drag = {
      pointerId: event.pointerId,
      start: { x: event.clientX, y: event.clientY },
      scroll: { x: scroller.scrollLeft, y: scroller.scrollTop },
      moved: false,
    };
  });
  window.addEventListener('pointermove', (event) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    const dx = event.clientX - drag.start.x;
    const dy = event.clientY - drag.start.y;
    if (!drag.moved && Math.hypot(dx, dy) < DRAG_THRESHOLD) return;
    drag.moved = true;
    overlay.classList.add('panning');
    scroller.scrollLeft = drag.scroll.x - dx;
    scroller.scrollTop = drag.scroll.y - dy;
  });
  const endDrag = (event: PointerEvent) => {
    if (!drag || event.pointerId !== drag.pointerId) return;
    suppressClick = drag.moved;
    drag = null;
    overlay.classList.remove('panning');
  };
  window.addEventListener('pointerup', endDrag);
  window.addEventListener('pointercancel', endDrag);

  // Capture phase, so the page zoom shortcuts never see keys meant for the image.
  window.addEventListener(
    'keydown',
    (event) => {
      if (!isOpen()) return;
      overlay.classList.toggle('zoom-out-mode', zoomOutModifier(event));
      if (event.key === 'Escape') {
        event.preventDefault();
        event.stopPropagation();
        close();
        return;
      }
      if (!event.ctrlKey && !event.metaKey) return;
      if (event.key === '=' || event.key === '+') zoomIn();
      else if (event.key === '-') zoomOut();
      else if (event.key === '0') setScale('fit');
      else return;
      event.preventDefault();
      event.stopPropagation();
    },
    true,
  );
  window.addEventListener(
    'keyup',
    (event) => {
      if (isOpen()) overlay.classList.toggle('zoom-out-mode', zoomOutModifier(event));
    },
    true,
  );
  window.addEventListener('blur', () => overlay.classList.remove('zoom-out-mode'));
}

/** A wheel delta in pixels, whichever unit the event reports it in. */
function wheelPixels(event: WheelEvent, delta: number): number {
  if (event.deltaMode === WheelEvent.DOM_DELTA_LINE) return delta * 40;
  if (event.deltaMode === WheelEvent.DOM_DELTA_PAGE) return delta * (event.currentTarget as HTMLElement).clientWidth;
  return delta;
}

function clamp01(value: number): number {
  return Math.min(1, Math.max(0, value));
}
