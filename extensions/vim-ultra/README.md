# Vim Ultra

Modal Vim editing for VSCode, powered by a from-scratch Vim engine written in
Rust (`crates/vim-engine`) and compiled to WASM. The TypeScript side is a thin
host: it forwards keys, applies the engine's effects (edits, selections,
commands), and mirrors external document changes back into the engine.

## Supported today

- **Modes**: normal, insert, visual, visual line. Status bar shows the mode
  and pending keys; the cursor is a block outside insert mode.
- **Motions**: `h j k l`, `0 ^ $`, `w W b B e E`, `gg G`, `{ }`, `%`,
  `[{ [( ]} ])` to the brace or paren that opens or closes the block the
  cursor is in, `f F t T` with `;`/`,`, `enter + -`, all with counts (`3w`,
  `2f,`, `2]}` for two blocks out). Bracket matching follows vim's: a brace
  inside a `"…"` string, a `'}'` literal or an escaped `\}` is not the
  block's edge.
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
  break takes their place, however many the count asked for. `o`/`O` open
  the new line through VSCode's own line-insert — the same thing `enter` in
  insert mode runs — so it arrives indented by the language's rules rather
  than at column 0.
- **EasyMotion jumps**: `<space><space>` — the leader twice — labels every
  place the motion after it could land, anywhere on screen, and one or two
  keystrokes go there. `w W b B` for word starts, `e E ge gE` for word ends,
  `j k` for lines, `f{char}` `F{char}` `t{char}` `T{char}` the way the
  motions themselves work, `s{char}` looking both ways at once,
  `2s{char}{char}` (any count) for that many characters, and `/{chars}⏎` for
  as many as you care to type. The nearest targets get one-key labels and the
  rest two, the screen behind them dims, and the label you have typed narrows
  what is left. It is a motion like any other, so `d<space><space>w` deletes
  up to the target you pick and a jump in visual mode extends the selection.
  Escape, or any key that spells no label, calls it off. With the leader on
  the space bar a lone `<space>` still moves one character right — it runs as
  soon as the next key turns out not to open a jump, which is what Vim does
  with an ambiguous mapping.
- **Editor integration**: `gh` pops the hover for the symbol under the
  cursor — its type and docs — like vscode-vim's binding of the same name.
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
- Mouse clicks and drags work: a drag enters visual mode, and so does
  `shift+arrow` from normal mode. A selection a *command* leaves behind does
  not: the match still highlighted after `cmd+f`, or the placeholder a
  completion lands on, leaves the mode alone — the next key moves the cursor
  or types over the text, rather than dragging a visual selection along.
  Insert mode is left by `escape` and nothing else.

## Settings

- `vimUltra.enabled` (default `true`) — modal editing on at startup;
  `Vim Ultra: Toggle Vim Mode` flips it for the session.
- `vimUltra.escapeAlwaysExitsInsert` (default `true`) — `escape` leaves insert
  mode even when a popup would otherwise eat it: with the suggestion widget, the
  parameter hints tooltip or an inline suggestion (Copilot ghost text) showing,
  it dismisses them *and* drops you into normal mode, rather than spending the
  key on the popup and leaving you still typing. Turn it off for VSCode's
  two-press behaviour.
- `vimUltra.leader` (default `<space>`) — the leader key, written the way a
  Vim mapping is (`<space>`, `,`, `\`).
- `vimUltra.easyMotion.enabled` (default `true`), `.trigger` (default
  `<leader><leader>`), `.markerKeys` (default `asdghklqwertyuiopzxcvbnmfj;` —
  vim-easymotion's own, home row first; more keys means fewer two-key labels)
  and `.dimBackground` (default `true`).

## Not yet implemented

Marks, macros, dot-repeat, named registers, replace mode (`R`), block visual
(`ctrl+v` — the column editing it is for is what multiple cursors do here)
and the jumplist. `:s` is the only ex command: no `:w`, `:g`, `:sort`, … —
an unknown one says so in the status bar rather than doing something
surprising.

EasyMotion labels only the lines on screen, matches the characters of a
`f`/`s`/`/` jump literally and case-sensitively (no regexp, no smartcase),
and stays out of the way while several cursors are up — a jump is one
cursor's choice of one landing place. Targets past what two label keys can
spell are dropped rather than left unreachable; with the default 27 keys that
is 729 of them, well past a screenful.

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
