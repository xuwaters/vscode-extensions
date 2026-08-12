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
- **Search**: `/` and `?` (the pattern shows in the status bar; `backspace`
  edits it, `escape` aborts, `enter` runs it), `n`/`N` to repeat, `*`/`#` for
  the word under the cursor. Searches wrap around the buffer, take counts
  (`3n`), an empty pattern repeats the last one, and — being exclusive
  motions — they combine with operators (`d/foo⏎`, `dn`, `y*`).
- **Regular expressions** in every pattern: `. * \+ \? \{n,m}` (and the
  non-greedy `\{-}`), `\(…\)` groups with `\%(…\)` non-capturing, `\|`
  alternation, `[a-z]` and `[^…]` collections with `[:alpha:]` names, the
  classes `\w \W \s \S \d \D \a \l \u \h \x \o` (and negations), `^ $ \< \>`
  anchors, `\zs`/`\ze` to trim the match, `\c`/`\C` for case, and the magic
  levels `\v \m \M \V`.
- **Search and replace**: `:[range]s/pattern/replacement/flags`, with ranges
  `%`, `5`, `1,$`, `.,+3`, `'<,'>` (typing `:` in visual mode fills that in)
  and flags `g i I n e`. Replacements take `&` / `\0` for the whole match,
  `\1`…`\9` for groups, `~` for the previous replacement, `\r` for a line
  break, and `\u \l \U \L \E` for case. `&` repeats the last substitution on
  the current line, as does a bare `:s`, and `:12` jumps to a line. The
  status bar reports the result (`3 substitutions on 2 lines`) and any error.
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

Marks, macros, dot-repeat, named registers, replace mode (`R`), block visual
(`ctrl+v`), jumplist, and multi-cursor integration (with multiple cursors the
extension steps aside and lets VSCode behave natively). `:s` is the only ex
command: no `:w`, `:g`, `:sort`, … — an unknown one says so in the status bar
rather than doing something surprising.

Pattern support stops at look-around (`\@=` and friends), `\&`, `~` for the
previous substitute pattern, and multi-line matching: a match never spans a
line break, so `\n` in a pattern matches nothing. Matching is case-sensitive
unless the pattern says otherwise (`ignorecase` and `smartcase` are not
implemented), and neither `hlsearch` nor `incsearch` is either — the cursor
simply lands on the match. The `c` (confirm) flag on `:s` is rejected instead
of silently replacing without asking. In a replacement, `\n` breaks the line
like `\r` does, rather than inserting Vim's NUL.

A pathological pattern (`\(a*\)*b` and relatives) gives up after a fixed step
budget and reports no match, so it can never hang the editor.

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
