import * as vscode from 'vscode';
import type { PreviewFont, PreviewTheme } from './messages';
import {
  chooseOverride,
  reconcileOverride,
  sameOverride,
  type OverrideState,
} from './override';

/**
 * One in-page switch, held for the *window* rather than for one page.
 *
 * Every preview is its own webview, and a webview's own state dies with it — so
 * a switch kept there would be forgotten the moment the reader opened the next
 * file. Holding it here instead is what makes it stick across files, across
 * both preview surfaces, and across a reload, while leaving the setting it
 * deviates from untouched.
 */
export class OverrideStore<T extends string> implements vscode.Disposable {
  private state: OverrideState<T>;
  private readonly emitter = new vscode.EventEmitter<void>();
  /** Fires when the switch is flipped or retired; pages are told to restyle. */
  public readonly onDidChange = this.emitter.event;
  private readonly disposables: vscode.Disposable[] = [];

  constructor(
    private readonly memento: vscode.Memento,
    /** Memento key holding the switch across window reloads. */
    private readonly key: string,
    /** The setting it deviates from; changing that by hand retires it. */
    setting: string,
    private readonly readConfigured: () => T,
  ) {
    this.state = reconcileOverride(
      memento.get<OverrideState<T>>(key) ?? {},
      readConfigured(),
    );
    this.disposables.push(
      this.emitter,
      vscode.workspace.onDidChangeConfiguration((e) => {
        // Setting the value by hand is the explicit choice that retires it.
        if (e.affectsConfiguration(setting)) {
          this.write(reconcileOverride(this.state, this.readConfigured()));
        }
      }),
    );
  }

  /** The switch's value, or `null` while the configured one is in force. */
  public get override(): T | null {
    return this.state.override ?? null;
  }

  /** The value a page should actually show. */
  public get value(): T {
    return this.state.override ?? this.readConfigured();
  }

  /** Flip the switch to `chosen`. The configuration is never written. */
  public set(chosen: T): void {
    this.write(chooseOverride(chosen, this.readConfigured()));
  }

  public dispose(): void {
    for (const d of this.disposables) d.dispose();
  }

  private write(next: OverrideState<T>): void {
    if (sameOverride(next, this.state)) return;
    this.state = next;
    void this.memento.update(this.key, next);
    this.emitter.fire();
  }
}

/** `markdownPreviewUltra.theme` — the default the light/dark switch deviates from. */
export function configuredTheme(): PreviewTheme {
  return vscode.workspace
    .getConfiguration('markdownPreviewUltra')
    .get<PreviewTheme>('theme', 'github-light');
}

/** `markdownPreviewUltra.font` — the default the font switch deviates from. */
export function configuredFont(): PreviewFont {
  return vscode.workspace
    .getConfiguration('markdownPreviewUltra')
    .get<PreviewFont>('font', 'proportional');
}

/** The window's light/dark switch. */
export function createThemeStore(
  memento: vscode.Memento,
): OverrideStore<PreviewTheme> {
  return new OverrideStore(
    memento,
    'markdownPreviewUltra.themeOverride',
    'markdownPreviewUltra.theme',
    configuredTheme,
  );
}

/** The window's prose/monospace switch. */
export function createFontStore(
  memento: vscode.Memento,
): OverrideStore<PreviewFont> {
  return new OverrideStore(
    memento,
    'markdownPreviewUltra.fontOverride',
    'markdownPreviewUltra.font',
    configuredFont,
  );
}
