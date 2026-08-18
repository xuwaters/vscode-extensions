import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { describe, expect, it } from 'vitest';

/**
 * The one thing no test in `webview/` can see: whether the bundle that actually
 * ships is a program a browser will run. It lives here rather than beside the
 * webview's own tests because it needs Node — which is exactly what a webview
 * file is compiled without.
 *
 * `@microsoft/fast-element` ships *legacy* decorators, and a transform that has
 * not been told so emits `@customElement(…) class …` verbatim — syntax no
 * engine parses. Every source test still passes, because they import the
 * TypeScript; the webview just fails to load as a whole, and the tab is an
 * empty `<body>` with no `<pdf-viewer>` in it. So the assertion here is on the
 * artifact, not the source: parse `dist/webview.js` the way the page does.
 *
 * Node's module goal is the closest parser to hand, hence the `.mjs` copy —
 * `--check` picks its goal from the extension, and the bundle uses
 * `import.meta`, which is an error under the script goal.
 */

// vitest runs from the package root; the bundle is only there after a build,
// and a checkout that has not run one skips rather than fails.
const dist = path.resolve(process.cwd(), 'dist');
const webviewBundle = path.join(dist, 'webview.js');
const hostBundle = path.join(dist, 'extension.js');
const built = fs.existsSync(webviewBundle) && fs.existsSync(hostBundle);

describe.skipIf(!built)('the built bundles', () => {
  it('give the page a module a browser can parse', () => {
    const dir = fs.mkdtempSync(path.join(os.tmpdir(), 'pdf-ultra-bundle-'));
    const copy = path.join(dir, 'webview.mjs');
    try {
      fs.copyFileSync(webviewBundle, copy);
      execFileSync(process.execPath, ['--check', copy], { stdio: 'pipe' });
    } finally {
      fs.rmSync(dir, { recursive: true, force: true });
    }
  });

  it('agree on the tag the page writes and the bootstrap waits for', () => {
    expect(fs.readFileSync(hostBundle, 'utf8')).toContain('<pdf-viewer>');
    expect(fs.readFileSync(webviewBundle, 'utf8')).toContain('pdf-viewer');
  });
});
