/**
 * The folder-name half of "new project from template".
 *
 * The scaffolder asks for a *parent* directory and a name, rather than an empty
 * directory the reader had to make first. That means turning a package spec
 * into a sensible default name, and telling a bad name from a good one before
 * anything touches the disk.
 *
 * Kept free of VSCode so it can be tested on its own.
 */

/** Characters no portable directory name should carry, plus the separators. */
const ILLEGAL_CHARS = /[\\/:*?"<>|\x00-\x1f]/;

/**
 * The folder name `spec` suggests: the package name, without its namespace or
 * version. `@preview/charged-ieee:0.1.4` → `charged-ieee`.
 */
export function defaultProjectName(spec: string): string {
  const withoutVersion = spec.trim().split(':')[0] ?? '';
  const name = withoutVersion.slice(withoutVersion.lastIndexOf('/') + 1);
  return name.replace(/^@/, '');
}

/**
 * Why `name` is unusable as a new project folder, or `undefined` if it is fine.
 *
 * The message is shown under the input box as the reader types, so it says what
 * to do rather than what went wrong.
 */
export function validateProjectName(name: string): string | undefined {
  const trimmed = name.trim();

  if (trimmed.length === 0) return 'Enter a name for the project folder.';
  if (trimmed === '.' || trimmed === '..') {
    return 'A folder name, not `.` or `..`.';
  }
  if (ILLEGAL_CHARS.test(trimmed)) {
    return 'A folder name, not a path — no slashes or `: * ? " < > |`.';
  }
  // Windows silently drops these, so a folder named `report.` is not the folder
  // the reader thinks they asked for.
  if (/[. ]$/.test(trimmed)) {
    return 'A folder name cannot end with a dot or a space.';
  }
  return undefined;
}
