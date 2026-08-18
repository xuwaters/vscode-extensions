/**
 * Where each document was last read to, by URI.
 *
 * A mode switch *replaces* a tab rather than moving it — Preview swaps the
 * source editor for the preview editor, Edit swaps it back — so the page the
 * reader was on has to outlive the webview that reported it. The panel and the
 * preview editor share one of these, which is what makes the switch between
 * them continuous rather than a jump back to page one.
 *
 * Free of the `vscode` module on purpose: keys are URI strings, so the rule can
 * be tested without stubbing the editor.
 */
export class PageMemory {
  private readonly pages = new Map<string, number>();

  /** Record the page a document is being read at. */
  park(uri: string, page: number): void {
    if (!Number.isInteger(page) || page < 0) return;
    this.pages.set(uri, page);
  }

  /** The page a document was last read at, leaving the record in place. */
  peek(uri: string): number | undefined {
    return this.pages.get(uri);
  }

  /** The page a document was last read at, clearing the record. */
  take(uri: string): number | undefined {
    const page = this.pages.get(uri);
    this.pages.delete(uri);
    return page;
  }

  /** Forget a document — it was closed, or its preview was reset. */
  forget(uri: string): void {
    this.pages.delete(uri);
  }
}
