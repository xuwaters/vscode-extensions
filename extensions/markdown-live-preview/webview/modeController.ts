import { DocumentModel, type TextChange } from './documentModel';
import { SourceRenderer } from './renderers/sourceRenderer';
import { ReadRenderer } from './renderers/readRenderer';
import { LivePreviewRenderer } from './renderers/livePreviewRenderer';
import type { EditCallback, CursorCallback, Renderer, EditorMode } from './renderers/types';

const MODE_ORDER: EditorMode[] = ['source', 'live-preview', 'read'];

export class ModeController {
  private model: DocumentModel;
  private mode: EditorMode;
  private renderers: Record<EditorMode, Renderer>;
  private container: HTMLElement;

  constructor(
    container: HTMLElement,
    onEdit: EditCallback,
    onCursor: CursorCallback,
    theme: 'light' | 'dark',
  ) {
    this.container = container;
    this.model = new DocumentModel();
    this.mode = 'live-preview';

    this.renderers = {
      source: new SourceRenderer(onEdit, onCursor, theme),
      read: new ReadRenderer(),
      'live-preview': new LivePreviewRenderer(onEdit, onCursor, theme),
    };
  }

  /** Initialize with document content and start rendering. */
  init(content: string, uri: string, mode: EditorMode): void {
    this.model.init(content, uri);
    this.mode = mode;
    this.renderers[this.mode].mount(this.container, this.model);
  }

  /** Switch to a new mode, preserving cursor and scroll position. */
  setMode(newMode: EditorMode): void {
    if (newMode === this.mode) return;

    const currentRenderer = this.renderers[this.mode];
    const cursorLine = currentRenderer.getCursorLine();
    const scrollOffset = currentRenderer.getScrollOffset();

    currentRenderer.teardown();
    this.mode = newMode;

    const newRenderer = this.renderers[this.mode];
    newRenderer.mount(this.container, this.model);

    if (cursorLine !== null) {
      newRenderer.setCursorLine(cursorLine);
    }
    newRenderer.setScrollOffset(scrollOffset);
  }

  /** Cycle through modes: source → live-preview → read → source. */
  cycleMode(): EditorMode {
    const currentIdx = MODE_ORDER.indexOf(this.mode);
    const nextIdx = (currentIdx + 1) % MODE_ORDER.length;
    const nextMode = MODE_ORDER[nextIdx];
    this.setMode(nextMode);
    return nextMode;
  }

  /** Get the current mode. */
  getMode(): EditorMode {
    return this.mode;
  }

  /** Apply document changes from external sources. */
  applyDocumentChanges(changes: TextChange[]): void {
    this.model.applyChanges(changes);
    this.renderers[this.mode].onDocumentChanged(changes);
  }

  /** Full document reset (used when model needs complete refresh). */
  resetDocument(content: string, uri: string): void {
    const cursorLine = this.renderers[this.mode].getCursorLine();
    const scrollOffset = this.renderers[this.mode].getScrollOffset();

    this.renderers[this.mode].teardown();
    this.model.init(content, uri);
    this.renderers[this.mode].mount(this.container, this.model);

    if (cursorLine !== null) {
      this.renderers[this.mode].setCursorLine(cursorLine);
    }
    this.renderers[this.mode].setScrollOffset(scrollOffset);
  }
}
