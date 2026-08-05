import * as vscode from 'vscode';
import type { PreviewTheme } from './messages';
import {
  chooseOverride,
  reconcileOverride,
  sameOverride,
  type ThemeOverrideState,
} from './themeOverride';

/** Memento key holding the switch across window reloads. */
const KEY = 'markdownPreviewUltra.themeOverride';

/**
 * The in-page light/dark switch, held for the *window* rather than for one
 * page.
 *
 * Every preview is its own webview, and a webview's own state dies with it — so
 * a switch kept there would be forgotten the moment the reader opened the next
 * file. Holding it here instead is what makes it stick across files, across
 * both preview surfaces, and across a reload, while leaving
 * `markdownPreviewUltra.theme` — the default it deviates from — untouched.
 */
export class ThemeOverrideStore implements vscode.Disposable {
  private state: ThemeOverrideState;
  private readonly emitter = new vscode.EventEmitter<void>();
  /** Fires when the switch is flipped or retired; pages are told to restyle. */
  public readonly onDidChange = this.emitter.event;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(private readonly memento: vscode.Memento) {
    this.state = reconcileOverride(
      memento.get<ThemeOverrideState>(KEY) ?? {},
      configuredTheme(),
    );
    this.disposables.push(
      this.emitter,
      vscode.workspace.onDidChangeConfiguration((e) => {
        // Setting the theme by hand is the explicit choice that retires it.
        if (e.affectsConfiguration('markdownPreviewUltra.theme')) {
          this.write(reconcileOverride(this.state, configuredTheme()));
        }
      }),
    );
  }

  /** The switch's value, or `null` while the configured theme is in force. */
  public get override(): PreviewTheme | null {
    return this.state.override ?? null;
  }

  /** The theme a page should actually show. */
  public get theme(): PreviewTheme {
    return this.state.override ?? configuredTheme();
  }

  /** Flip the switch to `chosen`. The configuration is never written. */
  public set(chosen: PreviewTheme): void {
    this.write(chooseOverride(chosen, configuredTheme()));
  }

  public dispose(): void {
    for (const d of this.disposables) d.dispose();
  }

  private write(next: ThemeOverrideState): void {
    if (sameOverride(next, this.state)) return;
    this.state = next;
    void this.memento.update(KEY, next);
    this.emitter.fire();
  }
}

/** `markdownPreviewUltra.theme` — the default the switch deviates from. */
export function configuredTheme(): PreviewTheme {
  return vscode.workspace
    .getConfiguration('markdownPreviewUltra')
    .get<PreviewTheme>('theme', 'github-light');
}
