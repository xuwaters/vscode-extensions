import * as vscode from 'vscode';

// v1 stored a single ComparisonSelection; read once to migrate it.
const LEGACY_STORAGE_KEY = 'gitCompare.selection.v1';
const STORAGE_KEY = 'gitCompare.selections.v2';

export interface ComparisonSelection {
  repoRoot: string;
  ref: string;
  label: string;
}

interface StoredRef {
  ref: string;
  label: string;
}

interface StoredState {
  /** The repository the view currently shows. */
  activeRepo?: string;
  /** The comparison remembered for each repository, keyed by repository root. */
  byRepo: Record<string, StoredRef>;
}

/**
 * Remembers one comparison per repository and which repository the view is
 * showing, so switching repositories brings back that repository's comparison.
 */
export class CompareState {
  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChange = this._onDidChange.event;

  private stored: StoredState;

  constructor(private readonly storage: vscode.Memento) {
    this.stored = storage.get<StoredState>(STORAGE_KEY) ?? migrate(storage);
  }

  /** The active repository's comparison, if it has one. */
  get(): ComparisonSelection | undefined {
    const repoRoot = this.stored.activeRepo;
    if (!repoRoot) return undefined;
    const entry = this.stored.byRepo[repoRoot];
    return entry ? { repoRoot, ref: entry.ref, label: entry.label } : undefined;
  }

  get activeRepo(): string | undefined {
    return this.stored.activeRepo;
  }

  /**
   * Remember `selection` for its repository and make that repository active;
   * `undefined` forgets the active repository's comparison.
   */
  async set(selection: ComparisonSelection | undefined): Promise<void> {
    const byRepo = { ...this.stored.byRepo };
    let activeRepo = this.stored.activeRepo;
    if (selection) {
      byRepo[selection.repoRoot] = { ref: selection.ref, label: selection.label };
      activeRepo = selection.repoRoot;
    } else if (activeRepo) {
      delete byRepo[activeRepo];
    }
    await this.save({ activeRepo, byRepo });
  }

  /** Show `repoRoot`'s comparison (or none, if it has none). */
  async setActiveRepo(repoRoot: string): Promise<void> {
    if (this.stored.activeRepo === repoRoot) return;
    await this.save({ ...this.stored, activeRepo: repoRoot });
  }

  private async save(next: StoredState): Promise<void> {
    this.stored = next;
    this._onDidChange.fire();
    await this.storage.update(STORAGE_KEY, next);
  }

  dispose(): void {
    this._onDidChange.dispose();
  }
}

function migrate(storage: vscode.Memento): StoredState {
  const legacy = storage.get<ComparisonSelection>(LEGACY_STORAGE_KEY);
  if (!legacy) return { byRepo: {} };
  return {
    activeRepo: legacy.repoRoot,
    byRepo: { [legacy.repoRoot]: { ref: legacy.ref, label: legacy.label } },
  };
}
