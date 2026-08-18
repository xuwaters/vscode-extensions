import * as path from 'path';

/**
 * The bundled font set, as the tests reach it.
 *
 * It ships in the `typst-ultra-fonts` extension rather than this one (decision
 * 0012), so from a checkout it is the sibling package. Kept in one place
 * because every test that compiles a document needs it, and because the path
 * crosses a package boundary — the kind of thing that should break once, and
 * obviously, rather than in six files.
 */
export const BUNDLED_FONTS = path.join(
  __dirname,
  '..',
  '..',
  'typst-ultra-fonts',
  'assets',
  'fonts',
);
