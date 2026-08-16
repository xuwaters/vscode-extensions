// Command registry and dispatch. Everything a command needs to describe itself
// — flags, help text, exit code — lives in the command module, so adding one
// means writing one file and listing it in main.mjs.

import { parseArgs } from 'node:util';

/**
 * A flag, described once and used twice: to parse it and to document it.
 *
 * @typedef {object} OptionSpec
 * @property {'boolean' | 'string'} type
 * @property {string} describe Help text, one line.
 * @property {string} [short] Single-character alias.
 * @property {boolean} [multiple] Allow repetition, collecting into an array.
 * @property {string} [placeholder] Value name in help, e.g. `<x.y.z>`.
 */

/**
 * Flags as `parseArgs` returns them: a repeatable flag arrives as an array.
 *
 * @typedef {Record<string, string | boolean | Array<string | boolean> | undefined>} ParsedValues
 */

/**
 * @typedef {object} CommandContext
 * @property {ParsedValues} values Parsed flags.
 * @property {string[]} positionals Remaining arguments.
 * @property {import('./repo.mjs').Repo} repo
 * @property {(message: string) => void} write Normal output; injected so tests can capture it.
 */

/**
 * @typedef {object} Command
 * @property {string} name Sub-command as typed on the command line.
 * @property {string} summary One line, shown in the command list.
 * @property {string[]} [usage] Usage lines, shown in the command's own help.
 * @property {string[]} [details] Paragraphs shown under the options.
 * @property {Record<string, OptionSpec>} [options]
 * @property {boolean} [allowPositionals]
 * @property {(ctx: CommandContext) => number | void | Promise<number | void>} run
 *   Returns a process exit code; `undefined` means success.
 */

/** Bad input from the user, as opposed to a bug: reported without a stack. */
export class UsageError extends Error {
  /**
   * @param {string} message
   * @param {string} [help] Help text to print after the message.
   */
  constructor(message, help) {
    super(message);
    this.name = 'UsageError';
    this.help = help;
  }
}

/** Flags every command understands. */
const GLOBAL_OPTIONS = /** @type {Record<string, OptionSpec>} */ ({
  help: { type: 'boolean', short: 'h', describe: 'Show this help.' },
});

/**
 * `parseArgs` rejects unknown keys in an option spec, so hand it only what it
 * knows about and keep `describe` and friends for the help text.
 *
 * @param {Record<string, OptionSpec>} options
 * @returns {Record<string, { type: 'boolean' | 'string', short?: string, multiple?: boolean }>}
 */
function toParseArgsOptions(options) {
  return Object.fromEntries(
    Object.entries(options).map(([name, spec]) => [
      name,
      {
        type: spec.type,
        ...(spec.short === undefined ? {} : { short: spec.short }),
        ...(spec.multiple === undefined ? {} : { multiple: spec.multiple }),
      },
    ]),
  );
}

/**
 * @param {Record<string, OptionSpec>} options
 * @returns {string[]}
 */
function formatOptions(options) {
  const entries = Object.entries(options).map(([name, spec]) => {
    const flags = [spec.short ? `-${spec.short}` : null, `--${name}`].filter(Boolean).join(', ');
    const value = spec.type === 'string' ? ` ${spec.placeholder ?? '<value>'}` : '';
    return [`${flags}${value}`, spec.describe];
  });
  const width = entries.reduce((max, [flags]) => Math.max(max, flags.length), 0);
  return entries.map(([flags, describe]) => `  ${flags.padEnd(width)}  ${describe}`);
}

/**
 * @param {string} binName
 * @param {Command[]} commands
 * @returns {string}
 */
export function formatOverviewHelp(binName, commands) {
  const width = commands.reduce((max, c) => Math.max(max, c.name.length), 0);
  return [
    `Usage: ${binName} <command> [options]`,
    '',
    'Commands:',
    ...commands.map((c) => `  ${c.name.padEnd(width)}  ${c.summary}`),
    '',
    `Run \`${binName} <command> --help\` for a command's options.`,
  ].join('\n');
}

/**
 * @param {string} binName
 * @param {Command} command
 * @returns {string}
 */
export function formatCommandHelp(binName, command) {
  const usage = command.usage ?? [`${binName} ${command.name} [options]`];
  return [
    command.summary,
    '',
    'Usage:',
    ...usage.map((line) => `  ${line}`),
    '',
    'Options:',
    ...formatOptions({ ...(command.options ?? {}), ...GLOBAL_OPTIONS }),
    ...(command.details?.length ? ['', ...command.details] : []),
  ].join('\n');
}

/**
 * Parse `argv` for one command. Split out from `run` so tests can check flag
 * handling without executing anything.
 *
 * @param {Command} command
 * @param {string[]} argv
 * @returns {{ values: ParsedValues, positionals: string[] }}
 */
export function parseCommandArgs(command, argv) {
  try {
    return parseArgs({
      args: argv,
      options: toParseArgsOptions({ ...(command.options ?? {}), ...GLOBAL_OPTIONS }),
      strict: true,
      allowPositionals: command.allowPositionals ?? false,
    });
  } catch (error) {
    throw new UsageError(
      error instanceof Error ? error.message : String(error),
      formatCommandHelp('repo', command),
    );
  }
}

/**
 * Dispatch `argv` to one of `commands`.
 *
 * @param {object} options
 * @param {string} options.binName Name to print in usage lines.
 * @param {Command[]} options.commands
 * @param {string[]} options.argv Arguments after the script name.
 * @param {import('./repo.mjs').Repo} options.repo
 * @param {(message: string) => void} [options.write] Defaults to stdout.
 * @returns {Promise<number>} Process exit code.
 */
export async function run({ binName, commands, argv, repo, write = (m) => console.log(m) }) {
  const [name, ...rest] = argv;

  if (name === undefined || name === '--help' || name === '-h' || name === 'help') {
    write(formatOverviewHelp(binName, commands));
    return 0;
  }

  const command = commands.find((c) => c.name === name);
  if (!command) {
    throw new UsageError(
      `Unknown command '${name}'.`,
      formatOverviewHelp(binName, commands),
    );
  }

  const { values, positionals } = parseCommandArgs(command, rest);
  if (values.help) {
    write(formatCommandHelp(binName, command));
    return 0;
  }

  return (await command.run({ values, positionals, repo, write })) ?? 0;
}
