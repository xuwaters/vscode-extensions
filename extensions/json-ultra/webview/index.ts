// Webview bootstrap — the only place `acquireVsCodeApi` exists, so the
// table itself stays mountable in a DOM test without the VSCode shim.

import type { HostToWebview, WebviewToHost } from '../src/messages.js';
import styles from './styles.css';
import { TableView } from './table.js';

interface VsCodeApi {
  postMessage(message: WebviewToHost): void;
}
declare function acquireVsCodeApi(): VsCodeApi;

const api = acquireVsCodeApi();

const styleTag = document.createElement('style');
styleTag.textContent = styles;
document.head.append(styleTag);

const mount = document.getElementById('app');
if (!mount) throw new Error('the page has no #app to mount into');

const view = new TableView(mount, { post: (message) => api.postMessage(message) });

window.addEventListener('message', (event: MessageEvent<unknown>) => {
  const message = event.data as HostToWebview;
  if (typeof message !== 'object' || message === null) return;
  try {
    view.handle(message);
  } catch (error) {
    api.postMessage({
      type: 'error',
      message: error instanceof Error ? (error.stack ?? error.message) : String(error),
    });
  }
});

api.postMessage({ type: 'ready' });
