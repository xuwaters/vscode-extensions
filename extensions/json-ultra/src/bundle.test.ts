// Asserts on the built artifacts, skipping when dist/ is absent.

import { execFileSync } from 'child_process';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import { describe, expect, it } from 'vitest';

const dist = path.resolve(__dirname, '..', 'dist');
const hostBundle = path.join(dist, 'extension.js');
const webviewBundle = path.join(dist, 'webview.js');
const built = fs.existsSync(hostBundle) && fs.existsSync(webviewBundle);

describe.skipIf(!built)('the built bundles', () => {
  it('give the page a module a browser can parse', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'json-ultra-'));
    const file = path.join(dir, 'webview.mjs');
    fs.copyFileSync(webviewBundle, file);
    execFileSync(process.execPath, ['--check', file]);
    fs.rmSync(dir, { recursive: true, force: true });
  });

  it('keep the host bundle free of the webview, and the page free of the host', () => {
    expect(fs.readFileSync(webviewBundle, 'utf8')).not.toContain('require("vscode")');
    expect(fs.readFileSync(hostBundle, 'utf8')).not.toContain('acquireVsCodeApi');
  });
});
