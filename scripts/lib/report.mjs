// Commands all report the same shape of work — one line per extension, a count
// at the end — so the formatting lives here rather than in each command.

/**
 * @typedef {object} ReportRow
 * @property {string} name Extension directory name.
 * @property {string} detail What happened, or would happen.
 * @property {boolean} [changed] Whether this row represents a real change.
 */

/**
 * Align the detail column so a run reads as a table.
 *
 * @param {ReportRow[]} rows
 * @returns {string[]}
 */
export function formatRows(rows) {
  const width = rows.reduce((max, row) => Math.max(max, row.name.length), 0);
  return rows.map((row) => `  ${row.name.padEnd(width)}  ${row.detail}`);
}

/**
 * @param {number} count
 * @param {string} singular
 * @param {string} [plural] Defaults to `singular` + 's'.
 * @returns {string}
 */
export function plural(count, singular, plural = `${singular}s`) {
  return `${count} ${count === 1 ? singular : plural}`;
}

/**
 * @param {object} options
 * @param {string} options.action Imperative description, e.g. `bump patch`.
 * @param {boolean} options.dryRun
 * @param {ReportRow[]} options.rows
 * @param {(message: string) => void} options.write
 * @returns {number} How many rows changed.
 */
export function report({ action, dryRun, rows, write }) {
  const changed = rows.filter((row) => row.changed).length;
  write(`${dryRun ? '[dry-run] ' : ''}${action} (${plural(rows.length, 'extension')}):`);
  for (const line of formatRows(rows)) write(line);
  write(
    dryRun
      ? `${plural(changed, 'extension')} would change.`
      : `${plural(changed, 'extension')} changed.`,
  );
  return changed;
}
