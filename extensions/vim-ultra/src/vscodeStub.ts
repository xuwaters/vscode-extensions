/**
 * The slice of the `vscode` API the controller touches, as a stub the tests
 * drive (vitest aliases `vscode` to this file; see vitest.config.mts). Events
 * are fired by hand so a test can replay VSCode's ordering — an editor handed
 * over before its saved position is restored, a selection change arriving
 * while another editor is still the active one.
 */

export class Position {
  constructor(
    readonly line: number,
    readonly character: number,
  ) {}
}

export class Range {
  readonly start: Position;
  readonly end: Position;
  constructor(a: Position | number, b: Position | number, c?: number, d?: number) {
    if (a instanceof Position && b instanceof Position) {
      this.start = a;
      this.end = b;
    } else {
      this.start = new Position(a as number, b as number);
      this.end = new Position(c as number, d as number);
    }
  }
}

export class Selection extends Range {
  readonly anchor: Position;
  readonly active: Position;
  constructor(
    anchorLine: number,
    anchorChar: number,
    activeLine: number,
    activeChar: number,
  ) {
    super(anchorLine, anchorChar, activeLine, activeChar);
    this.anchor = new Position(anchorLine, anchorChar);
    this.active = new Position(activeLine, activeChar);
  }
}

export class ThemeColor {
  constructor(readonly id: string) {}
}

export const StatusBarAlignment = { Left: 1, Right: 2 } as const;
export const EndOfLine = { LF: 1, CRLF: 2 } as const;
export const TextEditorRevealType = {
  Default: 0,
  InCenter: 1,
  InCenterIfOutsideViewport: 2,
  AtTop: 3,
} as const;
export const TextEditorCursorStyle = { Line: 1, Block: 2 } as const;
export const TextEditorSelectionChangeKind = { Keyboard: 1, Mouse: 2, Command: 3 } as const;

export interface Disposable {
  dispose(): void;
}

class Emitter<T> {
  private readonly handlers = new Set<(e: T) => void>();
  readonly event = (h: (e: T) => void): Disposable => {
    this.handlers.add(h);
    return { dispose: () => void this.handlers.delete(h) };
  };
  fire(e: T): void {
    for (const h of [...this.handlers]) h(e);
  }
}

export interface StubDocument {
  uri: { scheme: string; toString(): string };
  eol: number;
  getText(): string;
  lineCount: number;
  lineAt(line: number): { text: string; firstNonWhitespaceCharacterIndex: number };
}

export interface StubEditor {
  document: StubDocument;
  selections: Selection[];
  readonly selection: Selection;
  options: Record<string, unknown>;
  visibleRanges: Range[];
  edit(cb: (b: unknown) => void): Promise<boolean>;
  revealRange(range: Range, type?: number): void;
  setDecorations(type: unknown, ranges: Range[]): void;
}

/** Event sources the tests fire, standing in for VSCode's own. */
export const stubEvents = {
  docChange: new Emitter<unknown>(),
  selection: new Emitter<unknown>(),
  activeEditor: new Emitter<unknown>(),
  closeDoc: new Emitter<unknown>(),
};

/** Commands the controller ran, newest last. */
export const stubCommands: { command: string; args: unknown[] }[] = [];

export const window = {
  activeTextEditor: undefined as StubEditor | undefined,
  createStatusBarItem: () => ({
    name: '',
    text: '',
    show() {},
    hide() {},
    dispose() {},
  }),
  createTextEditorDecorationType: () => ({ dispose() {} }),
  showWarningMessage: () => Promise.resolve(undefined),
  onDidChangeTextEditorSelection: stubEvents.selection.event,
  onDidChangeActiveTextEditor: stubEvents.activeEditor.event,
};

export const workspace = {
  onDidChangeTextDocument: stubEvents.docChange.event,
  onDidCloseTextDocument: stubEvents.closeDoc.event,
  getConfiguration: () => ({ get: <T>(_key: string, fallback: T) => fallback }),
};

export const commands = {
  executeCommand: (command: string, ...args: unknown[]) => {
    stubCommands.push({ command, args });
    return Promise.resolve(undefined);
  },
  registerCommand: () => ({ dispose() {} }),
};

/** Reset the module-level state between tests. */
export function resetStub(): void {
  window.activeTextEditor = undefined;
  stubCommands.length = 0;
}
