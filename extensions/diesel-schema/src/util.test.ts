import type * as vscode from 'vscode';
import { describe, expect, it } from 'vitest';
import { isDieselSchema } from './util';

/** Only the two members `isDieselSchema` actually touches. */
function doc(languageId: string, text: string): vscode.TextDocument {
  return { languageId, getText: () => text } as unknown as vscode.TextDocument;
}

describe('isDieselSchema', () => {
  it('accepts the fully qualified macro', () => {
    expect(isDieselSchema(doc('rust', 'diesel::table! {\n    users (id) {}\n}'))).toBe(true);
  });

  it('accepts the imported `table!` form with either brace style', () => {
    expect(isDieselSchema(doc('rust', 'use diesel::table;\n\ntable! {\n    users (id) {}\n}'))).toBe(
      true,
    );
    expect(isDieselSchema(doc('rust', 'table!(users (id) {});'))).toBe(true);
  });

  it('rejects non-Rust documents even when they contain the macro', () => {
    expect(isDieselSchema(doc('markdown', 'diesel::table! {\n    users (id) {}\n}'))).toBe(false);
  });

  it('rejects Rust files without a table macro', () => {
    expect(isDieselSchema(doc('rust', 'fn main() {\n    println!("hi");\n}'))).toBe(false);
  });

  it('does not match identifiers that merely end in `table!`', () => {
    expect(isDieselSchema(doc('rust', 'my_table!(x);'))).toBe(false);
    expect(isDieselSchema(doc('rust', 'let t = table;'))).toBe(false);
  });
});
