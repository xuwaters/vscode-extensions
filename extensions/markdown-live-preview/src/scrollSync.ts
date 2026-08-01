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
