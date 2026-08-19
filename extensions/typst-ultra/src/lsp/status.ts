import * as vscode from 'vscode';
import type { Client } from './client.js';
import * as config from '../config.js';

/** What the server reports after each compile. */
interface CompileStatus {
  state: 'compiling' | 'ok' | 'error';
  pageCount?: number;
}

/** What the server reports about a package. */
interface PackageStatus {
  spec: string;
  state: 'downloading' | 'ready' | 'failed';
  error?: string;
}

/**
 * The compile-state status bar item, plus the heap watchdog.
 *
 * WASM linear memory is never returned to the OS, so restarting the child
 * process is the only real reclamation mechanism — and it is cheap, because the
 * server holds no unsaved state.
 */
export class StatusBar implements vscode.Disposable {
  private readonly compile: vscode.StatusBarItem;
  private readonly memory: vscode.StatusBarItem;
  private readonly disposables: vscode.Disposable[] = [];
  private watchdog: ReturnType<typeof setInterval> | undefined;

  constructor(
    private readonly client: Client,
    private readonly output: vscode.OutputChannel,
  ) {
    this.compile = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      99,
    );
    this.memory = vscode.window.createStatusBarItem(
      vscode.StatusBarAlignment.Right,
      98,
    );
    this.memory.command = 'typstUltra.restartServer';
    this.disposables.push(this.compile, this.memory);

    this.disposables.push(
      this.client.onNotification('typst/compileStatus', (params) =>
        this.onCompile(params as CompileStatus),
      ),
      this.client.onNotification('typst/packageStatus', (params) =>
        this.onPackage(params as PackageStatus),
      ),
      this.client.onNotification('typst/fontsChanged', (params) => {
        const { faces, ms } = params as { faces: number; ms: number };
        this.output.appendLine(`fonts: ${faces} faces available (indexed in ${ms} ms)`);
      }),
    );

    this.startWatchdog();
  }

  dispose(): void {
    if (this.watchdog) clearInterval(this.watchdog);
    for (const disposable of this.disposables) disposable.dispose();
  }

  private onCompile(status: CompileStatus): void {
    switch (status.state) {
      case 'compiling':
        this.compile.text = '$(sync~spin) Typst';
        this.compile.tooltip = 'Compiling…';
        break;
      case 'ok':
        this.compile.text = `$(check) Typst · ${status.pageCount ?? 0}p`;
        this.compile.tooltip = 'Compiled successfully';
        break;
      case 'error':
        this.compile.text = '$(error) Typst';
        this.compile.tooltip = 'The document has errors — see the Problems panel';
        break;
    }
    this.compile.show();
  }

  private onPackage(status: PackageStatus): void {
    switch (status.state) {
      case 'downloading':
        this.output.appendLine(`packages: downloading ${status.spec}`);
        break;
      case 'ready':
        this.output.appendLine(`packages: ${status.spec} ready`);
        break;
      case 'failed':
        this.output.appendLine(`packages: ${status.spec} failed — ${status.error ?? ''}`);
        void vscode.window.showWarningMessage(
          `Typst: could not fetch ${status.spec}. ${status.error ?? ''}`,
        );
        break;
    }
  }

  /** Poll the server's heap and offer a restart above the threshold. */
  private startWatchdog(): void {
    this.watchdog = setInterval(() => void this.checkHeap(), 30_000);
  }

  private async checkHeap(): Promise<void> {
    if (!this.client.running) {
      this.memory.hide();
      return;
    }

    const threshold = config.read().server.memory.restartThresholdMb;
    if (threshold <= 0) {
      this.memory.hide();
      return;
    }

    const bytes = await this.client.request<number>('typst/heapBytes', {});
    if (typeof bytes !== 'number') return;

    const mb = Math.round(bytes / 1_048_576);
    if (mb < threshold) {
      this.memory.hide();
      return;
    }

    this.memory.text = `$(warning) Typst ${mb} MB`;
    this.memory.tooltip =
      `The Typst engine is holding ${mb} MB. WASM memory is never returned to ` +
      `the OS, so a restart is the only way to reclaim it. Click to restart.`;
    this.memory.backgroundColor = new vscode.ThemeColor(
      'statusBarItem.warningBackground',
    );
    this.memory.show();
  }
}
