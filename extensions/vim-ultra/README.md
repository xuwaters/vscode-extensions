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
- **Search**: `/` and `?` (the pattern replaces the mode text in the status
  bar while you type it; `backspace` edits it, `escape` aborts, `enter` runs
  it), `n`/`N` to repeat, `*`/`#` for the word under the cursor. Searches
  wrap around the buffer, take counts (`3n`), an empty pattern repeats the
  last one, and — being exclusive motions — they combine with operators
  (`d/foo⏎`, `dn`, `y*`). Typing a pattern highlights its matches and peeks
  at the one `enter` would land on (vim's `incsearch`): the view scrolls
  there without moving the cursor, comes back if you `escape`, and stays if
  you commit. A landed search reports `match 3 of 11` in the status bar.
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
  VSCode undo/redo), `zz zt zb`, `ctrl+d/u/f/b` scrolling. `r<CR>` is vim's
  line-splitting special case: the characters go away and a single line
  break takes their place, however many the count asked for.
- **Registers**: the unnamed register, charwise and linewise, with counted
  pastes.
- **Multiple cursors**: make them the VSCode way — `cmd+alt+up/down` for a
  column, `cmd+d` for the next occurrence, alt-click — and every key runs at
  all of them. `I` opens insert mode at each line's first non-blank, `A` at
  each end, `i a o O` where you'd expect; motions, operators, text objects,
  `x`, `p` and `r` all work per cursor, and the status bar counts them.
  Escape leaves insert mode with the cursors intact; escape again drops back
  to one. Selections made with `cmd+d` come in as visual mode at every
  cursor, so `c` changes each occurrence. What is shared is the mode, the
  register (`p` pastes the same text everywhere) and the search history;
  what runs once is `:` commands and undo, which already cover the document.
- Mouse clicks and drags work: a drag enters visual mode.

## Not yet implemented

Marks, macros, dot-repeat, named registers, replace mode (`R`), block visual
(`ctrl+v` — the column editing it is for is what multiple cursors do here)
and the jumplist. `:s` is the only ex command: no `:w`, `:g`, `:sort`, … —
an unknown one says so in the status bar rather than doing something
surprising.

Multiple cursors edit one document, so two of them reaching for the same
text (`dd` on adjoining lines, `d/foo⏎` across a neighbour) is a conflict:
the editor rejects the whole transaction, the key does nothing, and the
engine rebuilds its mirror from the document rather than guessing. Each
cursor also reads the buffer as the cursors below it have already left it,
so a motion that runs *down* past another cursor's edit sees the new text.

Pattern support stops at look-around (`\@=` and friends), `\&`, `~` for the
previous substitute pattern, and multi-line matching: a match never spans a
line break, so `\n` in a pattern matches nothing. Matching is case-sensitive
unless the pattern says otherwise (`ignorecase` and `smartcase` are not
implemented). Match highlights show while a pattern is being typed
(`incsearch`) but clear once it runs — there is no persistent `hlsearch`.
The `c` (confirm) flag on `:s` is rejected instead of silently replacing
without asking. In a replacement, `\n` breaks the line like `\r` does,
rather than inserting Vim's NUL.

Searching is bounded, not linear-time: patterns run on a backtracking VM, so
a catastrophic one (`\%(\w*\)*\d\{9}` and relatives) is stopped by a step
budget sized to the buffer — a tenth of a second or so — and reported as
`gave up: pattern is too slow to run here` rather than passed off as "not
found" or allowed to freeze the editor. Ordinary patterns are nowhere near
it: most lines are rejected by a required-literal test before matching starts,
which puts a whole-buffer search over a 1 MB file at well under a millisecond.
See `crates/vim-engine/tests/perf.rs` for the measurements.

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
