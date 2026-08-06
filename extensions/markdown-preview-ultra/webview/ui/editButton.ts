/**
 * In-page twin of the Edit icon in the editor title bar: hands the tab back to
 * the text editor.
 *
 * A full-tab preview shows no source at all, so the way back to editing is
 * otherwise a trip to the title bar or the keyboard — a click away from the
 * page the reader is already looking at.
 */
export function installEditButton(
  toolbar: HTMLElement,
  onEdit: () => void,
): void {
  const button = document.createElement('button');
  button.id = 'edit-source';
  button.type = 'button';
  button.textContent = '✎';
  button.title = 'Edit — open the source in a text editor';
  button.addEventListener('click', () => onEdit());
  toolbar.append(button);
}
