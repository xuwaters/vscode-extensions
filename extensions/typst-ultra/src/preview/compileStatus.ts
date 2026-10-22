/**
 * The server's `typst/compileStatus` notification.
 *
 * Both preview surfaces listen for it, so both read it from here. Pure, so it
 * can be tested without VSCode.
 */

/** A `typst/compileStatus` notification, checked. */
export interface CompileStatus {
  state: 'compiling' | 'ok' | 'error';
  /** Pages in the last good document — `0` when it has never compiled. */
  pageCount?: number;
  /** The document compiled — the compile root, not necessarily the one edited. */
  uri?: string;
  /** With `error`: the compiler's first error, and where it is. */
  message?: string;
  location?: string;
}

/** Read a `typst/compileStatus` notification, or `undefined` if it is not one. */
export function parseCompileStatus(params: unknown): CompileStatus | undefined {
  if (typeof params !== 'object' || params === null) return undefined;
  const raw = params as Record<string, unknown>;
  const { state } = raw;
  if (state !== 'compiling' && state !== 'ok' && state !== 'error') return undefined;
  const text = (value: unknown) => (typeof value === 'string' ? value : undefined);
  return {
    state,
    pageCount: typeof raw.pageCount === 'number' ? raw.pageCount : undefined,
    uri: text(raw.uri),
    message: text(raw.message),
    location: text(raw.location),
  };
}
