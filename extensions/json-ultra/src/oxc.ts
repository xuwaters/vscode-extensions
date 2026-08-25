// oxfmt delegation. When the workspace is configured for oxc's
// formatter, formatting requests are piped through `oxfmt
// --stdin-filepath <file>` so oxc's own config discovery (nested
// configs, ignore files) applies. Anything short of a clean run — no
// config, no binary, non-zero exit, timeout — reports "not handled" and
// the caller falls back to the built-in WASM formatter.
//
// Detection mirrors oxfmt's auto-discovered config names
// (apps/oxfmt/src/core/config/mod.rs): .oxfmtrc.json, .oxfmtrc.jsonc,
// oxfmt.config.ts, oxfmt.config.mts. `oxfmt.config.json` is NOT one of
// them — projects using it pass `-c` explicitly.

import { execFile } from 'child_process';
import * as fs from 'fs';
import * as path from 'path';
import * as vscode from 'vscode';
import { readOxcMode, readOxcPath } from './config.js';

const OXFMT_CONFIG_NAMES = [
  '.oxfmtrc.json',
  '.oxfmtrc.jsonc',
  'oxfmt.config.ts',
  'oxfmt.config.mts',
];

const FORMAT_TIMEOUT_MS = 10_000;

/** Directories from the file's folder up to (and including) `root`. */
function directoriesUpTo(fileDir: string, root: string): string[] {
  const dirs: string[] = [];
  let current = fileDir;
  for (;;) {
    dirs.push(current);
    if (current === root) break;
    const parent = path.dirname(current);
    if (parent === current) break;
    current = parent;
  }
  return dirs;
}

function workspaceRootFor(document: vscode.TextDocument): string | undefined {
  return vscode.workspace.getWorkspaceFolder(document.uri)?.uri.fsPath;
}

/** An oxfmt config file anywhere between the file and the workspace root. */
export function hasOxfmtConfig(document: vscode.TextDocument): boolean {
  if (document.uri.scheme !== 'file') return false;
  const root = workspaceRootFor(document);
  if (!root) return false;
  for (const dir of directoriesUpTo(path.dirname(document.uri.fsPath), root)) {
    for (const name of OXFMT_CONFIG_NAMES) {
      if (fs.existsSync(path.join(dir, name))) return true;
    }
  }
  return false;
}

/**
 * The oxfmt binary to run: the explicit setting, else the nearest
 * `node_modules/.bin/oxfmt`, else bare `oxfmt` for PATH resolution.
 * `null` only when an explicit setting points at nothing.
 */
export function resolveOxfmtBinary(document: vscode.TextDocument): string | null {
  const explicit = readOxcPath(document.uri);
  if (explicit) {
    return fs.existsSync(explicit) ? explicit : null;
  }
  const root = workspaceRootFor(document);
  if (root && document.uri.scheme === 'file') {
    for (const dir of directoriesUpTo(path.dirname(document.uri.fsPath), root)) {
      for (const bin of ['oxfmt', 'oxfmt.exe'] as const) {
        const candidate = path.join(dir, 'node_modules', '.bin', bin);
        if (fs.existsSync(candidate)) return candidate;
      }
    }
  }
  return 'oxfmt';
}

/** Whether this format request should go to oxfmt at all. */
export function shouldUseOxc(document: vscode.TextDocument): boolean {
  // oxfmt rejects JSON Lines ("Unsupported file type for stdin-filepath");
  // don't spawn a process just to find that out.
  if (document.languageId === 'jsonl') return false;
  if (/\.(jsonl|ndjson)$/i.test(document.uri.fsPath)) return false;
  if (readOxcMode(document.uri) === 'never') return false;
  if (readOxcPath(document.uri)) return true;
  return hasOxfmtConfig(document);
}

/**
 * Pipe `text` (the possibly-dirty buffer) through oxfmt. Resolves to
 * the formatted text, or `null` when oxfmt could not handle it.
 */
export function formatWithOxfmt(
  document: vscode.TextDocument,
  text: string,
  log: (message: string) => void,
): Promise<string | null> {
  const binary = resolveOxfmtBinary(document);
  if (!binary) {
    log('oxfmt: configured path does not exist; falling back to built-in formatter');
    return Promise.resolve(null);
  }
  const cwd = workspaceRootFor(document) ?? path.dirname(document.uri.fsPath);
  return new Promise((resolve) => {
    const child = execFile(
      binary,
      ['--stdin-filepath', document.uri.fsPath],
      { cwd, timeout: FORMAT_TIMEOUT_MS, maxBuffer: 256 * 1024 * 1024 },
      (error, stdout, stderr) => {
        if (error) {
          const reason = (error as NodeJS.ErrnoException).code === 'ENOENT'
            ? 'binary not found'
            : stderr.trim() || error.message;
          log(`oxfmt: ${reason}; falling back to built-in formatter`);
          resolve(null);
          return;
        }
        resolve(stdout);
      },
    );
    child.stdin?.on('error', () => {
      // EPIPE when the binary exits early; the callback reports it.
    });
    child.stdin?.end(text);
  });
}
