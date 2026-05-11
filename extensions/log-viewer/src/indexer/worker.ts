// Worker-thread entry point for the streaming indexer. Receives an
// `IndexWorkerInit` via `workerData`, reads the file in chunks, emits
// anchor batches as `WorkerEvent`s, and posts `complete` (or
// `cancelled` / `error`) before exiting.

import * as fs from 'fs';
import { parentPort, workerData } from 'worker_threads';
import { scanIndex } from './scan.js';
import type {
  IndexWorkerInit,
  WorkerCommand,
  WorkerEvent,
} from './protocol.js';

if (!parentPort) {
  throw new Error('indexer worker spawned without a parent port');
}

const port = parentPort;
const init = workerData as IndexWorkerInit;

let cancelled = false;
port.on('message', (msg: WorkerCommand) => {
  if (msg && msg.type === 'cancel') cancelled = true;
});

function post(event: WorkerEvent): void {
  // Transfer the underlying buffer of any BigUint64Array to avoid a copy
  // (the worker has no further use for it after sending).
  if (event.type === 'progress' || event.type === 'complete') {
    const transfer = [event.anchors.buffer as ArrayBuffer];
    port.postMessage(event, transfer);
  } else {
    port.postMessage(event);
  }
}

try {
  const fd = fs.openSync(init.path, 'r');
  try {
    const result = scanIndex({
      stride: init.stride,
      chunkSize: init.chunkSize,
      batchAnchors: init.batchAnchors,
      isCancelled: () => cancelled,
      read: (offset, buf, len) => {
        // fs.readSync's position parameter accepts a bigint when reading
        // past the 32-bit signed range; older Node versions accept a
        // number, so prefer the number form when in range.
        const pos = offset <= BigInt(Number.MAX_SAFE_INTEGER) ? Number(offset) : offset;
        return fs.readSync(fd, buf, 0, len, pos as number);
      },
      emitAnchors: (anchors, lineCount, byteOffset) => {
        post({
          type: 'progress',
          lines: lineCount,
          bytes: Number(byteOffset),
          anchors,
        });
      },
    });
    if (result.cancelled) {
      post({ type: 'cancelled' });
    } else {
      // The final progress flush also carried the last anchors; send an
      // empty anchor array on completion (totals are authoritative).
      post({
        type: 'complete',
        totalLines: result.totalLines,
        fileSize: Number(result.fileSize),
        anchors: new BigUint64Array(0),
      });
    }
  } finally {
    fs.closeSync(fd);
  }
} catch (e) {
  post({ type: 'error', message: e instanceof Error ? e.message : String(e) });
}
