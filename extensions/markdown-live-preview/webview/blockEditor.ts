import { EditorView, basicSetup } from 'codemirror';
import { markdown } from '@codemirror/lang-markdown';
import { oneDark } from '@codemirror/theme-one-dark';
import { EditorState, type Extension } from '@codemirror/state';
import { keymap } from '@codemirror/view';
import type { KeyBinding } from '@codemirror/view';

export interface BlockEditorOptions {
  parent: HTMLElement;
  content: string;
  cursorLine?: number;
  cursorCol?: number;
  theme: 'light' | 'dark';
  readOnly?: boolean;
  extraKeymaps?: KeyBinding[];
  onContentChanged?: (newContent: string) => void;
  onFocus?: () => void;
  onBlur?: () => void;
}

/** Create a CodeMirror 6 editor instance for editing markdown blocks. */
export function createBlockEditor(opts: BlockEditorOptions): EditorView {
  const extensions: Extension[] = [
    basicSetup,
    markdown(),
    EditorView.lineWrapping,
  ];

  if (opts.theme === 'dark') {
    extensions.push(oneDark);
  }

  if (opts.readOnly) {
    extensions.push(EditorState.readOnly.of(true));
  }

  if (opts.extraKeymaps && opts.extraKeymaps.length > 0) {
    extensions.push(keymap.of(opts.extraKeymaps));
  }

  if (opts.onContentChanged) {
    const callback = opts.onContentChanged;
    extensions.push(
      EditorView.updateListener.of((update) => {
        if (update.docChanged) {
          callback(update.state.doc.toString());
        }
      }),
    );
  }

  if (opts.onFocus || opts.onBlur) {
    extensions.push(
      EditorView.domEventHandlers({
        focus: () => {
          opts.onFocus?.();
          return false;
        },
        blur: () => {
          opts.onBlur?.();
          return false;
        },
      }),
    );
  }

  const view = new EditorView({
    doc: opts.content,
    extensions,
    parent: opts.parent,
  });

  // Set cursor position if specified
  if (opts.cursorLine != null) {
    const lineNum = Math.min(opts.cursorLine + 1, view.state.doc.lines);
    const line = view.state.doc.line(lineNum);
    const col = Math.min(opts.cursorCol ?? 0, line.length);
    view.dispatch({ selection: { anchor: line.from + col } });
  }

  view.focus();
  return view;
}
