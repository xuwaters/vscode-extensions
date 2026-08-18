import type * as vscode from 'vscode';
import type { ViewerPlace } from './messages.js';

/** Where the places live in workspace state. */
export const PLACES_KEY = 'pdfUltra.places';

/** How many documents are remembered before the least recently read is dropped. */
export const PLACES_LIMIT = 200;

/** One remembered place, with the reading that produced it. */
export interface StoredPlace {
  place: ViewerPlace;
  /** `Date.now()` when it was last written — the eviction order. */
  at: number;
}

export type StoredPlaces = Record<string, StoredPlace>;

/**
 * Record a place, evicting the least recently read documents past the limit.
 *
 * Pure so the eviction rule can be exercised without a workspace: the store is
 * unbounded otherwise, and a reader who opens a thousand PDFs should not carry
 * a thousand of these around for the life of the workspace.
 */
export function withPlace(
  places: StoredPlaces,
  key: string,
  place: ViewerPlace,
  at: number,
): StoredPlaces {
  const next: StoredPlaces = { ...places, [key]: { place, at } };
  const total = Object.keys(next).length;
  if (total <= PLACES_LIMIT) return next;

  // Oldest first. The entry just written is never a candidate — a clock that
  // went backwards must not evict the document being read right now.
  const candidates = Object.keys(next)
    .filter((candidate) => candidate !== key)
    .sort((a, b) => (next[a]?.at ?? 0) - (next[b]?.at ?? 0));
  for (const stale of candidates.slice(0, total - PLACES_LIMIT)) delete next[stale];
  return next;
}

/**
 * Where each document was last read to.
 *
 * Backed by workspace state rather than global state on purpose: "page 40 of
 * the spec" is a fact about the project being worked on, and a document read in
 * two workspaces is usually being read for two different reasons.
 */
export class PlaceMemory {
  constructor(private readonly state: vscode.Memento) {}

  get(uri: vscode.Uri): ViewerPlace | undefined {
    return this.all()[uri.toString()]?.place;
  }

  async park(uri: vscode.Uri, place: ViewerPlace): Promise<void> {
    await this.state.update(
      PLACES_KEY,
      withPlace(this.all(), uri.toString(), place, Date.now()),
    );
  }

  private all(): StoredPlaces {
    return this.state.get<StoredPlaces>(PLACES_KEY) ?? {};
  }
}
