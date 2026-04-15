import type { DocumentModel, TextChange } from '../documentModel';

export type EditorMode = 'source' | 'read' | 'live-preview';

export interface Renderer {
  /** Mount the renderer into the given container using the document model. */
  mount(container: HTMLElement, model: DocumentModel): void;

  /** Tear down the renderer, removing all DOM content and event listeners. */
  teardown(): void;

  /** Handle incremental document changes from external sources. */
  onDocumentChanged(changes: TextChange[]): void;

  /** Get the current cursor line (0-based) if available, for position preservation. */
  getCursorLine(): number | null;

  /** Get the current scroll offset for position preservation. */
  getScrollOffset(): number;

  /** Restore cursor to a given line after mount. */
  setCursorLine(line: number): void;

  /** Restore scroll offset after mount. */
  setScrollOffset(offset: number): void;
}

/** Callback to send an edit back to the host. */
export type EditCallback = (startLine: number, endLine: number, newText: string) => void;

/** Callback to notify the host about cursor position changes. */
export type CursorCallback = (line: number) => void;
