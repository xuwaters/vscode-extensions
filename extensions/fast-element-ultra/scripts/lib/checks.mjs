// Commands here fail the same way — check a condition, say which one, keep
// going so one run reports every problem rather than only the first — so the
// bookkeeping lives here instead of in each command.

/**
 * @typedef {object} Checks
 * @property {(condition: unknown, what: string) => boolean} check
 *   Record one condition and report it. Returns whether it held.
 * @property {() => number} failures How many checks did not hold.
 * @property {() => number} exitCode 0 when everything held, 1 otherwise.
 */

/**
 * @param {(message: string) => void} write Where the per-check lines go.
 * @returns {Checks}
 */
export function createChecks(write) {
  let failures = 0;

  return {
    check(condition, what) {
      const held = Boolean(condition);
      if (held) {
        write(`  ok: ${what}`);
      } else {
        write(`  FAILED: ${what}`);
        failures += 1;
      }
      return held;
    },
    failures: () => failures,
    exitCode: () => (failures > 0 ? 1 : 0),
  };
}

/**
 * A missing build product is a broken build, not bad input from the user, so
 * it is reported as a failed run rather than as a usage error.
 *
 * @param {readonly string[]} missing What was not found.
 * @param {string} what Noun for the missing things, e.g. `VSIX entries`.
 * @param {string} remedy Command that would produce them.
 * @param {(message: string) => void} write
 * @returns {number} Exit code: 0 when nothing was missing.
 */
export function reportMissing(missing, what, remedy, write) {
  if (missing.length === 0) return 0;
  write(`Missing ${what}:`);
  for (const entry of missing) write(`  ${entry}`);
  write(`Run \`${remedy}\` first.`);
  return 1;
}
