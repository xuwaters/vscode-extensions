import type * as vscode from 'vscode';
import { describe, expect, it } from 'vitest';
import type { Dialect } from '../csv/dialect.js';
import { parse } from '../csv/parse.js';
import { columnAt, recordIndexAt, TableCache } from './table.js';

const comma: Dialect = { delimiter: ',', quote: '"', newline: '\n' };

/**
 * Just enough `TextDocument` for the cache: what it reads is the URI, the
 * version and the text. Nothing here touches VSCode itself, which is what keeps
 * the parse cache — the hottest code in the extension — testable at all.
 */
function fakeDocument(text: string, version = 1, path = '/a.csv'): vscode.TextDocument {
  return {
    uri: { toString: () => path } as vscode.Uri,
    version,
    getText: () => text,
  } as vscode.TextDocument;
}

describe('which record an offset is in', () => {
  const table = parse('aa,bb\ncc,dd\nee,ff\n', comma);

  it('finds the record containing each offset', () => {
    expect(recordIndexAt(table, 0)).toBe(0);
    expect(recordIndexAt(table, 4)).toBe(0);
    expect(recordIndexAt(table, 6)).toBe(1);
    expect(recordIndexAt(table, 12)).toBe(2);
  });

  it('counts a line terminator as belonging to the record before it', () => {
    expect(recordIndexAt(table, 5)).toBe(0);
  });

  it('has no answer before the first record', () => {
    expect(recordIndexAt(parse('﻿a\n', comma), 0)).toBe(-1);
    expect(recordIndexAt(parse('', comma), 0)).toBe(-1);
  });

  it('gives the last record for an offset past the end', () => {
    expect(recordIndexAt(table, 9999)).toBe(2);
  });
});

describe('which column an offset is in, within a record', () => {
  it('counts the delimiters before it', () => {
    expect(columnAt('a,b,c', 0, comma)).toBe(0);
    expect(columnAt('a,b,c', 2, comma)).toBe(1);
    expect(columnAt('a,b,c', 4, comma)).toBe(2);
  });

  it('does not count a delimiter inside quotes', () => {
    // `"x,y",z` — the cursor at the `z` is in column 1, not column 2.
    expect(columnAt('"x,y",z', 6, comma)).toBe(1);
    expect(columnAt('"x,y",z', 2, comma)).toBe(0);
  });

  it('reads a doubled quote as one character of the value', () => {
    expect(columnAt('"a""b",z', 7, comma)).toBe(1);
  });

  it('clamps an offset past the end of the record', () => {
    expect(columnAt('a,b', 999, comma)).toBe(1);
  });
});

describe('the parse cache', () => {
  it('parses once for a version, and again when it changes', () => {
    const cache = new TableCache();
    let reads = 0;
    const document = {
      uri: { toString: () => '/a.csv' } as vscode.Uri,
      version: 1,
      getText: () => {
        reads += 1;
        return 'a,b\n';
      },
    } as unknown as { uri: vscode.Uri; version: number } & vscode.TextDocument;

    cache.of(document, comma);
    cache.of(document, comma);
    expect(reads).toBe(1);

    document.version = 2;
    cache.of(document, comma);
    expect(reads).toBe(2);
  });

  it('parses again when the delimiter changes', () => {
    const cache = new TableCache();
    const document = fakeDocument('a\tb\n');
    expect(cache.of(document, comma).table.columns).toBe(1);
    expect(cache.of(document, { ...comma, delimiter: '\t' }).table.columns).toBe(2);
  });

  it('reports the length the edits append at', () => {
    expect(new TableCache().of(fakeDocument('a,b\n'), comma).length).toBe(4);
  });

  it('forgets a document on request', () => {
    const cache = new TableCache();
    let reads = 0;
    const document = {
      uri: { toString: () => '/a.csv' } as vscode.Uri,
      version: 1,
      getText: () => {
        reads += 1;
        return 'a\n';
      },
    } as unknown as vscode.TextDocument;
    cache.of(document, comma);
    cache.forget(document.uri);
    cache.of(document, comma);
    expect(reads).toBe(2);
  });

  it('holds only a handful of documents', () => {
    const cache = new TableCache(2);
    for (const path of ['/a', '/b', '/c']) cache.of(fakeDocument('x\n', 1, path), comma);
    let reads = 0;
    const first = {
      uri: { toString: () => '/a' } as vscode.Uri,
      version: 1,
      getText: () => {
        reads += 1;
        return 'x\n';
      },
    } as unknown as vscode.TextDocument;
    cache.of(first, comma);
    expect(reads).toBe(1);
  });
});
