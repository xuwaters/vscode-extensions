import * as vscode from 'vscode';

const STORAGE_KEY = 'gitCompare.selection.v1';

export interface ComparisonSelection {
  repoRoot: string;
  ref: string;
  label: string;
}

export class CompareState {
  private readonly _onDidChange = new vscode.EventEmitter<void>();
  readonly onDidChange = this._onDidChange.event;

  private current: ComparisonSelection | undefined;

  constructor(private readonly storage: vscode.Memento) {
    this.current = storage.get<ComparisonSelection>(STORAGE_KEY);
  }

  get(): ComparisonSelection | undefined {
    return this.current;
  }

  async set(selection: ComparisonSelection | undefined): Promise<void> {
    this.current = selection;
    await this.storage.update(STORAGE_KEY, selection);
    this._onDidChange.fire();
  }

  dispose(): void {
    this._onDidChange.dispose();
  }
}
