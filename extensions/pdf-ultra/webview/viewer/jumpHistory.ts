import { observable } from '@microsoft/fast-element';

/** Somewhere the reader was: a page, and how far into it the viewport started. */
export interface JumpPlace {
  /** 1-based. */
  page: number;
  /** A fraction of the page's height — the same units as `ViewerPlace`. */
  offsetRatio: number;
}

/** How many jumps back the button can undo before the oldest is forgotten. */
const DEPTH = 50;

/** Below this two places are the same spot, and the second is not worth a step. */
const SAME_RATIO = 0.005;

/**
 * Where the reader was before each jump.
 *
 * A stack, not a browser history: this only remembers what a *jump* left
 * behind — a link, an outline entry, a page typed into the box — because those
 * are the moves that take the reader somewhere they did not scroll to and
 * cannot scroll back from. Turning the page and scrolling leave nothing here;
 * a back button that undid a scroll would be a button that does nothing you
 * could not do yourself.
 *
 * `depth` is the observable rather than a plain `length`, so the toolbar's
 * disabled binding re-evaluates when a jump is recorded — a private array
 * pushed to in place notifies nobody.
 */
export class JumpHistory {
  /** How many jumps can still be undone. */
  @observable depth = 0;

  private stack: JumpPlace[] = [];

  get canGoBack(): boolean {
    return this.depth > 0;
  }

  /**
   * Remember a place, unless it is the one already on top: clicking a link
   * that lands where the reader already stands, twice, is one place, not two.
   */
  push(place: JumpPlace): void {
    const top = this.stack[this.stack.length - 1];
    if (top && samePlace(top, place)) return;
    this.stack.push(place);
    if (this.stack.length > DEPTH) this.stack.shift();
    this.depth = this.stack.length;
  }

  /** The place to go back to, taken off the stack. */
  pop(): JumpPlace | undefined {
    const place = this.stack.pop();
    this.depth = this.stack.length;
    return place;
  }

  /** A different document's places are not this one's. */
  clear(): void {
    this.stack = [];
    this.depth = 0;
  }
}

function samePlace(a: JumpPlace, b: JumpPlace): boolean {
  return a.page === b.page && Math.abs(a.offsetRatio - b.offsetRatio) < SAME_RATIO;
}
