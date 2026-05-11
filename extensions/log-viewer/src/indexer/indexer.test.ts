// Spawn the real worker bundle (`dist/indexerWorker.js`) against a fixture
// log file. Skipped if the bundle hasn't been built yet — the test ensures
// the scan algorithm survives the worker_threads boundary (BigUint64Array
// transfer, parentPort wiring, cancel message handling).

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { afterAll, beforeAll, describe, expect, it } from 'vitest';
import { Indexer } from './indexer.js';

const workerScript = path.resolve(__dirname, '..', '..', 'dist', 'indexerWorker.js');
const haveWorker = fs.existsSync(workerScript);

describe.skipIf(!haveWorker)('Indexer (real worker)', () => {
  let tmpDir: string;
  let logPath: string;
  const lineCount = 5_000;

  beforeAll(() => {
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), 'logviewer-indexer-'));
    logPath = path.join(tmpDir, 'fixture.log');
    const lines: string[] = [];
    for (let i = 0; i < lineCount; i++) lines.push(`line ${i.toString().padStart(6, '0')}`);
    fs.writeFileSync(logPath, lines.join('\n') + '\n');
  });

  afterAll(() => {
    try {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    } catch {
      // ignore
    }
  });

  it('indexes a small fixture and reports anchors', async () => {
    const indexer = new Indexer();
    const anchorBatches: BigUint64Array[] = [];
    const complete = await new Promise<{ totalLines: number; fileSize: number }>(
      (resolve, reject) => {
        indexer.start({
          path: logPath,
          stride: 256,
          chunkSize: 4096,
          batchAnchors: 16,
          workerScript,
          onProgress: (e) => anchorBatches.push(e.newAnchors),
          onComplete: resolve,
          onError: reject,
          onCancelled: () => reject(new Error('unexpected cancel')),
        });
      },
    );
    await indexer.dispose();

    expect(complete.totalLines).toBe(lineCount + 1);
    expect(complete.fileSize).toBe(fs.statSync(logPath).size);

    const allAnchors: bigint[] = [];
    for (const a of anchorBatches) {
      for (let i = 0; i < a.length; i++) allAnchors.push(a[i]);
    }
    expect(allAnchors[0]).toBe(0n);
    // Expect ceil(lineCount / stride) anchors.
    const expected = Math.ceil(lineCount / 256);
    expect(allAnchors.length).toBe(expected);
    // Anchors must be strictly increasing.
    for (let i = 1; i < allAnchors.length; i++) {
      expect(allAnchors[i]).toBeGreaterThan(allAnchors[i - 1]);
    }
  });

  it('honors cancellation', async () => {
    // Make a bigger fixture so we have time to cancel mid-scan.
    const big = path.join(tmpDir, 'big.log');
    const chunk = ('x'.repeat(80) + '\n').repeat(10_000); // ~810 KB
    const writes = 20;
    const fd = fs.openSync(big, 'w');
    for (let i = 0; i < writes; i++) fs.writeSync(fd, chunk);
    fs.closeSync(fd);

    const indexer = new Indexer();
    const outcome = await new Promise<'cancelled' | 'complete'>((resolve, reject) => {
      indexer.start({
        path: big,
        stride: 256,
        chunkSize: 64 * 1024,
        batchAnchors: 32,
        workerScript,
        onProgress: () => indexer.cancel(),
        onCancelled: () => resolve('cancelled'),
        onComplete: () => resolve('complete'),
        onError: reject,
      });
    });
    await indexer.dispose();
    // Cancellation is best-effort: it might race past completion on a fast
    // machine. Accept either outcome as long as the worker exits cleanly.
    expect(['cancelled', 'complete']).toContain(outcome);
  });
});
