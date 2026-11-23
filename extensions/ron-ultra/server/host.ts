import * as fs from 'fs';
import { fileURLToPath } from 'url';

/** The callbacks the WASM server calls back into, synchronously. */
export interface HostServices {
  /** The text of the file at `uri`, or `null` when there is none. */
  readFile(uri: string): string | null;
}

export const host: HostServices = {
  readFile(uri: string): string | null {
    if (!uri.startsWith('file:')) return null;
    try {
      return fs.readFileSync(fileURLToPath(uri), 'utf8');
    } catch {
      // Missing, unreadable, or a directory: all mean "no such file" to a
      // resolver probing `foo.rs` before `foo/mod.rs`.
      return null;
    }
  },
};
