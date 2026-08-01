/**
 * Back/forward buttons for the preview's own link history.
 *
 * Following a markdown link swaps the document the panel shows, so the panel
 * needs its own way back — the editor's navigation stack is not the preview's.
 * The pair stays hidden until there is somewhere to go.
 */
export class NavButtons {
  private readonly back: HTMLButtonElement;
  private readonly forward: HTMLButtonElement;

  constructor(
    toolbar: HTMLElement,
    onNavigate: (direction: 'back' | 'forward') => void,
  ) {
    this.back = makeButton('‹', 'Back', () => onNavigate('back'));
    this.forward = makeButton('›', 'Forward', () => onNavigate('forward'));
    toolbar.append(this.back, this.forward);
    this.update(false, false);
  }

  update(canGoBack: boolean, canGoForward: boolean): void {
    this.back.disabled = !canGoBack;
    this.forward.disabled = !canGoForward;
    const idle = !canGoBack && !canGoForward;
    this.back.hidden = idle;
    this.forward.hidden = idle;
  }
}

function makeButton(
  label: string,
  title: string,
  onClick: () => void,
): HTMLButtonElement {
  const button = document.createElement('button');
  button.type = 'button';
  button.className = 'nav-button';
  button.textContent = label;
  button.title = title;
  button.addEventListener('click', onClick);
  return button;
}
