// Messages exchanged between the indexer worker and the host. Kept in a
// separate module so both ends can import without pulling in
// `worker_threads` / `fs`.

export interface IndexWorkerInit {
  /** Absolute filesystem path of the log file. */
  path: string;
  /** Anchor every `stride` lines (default 1024). */
  stride: number;
  /** Read chunk size in bytes (default 4 MB). */
  chunkSize: number;
  /** Flush threshold for anchor batches (default 4096). */
  batchAnchors: number;
}

export type WorkerCommand = { type: 'cancel' };

export type WorkerEvent =
  | {
      type: 'progress';
      lines: number;
      bytes: number;
      anchors: BigUint64Array;
    }
  | {
      type: 'complete';
      totalLines: number;
      fileSize: number;
      anchors: BigUint64Array;
    }
  | { type: 'cancelled' }
  | { type: 'error'; message: string };
