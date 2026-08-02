/**
 * Feedback-loop protection for two-way scroll sync.
 *
 * Each side stamps the moment it applies a programmatic scroll originating
 * from the other side, and ignores reciprocal sync events for a short window
 * afterwards. The window can be tight (~150 ms) because patch application
 * doesn't move the scroll position.
 */
export class SyncGuard {
  private until = 0;

  constructor(private readonly windowMs = 150) {}

  /** Mark that a programmatic scroll was just applied. */
  suppress(): void {
    this.until = Date.now() + this.windowMs;
  }

  /** Whether a scroll event happening now should be ignored. */
  get suppressed(): boolean {
    return Date.now() < this.until;
  }
}

/**
 * A scroll position aimed at the side of the pair that is currently off screen.
 *
 * In Preview mode the editor and the panel are two tabs of one column, so only
 * one of them is ever on screen: a hidden webview has no layout to scroll
 * against, and a background tab has no `TextEditor` to reveal in. Sending to
 * the absent side either does nothing or lands somewhere arbitrary, so its
 * position is parked here and claimed the moment it comes forward — which is
 * what keeps flipping between the two tabs from losing the reader's place.
 */
export class ParkedScroll {
  private line: number | undefined;

  /** Remember a line for the absent side to adopt when it returns. */
  park(line: number): void {
    this.line = line;
  }

  /** Take the parked line, if any. Claiming spends it. */
  claim(): number | undefined {
    const line = this.line;
    this.line = undefined;
    return line;
  }

  /** Drop it: this side already sits where it belongs. */
  clear(): void {
    this.line = undefined;
  }

  get pending(): boolean {
    return this.line !== undefined;
  }
}
