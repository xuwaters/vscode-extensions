#!/usr/bin/env node
// Entry point for this extension's packaging commands. Add one by writing a
// module under commands/ and listing it here.
//
// Dispatch, help text, and flag parsing come from the repo's script framework
// (scripts/lib/cli.mjs) rather than a copy of it; what is specific to this
// extension is the context those commands receive — a `Layout` instead of the
// monorepo's `Repo`.

import { UsageError, run } from '../../../scripts/lib/cli.mjs';
import { assembleTspluginCommand } from './commands/assemble-tsplugin.mjs';
import { injectTspluginCommand } from './commands/inject-tsplugin.mjs';
import { verifyVsixCommand } from './commands/verify-vsix.mjs';
import { createLayout } from './lib/layout.mjs';

/**
 * @type {import('../../../scripts/lib/cli.mjs').Command<{
 *   layout: import('./lib/layout.mjs').Layout,
 * }>[]}
 */
export const COMMANDS = [assembleTspluginCommand, injectTspluginCommand, verifyVsixCommand];

try {
  process.exitCode = await run({
    binName: 'fast-element',
    commands: COMMANDS,
    argv: process.argv.slice(2),
    context: { layout: createLayout() },
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
