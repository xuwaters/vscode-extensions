/**
 * Image lightbox: click any preview image for a dimmed full-size overlay;
 * `esc` or click closes it.
 */
export function installLightbox(content: HTMLElement): void {
  const overlay = document.createElement('div');
  overlay.id = 'lightbox';
  const img = document.createElement('img');
  overlay.append(img);
  document.body.append(overlay);

  const close = () => {
    overlay.classList.remove('visible');
    img.removeAttribute('src');
  };

  content.addEventListener('click', (event) => {
    const target = event.target as HTMLElement;
    if (!(target instanceof HTMLImageElement)) return;
    // Don't hijack images that act as links.
    if (target.closest('a')) return;
    event.preventDefault();
    img.src = target.currentSrc || target.src;
    overlay.classList.add('visible');
  });

  overlay.addEventListener('click', close);
  document.addEventListener('keydown', (event) => {
    if (event.key === 'Escape' && overlay.classList.contains('visible')) {
      close();
    }
  });
}
