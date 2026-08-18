/**
 * Loop protection for two-way sync.
 *
 * Both sides scroll each other, so without a guard an editor scroll moves the
 * preview, which reports its new position, which moves the editor, and the two
 * chase each other across the document. Each side stamps the origin of its last
 * programmatic move and ignores reciprocal sync for a short window — the same
 * mechanism markdown-preview-ultra uses, for the same reason.
 */
export class SyncGuard {
  /** How long a stamp suppresses the other direction. */
  static readonly WINDOW_MS = 100;

  // `-Infinity`, not 0: with a clock that starts near zero, a zero stamp reads
  // as "just moved" and suppresses the very first sync in each direction.
  private editorAt = Number.NEGATIVE_INFINITY;
  private previewAt = Number.NEGATIVE_INFINITY;
  private timer: ReturnType<typeof setTimeout> | undefined;

  constructor(private readonly now: () => number = () => Date.now()) {}

  /** Record that the editor just caused a move. */
  markEditorOrigin(): void {
    this.editorAt = this.now();
  }

  /** Record that the preview just caused a move. */
  markPreviewOrigin(): void {
    this.previewAt = this.now();
  }

  /** Whether the editor moved recently enough to suppress a reply. */
  isEditorOrigin(): boolean {
    return this.now() - this.editorAt < SyncGuard.WINDOW_MS;
  }

  /** Whether the preview moved recently enough to suppress a reply. */
  isPreviewOrigin(): boolean {
    return this.now() - this.previewAt < SyncGuard.WINDOW_MS;
  }

  /** Run `action` after `delay` ms of quiet, cancelling any pending run. */
  debounce(delay: number, action: () => void): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = setTimeout(() => {
      this.timer = undefined;
      action();
    }, delay);
  }

  /** Drop a pending debounced action. */
  cancel(): void {
    if (this.timer) clearTimeout(this.timer);
    this.timer = undefined;
  }
}
