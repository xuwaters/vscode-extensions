import type { KeyBinding, EditorView } from '@codemirror/view';

/** Describes where to place the cursor when navigating to a block. */
export interface CursorPlacement {
  /** 0-based line within the block. Negative values count from end (-1 = last line). */
  line?: number;
  /** 0-based column. Clamped to line length. */
  col?: number;
  /** Absolute position shorthand (overrides line/col). */
  position?: 'start' | 'end';
}

export interface BlockNavigationCallbacks {
  /** Navigate to the previous block. Returns true if handled. */
  goToPreviousBlock: (placement: CursorPlacement) => boolean;
  /** Navigate to the next block. Returns true if handled. */
  goToNextBlock: (placement: CursorPlacement) => boolean;
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
        const cursor = view.state.selection.main.head;
        const firstLine = view.state.doc.line(1);
        if (cursor <= firstLine.to) {
          const col = cursor - firstLine.from;
          return callbacks.goToPreviousBlock({ line: -1, col });
        }
        return false; // let default handler move cursor up
      },
    },
    {
      key: 'ArrowDown',
      run: (view: EditorView): boolean => {
        const cursor = view.state.selection.main.head;
        const lastLine = view.state.doc.line(view.state.doc.lines);
        if (cursor >= lastLine.from) {
          const col = cursor - lastLine.from;
          return callbacks.goToNextBlock({ line: 0, col });
        }
        return false;
      },
    },
    {
      key: 'ArrowLeft',
      run: (view: EditorView): boolean => {
        const cursor = view.state.selection.main.head;
        if (cursor === 0) {
          return callbacks.goToPreviousBlock({ position: 'end' });
        }
        return false;
      },
    },
    {
      key: 'ArrowRight',
      run: (view: EditorView): boolean => {
        const cursor = view.state.selection.main.head;
        if (cursor === view.state.doc.length) {
          return callbacks.goToNextBlock({ position: 'start' });
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
