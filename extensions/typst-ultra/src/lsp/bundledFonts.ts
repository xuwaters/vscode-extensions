import * as fs from 'fs';
import * as path from 'path';

/**
 * Finding typst's default font set, which ships in its own extension.
 *
 * The fonts are 6.4 MB of a 16.8 MB VSIX and they only move when typst-assets
 * does, so they live in `wx-vsce-typst-ultra-fonts` and this extension reads
 * them from wherever that landed (decision 0012).
 *
 * The manifest names that extension in `extensionDependencies`, which is a hard
 * requirement: VS Code refuses to *activate* this extension when a declared
 * dependency is missing, so none of the fallbacks below can run in that case —
 * the user gets VS Code's own "cannot activate … which is not installed"
 * instead. They are here because the declaration is one line to drop, and
 * without it a document typeset in system fonts is the right failure: worse
 * output, not a dead editor.
 */

/** The extension that carries `assets/fonts/`. */
export const FONTS_EXTENSION_ID = 'weixu.wx-vsce-typst-ultra-fonts';

/** Where a font directory was found, for the log line. */
export type FontSource =
  /** The companion extension, as installed. The normal case. */
  | 'companion'
  /** Inside this extension, where the fonts lived before they moved out. */
  | 'in-place'
  /** The sibling package in a checkout — what `F5` sees, with nothing installed. */
  | 'sibling';

export interface BundledFonts {
  /** Absolute path to the directory to index. */
  path: string;
  source: FontSource;
}

const FONT_EXTENSIONS = new Set(['.ttf', '.otf', '.ttc', '.otc']);

/**
 * Every directory that could hold the set, best first.
 *
 * Pure, so the order is a test rather than a claim.
 *
 * @param extensionPath This extension's install directory.
 * @param companionPath The companion extension's, if it is installed.
 */
export function fontCandidates(
  extensionPath: string,
  companionPath?: string,
): BundledFonts[] {
  const candidates: BundledFonts[] = [];

  if (companionPath) {
    candidates.push({
      path: path.join(companionPath, 'assets', 'fonts'),
      source: 'companion',
    });
  }

  candidates.push({
    path: path.join(extensionPath, 'assets', 'fonts'),
    source: 'in-place',
  });

  // The development loop: the Extension Development Host runs this extension
  // from the checkout, where the companion is a directory next door rather
  // than an installed extension, so `getExtension` returns nothing for it.
  candidates.push({
    path: path.join(extensionPath, '..', 'typst-ultra-fonts', 'assets', 'fonts'),
    source: 'sibling',
  });

  return candidates;
}

/** Whether a directory holds at least one font file. */
export function containsFonts(dir: string): boolean {
  let entries: string[];
  try {
    entries = fs.readdirSync(dir);
  } catch {
    return false;
  }
  return entries.some((entry) => FONT_EXTENSIONS.has(path.extname(entry).toLowerCase()));
}

/**
 * The first candidate that actually holds fonts, or `undefined`.
 *
 * "Holds fonts" rather than "exists" on purpose: `pnpm run clean` leaves an
 * empty directory behind, and indexing it would report success while producing
 * a document typeset in whatever the system had lying around.
 */
export function resolveBundledFonts(
  extensionPath: string,
  companionPath?: string,
  contains: (dir: string) => boolean = containsFonts,
): BundledFonts | undefined {
  return fontCandidates(extensionPath, companionPath).find((candidate) =>
    contains(candidate.path),
  );
}
