// End-to-end streaming session test: write a fixture file, open a session
// against it, and verify windows render correctly. Requires the WASM
// bundle and the worker bundle to be built.

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { loadWasm } from '../wasm.js';
import { StreamingSession } from './streamingSession.js';

const extensionPath = path.resolve(__dirname, '..', '..');
const wasm = loadWasm(extensionPath);
const workerScript = path.join(extensionPath, 'dist', 'indexerWorker.js');
const haveWorker = fs.existsSync(workerScript);

describe.skipIf(!wasm || !haveWorker)('StreamingSession', () => {
  let tmpDir: string;
  let logPath: string;
  let cacheDir: string;

  beforeEach(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'logviewer-streaming-'));
    logPath = path.join(tmpDir, 'fixture.log');
    cacheDir = path.join(tmpDir, 'cache');
    const lines: string[] = [];
    for (let i = 0; i < 500; i++) lines.push(`line ${i.toString().padStart(4, '0')}`);
    fs.writeFileSync(logPath, lines.join('\n') + '\n');
  });

  afterEach(() => {
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch {
      // ignore
    }
  });

  function makeSession(): Promise<StreamingSession> {
    return new Promise((resolve, reject) => {
      const session = new StreamingSession({
        fsPath: logPath,
        wasm: wasm!,
        cache: {
          globalStorageDir: cacheDir,
          mode: 'globalStorage',
          budgetBytes: 100 * 1024 * 1024,
        },
        stride: 32,
        chunkSize: 8 * 1024,
        batchAnchors: 16,
        workerScript,
        onIndexProgress: (e) => {
          if (e.complete) resolve(session);
        },
        onError: (e) => reject(e),
      });
      session.start().catch(reject);
    });
  }

  it('renders a small window from the head of the file', async () => {
    const session = await makeSession();
    try {
      const win = session.requestWindow(0, 5);
      expect(win.lines.length).toBe(5);
      expect(win.lines[0].text).toBe('line 0000');
      expect(win.lines[4].text).toBe('line 0004');
    } finally {
      await session.dispose();
    }
  });

  it('renders a window in the middle of the file', async () => {
    const session = await makeSession();
    try {
      const win = session.requestWindow(200, 210);
      expect(win.lines.length).toBe(10);
      expect(win.lines[0].text).toBe('line 0200');
      expect(win.lines[9].text).toBe('line 0209');
    } finally {
      await session.dispose();
    }
  });

  it('renders the tail (past last full anchor) correctly', async () => {
    const session = await makeSession();
    try {
      const total = session.getTotalLines();
      const win = session.requestWindow(total - 5, total + 10);
      // totalLines includes a trailing empty line from the final '\n'.
      // The last real line is "line 0499" at index 499.
      expect(win.lines.length).toBeGreaterThanOrEqual(4);
      const last = win.lines.find((l) => l.text === 'line 0499');
      expect(last).toBeDefined();
    } finally {
      await session.dispose();
    }
  });

  it('serves a tail window before indexing completes', async () => {
    // Stand up a session but don't wait for indexing to finish; instead
    // request a tail window right away. The tail buffer is built during
    // `start()` so the very last lines are servable immediately.
    const session = await new Promise<StreamingSession>((resolve, reject) => {
      let resolved = false;
      const s = new StreamingSession({
        fsPath: logPath,
        wasm: wasm!,
        cache: {
          globalStorageDir: cacheDir,
          mode: 'globalStorage',
          budgetBytes: 100 * 1024 * 1024,
        },
        stride: 32,
        chunkSize: 8 * 1024,
        batchAnchors: 16,
        workerScript,
        onIndexProgress: () => {
          if (!resolved) {
            resolved = true;
            resolve(s);
          }
        },
        onError: (e) => reject(e),
      });
      s.start().catch(reject);
    });
    try {
      const total = session.getTotalLines();
      const win = session.requestWindow(total - 5, total);
      // Tail buffer covers the last lines even if anchors don't yet.
      expect(win.lines.length).toBeGreaterThan(0);
      // The last real line is "line 0499".
      const last = win.lines.find((l) => l.text === 'line 0499');
      expect(last).toBeDefined();
    } finally {
      await session.dispose();
    }
  });

  it('serves the head window before indexing completes', async () => {
    // The head buffer is built synchronously during `start()`, so the very
    // first page is renderable immediately — without waiting for the indexer
    // to produce any anchors. Request it right after start() resolves, before
    // awaiting indexing progress.
    const session = new StreamingSession({
      fsPath: logPath,
      wasm: wasm!,
      cache: {
        globalStorageDir: cacheDir,
        mode: 'memory',
        budgetBytes: 100 * 1024 * 1024,
      },
      stride: 32,
      chunkSize: 8 * 1024,
      batchAnchors: 16,
      workerScript,
      onIndexProgress: () => {},
      onError: () => {},
    });
    await session.start();
    try {
      const win = session.requestWindow(0, 5);
      expect(win.lines.length).toBeGreaterThanOrEqual(5);
      expect(win.lines[0].text).toBe('line 0000');
      expect(win.lines[4].text).toBe('line 0004');
    } finally {
      await session.dispose();
    }
  });

  it('memory mode renders windows but writes nothing to disk', async () => {
    const session = await new Promise<StreamingSession>((resolve, reject) => {
      const s = new StreamingSession({
        fsPath: logPath,
        wasm: wasm!,
        cache: {
          globalStorageDir: cacheDir,
          mode: 'memory',
          budgetBytes: 100 * 1024 * 1024,
        },
        stride: 32,
        chunkSize: 8 * 1024,
        batchAnchors: 16,
        workerScript,
        onIndexProgress: (e) => {
          if (e.complete) resolve(s);
        },
        onError: (e) => reject(e),
      });
      s.start().catch(reject);
    });
    try {
      // The in-memory index serves windows just like the persisted modes.
      const win = session.requestWindow(100, 105);
      expect(win.lines[0].text).toBe('line 0100');
      // No index directory should have been created under globalStorage.
      expect(fs.existsSync(path.join(cacheDir, 'index'))).toBe(false);
    } finally {
      await session.dispose();
    }
  });

  it('reuses the persistent cache on a second open', async () => {
    const first = await makeSession();
    await first.dispose();
    // Look for the index file.
    const entries = fs.readdirSync(path.join(cacheDir, 'index'));
    expect(entries.some((e) => e.endsWith('.bin'))).toBe(true);

    // Second open should complete synchronously (from cache) before
    // we'd otherwise be able to indexer-progress.
    const cachedSession = await new Promise<StreamingSession>((resolve, reject) => {
      let resolved = false;
      const s = new StreamingSession({
        fsPath: logPath,
        wasm: wasm!,
        cache: {
          globalStorageDir: cacheDir,
          mode: 'globalStorage',
          budgetBytes: 100 * 1024 * 1024,
        },
        stride: 32,
        workerScript,
        onIndexProgress: (e) => {
          if (e.complete && !resolved) {
            resolved = true;
            resolve(s);
          }
        },
        onError: (e) => reject(e),
      });
      s.start().catch(reject);
    });
    try {
      expect(cachedSession.isComplete()).toBe(true);
      const win = cachedSession.requestWindow(100, 105);
      expect(win.lines[0].text).toBe('line 0100');
    } finally {
      await cachedSession.dispose();
    }
  });
});
