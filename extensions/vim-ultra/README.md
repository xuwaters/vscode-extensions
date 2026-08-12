# Vim Ultra

Modal Vim editing for VSCode, powered by a from-scratch Vim engine written in
Rust (`crates/vim-engine`) and compiled to WASM. The TypeScript side is a thin
host: it forwards keys, applies the engine's effects (edits, selections,
commands), and mirrors external document changes back into the engine.

## Supported today

- **Modes**: normal, insert, visual, visual line. Status bar shows the mode
  and pending keys; the cursor is a block outside insert mode.
- **Motions**: `h j k l`, `0 ^ $`, `w W b B e E`, `gg G`, `{ }`, `%`,
  `f F t T` with `;`/`,`, `enter + -`, all with counts (`3w`, `2f,`).
- **Operators**: `d c y > <` with any motion (`d2w`, `ci"`, `y$`, `>j`),
  doubled for lines (`dd`, `3yy`, `cc`, `>>`), and on visual selections.
- **Text objects**: `iw aw iW aW`, `i" a" i' a' i\` a\``,
  `i( a( i[ a[ i{ a{ i< a<` (aliases `b`/`B`).
- **Actions**: `x X s S D C Y r ~ J p P o O`, `u` / `ctrl+r` (delegates to
  VSCode undo/redo), `zz zt zb`, `ctrl+d/u/f/b` scrolling.
- **Registers**: the unnamed register, charwise and linewise, with counted
  pastes.
- Mouse clicks and drags work: a drag enters visual mode.

## Not yet implemented

Search (`/ ? n N * #`), ex commands (`:`), marks, macros, dot-repeat,
named registers, replace mode (`R`), block visual (`ctrl+v`), jumplist,
and multi-cursor integration (with multiple cursors the extension steps
aside and lets VSCode behave natively).

## Development

```sh
pnpm run build:wasm   # wasm-pack build of crates/vim-engine into wasm/
pnpm run build        # tsdown bundle into dist/
pnpm run test         # vitest (host helpers)
cargo test -p vim-engine   # the engine's real test suite
```

The engine keeps a line-based mirror of the document (UTF-16 columns, so
positions round-trip with the VSCode API). Every edit the engine emits is
self-applied to its mirror and applied to the document under a suppression
flag; all other document changes flow back via `apply_changes`. If an edit is
rejected (readonly file), the mirror is rebuilt from the document.
