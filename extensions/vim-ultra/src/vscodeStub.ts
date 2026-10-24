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
  uri: { scheme: string; path: string; toString(): string };
  eol: number;
  isDirty: boolean;
  isUntitled: boolean;
  getText(): string;
  lineCount: number;
  lineAt(line: number): { text: string; firstNonWhitespaceCharacterIndex: number };
}

/** `vscode.Uri`, as far as a URI string goes. */
export const Uri = {
  parse(value: string) {
    return {
      scheme: value.split(':')[0],
      path: value.replace(/^\w+:\/\//, ''),
      toString: () => value,
    };
  },
};

/** A tab showing a text document, as `window.tabGroups` lists it. */
export class TabInputText {
  constructor(readonly uri: { toString(): string }) {}
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

/**
 * What running a command does to the stub world, by command id — a save
 * that clears the dirty flag, a close that takes the editor away. Commands
 * without one are only recorded.
 */
export const stubCommandEffects = new Map<string, (...args: unknown[]) => void>();

/** The one status bar item the controller makes; tests read its text. */
export const stubStatus = { name: '', text: '', show() {}, hide() {}, dispose() {} };

/** Every QuickPick shown, newest last: the items and the options. */
export const stubQuickPicks: { items: unknown[]; options: unknown }[] = [];

/** Documents `workspace.openTextDocument` can open, by URI string. */
export const stubDocuments = new Map<string, StubDocument>();

/**
 * The system clipboard. `read` is what `readText` answers with — a test can
 * swap in a promise it settles itself, to hold a read in flight.
 */
export const stubClipboard = {
  text: '',
  writes: [] as string[],
  read: (): Promise<string> => Promise.resolve(stubClipboard.text),
};

export const window = {
  activeTextEditor: undefined as StubEditor | undefined,
  /** Open tabs, by group: what tells the controller a document is shown twice. */
  tabGroups: { all: [] as { tabs: { input: unknown }[] }[] },
  createStatusBarItem: () => stubStatus,
  createTextEditorDecorationType: () => ({ dispose() {} }),
  showWarningMessage: () => Promise.resolve(undefined),
  showTextDocument: (doc: StubDocument, options?: unknown) => {
    stubCommands.push({ command: 'showTextDocument', args: options ? [doc, options] : [doc] });
    return Promise.resolve(undefined);
  },
  showQuickPick: (items: unknown[], options: unknown) => {
    stubQuickPicks.push({ items, options });
    return Promise.resolve(undefined);
  },
  onDidChangeTextEditorSelection: stubEvents.selection.event,
  onDidChangeActiveTextEditor: stubEvents.activeEditor.event,
};

export const workspace = {
  /** Every open document, as the controller checks them for `:qa`. */
  textDocuments: [] as StubDocument[],
  asRelativePath: (uri: { path: string }) => uri.path.replace(/^\//, ''),
  openTextDocument: (uri: { toString(): string }) => {
    const doc = stubDocuments.get(uri.toString());
    return doc ? Promise.resolve(doc) : Promise.reject(new Error(`cannot open ${uri.toString()}`));
  },
  onDidChangeTextDocument: stubEvents.docChange.event,
  onDidCloseTextDocument: stubEvents.closeDoc.event,
  getConfiguration: () => ({ get: <T>(_key: string, fallback: T) => fallback }),
};

export const env = {
  clipboard: {
    readText: () => stubClipboard.read(),
    writeText: (text: string) => {
      stubClipboard.text = text;
      stubClipboard.writes.push(text);
      return Promise.resolve();
    },
  },
};

export const commands = {
  executeCommand: (command: string, ...args: unknown[]) => {
    stubCommands.push({ command, args });
    stubCommandEffects.get(command)?.(...args);
    return Promise.resolve(undefined);
  },
  registerCommand: () => ({ dispose() {} }),
};

/** Reset the module-level state between tests. */
export function resetStub(): void {
  window.activeTextEditor = undefined;
  window.tabGroups.all = [];
  workspace.textDocuments = [];
  stubCommands.length = 0;
  stubCommandEffects.clear();
  stubStatus.text = '';
  stubQuickPicks.length = 0;
  stubDocuments.clear();
  stubClipboard.text = '';
  stubClipboard.writes.length = 0;
  stubClipboard.read = () => Promise.resolve(stubClipboard.text);
}
