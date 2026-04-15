import { ModeController } from './modeController';
import type {
  HostToWebviewMessage,
  DocInitMessage,
  DocUpdateMessage,
  ModeSetMessage,
} from '../src/messages';
import type { EditorMode } from './renderers/types';

// Import styles
import './styles/preview.css';
import './styles/editor.css';
import './styles/frontmatter.css';
import './styles/math.css';
import './styles/mermaid.css';

// Acquire the VSCode API
const vscode = acquireVsCodeApi();

// Root container
const root = document.getElementById('root')!;
root.className = 'live-preview-container';

// Detect theme from body class
function getTheme(): 'light' | 'dark' {
  return document.body.classList.contains('vscode-light') ? 'light' : 'dark';
}

let controller: ModeController | null = null;
let documentUri = '';
let documentVersion = 0;

// Edit debounce
let editTimeout: ReturnType<typeof setTimeout> | null = null;
let pendingEdit: { startLine: number; endLine: number; newText: string } | null = null;

function sendEdit(startLine: number, endLine: number, newText: string): void {
  // Debounce edits to avoid flooding the host
  pendingEdit = { startLine, endLine, newText };
  if (editTimeout) clearTimeout(editTimeout);
  editTimeout = setTimeout(() => {
    if (pendingEdit) {
      vscode.postMessage({
        type: 'edit:apply',
        startLine: pendingEdit.startLine,
        endLine: pendingEdit.endLine,
        newText: pendingEdit.newText,
        version: documentVersion,
      });
      pendingEdit = null;
    }
  }, 50);
}

function sendCursorChanged(line: number): void {
  vscode.postMessage({ type: 'cursor:changed', line });
}

// Handle messages from the extension host
window.addEventListener('message', (event) => {
  const msg = event.data as HostToWebviewMessage & { type: string };

  switch (msg.type) {
    case 'doc:init':
      handleDocInit(msg as DocInitMessage);
      break;

    case 'doc:update':
      handleDocUpdate(msg as DocUpdateMessage);
      break;

    case 'edit:ack':
      documentVersion = (msg as { version: number }).version;
      break;

    case 'mode:set':
      handleModeSet(msg as ModeSetMessage);
      break;

    case 'mode:cycle':
      handleModeCycle();
      break;

    case 'config:update':
      // Could re-create controller with new theme, but for now just note it
      break;
  }
});

function handleDocInit(msg: DocInitMessage): void {
  documentUri = msg.uri;
  documentVersion = 0;

  controller = new ModeController(
    root,
    sendEdit,
    sendCursorChanged,
    getTheme(),
  );

  controller.init(msg.content, msg.uri, msg.mode as EditorMode);
}

function handleDocUpdate(msg: DocUpdateMessage): void {
  if (!controller) return;
  documentVersion = msg.version;
  controller.applyDocumentChanges(msg.changes);
}

function handleModeSet(msg: ModeSetMessage): void {
  if (!controller) return;
  controller.setMode(msg.mode as EditorMode);
  vscode.postMessage({ type: 'mode:changed', mode: msg.mode });
}

function handleModeCycle(): void {
  if (!controller) return;
  const newMode = controller.cycleMode();
  vscode.postMessage({ type: 'mode:changed', mode: newMode });
}

// Signal to the host that the webview is ready
vscode.postMessage({ type: 'webview:ready' });

// Declare the acquireVsCodeApi function type
declare function acquireVsCodeApi(): {
  postMessage(msg: unknown): void;
  getState(): unknown;
  setState(state: unknown): void;
};
