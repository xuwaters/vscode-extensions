// Host-side wrapper around the indexer worker.
//
// Owns the worker lifetime, accumulates anchor batches into a single
// `BigUint64Array`, surfaces progress / completion to the caller, and
// supports cancellation.

import { Worker } from 'worker_threads';
import * as path from 'path';
import type { IndexWorkerInit, WorkerEvent } from './protocol.js';

export const DEFAULT_STRIDE = 1024;
export const DEFAULT_CHUNK_SIZE = 4 * 1024 * 1024;
export const DEFAULT_BATCH_ANCHORS = 4096;

export interface IndexerStartOptions {
  /** Absolute filesystem path. */
  path: string;
  stride?: number;
  chunkSize?: number;
  batchAnchors?: number;
  onProgress?: (e: {
    lines: number;
    bytes: number;
    newAnchors: BigUint64Array;
  }) => void;
  onComplete?: (e: { totalLines: number; fileSize: number }) => void;
  onCancelled?: () => void;
  onError?: (err: Error) => void;
  /**
   * Override the worker script path. Defaults to `dist/indexerWorker.js`
   * resolved against the directory the host module was bundled into.
   */
  workerScript?: string;
}

export class Indexer {
  private worker: Worker | null = null;
  private disposed = false;

  start(opts: IndexerStartOptions): void {
    if (this.worker) throw new Error('Indexer already started');
    if (this.disposed) throw new Error('Indexer disposed');

    const init: IndexWorkerInit = {
      path: opts.path,
      stride: opts.stride ?? DEFAULT_STRIDE,
      chunkSize: opts.chunkSize ?? DEFAULT_CHUNK_SIZE,
      batchAnchors: opts.batchAnchors ?? DEFAULT_BATCH_ANCHORS,
    };

    const script = opts.workerScript ?? defaultWorkerScript();
    const worker = new Worker(script, { workerData: init });
    this.worker = worker;

    worker.on('message', (msg: WorkerEvent) => {
      switch (msg.type) {
        case 'progress':
          opts.onProgress?.({
            lines: msg.lines,
            bytes: msg.bytes,
            newAnchors: msg.anchors,
          });
          break;
        case 'complete':
          opts.onComplete?.({
            totalLines: msg.totalLines,
            fileSize: msg.fileSize,
          });
          break;
        case 'cancelled':
          opts.onCancelled?.();
          break;
        case 'error':
          opts.onError?.(new Error(msg.message));
          break;
      }
    });
    worker.on('error', (err: unknown) => {
      opts.onError?.(err instanceof Error ? err : new Error(String(err)));
    });
  }

  cancel(): void {
    this.worker?.postMessage({ type: 'cancel' });
  }

  async dispose(): Promise<void> {
    this.disposed = true;
    const w = this.worker;
    this.worker = null;
    if (w) {
      try {
        await w.terminate();
      } catch {
        // ignore
      }
    }
  }
}

function defaultWorkerScript(): string {
  // The host extension is bundled to `dist/extension.js`; the worker
  // ships next to it as `dist/indexerWorker.js`.
  return path.join(__dirname, 'indexerWorker.js');
}
