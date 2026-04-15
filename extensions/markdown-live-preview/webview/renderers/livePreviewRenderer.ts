import type { EditorView } from '@codemirror/view';
import type { DocumentModel, BlockRange, TextChange } from '../documentModel';
import { createBlockEditor } from '../blockEditor';
import { createBlockNavigationKeymap, type CursorPlacement } from '../keymap';
import type { EditCallback, CursorCallback, Renderer } from './types';

export class LivePreviewRenderer implements Renderer {
  private container: HTMLElement | null = null;
  private model: DocumentModel | null = null;
  private activeBlock: BlockRange | null = null;
  private activeEditor: EditorView | null = null;
  private activeBlockElement: HTMLElement | null = null;
  private originalBlockText: string | null = null;

  /** Index of the focused (but not editing) block, used for keyboard re-entry after Escape. */
  private focusedBlockIndex: number = -1;
  private containerKeydownHandler: ((e: KeyboardEvent) => void) | null = null;

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
    this.setupContainerKeyboard();
  }

  teardown(): void {
    this.deactivateBlock();
    this.removeContainerKeyboard();
    if (this.container) {
      this.container.innerHTML = '';
      this.container.className = '';
    }
    this.container = null;
    this.model = null;
    this.focusedBlockIndex = -1;
  }

  onDocumentChanged(_changes: TextChange[]): void {
    if (!this.container || !this.model) return;

    // For simplicity, re-render all non-active blocks
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
      this.activateBlock(block, { line: localLine, col: 0 });
    }
  }

  setScrollOffset(offset: number): void {
    if (this.container) {
      this.container.scrollTop = offset;
    }
  }

  /** Activate edit mode for a specific block. */
  activateBlock(block: BlockRange, placement?: CursorPlacement): void {
    if (!this.container || !this.model) return;

    // Deactivate current block first (without setting focus — we're about to activate another)
    this.deactivateBlockSilent();
    this.clearFocusedBlock();

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

    // Resolve cursor placement
    const { cursorLine, cursorCol } = this.resolvePlacement(
      placement,
      this.originalBlockText,
    );

    // Create CodeMirror editor for this block
    const keymaps = createBlockNavigationKeymap({
      goToPreviousBlock: (p) => this.navigateToPreviousBlock(p),
      goToNextBlock: (p) => this.navigateToNextBlock(p),
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

  /** Deactivate the current editing block, returning to preview and setting keyboard focus. */
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

    // Track which block was active for keyboard re-entry
    const blocks = this.model.getBlocks();
    const deactivatedIndex = blocks.findIndex(
      (b) => b.startLine === this.activeBlock!.startLine,
    );

    this.commitAndTeardownEditor();

    // Focus the deactivated block for keyboard navigation
    this.setFocusedBlock(deactivatedIndex >= 0 ? deactivatedIndex : 0);
  }

  // ── Private helpers ──────────────────────────────────────────────────

  /** Deactivate without setting focus (used when immediately activating another block). */
  private deactivateBlockSilent(): void {
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

    this.commitAndTeardownEditor();
  }

  /** Commit pending edits, destroy the editor, and re-render the block as preview. */
  private commitAndTeardownEditor(): void {
    if (!this.activeEditor || !this.activeBlock || !this.activeBlockElement || !this.model) return;

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

  /** Resolve a CursorPlacement to concrete cursorLine/cursorCol values. */
  private resolvePlacement(
    placement: CursorPlacement | undefined,
    blockText: string,
  ): { cursorLine: number; cursorCol: number } {
    if (!placement) {
      return { cursorLine: 0, cursorCol: 0 };
    }

    const lines = blockText.split('\n');

    if (placement.position === 'end') {
      const lastLine = lines.length - 1;
      return { cursorLine: lastLine, cursorCol: lines[lastLine].length };
    }

    if (placement.position === 'start') {
      return { cursorLine: 0, cursorCol: 0 };
    }

    let line = placement.line ?? 0;
    if (line < 0) {
      line = lines.length + line; // -1 → last line
    }
    line = Math.max(0, Math.min(line, lines.length - 1));

    const col = Math.min(placement.col ?? 0, lines[line].length);
    return { cursorLine: line, cursorCol: col };
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
      div.setAttribute('tabindex', '0');
      div.setAttribute('role', 'button');
      div.setAttribute('aria-label', `Edit ${block.type} block`);
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

      this.activateBlock(block);
    });
  }

  // ── Container-level keyboard handling (when no block is being edited) ──

  private setupContainerKeyboard(): void {
    if (!this.container) return;
    // Make container focusable so it can receive keyboard events after Escape
    this.container.setAttribute('tabindex', '-1');

    this.containerKeydownHandler = (e: KeyboardEvent) => {
      // Only handle when no block is actively being edited
      if (this.activeEditor) return;
      this.handlePreviewKeydown(e);
    };

    this.container.addEventListener('keydown', this.containerKeydownHandler);
  }

  private removeContainerKeyboard(): void {
    if (this.container && this.containerKeydownHandler) {
      this.container.removeEventListener('keydown', this.containerKeydownHandler);
    }
    this.containerKeydownHandler = null;
  }

  private handlePreviewKeydown(e: KeyboardEvent): void {
    if (!this.model) return;
    const blocks = this.model.getBlocks();
    if (blocks.length === 0) return;

    const currentIdx = this.focusedBlockIndex >= 0
      ? this.focusedBlockIndex
      : 0;

    switch (e.key) {
      case 'ArrowDown': {
        e.preventDefault();
        const nextIdx = Math.min(currentIdx + 1, blocks.length - 1);
        if (nextIdx === this.focusedBlockIndex) {
          // Already at last block — activate it at the start
          this.activateBlock(blocks[nextIdx], { line: 0, col: 0 });
        } else {
          this.setFocusedBlock(nextIdx);
        }
        break;
      }
      case 'ArrowUp': {
        e.preventDefault();
        const prevIdx = Math.max(currentIdx - 1, 0);
        if (prevIdx === this.focusedBlockIndex) {
          // Already at first block — activate it at the end
          this.activateBlock(blocks[prevIdx], { position: 'end' });
        } else {
          this.setFocusedBlock(prevIdx);
        }
        break;
      }
      case 'Enter': {
        e.preventDefault();
        const idx = Math.min(currentIdx, blocks.length - 1);
        this.activateBlock(blocks[idx]);
        break;
      }
      case 'Escape': {
        // Clear focus entirely
        e.preventDefault();
        this.clearFocusedBlock();
        this.focusedBlockIndex = -1;
        break;
      }
      default:
        // Printable character → activate the focused block and insert the character
        if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
          e.preventDefault();
          const idx = Math.min(currentIdx, blocks.length - 1);
          this.activateBlock(blocks[idx], { position: 'end' });
          // Insert the typed character into the newly created editor
          if (this.activeEditor) {
            const pos = this.activeEditor.state.selection.main.head;
            this.activeEditor.dispatch({
              changes: { from: pos, insert: e.key },
            });
          }
        }
        break;
    }
  }

  // ── Focused block management (visual indicator for keyboard navigation) ──

  private setFocusedBlock(index: number): void {
    this.clearFocusedBlock();
    if (!this.container || !this.model) return;

    const blocks = this.model.getBlocks();
    if (index < 0 || index >= blocks.length) return;

    this.focusedBlockIndex = index;
    const block = blocks[index];
    const el = this.container.querySelector<HTMLElement>(
      `[data-line-start="${block.startLine}"]`,
    );
    if (el) {
      el.classList.add('block-focused');
      el.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
    }
    // Keep focus on the container so we keep receiving keyboard events
    this.container.focus();
  }

  private clearFocusedBlock(): void {
    if (!this.container) return;
    const prev = this.container.querySelector('.block-focused');
    if (prev) {
      prev.classList.remove('block-focused');
    }
  }

  // ── Block-to-block navigation (called from within an active editor) ──

  private navigateToPreviousBlock(placement: CursorPlacement): boolean {
    if (!this.activeBlock || !this.model) return false;
    const blocks = this.model.getBlocks();
    const idx = blocks.findIndex(
      (b) => b.startLine === this.activeBlock!.startLine,
    );
    if (idx <= 0) return false;

    const prevBlock = blocks[idx - 1];
    // For ArrowUp: default to last line with the given column
    // For ArrowLeft (position: 'end'): use position directly
    const resolved: CursorPlacement = placement.position
      ? placement
      : { line: -1, col: placement.col ?? 0 };
    this.activateBlock(prevBlock, resolved);
    return true;
  }

  private navigateToNextBlock(placement: CursorPlacement): boolean {
    if (!this.activeBlock || !this.model) return false;
    const blocks = this.model.getBlocks();
    const idx = blocks.findIndex(
      (b) => b.startLine === this.activeBlock!.startLine,
    );
    if (idx < 0 || idx >= blocks.length - 1) return false;

    const nextBlock = blocks[idx + 1];
    this.activateBlock(nextBlock, placement);
    return true;
  }
}
