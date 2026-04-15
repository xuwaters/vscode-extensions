/** Editor mode: source (CodeMirror), read (HTML preview), or live-preview (hybrid). */
export type EditorMode = 'source' | 'read' | 'live-preview';

// ── Host → Webview messages ──────────────────────────────────────────

export interface DocInitMessage {
  type: 'doc:init';
  content: string;
  uri: string;
  mode: EditorMode;
}

export interface DocUpdateChange {
  rangeOffset: number;
  rangeLength: number;
  text: string;
}

export interface DocUpdateMessage {
  type: 'doc:update';
  changes: DocUpdateChange[];
  version: number;
}

export interface EditAckMessage {
  type: 'edit:ack';
  version: number;
}

export interface ModeSetMessage {
  type: 'mode:set';
  mode: EditorMode;
}

export interface ConfigUpdateMessage {
  type: 'config:update';
  fontSize: number;
  fontFamily: string;
  theme: 'light' | 'dark' | 'high-contrast';
}

export type HostToWebviewMessage =
  | DocInitMessage
  | DocUpdateMessage
  | EditAckMessage
  | ModeSetMessage
  | ConfigUpdateMessage;

// ── Webview → Host messages ──────────────────────────────────────────

export interface EditApplyMessage {
  type: 'edit:apply';
  startLine: number;
  endLine: number;
  newText: string;
  version: number;
}

export interface CursorChangedMessage {
  type: 'cursor:changed';
  line: number;
}

export interface ModeChangedMessage {
  type: 'mode:changed';
  mode: EditorMode;
}

export type WebviewToHostMessage =
  | EditApplyMessage
  | CursorChangedMessage
  | ModeChangedMessage;
