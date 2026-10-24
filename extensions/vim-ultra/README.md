# Vim Ultra

Modal Vim editing for VSCode, powered by a from-scratch Vim engine written in
Rust (`crates/vim-engine`) and compiled to WASM. The TypeScript side is a thin
host: it forwards keys, applies the engine's effects (edits, selections,
commands), and mirrors external document changes back into the engine.

## Supported today

- **Modes**: normal, insert, visual, visual line. Status bar shows the mode
  and pending keys; the cursor is a block outside insert mode.
- **Motions**: `h j k l`, `0 ^ $`, `_` (first non-blank, `count - 1` lines
  down), `g_` (last non-blank), `|` (to a column: `12|`), `w W b B e E`,
  `ge gE` (back to the end of a word), `gg G`, `H M L` (top, middle and
  bottom of the screen; `3H` is three lines in), `{ }`, `( )`, `%`,
  `[{ [( ]} ])` to the brace or paren that opens or closes the block the
  cursor is in, `f F t T` with `;`/`,`, `enter + -`, all with counts (`3w`,
  `2f,`, `2]}` for two blocks out), all usable after an operator (`d_` is
  `dd`, `dge`, `yL`) and in visual mode. Bracket matching follows vim's: a
  brace inside a `"…"` string, a `'}'` literal or an escaped `\}` is not
  the block's edge. `(` and `)` go by vim's sentences: one ends at a `.`,
  `!` or `?` (closing `)`, `]`, `"` or `'` may follow) before a space, a tab
  or the end of the line, and an empty line is a sentence by itself. With an
  operator they stop short of the line break, so `d)` on a line's last
  sentence leaves the next line where it is.
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
- **Line commands**, on the same ranges (plus `;`, which counts the second
  address from the first: `:5;+2`, and marks: `:'a,'bd`):
  `:d[elete] [x]` and `:y[ank] [x]` into the registers `dd` and `yy` use, or
  register `x` too (`:d _` throws the lines away; `:y A` appends; `:d 3`
  takes a count); `:pu[t] [x]` puts a register below the line as lines of
  its own, `:pu!` above it and `:0pu` above the first line; `:m[ove] {address}` and
  `:t` / `:co[py] {address}` — `:m0` to the top, `:m$` to the bottom, `:m+1`
  down a line, `:t.` to duplicate; `:j[oin]`, with `:j!` keeping the white
  space; `:>` and `:<` through VSCode's own indent, so your tab settings
  apply (`:>>` shifts twice); and `:sor[t]`, with `!` to reverse, `i` to
  ignore case, `n` to sort by the first number, `u` to drop duplicates, and
  `/pattern/` to sort on what follows a match (with `r`, on the match). Like
  Vim, a command over more than two lines says how many (`5 fewer lines`).
- **`:g[lobal]/pattern/cmd`** and **`:v[global]`** (or `:g!`) run any of the
  line commands — `:s` too — on every line that matches, or every line that
  doesn't: `:g/TODO/d`, `:v/\S/d`, `:g/^/m0` to reverse the buffer,
  `:g/log/s/foo/bar/g`, nested as `:g/a/v/b/d`. They work the way Vim's do,
  so lines that move or merge along the way are still visited exactly once,
  and the whole run is a single undo step.
- **Files**: `:w`, `:wq`, `:x` and `ZZ` (write only if modified, then close),
  `:q`, `:q!` and `ZQ`, `:up[date]`, `:wa`, `:qa`, `:qa!`, `:wqa` / `:xa`,
  and `:e!` to read the file back from disk. Writing is VSCode's own save,
  so format-on-save and the other save hooks run, and the status bar
  answers the way Vim does: `"src/a.ts" 12L, 340B written`. `:q` won't throw
  away unsaved changes (`E37: No write since last change (add ! to
  override)`) unless another editor still shows the document, and `:qa`
  won't close anything while any document has them: it switches to the
  first one (`E162`). `:qa` closes every editor rather than quitting VSCode.
- Command names abbreviate the way Vim's do — `:wr`, `:sor`, `:co`, `:noh` —
  and anything else says `E492: Not an editor command`. `:noh` is accepted
  and does nothing, since match highlights never outlive the search prompt.
- **Operators**: `d c y > <` with any motion (`d2w`, `ci"`, `y$`, `>j`),
  doubled for lines (`dd`, `3yy`, `cc`, `>>`), and on visual selections.
  The case operators `gu` (lower), `gU` (upper) and `g~` (toggle) work the
  same way — `gUiw`, `gu$`, `g~j` — doubled as `guu`/`gugu`, `gUU`/`gUgU`
  and `g~~`/`g~g~`; in visual mode `u` and `U` lower- and uppercase the
  selection, beside `~`.
- **Text objects**: `iw aw iW aW`, `is as` (sentences), `ip ap`
  (paragraphs), `i" a" i' a' i\` a\``, `i( a( i[ a[ i{ a{ i< a<` (aliases
  `b`/`B`) and `it at` (HTML/XML tags), all chosen the way vim chooses them.
  A count takes more of them — `d2aw` two words, `2i(` the block around the
  one you're in, `2it` the enclosing tag. `a` objects bring the blanks after
  them, or the ones before when there are none (`daw` on a line's last word,
  `das` on its last sentence). `ip`/`ap` work on whole lines: `dap` deletes a
  paragraph and the blank lines after it, and lines of nothing but spaces
  count as blank. `i{` on a block whose braces sit on their own lines takes
  just the lines between, so `di{` leaves `{` and `}` touching. Brackets
  work from either bracket, and outside any block `i(` uses the next one
  ahead. Quotes stay on the line, pair up from its start when the cursor is
  on one, skip `\"`, and `2i"` takes the quotes too. Tags ignore case, skip
  `<br/>` and a `<br>` with no end tag, and work from the tag itself. In
  visual mode an object selects (`vip` turns it linewise), and repeating it
  grows the selection — `vawaw`, `vipip`, `vi(i(` one block out, `vitit` the
  tags and then the enclosing block.
- **Actions**: `x X s S D C Y r ~ J p P o O`, `u` / `ctrl+r` (delegates to
  VSCode undo/redo), `zz zt zb`, `ctrl+d/u/f/b` scrolling. `r<CR>` is vim's
  line-splitting special case: the characters go away and a single line
  break takes their place, however many the count asked for. `o`/`O` open
  the new line through VSCode's own line-insert — the same thing `enter` in
  insert mode runs — so it arrives indented by the language's rules rather
  than at column 0.
- **More actions**: `gJ` joins lines as they are, without adding or
  removing spaces. `gv` brings back the last visual selection (and, from
  visual mode, swaps it with the current one). `gi` resumes inserting where
  you last left insert mode, `gI` inserts at column 0. `gp`/`gP` paste like
  `p`/`P` but leave the cursor just after the pasted text. `ctrl+a`/`ctrl+x`
  add to or subtract from the number under or after the cursor — with a
  count, negative decimals, `0x1f` hex and `0b101` binary kept in their own
  base, case and width (`007` becomes `008`); in visual mode they change the
  first number on every selected line, and `g ctrl+a` counts up line by line
  (`1 1 1` becomes `2 3 4`). `ctrl+e`/`ctrl+y` scroll the view a line (or a
  count of lines) without moving the text under the cursor.
- **Repeat**: `.` does the last change again at the cursor — an operator
  with its motion or text object (`dw`, `ciw` and the word typed in its
  place, `>j`, `gUiw`), the edits that stand alone (`x X s S D C r J gJ p P
  ~ & ctrl+a ctrl+x`), and the inserts with what they typed (`i a I A o O gi
  gI`, `A;` then `j.` down a column of lines). A count given to `.` replaces
  the first one, for the repeats after it too (`d2w` then `3.` deletes three
  words); the register stays the one named, except that `"1`–`"8` step on to
  the next, so `"1p..` puts the last three deletes. A change made in visual
  mode repeats over as much text from the cursor: as many lines, or as many
  characters within a line, or up to the same column on the last of several
  lines. Motions, yanks, searches, undo and `:` commands are not changes and
  leave what `.` repeats alone. With several cursors `.` repeats at every
  one. The change and the text typed after it reach the document as one
  edit, so one `u` undoes the repeat. `o`/`O` are repeated through VSCode's
  line-insert again, so the repeated line is indented as the first one was.
- **Counted inserts**: `3ix` then `escape` puts `xxx`; `3o` opens three
  lines with the text typed on each, indented like the first.
- **Macros**: `q{a-z}` starts recording into a register (`q{A-Z}` appends to
  it, `q{0-9}` and `q"` work too), and the status bar says `recording @a`
  until `q` stops it. `@a` runs it, `3@a` three times, `@@` runs the last
  register run again and `@:` the last `:` command line. What you type in
  insert mode goes into the recording as the text it left in the document —
  a backspace, an accepted completion or a bracket VSCode closed for you is
  recorded as what it did — and comes back as that text when the macro runs,
  without auto-indent or auto-closing having another go at it. The register
  reads in Vim's key notation (`ciwfoo<esc>j`), so `:reg a` shows what was
  recorded and `"ap` puts it as text you can edit and yank back. A key that
  fails stops the macro and anything still queued behind it, as in Vim —
  a motion with nowhere to go (`j` on the last line, `h` in the first
  column, `f` finding nothing), a search with no match, an error — which is
  what ends a recursive macro (`qaq`, then `qa…@aq`). A macro runs at every
  cursor when there are several. `escape` stops one that is running, and
  keys typed meanwhile wait until it has.
- **Visual `I` and `A`** insert once, at the selection's edge, as Vim's do
  outside block mode: `I` at the start of the selection's first line (at the
  cursor, if you selected upward), `A` after the last selected character.
  Typing at every line is block visual's job, which multiple cursors do
  here.
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
  cursor — its type and docs — and `gd` goes to its definition, like
  vscode-vim's bindings of the same names.
- **Registers**: `"x` in front of any yank, delete, change or put — before
  or after a count (`"a3yy`, `3"ayy`), in normal and visual mode, for
  `x X s S D C Y` as much as `d c y p P gp gP`:
  - `"a`–`"z`, and `"A`–`"Z` to append to them — lines after characters
    make the register linewise, as in Vim.
  - `"0` holds the last yank; `"1`–`"9` the last nine deletes of whole
    lines or across lines, shifting down with each one; `"-` the last
    delete within a line. Deletes with `%`, `(`, `)`, `` ` ``, `/`, `?`,
    `n`, `N`, `{` and `}` always go to `"1` too, as Vim's do. `""` is
    whichever register was written last.
  - `"_` is the black hole: `"_dd` deletes without touching any register.
    Visual `p` puts the register over the selection and files the selection
    the way `d` would; visual `P` leaves every register as it was.
  - Read-only `".` (the text the last insert typed), `":` (the last command
    line) and `"/` (the last search pattern): put them, or name them for
    `:pu`. A yank or delete into one does nothing.
  - `"+` and `"*` are the system clipboard: a yank or delete into them copies
    to it (lines with a final line break, so other programs — and Vim — see
    lines), and a put reads whatever is on it right then, from this or any
    other program. The unnamed register stays separate from the clipboard,
    as in Vim without `clipboard=unnamed`.
  - The registers belong to the editor, not the document: `"ayy` in one
    file, `"ap` in another.
  - `:reg[isters]` and `:di[splay]` (optionally followed by the names to
    show, `:reg a0`) list what each register holds in a list over the editor
    — the status bar has room for one line — with a line break shown as
    `^J`. `escape` closes it.
- **Marks**: `m{a-z}` sets a mark in the document; `'a` goes to its line
  (first non-blank, linewise) and `` `a `` to the very spot (charwise,
  exclusive). Both are motions, so `d'a`, `` y`a `` and `v`a` work. Marks
  follow the text they were set on: lines added or deleted above move them,
  `J` and `:m` carry them along, and deleting a mark's line deletes the mark,
  as in Vim. Special marks: `''` / ``` `` ``` where the last jump left from,
  `'[` / `']` the first and last character of the text last changed, yanked
  or put, `'.` the last change, `'^` where insert mode was left (where `gi`
  picks up), and `'<` / `'>` the last visual selection. `m'`, `m[`, `m]`,
  `m<` and `m>` set those by hand. `m{A-Z}` sets a file mark: `'A` from any
  other document opens the one it was set in and lands on it.
  - Ex commands: marks as addresses (`:'a,'bd`, `:'a` to go there),
    `:marks [names]` lists them, `:k{a-z}` and `:[range]ma[rk] {a-z}` set
    one on a line, and `:delm[arks] {marks}` deletes some (`:delm a-d x`),
    `:delm!` every lowercase one.
- **Jumplist**: `G`, `gg`, `/`, `?`, `n`, `N`, `*`, `#`, `%`, `(`, `)`, `{`,
  `}`, `H`, `M`, `L`, mark jumps, `:{line}` and EasyMotion jumps leave
  `''` behind and go on the jumplist; `ctrl+o` steps back through it and
  `ctrl+i` forward again, with counts. As in Vim it keeps one entry per
  line — the latest visit — and the last hundred jumps; a jump that went
  nowhere (`G` on the last line) leaves `''` as it was. `:ju[mps]` lists it.
- **Multiple cursors**: make them the VSCode way — `cmd+alt+up/down` for a
  column, `cmd+d` for the next occurrence, alt-click — and every key runs at
  all of them. `I` opens insert mode at each line's first non-blank, `A` at
  each end, `i a o O` where you'd expect; motions, operators, text objects,
  `x`, `p`, `r` and `ctrl+a` all work per cursor, and the status bar counts
  them; `gv` and `gi` remember a selection and an insert spot per cursor.
  Escape leaves insert mode with the cursors intact; escape again drops back
  to one. Selections made with `cmd+d` come in as visual mode at every
  cursor, so `c` changes each occurrence. What is shared is the mode, the
  registers, the marks and the search history; what runs once is `:`
  commands, undo, scrolling and `gd`, which already cover the document.
  Registers work like VSCode's own copy and paste: a yank or delete at
  several cursors fills the register with one piece per cursor, and a put at
  as many cursors gives each cursor its own piece — `yiw` then `P` doubles
  every cursor's word. A put at any other number of cursors (one included)
  puts every piece, top to bottom, one per line. The key counts as one
  register write however many cursors ran it: one `"1` shift for a `dd` at
  three cursors, one `"A` append, one copy to the clipboard. Marks are set
  from the primary cursor (`ma` at every cursor is one mark a), `ctrl+o` and
  `ctrl+i` drop back to the primary cursor first, and the status bar
  reports what the primary cursor did.
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

Replace mode (`R`) and block visual (`ctrl+v` — the column editing it is
for is what multiple cursors do here).

Repeating and macros stop short of Vim's in a few places. What `.` repeats
is remembered per document, where Vim has one for the whole editor. Moving
the cursor while inserting (an arrow key, a click) starts the insert over,
as in Vim, so `.` then repeats an `i` of what was typed since; a macro
cannot record the move, and keeps the text typed before it as though it had
all been typed in one place. `ctrl+d/u/f/b`, mouse clicks and the editor's
own commands are not recorded (the scroll keys do run when a register spells
them out), there is no `q:` / `q/` command-line window, no `@=`, and no
`:normal`. Some keys that make Vim beep do not stop a macro here: `u` with
nothing left to undo is VSCode's to report. A macro is cut off after 100,000
keys, or 1,000 registers deep (`E169`), rather than running until
interrupted.

Registers stop short of Vim's in a few places. There is no `"%` / `"#`
(file names), `"=` (expressions), `:let @a`, or `ctrl+r` in insert mode.
`".` is the text between where the insert began and where it ended (and a
bracket the editor closed after the cursor), so backspacing past the start
of an insert, or a completion that rewrites the word before it, gives what is
left rather than Vim's record of the keys.
Moving the cursor while inserting starts it over, as in Vim. A yank or
delete into a read-only register does nothing, where Vim also beeps; a
visual `p` with nothing in the register leaves the selection alone, where
Vim deletes it first. `"+` and `"*` are the same clipboard.

Marks are kept for as long as VSCode runs — nothing is saved between
sessions, as Vim's viminfo does — and the numbered marks `'0`–`'9` and `'"`
are not there. The jumplist is the document's own: `ctrl+o` does not go back
to the previous file, and `'A` into another document is recorded in the
document it left, not the one it lands in.
`:marks` and `:jumps` count columns in UTF-16 units, where Vim counts bytes.
`'<` and `'>` are the two ends of one remembered selection, so `:delm <`
forgets both. `'[ ']` are not set by `>` and `<`, which VSCode's indent does.

Ex commands stop at the ones listed above: no `:normal`, `:r`,
buffers, windows or tabs, and no `|` to chain commands. An unknown one says
so in the status bar rather than doing something surprising. File names are
not taken — `:w other.ts`, `:e other.ts` and `:sav` say they are not
supported; VSCode's Save As and Open do those — and nor is writing part of
a buffer (`:'<,'>w`). Saving a new, untitled document is VSCode's Save As,
so `:wq` there leaves the newly saved file open. `:sort` has no `b o x f`
(binary, octal, hex and float numbers) or `l` (locale). `:>` leaves the
cursor on the first line it shifted where Vim leaves it on the last, and
each `>` of `:>>` is an undo step of its own. Under `:g`, an empty command —
Vim's print — reports how many lines match instead, and file commands and
the listings (`:reg`, `:marks`, `:jumps`) are refused.

A visual selection stops at a line's last character and never takes the line
break after it, so `vi{` on braces that sit on their own lines selects the
lines between without their final line break: `d` then leaves an empty line
where vim leaves none (`di{` without visual mode removes the lines).
`{ }` stop at empty lines only, where `( )`, `is`/`as` and `ip`/`ap` also
take nroff paragraph and section macros (`.PP`, `.SH`, …) and form feeds as
boundaries, as vim does.

Some of the motions and actions stop short of Vim's details. `|` counts
characters, not screen columns, so a tab is one. `H` and `L` land on the
very first and last lines on screen: `scrolloff` (VSCode's
`cursorSurroundingLines`) is not applied. `ctrl+e`/`ctrl+y` in visual mode,
or with several cursors, let the cursor scroll off screen rather than drag
it along, since VSCode would drop the selections to do that. A count before visual `I` or
`A` does not repeat the typed text, a count given to `.` after a visual
change is not used, and `ctrl+a` reads only decimal, hex
and binary (Vim's default `nrformats`): no octal, no letters.

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
