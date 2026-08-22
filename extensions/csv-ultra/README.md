# CSV Ultra

Separated values as a spreadsheet *and* as text, with neither view treated as
the poor relation.

Open a `.csv`, `.tsv`, `.tab` or `.psv` and you get an Excel-style table: frozen
column titles, in-place editing, sorting, row and column selection, columns and
rows you can drag to size. Click `</>` in the title bar and the same tab becomes
a text editor with **every column in its own colour** — which is the one thing
that makes a forty-column CSV readable as text, because the problem was never
the characters, it was telling which comma you are between.

Every edit the table makes is an edit to the file's own text. So undo, redo, the
dirty dot, `⌘S`, hot exit, revert, the diff view and a text editor open on the
same file in the next group are all VS Code's own, and all behave exactly as
they do everywhere else.

## The table

- **Excel-style grid.** Column letters (A, B, … AA) with the file's own header
  names beneath them, a row number gutter carrying each row's place *in the
  file*, zebra stripes, grid lines, and numbers right-aligned the way a
  spreadsheet aligns them.

- **Virtualized.** Only what is on screen exists: a few hundred recycled boxes
  positioned from arithmetic, over a table of any size. A million-row file
  scrolls like a hundred-row one.

- **Edit in place.** Double-click, press `Enter`, press `F2`, or just start
  typing. `Enter` commits and moves down, `Shift+Enter` up, `Tab` right,
  `Shift+Tab` left, `Escape` throws it away, `Alt+Enter` puts a line break
  *inside* the cell (which is written back quoted, so it stays one field).
  `Delete` clears whatever is selected.

- **A blank row and column past the end.** Typing in them extends the file.
  Growing a table is the same gesture as editing one.

- **Select like a spreadsheet.** Click and drag for a block, click a column
  letter or a row number for the whole thing, `⌘`/`Ctrl`-click to add a second
  band that is nowhere near the first, `Shift`-click to extend, `⌘A` for
  everything. `⌘`/`Ctrl` with an arrow jumps to the edge of the run of data
  you are in — the Excel rule, including skipping a gap to the next filled cell.

- **Sort by clicking a column head** (or double-clicking it): ascending,
  descending, back to the file's own order. Numbers sort as numbers, ISO dates
  as dates, text with a natural collator so `item2` comes before `item10`, and
  blanks sink to the bottom in both directions. The header row never sorts into
  the data.

  **A sort is a view, not a change.** Nothing is written and nothing is dirty
  until you press **Write** on the sorted chip in the toolbar — right for a
  format whose row order is often the data itself.

- **Resize anything.** Drag a column's right edge or a row's bottom edge.
  Double-click either edge to fit it to its content. Widths, heights, the sort
  and the header choice are remembered per file, per workspace.

- **Copy and paste as a grid.** Copying puts tab-separated text on the
  clipboard, so it lands as cells in Excel, Numbers or Sheets. Pasting reads
  tabs, commas or semicolons — with quotes honoured, so `"Smith, John"` stays
  one cell — and a single value with no line break is pasted as a *value*
  rather than split on the comma inside it. Copying three `⌘`-clicked columns
  gives you those three columns side by side.

- **Find in the table** (`⌘F`/`Ctrl+F`) with match-case and whole-cell toggles,
  a running count, and `F3` / `Shift+F3` to step through.

- **Right-click** a cell, a row number or a column letter for insert, delete,
  sort and fit.

- **A read-only lock.** The padlock in the toolbar puts the table into look,
  don't touch: no cell editing, no paste, no insert or delete, no writing a sort
  down. Sorting, finding, resizing and copying all still work, and the
  right-click menu shrinks to what the table can actually do.

  It is a *setting* (`csvUltra.readOnly`), not a per-tab switch, so it stays on
  for every table you open until you turn it off — which is the point when you
  are picking your way through a production export.

- **A footer that adds up.** Select a block and it reports the count, how many
  cells are filled, and the sum, average, minimum and maximum of the numbers in
  it.

## Rainbow columns in the text editor

Open the source and each column is painted in its own colour, cycling through
ten. The status bar names the cell your cursor is in — `R412 · C price` — using
the file's own header for the column, which is the fastest way to read a wide
CSV as text.

The ten are not in spectral order, and that is the point: neighbouring columns
would then get neighbouring hues, and a salmon column beside an apricot one is
one column as far as the eye is concerned. Instead the hues are handed out in
steps of a third of the wheel, and lightness alternates on top of that, so a
column and the one beside it differ twice over.

The colours are **theme colours**, `csvUltra.column1` … `csvUltra.column10`,
with defaults for dark, light and both high-contrast themes. Override them where
you override every other colour:

```jsonc
"workbench.colorCustomizations": {
  "csvUltra.column1": "#ff8ab3",
  "csvUltra.column2": "#8ad0ff"
}
```

Those same values reach the table's column heads, so a column is the same colour
in both views of the same file. Only what is on screen is painted, so a
hundred-thousand-line file costs nothing to scroll.

## CSV ⇄ TSV

- **Convert Delimiter…**, **Convert to TSV** and **Convert to CSV** rewrite the
  open file in place — undoable like any other edit. Values are untouched; what
  changes is which of them need quotes, because a field holding a comma needs
  them as CSV and does not as TSV, and one holding a tab is the other way round.
  A search and replace gets this wrong and produces a row one field too long.

- **Save a Copy as TSV… / as CSV…** writes a sibling file under the matching
  extension and leaves the original alone. This is usually the one you want: a
  `.csv` full of tabs is a file every other tool reads wrong, so converting in
  place warns you and offers to finish the job.

- **Read With Delimiter…** changes how *this tab* reads the file without
  changing a single byte — for a file whose extension is lying.

Delimiters are detected automatically: `.tsv`/`.tab` are tab-separated and
`.psv` pipe-separated whatever is inside them, and anything else is scored
against comma, tab, semicolon, pipe and colon over the first few dozen records.
The scorer parses each candidate properly, so a comma inside `"Smith, John"`
never counts as a separator.

## Writing back

A cell or a row edit rewrites **only the record it touches**. Every other byte
of the file — the other rows' quoting, its line endings, its trailing newline,
the raggedness of a file whose rows are not all the same length — is left
exactly where it was, so the diff of a one-cell edit is one cell. A column
insert, a written sort and a delimiter conversion touch every record and are
written as one edit, which is one undo step.

`csvUltra.quoteStyle` decides how a rewritten field spells its quotes.
`preserve` (the default) keeps the file's own habit: a value quoted on the way
in stays quoted on the way out. `minimal` quotes only what has to be, `always`
quotes everything.

## Keyboard

Inside the table (the arrows and their friends are handled by the page, so they
are never taken away from other editors):

| Key | |
| --- | --- |
| Arrows, `Tab`, `Shift+Tab` | Move |
| `Shift` + arrows | Extend the selection |
| `⌘`/`Ctrl` + arrows | Jump to the edge of the data |
| `Page Up` / `Page Down`, `Home` / `End` | Move by a screen, or to the ends |
| `⌘A` | Select everything |
| `Enter`, `F2`, or any character | Edit the cell |
| `Alt+Enter` | Line break inside the cell |
| `Escape` | Cancel the edit, or close find |
| `Delete` / `Backspace` | Clear the selection |
| `⌘C` / `⌘X` / `⌘V` | Copy, cut, paste |

Contributed, and rebindable — all of them require the table to be the active tab
and the keyboard not to be in the terminal or an input box:

| Key | Command |
| --- | --- |
| `⌘F` / `Ctrl+F` | Find in Table |
| `F3` / `Shift+F3` | Find Next / Previous |
| `⌘G` / `Ctrl+G` | Go to Row… |
| `⌘K ⌘H` | Toggle Header Row |
| `⌘K ⌘W` | Toggle Cell Wrapping |
| `⌘K ⌘A` | Fit Column Widths |
| `⌘=` / `⌘-` / `⌘0` | Font size |
| `⌘⇧=` / `⌘⇧-` | Row height |
| `⌘Z` / `⌘⇧Z` | Undo / redo — stands down while a cell editor is open |

## Settings

| Setting | Default | |
| --- | --- | --- |
| `csvUltra.delimiter` | `auto` | Detect, or force one |
| `csvUltra.quoteStyle` | `preserve` | How a rewritten field is quoted |
| `csvUltra.headerRow` | `auto` | Whether the first record names the columns |
| `csvUltra.rowHeight` | `24` | Row height in px |
| `csvUltra.columnWidth` | `140` | Width a column opens at |
| `csvUltra.maxColumnWidth` | `480` | Ceiling on an auto-fitted column |
| `csvUltra.autoFitOnOpen` | `true` | Fit the columns to their content on open |
| `csvUltra.fontSize` | `0` | 0 follows the editor |
| `csvUltra.fontFamily` | `editor` | Monospace, so digits line up |
| `csvUltra.wrap` | `false` | Wrap cell text instead of clipping it |
| `csvUltra.zebraStripes` | `true` | Tint every other row |
| `csvUltra.alignNumbers` | `true` | Right-align cells holding a number |
| `csvUltra.readOnly` | `false` | Look, don't touch — the toolbar's padlock |
| `csvUltra.rememberLayout` | `true` | Reopen with the widths, heights and sort you left |
| `csvUltra.maxFileSizeBytes` | `33554432` | Largest file the table builds from |
| `csvUltra.rainbow.enabled` | `true` | Colour the columns in the text editor |
| `csvUltra.rainbow.maxFileSizeBytes` | `8388608` | Largest file the text editor colours |
| `csvUltra.statusBar` | `true` | Announce the cell under the cursor |

## Prefer the text editor?

The table registers as the default editor for these extensions. To keep the text
editor and reach the table only when you ask for it (*Open in CSV Ultra*):

```jsonc
"workbench.editorAssociations": {
  "*.csv": "default",
  "*.tsv": "default"
}
```

## Privacy and safety

Nothing in the table reaches the network: no CDN, no web fonts, no telemetry, no
remote origin of any kind. Your data never leaves your machine, and a CSV opens
the same offline as online.

The only file the extension writes is the one you are editing — through VS
Code's own edit and save, so nothing is written until you save it. **Save a Copy
as TSV…** writes a new file, through a save dialog, and that is its only other
write.

A cell holding `<script>` or a `javascript:` URL is shown as the text it is. It
is never treated as markup, whoever sent you the file.
