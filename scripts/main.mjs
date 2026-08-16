#!/usr/bin/env node
// Entry point for the repo's maintenance commands. Add a command by writing a
// module under commands/ and listing it here.

import { UsageError, run } from './lib/cli.mjs';
import { createRepo } from './lib/repo.mjs';
import { bumpCommand } from './commands/bump.mjs';
import { syncVscodeignoreCommand } from './commands/sync-vscodeignore.mjs';

/** @type {import('./lib/cli.mjs').Command[]} */
export const COMMANDS = [bumpCommand, syncVscodeignoreCommand];

try {
  process.exitCode = await run({
    binName: 'repo',
    commands: COMMANDS,
    argv: process.argv.slice(2),
    repo: createRepo(),
  });
} catch (error) {
  if (error instanceof UsageError) {
    console.error(error.message);
    if (error.help) console.error(`\n${error.help}`);
  } else {
    console.error(error);
  }
  process.exitCode = 1;
}
