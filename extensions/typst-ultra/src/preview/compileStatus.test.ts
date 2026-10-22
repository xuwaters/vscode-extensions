import { describe, expect, it } from 'vitest';
import { parseCompileStatus } from './compileStatus.js';

describe('parseCompileStatus', () => {
  it('reads a failure with the error and where it is', () => {
    expect(
      parseCompileStatus({
        state: 'error',
        pageCount: 0,
        uri: 'file:///p/main.typ',
        message: 'unknown variable: tab',
        location: 'main.typ:3:2',
      }),
    ).toEqual({
      state: 'error',
      pageCount: 0,
      uri: 'file:///p/main.typ',
      message: 'unknown variable: tab',
      location: 'main.typ:3:2',
    });
  });

  /** An older server sends the state alone; the preview must still work with it. */
  it('leaves out what the server did not send', () => {
    expect(parseCompileStatus({ state: 'compiling' })).toEqual({
      state: 'compiling',
      pageCount: undefined,
      uri: undefined,
      message: undefined,
      location: undefined,
    });
  });

  it('drops fields of the wrong type rather than passing them on', () => {
    const status = parseCompileStatus({ state: 'ok', pageCount: '3', uri: null });
    expect(status?.pageCount).toBeUndefined();
    expect(status?.uri).toBeUndefined();
  });

  it('refuses anything that is not a status', () => {
    expect(parseCompileStatus(null)).toBeUndefined();
    expect(parseCompileStatus('ok')).toBeUndefined();
    expect(parseCompileStatus({ state: 'done' })).toBeUndefined();
  });
});
