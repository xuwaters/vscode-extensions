import * as path from 'path';
import { defineConfig } from 'vitest/config';

/**
 * `vscode` only exists inside the editor's own host, so tests that drive
 * extension code resolve the import to the stub instead.
 */
export default defineConfig({
  test: {
    alias: {
      vscode: path.join(import.meta.dirname, 'src', 'vscodeStub.ts'),
    },
  },
});
