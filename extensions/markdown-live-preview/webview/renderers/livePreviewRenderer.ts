import type { EditorView } from '@codemirror/view';
import type { DocumentModel, BlockRange, TextChange } from '../documentModel';
import { createBlockEditor } from '../blockEditor';
import { createBlockNavigationKeymap } from '../keymap';
import type { EditCallback, CursorCallback, Renderer } from './types';

export class LivePreviewRenderer implements Renderer {
  private container: HTMLElement | null = null;
  private model: DocumentModel | null = null;
  private activeBlock: BlockRange | null = null;
  private activeEditor: EditorView | null = null;
  private activeBlockElement: HTMLElement | null = null;
  private originalBlockText: string | null = null;

  constructor(
    private readonly onEdit: EditCallback,
    private readonly onCursor: CursorCallback,
    private readonly theme: 'light' | 'dark',
  ) {}

  mount(container: HTMLElement, model: DocumentModel): void {
    this.container = container;
    this.model = model;
    container.className = 'live-preview-mode';
    container.innerHTML = '';

    this.renderAllBlocks();
    this.attachClickHandlers();
  }

  teardown(): void {
    this.deactivateBlock();
    if (this.container) {
      this.container.innerHTML = '';
      this.container.className = '';
    }
    this.container = null;
    this.model = null;
  }

  onDocumentChanged(_changes: TextChange[]): void {
    if (!this.container || !this.model) return;

    // For simplicity, re-render all non-active blocks
    // The active block's editor content is managed by the user
    const activeStart = this.activeBlock?.startLine ?? -1;
    const activeEnd = this.activeBlock?.endLine ?? -1;

    const blocks = this.model.getBlocks();
    const blockElements = this.container.querySelectorAll<HTMLElement>('[data-line-start]');

    for (const el of blockElements) {
      const startLine = parseInt(el.getAttribute('data-line-start') ?? '-1', 10);
      const endLine = parseInt(el.getAttribute('data-line-end') ?? '-1', 10);

      // Skip the active block being edited
      if (startLine === activeStart && endLine === activeEnd) continue;

      // Find the corresponding block and re-render
      const block = blocks.find(
        (b) => b.startLine === startLine && b.endLine === endLine,
      );
      if (block) {
        el.innerHTML = this.model.renderBlock(block);
      }
    }
  }

  getCursorLine(): number | null {
    if (!this.activeBlock || !this.activeEditor) return null;
    const pos = this.activeEditor.state.selection.main.head;
    const editorLine = this.activeEditor.state.doc.lineAt(pos).number - 1;
    return this.activeBlock.startLine + editorLine;
  }

  getScrollOffset(): number {
    return this.container?.scrollTop ?? 0;
  }

  setCursorLine(line: number): void {
    if (!this.model) return;
    const block = this.model.getBlockForLine(line);
    if (block) {
      const localLine = line - block.startLine;
      this.activateBlock(block, localLine, 0);
    }
  }

  setScrollOffset(offset: number): void {
    if (this.container) {
      this.container.scrollTop = offset;
    }
  }

  /** Activate edit mode for a specific block. */
  activateBlock(
    block: BlockRange,
    cursorLine = 0,
    cursorCol = 0,
  ): void {
    if (!this.container || !this.model) return;

    // Deactivate current block first
    this.deactivateBlock();

    this.activeBlock = block;
    this.originalBlockText = this.model.getBlockText(block);

    // Find the DOM element for this block
    const el = this.container.querySelector<HTMLElement>(
      `[data-line-start="${block.startLine}"]`,
    );
    if (!el) return;

    this.activeBlockElement = el;
    el.classList.add('block-editing');
    el.innerHTML = '';

    // Create CodeMirror editor for this block
    const keymaps = createBlockNavigationKeymap({
      goToPreviousBlock: () => this.navigateToPreviousBlock(),
      goToNextBlock: () => this.navigateToNextBlock(),
      deactivateBlock: () => this.deactivateBlock(),
    });

    this.activeEditor = createBlockEditor({
      parent: el,
      content: this.originalBlockText,
      cursorLine,
      cursorCol,
      theme: this.theme,
      extraKeymaps: keymaps,
      onContentChanged: (newContent) => {
        if (!this.activeBlock) return;
        this.onEdit(
          this.activeBlock.startLine,
          this.activeBlock.endLine,
          newContent,
        );
      },
    });

    this.onCursor(block.startLine + cursorLine);
  }

  /** Deactivate the current editing block, committing changes and returning to preview. */
  deactivateBlock(): void {
    if (
      !this.activeEditor ||
      !this.activeBlock ||
      !this.activeBlockElement ||
      !this.model
    ) {
      this.activeEditor = null;
      this.activeBlock = null;
      this.activeBlockElement = null;
      this.originalBlockText = null;
      return;
    }

    const newText = this.activeEditor.state.doc.toString();

    // Check if text changed and send edit if so
    if (newText !== this.originalBlockText) {
      this.onEdit(
        this.activeBlock.startLine,
        this.activeBlock.endLine,
        newText,
      );
    }

    // Destroy editor
    this.activeEditor.destroy();
    this.activeEditor = null;

    // Re-render block as preview
    this.activeBlockElement.classList.remove('block-editing');

    // Re-read the block from the model (it may have been updated)
    const block = this.model.getBlockForLine(this.activeBlock.startLine);
    if (block) {
      this.activeBlockElement.innerHTML = this.model.renderBlock(block);
      this.activeBlockElement.setAttribute(
        'data-line-start',
        String(block.startLine),
      );
      this.activeBlockElement.setAttribute(
        'data-line-end',
        String(block.endLine),
      );
    }

    this.activeBlock = null;
    this.activeBlockElement = null;
    this.originalBlockText = null;
  }

  private renderAllBlocks(): void {
    if (!this.container || !this.model) return;

    const blocks = this.model.getBlocks();
    const fragment = document.createDocumentFragment();

    for (const block of blocks) {
      const div = document.createElement('div');
      div.className = `block block-${block.type}`;
      div.setAttribute('data-line-start', String(block.startLine));
      div.setAttribute('data-line-end', String(block.endLine));
      div.setAttribute('role', 'button');
      div.setAttribute('aria-label', `Click to edit ${block.type} block`);
      div.innerHTML = this.model.renderBlock(block);
      fragment.appendChild(div);
    }

    this.container.appendChild(fragment);
  }

  private attachClickHandlers(): void {
    if (!this.container) return;

    this.container.addEventListener('click', (e) => {
      const target = e.target as HTMLElement;

      // Find the closest block element
      const blockEl = target.closest<HTMLElement>('[data-line-start]');
      if (!blockEl || !this.model) return;

      // Don't re-activate the already-active block
      if (blockEl === this.activeBlockElement) return;

      const startLine = parseInt(
        blockEl.getAttribute('data-line-start') ?? '0',
        10,
      );
      const block = this.model.getBlockForLine(startLine);
      if (!block) return;

      // Estimate cursor position from click
      this.activateBlock(block, 0, 0);
    });
  }

  private navigateToPreviousBlock(): boolean {
    if (!this.activeBlock || !this.model) return false;
    const blocks = this.model.getBlocks();
    const idx = blocks.findIndex(
      (b) => b.startLine === this.activeBlock!.startLine,
    );
    if (idx <= 0) return false;

    const prevBlock = blocks[idx - 1];
    const lastLine = prevBlock.endLine - prevBlock.startLine - 1;
    this.activateBlock(prevBlock, Math.max(0, lastLine), 0);
    return true;
  }

  private navigateToNextBlock(): boolean {
    if (!this.activeBlock || !this.model) return false;
    const blocks = this.model.getBlocks();
    const idx = blocks.findIndex(
      (b) => b.startLine === this.activeBlock!.startLine,
    );
    if (idx < 0 || idx >= blocks.length - 1) return false;

    const nextBlock = blocks[idx + 1];
    this.activateBlock(nextBlock, 0, 0);
    return true;
  }
}
