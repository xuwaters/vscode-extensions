import type { EditorView } from '@codemirror/view';
import type { DocumentModel, TextChange } from '../documentModel';
import { createBlockEditor } from '../blockEditor';
import type { EditCallback, CursorCallback, Renderer } from './types';

export class SourceRenderer implements Renderer {
  private editor: EditorView | null = null;
  private container: HTMLElement | null = null;
  private model: DocumentModel | null = null;
  private suppressChange = false;

  constructor(
    private readonly onEdit: EditCallback,
    private readonly onCursor: CursorCallback,
    private readonly theme: 'light' | 'dark',
  ) {}

  mount(container: HTMLElement, model: DocumentModel): void {
    this.container = container;
    this.model = model;
    container.innerHTML = '';
    container.className = 'source-mode';

    const wrapper = document.createElement('div');
    wrapper.className = 'source-editor-wrapper';
    container.appendChild(wrapper);

    this.editor = createBlockEditor({
      parent: wrapper,
      content: model.getFullText(),
      theme: this.theme,
      onContentChanged: (newContent) => {
        if (this.suppressChange) return;
        this.onEdit(0, model.getLineCount(), newContent);
      },
    });

    // Track cursor changes
    // Use a debounced update listener approach
    this.editor.dom.addEventListener('keyup', () => this.reportCursor());
    this.editor.dom.addEventListener('mouseup', () => this.reportCursor());
  }

  teardown(): void {
    this.editor?.destroy();
    this.editor = null;
    if (this.container) {
      this.container.innerHTML = '';
      this.container.className = '';
    }
    this.container = null;
    this.model = null;
  }

  onDocumentChanged(changes: TextChange[]): void {
    if (!this.editor || !this.model) return;
    // Re-sync the full document content from the model
    this.suppressChange = true;
    const currentContent = this.editor.state.doc.toString();
    const newContent = this.model.getFullText();
    if (currentContent !== newContent) {
      this.editor.dispatch({
        changes: {
          from: 0,
          to: currentContent.length,
          insert: newContent,
        },
      });
    }
    this.suppressChange = false;
  }

  getCursorLine(): number | null {
    if (!this.editor) return null;
    const pos = this.editor.state.selection.main.head;
    return this.editor.state.doc.lineAt(pos).number - 1;
  }

  getScrollOffset(): number {
    return this.container?.scrollTop ?? 0;
  }

  setCursorLine(line: number): void {
    if (!this.editor) return;
    const lineNum = Math.min(line + 1, this.editor.state.doc.lines);
    const lineObj = this.editor.state.doc.line(lineNum);
    this.editor.dispatch({ selection: { anchor: lineObj.from } });
    this.editor.focus();
  }

  setScrollOffset(offset: number): void {
    if (this.container) {
      this.container.scrollTop = offset;
    }
  }

  private reportCursor(): void {
    const line = this.getCursorLine();
    if (line !== null) {
      this.onCursor(line);
    }
  }
}
