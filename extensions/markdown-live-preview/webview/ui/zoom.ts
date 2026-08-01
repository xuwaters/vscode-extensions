/**
 * Preview zoom: `ctrl/cmd +` / `-` / `0` scales the content root. The level
 * is persisted by the caller via webview state.
 */
const MIN = 0.5;
const MAX = 3;
const STEP = 0.1;

export function installZoom(
  content: HTMLElement,
  initial: number,
  persist: (zoom: number) => void,
): void {
  let zoom = clamp(initial || 1);
  apply();

  function apply(): void {
    // Chromium supports non-standard `zoom`, which reflows (unlike transform).
    content.style.setProperty('zoom', String(zoom));
  }

  document.addEventListener('keydown', (event) => {
    if (!event.ctrlKey && !event.metaKey) return;
    let next: number;
    if (event.key === '=' || event.key === '+') next = zoom + STEP;
    else if (event.key === '-') next = zoom - STEP;
    else if (event.key === '0') next = 1;
    else return;
    event.preventDefault();
    zoom = clamp(next);
    apply();
    persist(zoom);
  });
}

function clamp(z: number): number {
  return Math.min(MAX, Math.max(MIN, Math.round(z * 100) / 100));
}
