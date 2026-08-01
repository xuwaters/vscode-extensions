/**
 * Per-fence copy button. Buttons are injected into changed blocks after each
 * patch; the click handler is delegated from the content root.
 */
export function addCopyButtons(roots: Element[]): void {
  for (const root of roots) {
    const pres: Element[] = root.matches('pre') ? [root] : [];
    pres.push(...root.querySelectorAll('pre'));
    for (const pre of pres) {
      if (pre.querySelector(':scope > .copy-code')) continue;
      if (!pre.querySelector(':scope > code')) continue;
      const button = document.createElement('button');
      button.className = 'copy-code';
      button.type = 'button';
      button.title = 'Copy code';
      button.textContent = 'Copy';
      pre.append(button);
    }
  }
}

/** Wire the delegated click handler (once, on the content root). */
export function installCopyHandler(content: HTMLElement): void {
  content.addEventListener('click', (event) => {
    const button = (event.target as HTMLElement).closest('button.copy-code');
    if (!button) return;
    const code = button.parentElement?.querySelector(':scope > code');
    const text = code?.textContent ?? '';
    void navigator.clipboard.writeText(text).then(() => {
      button.textContent = 'Copied';
      button.classList.add('copied');
      setTimeout(() => {
        button.textContent = 'Copy';
        button.classList.remove('copied');
      }, 1200);
    });
  });
}
