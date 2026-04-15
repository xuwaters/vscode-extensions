import type { KeyBinding } from '@codemirror/view';
import type { EditorView } from '@codemirror/view';

export interface BlockNavigationCallbacks {
  /** Navigate to the previous block. Returns true if handled. */
  goToPreviousBlock: () => boolean;
  /** Navigate to the next block. Returns true if handled. */
  goToNextBlock: () => boolean;
  /** Deactivate the current block (return to preview). */
  deactivateBlock: () => void;
}

/** Create keybindings for block-level navigation in live preview mode. */
export function createBlockNavigationKeymap(
  callbacks: BlockNavigationCallbacks,
): KeyBinding[] {
  return [
    {
      key: 'ArrowUp',
      run: (view: EditorView): boolean => {
        // If cursor is on the first line of the editor, go to previous block
        const cursor = view.state.selection.main.head;
        const firstLine = view.state.doc.line(1);
        if (cursor <= firstLine.to) {
          return callbacks.goToPreviousBlock();
        }
        return false; // let default handler move cursor up
      },
    },
    {
      key: 'ArrowDown',
      run: (view: EditorView): boolean => {
        // If cursor is on the last line of the editor, go to next block
        const cursor = view.state.selection.main.head;
        const lastLine = view.state.doc.line(view.state.doc.lines);
        if (cursor >= lastLine.from) {
          return callbacks.goToNextBlock();
        }
        return false;
      },
    },
    {
      key: 'Escape',
      run: (): boolean => {
        callbacks.deactivateBlock();
        return true;
      },
    },
  ];
}
