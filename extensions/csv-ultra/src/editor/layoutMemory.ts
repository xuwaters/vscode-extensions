import type * as vscode from 'vscode';
import type { GridLayout } from '../messages.js';

/** Where the layouts live in workspace state. */
export const LAYOUTS_KEY = 'csvUltra.layouts';

/** How many files are remembered before the least recently opened is dropped. */
export const LAYOUTS_LIMIT = 200;

/** One remembered layout, with the reading that produced it. */
export interface StoredLayout {
  layout: GridLayout;
  /** `Date.now()` when it was last written — the eviction order. */
  at: number;
}

export type StoredLayouts = Record<string, StoredLayout>;

/**
 * Record a layout, evicting the least recently opened files past the limit.
 *
 * Pure so the eviction rule can be exercised without a workspace: the store is
 * unbounded otherwise, and a reader who opens a thousand CSVs should not carry a
 * thousand column-width tables around for the life of the workspace.
 */
export function withLayout(
  layouts: StoredLayouts,
  key: string,
  layout: GridLayout,
  at: number,
): StoredLayouts {
  const next: StoredLayouts = { ...layouts, [key]: { layout, at } };
  const total = Object.keys(next).length;
  if (total <= LAYOUTS_LIMIT) return next;

  // Oldest first. The entry just written is never a candidate — a clock that
  // went backwards must not evict the file being read right now.
  const candidates = Object.keys(next)
    .filter((candidate) => candidate !== key)
    .sort((a, b) => (next[a]?.at ?? 0) - (next[b]?.at ?? 0));
  for (const stale of candidates.slice(0, total - LAYOUTS_LIMIT)) delete next[stale];
  return next;
}

/**
 * How each file's table was last laid out.
 *
 * Backed by workspace state rather than global state on purpose: a column widened
 * to read a particular file is a fact about the project it belongs to, and the
 * same file opened in two workspaces is usually open for two different reasons.
 */
export class LayoutMemory {
  constructor(private readonly state: vscode.Memento) {}

  get(uri: vscode.Uri): GridLayout | undefined {
    return this.all()[uri.toString()]?.layout;
  }

  async park(uri: vscode.Uri, layout: GridLayout): Promise<void> {
    await this.state.update(
      LAYOUTS_KEY,
      withLayout(this.all(), uri.toString(), layout, Date.now()),
    );
  }

  private all(): StoredLayouts {
    return this.state.get<StoredLayouts>(LAYOUTS_KEY) ?? {};
  }
}
