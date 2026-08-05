#!/usr/bin/env node
/**
 * A stand-in for the Claude Code CLI, speaking just enough of the stream-JSON
 * control protocol for `cli.test.ts`. The behaviour is picked with STUB_MODE:
 *
 *   ok      answer initialize, then get_usage with a usage payload
 *   noise   the same, but interleaved with the debug lines the real CLI prints
 *   error   answer initialize, then fail get_usage
 *   silent  answer initialize and nothing else
 *   exit    quit before answering anything
 *
 * The usage payload echoes the arguments it was launched with, so a test can
 * assert the CLI flags without a second fixture.
 */
const mode = process.env.STUB_MODE ?? 'ok';

if (mode === 'exit') process.exit(3);

const write = (value) => process.stdout.write(`${JSON.stringify(value)}\n`);

const success = (requestId, response) =>
  write({ type: 'control_response', response: { subtype: 'success', request_id: requestId, response } });

let buffer = '';
process.stdin.setEncoding('utf8');
process.stdin.on('data', (chunk) => {
  buffer += chunk;
  let newline;
  while ((newline = buffer.indexOf('\n')) >= 0) {
    const line = buffer.slice(0, newline);
    buffer = buffer.slice(newline + 1);
    if (line.trim()) handle(JSON.parse(line));
  }
});

function handle(message) {
  if (message.type !== 'control_request') return;
  const { request_id: requestId, request } = message;

  if (request.subtype === 'initialize') {
    if (mode === 'noise') {
      process.stdout.write('[DEBUG] starting up\n');
      write({ type: 'system', subtype: 'init' });
    }
    success(requestId, { commands: [] });
    return;
  }

  if (request.subtype !== 'get_usage') return;
  if (mode === 'silent') return;

  if (mode === 'error') {
    write({
      type: 'control_response',
      response: { subtype: 'error', request_id: requestId, error: 'usage unavailable' },
    });
    return;
  }

  if (mode === 'noise') process.stdout.write('[DEBUG] fetchUtilization: GET /api/oauth/usage\n');

  success(requestId, {
    subscription_type: 'max',
    rate_limits_available: true,
    rate_limits: {
      limits: [{ kind: 'session', group: 'session', percent: 7, severity: 'normal' }],
    },
    argv: process.argv.slice(2),
    cwd: process.cwd(),
  });
}
