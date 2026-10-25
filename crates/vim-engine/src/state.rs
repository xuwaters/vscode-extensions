//! The modal state machine: modes, counts, operators, registers, marks, and
//! the effects protocol the TypeScript host consumes.
//!
//! The engine mirrors the document (see buffer.rs) and *self-applies* every
//! edit it emits; the host applies the same edits to the real document and
//! suppresses mirroring for those. External changes (insert-mode typing,
//! undo, other extensions) reach the engine through `apply_change`.

use serde::Serialize;

use crate::buffer::{Buffer, Pos, chars_with_cols, utf16_len};
use crate::easymotion::{self, Dir};
use crate::ex;
use crate::keys::{self, Key};
use crate::mark::{self, Marks, Shift};
use crate::motion::{self, FindKind, char_before};
use crate::number;
use crate::record;
use crate::register::{self, Kind, Register};
use crate::search::{self, Search};
use crate::textobj::{self, TextObject};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Normal,
    Insert,
    Visual { linewise: bool },
}

impl Mode {
    fn label(self) -> &'static str {
        match self {
            Mode::Normal => "normal",
            Mode::Insert => "insert",
            Mode::Visual { linewise: false } => "visual",
            Mode::Visual { linewise: true } => "visualLine",
        }
    }
}

/// A selection in VSCode coordinates (end-exclusive, UTF-16 columns).
#[derive(Serialize, Clone, Copy, Debug)]
pub struct Selection {
    pub anchor: Pos,
    pub active: Pos,
}

/// A text edit in pre-state coordinates. When several edits are emitted for
/// one key they never overlap, so the host can hand them to one
/// `editor.edit` transaction verbatim — every range refers to the document
/// as it was before the key. (A `:s` replacement containing `\r` or a join
/// does change the line count, which is why the engine applies them to its
/// own mirror bottom-up.)
#[derive(Serialize, Clone, Debug)]
pub struct Edit {
    pub start: Pos,
    pub end: Pos,
    pub text: String,
}

/// Side effects the engine cannot express as edits; interpreted by the host.
#[derive(Serialize, Clone, Debug)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Command {
    Undo,
    Redo,
    #[serde(rename_all = "camelCase")]
    IndentLines {
        start_line: usize,
        end_line: usize,
        dedent: bool,
        /// How many shifts: 1 but for `:>>` and its longer spellings.
        times: usize,
    },
    Scroll {
        to: &'static str, // "center" | "top" | "bottom"
    },
    /// `o` / `O`: split a line open and leave the cursor on the new one.
    /// The indent it gets follows the language's rules, which only the host
    /// knows, so this travels as a command rather than an edit: the host runs
    /// its own line-insert — what `<cr>` in insert mode does — and the change
    /// mirrors back like any other edit the engine did not make. With several
    /// cursors it is emitted once; the host's command opens a line at each.
    OpenLine {
        above: bool,
    },
    /// `gh`: pop the hover — the docs/type of whatever is under the cursor.
    /// Purely a host affordance; the engine has no idea what a symbol is.
    ShowHover,
    /// `:w :q :x :wq :wa :qa :up`, `ZZ` and `ZQ`. The engine cannot tell a
    /// document with unsaved changes from one without, so it says what was
    /// asked for and the host decides: `:q` on a modified document is
    /// refused with Vim's E37, `:x` writes only if there is something to
    /// write (`:help :x`).
    File(ex::FileCommand),
    /// `:e!` (and `:e` on an unmodified document): read it back from disk.
    Reload { force: bool },
    /// `ctrl-e` / `ctrl-y`: scroll the view `lines` lines without moving
    /// the text under the cursor. Only the host knows the viewport. Vim
    /// drags the cursor along when it would scroll off screen; `move_cursor`
    /// says to do that too, and is off in visual mode and with several
    /// cursors, where VSCode's way of doing it would collapse the
    /// selections to one bare cursor.
    #[serde(rename_all = "camelCase")]
    ScrollLines {
        down: bool,
        lines: usize,
        move_cursor: bool,
    },
    /// `gd`: go to the definition of the symbol under the cursor, the
    /// language server's answer rather than Vim's search for the first
    /// occurrence in the function — vscode-vim's binding of the same name.
    GotoDefinition,
    /// Put `text` on the system clipboard: a yank or delete into `"+` or
    /// `"*` wrote it. Linewise text carries a final line break, which is how
    /// whatever reads the clipboard next — Vim included — tells it is lines.
    ///
    /// The clipboard is the host's, and the host's API for it is
    /// asynchronous while a key runs to completion here, so the two
    /// directions are handled apart. Writing is this command, at most one per
    /// key, carrying what the register holds once every cursor has run.
    /// Reading is the host's job *before* a key: it asks `wants_clipboard`
    /// whether the key about to run would read `"+` / `"*` — a `"+` prefix
    /// is pending, or the `:` line it ends puts or lists one — and if so
    /// reads the clipboard and hands it over with `set_clipboard`, then
    /// sends the key. Keys that arrive while that read is in flight wait for
    /// it, so they still run in the order they were typed.
    SetClipboard { text: String },
    /// `:registers`, `:marks` and `:jumps`: a listing too long for the
    /// status bar, for the host to show as a list.
    ShowList { title: String, rows: Vec<ListRow> },
    /// `'A` / `` `A ``: file mark A is in another document, which the host
    /// opens with the cursor at `line`/`col` — or, for `'A`, on that line's
    /// first non-blank, which only the host can see.
    #[serde(rename_all = "camelCase")]
    GotoMark {
        uri: String,
        line: usize,
        col: usize,
        first_non_blank: bool,
    },
    /// `@{reg}`, and `.` after `o` / `O`: run `keys`, `count` times over,
    /// as though they were typed — each one sent with `feed` once the one
    /// before it has landed. They are spelled the way `key` reads them.
    ///
    /// The host runs them because a key may have to wait on it: for an edit
    /// to apply, for its line-insert to open the line the text after `o`
    /// goes on, for the clipboard. In insert mode a run of plain characters
    /// goes in with `insert_text`, as the engine's own edit, rather than
    /// through the editor's typing: what a recording holds is the text the
    /// insert left behind, after auto-indent and auto-closing had their say,
    /// and typing it again would have them say it twice.
    ///
    /// A key whose effects come back `failed` stops the run along with
    /// everything queued behind it, the way an error flushes Vim's typeahead
    /// (`:help @`); a register run from inside one goes in front of what is
    /// left of it. That is what ends a recursive macro (`qaq`, then
    /// `qa…@aq`): the motion that finds no more to do.
    Execute { keys: Vec<String>, count: usize },
}

/// One row of a `ShowList`: what the row is (`"a`, `'b`), a short note
/// beside it (a register's type, a mark's line and column) and its content.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct ListRow {
    pub label: String,
    pub description: String,
    pub detail: String,
    /// A file mark in another document: the row's content is that document,
    /// which the host names the way it names documents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uri: Option<String>,
}

/// Incremental-search UI state, present while a `/` or `?` prompt is open
/// and on the key that closes it. The host paints and peeks; the engine's
/// cursor stays put until the prompt commits.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SearchUi {
    /// The pattern typed so far has these matches: highlight them and peek
    /// at `current` without moving the cursor.
    Active {
        /// Flat `[line, startCol, endCol]` per match (UTF-16 columns), the
        /// current match excluded, capped in search.rs.
        matches: Vec<u32>,
        /// The match `<cr>` would land on, to scroll into view.
        #[serde(skip_serializing_if = "Option::is_none")]
        current: Option<[u32; 3]>,
    },
    /// Prompt closed by `<cr>`: keep the view where the search landed.
    Committed,
    /// Prompt abandoned (`<esc>`, `<bs>` past the start, an outside click):
    /// restore the view the search started from.
    Cancelled,
}

/// EasyMotion overlay state, present on the key that paints it and on the
/// one that takes it down. The engine picks the targets and spells the
/// labels; the host paints them over the buffer and dims the rest.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum EasyUi {
    /// Paint these labels, and dim the visible lines they were chosen from.
    #[serde(rename_all = "camelCase")]
    Labels {
        labels: Vec<EasyLabel>,
        first_line: usize,
        last_line: usize,
    },
    /// The jump ended — taken, abandoned or interrupted: clear the paint.
    Done,
}

/// One label: where it sits and what is left to type for it.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
pub struct EasyLabel {
    pub line: usize,
    /// UTF-16 column, like every other position that crosses the boundary.
    pub col: usize,
    pub text: String,
}

/// Result of one engine call. `selections` is what the editor selection
/// should become (empty = leave it alone); `edits` are applied first.
#[derive(Serialize, Debug)]
pub struct Effects {
    pub mode: &'static str,
    pub selections: Vec<Selection>,
    /// Not serialized: the WASM layer hands edits to the host as one binary
    /// block (see wasm_api.rs), which is far cheaper than JSON at `:%s` size.
    #[serde(skip_serializing)]
    pub edits: Vec<Edit>,
    pub commands: Vec<Command>,
    /// Keys buffered toward an incomplete command, for the status bar.
    pub pending: String,
    /// A one-line report for the status bar: `:s` counts, ex errors, a search
    /// that found nothing. `None` leaves whatever was shown before.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// Search-typing UI state; `None` outside a `/`/`?` prompt's lifetime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search: Option<SearchUi>,
    /// EasyMotion overlay; `None` outside a jump's lifetime.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub easy: Option<EasyUi>,
    /// The key failed the way a Vim command beeps: a motion with nowhere to
    /// go, a search that found nothing, an error. A register being run
    /// stops here (see `Command::Execute`).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub failed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Dedent,
    /// `gu`, `gU`, `g~`: the case operators, spelled with a `g` in front.
    Lower,
    Upper,
    ToggleCase,
}

impl Op {
    fn from_char(ch: char) -> Option<Op> {
        match ch {
            'd' => Some(Op::Delete),
            'c' => Some(Op::Change),
            'y' => Some(Op::Yank),
            '>' => Some(Op::Indent),
            '<' => Some(Op::Dedent),
            _ => None,
        }
    }

    /// The case operator `g{ch}` names; `ch` is also the key that doubles
    /// it (`guu`, `gUU`, `g~~`).
    fn case_from_char(ch: char) -> Option<Op> {
        match ch {
            'u' => Some(Op::Lower),
            'U' => Some(Op::Upper),
            '~' => Some(Op::ToggleCase),
            _ => None,
        }
    }

    /// What a case operator does to one char. A char whose other case is
    /// more than one char (`ß` -> `SS`) stays as it is, as in Vim, so a
    /// case change never moves a column.
    fn recase(self, ch: char) -> char {
        fn sole(mut it: impl Iterator<Item = char>, or: char) -> char {
            match (it.next(), it.next()) {
                (Some(c), None) => c,
                _ => or,
            }
        }
        let upper = match self {
            Op::Upper => true,
            Op::ToggleCase => !ch.is_uppercase(),
            _ => false,
        };
        if upper {
            sole(ch.to_uppercase(), ch)
        } else {
            sole(ch.to_lowercase(), ch)
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum Awaiting {
    #[default]
    None,
    Find(FindKind),
    Replace,
    G,
    Z,
    /// `Z`, waiting for the `Z` or `Q` that says how to leave (`ZZ`, `ZQ`).
    Quit,
    /// `[` or `]`, waiting for the bracket that names the pair (`]}`, `[(`).
    Bracket {
        forward: bool,
    },
    Object {
        around: bool,
    },
    /// Typing a `/` or `?` pattern, terminated by `<cr>`.
    Search {
        backward: bool,
    },
    /// Typing an ex command line after `:`, terminated by `<cr>`.
    Ex,
    /// `"`, waiting for the register's name.
    Register,
    /// `'` (linewise) or `` ` ``, waiting for the mark to go to.
    Mark {
        linewise: bool,
    },
    /// `m`, waiting for the mark to set.
    SetMark,
    /// `q`, waiting for the register to record into.
    Record,
    /// `@`, waiting for the register to run.
    Execute,
    /// Somewhere inside an EasyMotion jump (see `EasyStage`).
    Easy(EasyStage),
}

/// The three steps of a jump: name the motion, say what it looks for, pick a
/// label.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum EasyStage {
    /// The trigger has been typed; waiting for the motion key — `w`, `ge`,
    /// `f`, `2s`, `/` — with `count` collected so far and `after_g` set once
    /// `g` opened a two-key name.
    Motion { count: usize, after_g: bool },
    /// Collecting what a char jump looks for: `want` characters, or as many
    /// as are typed before `<cr>` when it is 0 (the `/` jump).
    Chars { dir: Dir, before: bool, want: usize },
    /// The labels are up; the keys pick one.
    Pick,
}

/// How a jump feeds a pending operator. `Char` is `f`/`F`'s rule — inclusive
/// going forward, exclusive going back — which a two-way jump can only settle
/// once a target is picked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum JumpKind {
    Fixed(MotionKind),
    Char,
}

impl JumpKind {
    fn motion(self, from: Pos, to: Pos) -> MotionKind {
        match self {
            JumpKind::Fixed(kind) => kind,
            JumpKind::Char if to > from => MotionKind::Inclusive,
            JumpKind::Char => MotionKind::Exclusive,
        }
    }
}

/// A jump waiting to be picked: every labelled landing place, the label keys
/// typed so far, and the lines the labels were chosen from (repainting after
/// a keystroke must not follow the viewport around).
#[derive(Clone, Debug)]
struct Jump {
    targets: Vec<(Pos, String)>,
    typed: String,
    kind: JumpKind,
    view: (usize, usize),
}

/// EasyMotion settings, as the host configures them.
#[derive(Clone, Debug)]
struct EasyConfig {
    enabled: bool,
    /// The keys that open a jump — `<space><space>` by default, Vim's
    /// `<Leader><Leader>` with `<Leader>` on the space bar.
    trigger: Vec<Key>,
    /// The keys labels are spelled with, nearest target first.
    keys: Vec<char>,
}

/// vim-easymotion's own `g:EasyMotion_keys`, home-row first.
const DEFAULT_MARKER_KEYS: &str = "asdghklqwertyuiopzxcvbnmfj;";

impl Default for EasyConfig {
    fn default() -> Self {
        EasyConfig {
            enabled: true,
            trigger: vec![Key::Char(' '), Key::Char(' ')],
            keys: DEFAULT_MARKER_KEYS.chars().collect(),
        }
    }
}

#[derive(Clone, Default, Debug)]
struct Pending {
    keys: String,
    /// A count typed before `"x`; it multiplies the ones after, as Vim's
    /// `opcount` does (`2"a3yy` yanks six lines).
    count0: usize,
    count1: usize,
    op: Option<Op>,
    count2: usize,
    awaiting: Awaiting,
    /// The `/`, `?` or `:` line typed so far (only while `awaiting` is
    /// `Search` or `Ex`).
    prompt: String,
    /// `"x`: the register the command is for. `None` is the default, which
    /// is not the same as `"` — a delete with no name fills `"-` or `"1`.
    register: Option<char>,
    /// The operator's motion is one of those whose deletes always go to
    /// `"1` (`:help quote_number`): `% ( ) \` / ? n N { }`.
    reg_one: bool,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.keys.is_empty()
    }

    /// Keys buffered so far, as the status bar shows them.
    fn display(&self) -> String {
        let mut s = self.keys.clone();
        s.push_str(&self.prompt);
        s
    }

    fn count(&self) -> usize {
        self.count0.max(1).saturating_mul(self.count1.max(1)).saturating_mul(self.count2.max(1))
    }

    fn has_count(&self) -> bool {
        self.count0 > 0 || self.count1 > 0 || self.count2 > 0
    }

    fn add_digit(&mut self, d: usize) {
        let slot = if self.op.is_some() {
            &mut self.count2
        } else {
            &mut self.count1
        };
        *slot = slot.saturating_mul(10).saturating_add(d);
    }
}

/// A register write made by one cursor of a multi-cursor key, held until
/// every cursor has run (see `key_multi`).
#[derive(Clone, Debug)]
struct Staged {
    name: Option<char>,
    reg: Register,
    kind: Kind,
}

/// What the insert in progress has typed, for `".` — and for `'[ ']` once it
/// ends. Only the primary cursor's insert is followed.
///
/// The host types; the engine sees the document change and then the cursor
/// move. A move that lands inside what the changes just put in is typing; one
/// that lands anywhere else is the cursor being moved — an arrow key, a click
/// — which starts the insert over from there, as moving the cursor in Vim's
/// insert mode does. `".` is then whatever lies between where the insert
/// (re)started and where it ended.
#[derive(Clone, Copy, Debug)]
struct InsertRecord {
    /// Where typing began, carried through the changes since.
    start: Pos,
    /// Where the host last put the cursor, carried through the changes since.
    at: Pos,
    /// The span the changes since then put in.
    typed: Option<(Pos, Pos)>,
    /// `o` / `O`: the host is still opening the line, and typing starts
    /// wherever its cursor lands.
    opening: bool,
    /// How far the typing has reached, carried through the changes since.
    /// It can lie past the cursor: the `)` an editor closes a typed `(` with
    /// is text the insert put in, though the cursor stays in front of it.
    reach: Pos,
    /// The cursor was moved and the insert started over from there.
    restarted: bool,
}

/// What an insert typed, split at the cursor it ended on: `after` is what
/// it put in beyond the cursor (see `InsertRecord::reach`).
#[derive(Clone, Default, Debug, PartialEq, Eq)]
struct Typed {
    before: String,
    after: String,
}

impl Typed {
    fn text(&self) -> String {
        format!("{}{}", self.before, self.after)
    }
}

/// The last change, for `.` (`:help .`): the keys that made it, which is
/// what Vim's redo buffer holds, and what the insert it ended in typed.
///
/// The count and the `"x` in front are kept apart from the keys: a count
/// given to `.` replaces the first one — for the repeats after it too — and
/// a numbered register steps on to the next on every repeat, so `"1p..`
/// puts `"1`, `"2`, `"3` (`:help redo-register`). A count after the
/// operator is folded in with the one before it, as Vim's redo has it:
/// `d3w` repeats as `3dw`.
#[derive(Clone, Debug)]
struct Redo {
    keys: Vec<Key>,
    count: Option<usize>,
    register: Option<char>,
    /// A change made in visual mode works on a selection of the same size,
    /// made afresh from the cursor.
    visual: Option<VisualSize>,
    /// What the insert the change ended in typed; `None` while it is still
    /// going on.
    insert: Option<Typed>,
}

/// The size of the selection a visual change worked on (normal.c
/// `redo_VIsual`): how many lines and, charwise, either how many characters
/// it covered within one line or the column it ended on in the last of
/// several — `STICKY_EOL` when that was a `$`.
#[derive(Clone, Copy, Debug)]
struct VisualSize {
    linewise: bool,
    lines: usize,
    cols: usize,
}

/// What `key` notes about a key before running it, for the redo record and
/// the recording (see `after_key`).
struct KeyStart {
    /// No command was under way: this key starts one.
    fresh: bool,
    /// A count digit, or the `"x` in front of a command, which the redo
    /// record keeps apart from its keys.
    prefix: bool,
    count: Option<usize>,
    register: Option<char>,
    /// The selection a command starting in visual mode works on.
    visual: Option<VisualSize>,
    /// The key is `.` itself.
    dot: bool,
    /// The key goes to an EasyMotion jump, which `.` does not repeat: what
    /// it lands on depends on labels nobody will type again.
    easy: bool,
    insert: bool,
    /// The `<esc>` ending an insert: what the insert typed, and whether it
    /// started over on the way.
    typed: Option<(Typed, bool)>,
    recording: bool,
}

/// How a motion combines with an operator.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MotionKind {
    Exclusive,
    Inclusive,
    Linewise,
}

/// Sticky column for `j`/`k`; `usize::MAX` means end-of-line (`$`).
const STICKY_EOL: usize = usize::MAX;

/// Everything one cursor owns. The primary cursor lives in `Session`'s own
/// fields (so the single-cursor paths read exactly as they always did); the
/// extra cursors of a multi-cursor session are swapped through them one at a
/// time by `key_multi`.
#[derive(Clone, Default, Debug)]
struct CursorState {
    pos: Pos,
    anchor: Pos,
    desired_col: usize,
    pending: Pending,
    last_area: Option<VisualArea>,
    last_insert: Option<Pos>,
}

/// A visual selection as it stood when visual mode ended, for `gv`.
#[derive(Clone, Copy, Debug)]
struct VisualArea {
    anchor: Pos,
    cursor: Pos,
    linewise: bool,
}

pub struct Session {
    buf: Buffer,
    mode: Mode,
    cursor: Pos,
    /// Visual-mode anchor (inclusive char position).
    anchor: Pos,
    desired_col: usize,
    pending: Pending,
    /// Extra cursors, in the host's order after the primary. Empty unless the
    /// editor has a multi-cursor selection.
    secondaries: Vec<CursorState>,
    last_find: Option<(FindKind, char)>,
    last_search: Option<Search>,
    /// The replacement `:s` last used, behind a bare `:s`, `&` and `~`.
    last_replacement: Option<String>,
    /// `'a`–`'z`, `''`, `'.`, `'[ ']` and the jumplist; set from the primary
    /// cursor only, and carried through every edit (see `mirror`).
    marks: Marks,
    /// The document this session mirrors, as the host names it — what file
    /// marks (`mA`) remember. `None` until the host says.
    doc: Option<String>,
    /// While `key_multi` runs: every register write a cursor makes, held so
    /// the key's writes land once, as one register with a piece per cursor.
    staged: Option<Vec<Staged>>,
    /// While `key_multi` runs: this cursor's place in document order and how
    /// many cursors there are, which a put uses to pick its piece.
    put_slot: Option<(usize, usize)>,
    /// The insert in progress, for `".`.
    insert: Option<InsertRecord>,
    /// The key has already changed text, so another edit widens `'[ ']`
    /// rather than resetting it.
    key_changed: bool,
    /// Every edit's shift since `key_multi` last took them, to carry the
    /// marks of the cursors that are parked while another one runs.
    shifts: Vec<Shift>,
    /// Set while `:g` runs its commands: Vim sets `''` once for all of them.
    in_global: bool,
    /// Report for the next `Effects`, taken when one is built.
    message: Option<String>,
    /// Search-typing UI for the next `Effects`, taken when one is built.
    search_ui: Option<SearchUi>,
    /// Set while `key_multi` runs a cursor whose status output is dropped
    /// anyway: the whole-buffer work behind it (the incremental-search
    /// preview, the `match N of M` count) is skipped rather than repeated
    /// once per cursor.
    quiet: bool,
    /// EasyMotion settings (see `set_easy_motion`).
    easy: EasyConfig,
    /// Trigger keys matched so far — a jump has not started until they all
    /// are.
    lead: usize,
    /// The jump whose labels are on screen, waiting to be picked.
    jump: Option<Jump>,
    /// EasyMotion overlay for the next `Effects`, taken when one is built.
    easy_ui: Option<EasyUi>,
    /// The lines the host says are on screen; jumps only label those. `None`
    /// until the host reports one, which stands for the whole buffer.
    view: Option<(usize, usize)>,
    /// Set while a buffered trigger key is being replayed as an ordinary one,
    /// so it cannot start matching the trigger all over again.
    replay: bool,
    /// Set while `key_multi` runs. A jump is one cursor's choice of one
    /// landing place, so the trigger stays inert with several of them.
    multi: bool,
    /// The selection visual mode last ended on, behind `gv` and `'< '>`.
    /// Like the cursor itself, every cursor of a multi-cursor session has its
    /// own.
    last_area: Option<VisualArea>,
    /// Where insert mode was last left — Vim's `'^` mark, behind `gi`.
    /// Per cursor too.
    last_insert: Option<Pos>,
    /// The last change, for `.`.
    redo: Option<Redo>,
    /// The redo record's change went on into insert mode, and the text the
    /// insert types still has to be added to it.
    redo_inserting: bool,
    /// The keys of the command under way, without its count and register.
    cmd: Vec<Key>,
    /// The selection it started on, in visual mode.
    cmd_visual: Option<VisualSize>,
    /// The count of the insert in progress (`3ix<esc>`), and whether it was
    /// opened by `o` / `O`: what it typed goes in that many times over.
    insert_count: usize,
    insert_opened: bool,
    /// While an `<esc>` ends an insert: what the insert typed, which every
    /// cursor repeats for a counted insert.
    esc_typed: Option<Typed>,
    /// A command failed during this key (see `Effects::failed`).
    failed: bool,
    /// The key comes from a register being run (`feed`), not from the
    /// keyboard, so a recording in progress leaves it out.
    fed: bool,
    /// A recording was going on when this key started, which decides what
    /// `q` means at every cursor alike.
    was_recording: bool,
}

impl Session {
    pub fn new(text: &str) -> Session {
        Session {
            buf: Buffer::from_text(text),
            mode: Mode::Normal,
            cursor: Pos::new(0, 0),
            anchor: Pos::new(0, 0),
            desired_col: 0,
            pending: Pending::default(),
            secondaries: Vec::new(),
            last_find: None,
            last_search: None,
            last_replacement: None,
            marks: Marks::default(),
            doc: None,
            staged: None,
            put_slot: None,
            insert: None,
            key_changed: false,
            shifts: Vec::new(),
            in_global: false,
            message: None,
            search_ui: None,
            quiet: false,
            easy: EasyConfig::default(),
            lead: 0,
            jump: None,
            easy_ui: None,
            view: None,
            replay: false,
            multi: false,
            last_area: None,
            last_insert: None,
            redo: None,
            redo_inserting: false,
            cmd: Vec::new(),
            cmd_visual: None,
            insert_count: 1,
            insert_opened: false,
            esc_typed: None,
            failed: false,
            fed: false,
            was_recording: false,
        }
    }

    /// The lines the host has on screen, as a jump's labels are limited to
    /// what the user can see. Both ends are inclusive line numbers.
    pub fn set_view(&mut self, first: usize, last: usize) {
        self.view = Some((first.min(last), last.max(first)));
    }

    /// EasyMotion settings. `trigger` is a key sequence written the way a Vim
    /// mapping is (`<space><space>`, `,,`); `keys` are the characters labels
    /// are spelled with, in the order they are handed out. A trigger with no
    /// keys in it, or fewer than two marker keys, leaves the feature off
    /// rather than half-working.
    pub fn set_easy_motion(&mut self, enabled: bool, trigger: &str, keys: &str) {
        let trigger = keys::parse_sequence(trigger);
        let mut marker: Vec<char> = Vec::new();
        for ch in keys.chars() {
            if !ch.is_whitespace() && !marker.contains(&ch) {
                marker.push(ch);
            }
        }
        let usable = !trigger.is_empty() && marker.len() >= 2;
        self.cancel_easy();
        self.easy = EasyConfig {
            enabled: enabled && usable,
            trigger,
            keys: marker,
        };
    }

    pub fn mode_label(&self) -> &'static str {
        self.mode.label()
    }

    pub fn cursor(&self) -> Pos {
        self.cursor
    }

    /// Full mirrored text (primarily for tests and resync checks).
    pub fn text(&self) -> String {
        self.buf.text()
    }

    /// Replace the mirror wholesale (document open / desync recovery).
    pub fn reset(&mut self, text: &str, line: usize, col: usize) {
        self.buf = Buffer::from_text(text);
        self.mode = Mode::Normal;
        self.pending = Pending::default();
        self.secondaries.clear();
        self.search_ui = None;
        // The host rebuilds its own paint after a resync; nothing to report.
        self.easy_ui = None;
        self.jump = None;
        self.lead = 0;
        self.insert = None;
        self.redo_inserting = false;
        self.cmd.clear();
        self.cursor = self.clamp_normal(Pos::new(line, col));
        self.desired_col = self.cursor.col;
    }

    /// The document this session mirrors, as the host names it (its URI).
    /// File marks remember it, so `'A` can tell the host which document to
    /// open.
    pub fn set_document(&mut self, doc: &str) {
        self.doc = Some(doc.to_string());
    }

    /// Mirror an external document change (host-applied edits are already
    /// reflected and must not be passed back in). It is a change like any
    /// other for `'.`, and in insert mode it is the typing `".` records.
    pub fn apply_change(&mut self, start: Pos, end: Pos, text: &str) {
        let edit = Edit { start, end, text: text.to_string() };
        if self.mode == Mode::Insert
            && let Some(rec) = &mut self.insert
        {
            rec.start = shifted(rec.start, &edit);
            rec.at = shifted(rec.at, &edit);
            rec.reach = shifted(rec.reach, &edit);
            if !rec.opening {
                let put = (start, text_end(start, text));
                rec.typed = Some(match rec.typed {
                    Some((a, b)) => (shifted(a, &edit).min(put.0), shifted(b, &edit).max(put.1)),
                    None => put,
                });
                // Typing within what this insert put in takes it further; a
                // change anywhere else — another cursor's typing, a
                // formatter — does not.
                if (rec.start..=rec.reach.max(rec.at)).contains(&start) {
                    rec.reach = rec.reach.max(put.1);
                }
            }
        }
        self.mirror(start, end, text);
        self.marks.change = Some(start);
        self.cursor = self.clamp_to_mode(self.cursor);
        for i in 0..self.secondaries.len() {
            let (pos, anchor) = (self.secondaries[i].pos, self.secondaries[i].anchor);
            self.secondaries[i].pos = self.clamp_to_mode(pos);
            self.secondaries[i].anchor = self.clamp_to_mode(anchor);
        }
    }

    /// The editor cursor moved for a reason the engine didn't cause (mouse
    /// click, undo, host-side scrolling commands, insert-mode typing).
    pub fn set_position(&mut self, line: usize, col: usize) -> Effects {
        self.remember_visual();
        let aborted_search = matches!(self.pending.awaiting, Awaiting::Search { .. });
        self.cancel_easy();
        self.pending = Pending::default();
        self.secondaries.clear();
        if matches!(self.mode, Mode::Visual { .. }) {
            self.mode = Mode::Normal;
        }
        let requested = Pos::new(line, col);
        self.cursor = self.clamp_to_mode(requested);
        self.desired_col = self.cursor.col;
        self.insert_moved(self.cursor);
        let selections = if self.cursor != requested {
            self.current_selections()
        } else {
            Vec::new()
        };
        Effects {
            mode: self.mode.label(),
            selections,
            edits: Vec::new(),
            commands: Vec::new(),
            pending: String::new(),
            message: None,
            search: aborted_search.then_some(SearchUi::Cancelled),
            easy: self.easy_ui.take(),
            failed: false,
        }
    }

    /// A non-empty selection was made outside the engine: enter charwise
    /// visual around it. Coordinates are VSCode-style (end-exclusive); the
    /// engine keeps inclusive char positions.
    ///
    /// `by_hand` is the host saying the user drew this selection themselves —
    /// dragging with the pointer, or holding shift on the arrows. A selection
    /// a *command* left behind is not an invitation to visual mode: the match
    /// `cmd+f` leaves highlighted, or the placeholder a completion lands on,
    /// is something the next keystroke clears or replaces. Entering visual
    /// there is what makes the `j` after closing the find box extend a
    /// selection nobody asked for.
    ///
    /// Insert mode is left by `<esc>` and nothing else, whoever made the
    /// selection: `shift+arrow` and a snippet placeholder both mean "keep
    /// typing", and typing over a selection is what any editor does.
    ///
    /// A right-to-left drag reads its far end through `drag_start`, which is
    /// where a line dragged end-to-start keeps its last character.
    pub fn set_selection(&mut self, anchor: Pos, active: Pos, by_hand: bool) -> Effects {
        self.remember_visual();
        let aborted_search = matches!(self.pending.awaiting, Awaiting::Search { .. });
        self.cancel_easy();
        self.pending = Pending::default();
        self.secondaries.clear();
        // A drag anchored on the last character of a line covers it, so the
        // host's highlight — which stops one character short — is the one that
        // has to move. Every other case leaves the host's selection alone.
        let mut widened = false;
        if !by_hand || self.mode == Mode::Insert {
            // No visual anchor: the cursor goes where the host's caret is,
            // and the body stays the host's business. A command that moves
            // the caret ends visual mode (`set_position`); one that leaves a
            // selection behind is no different.
            if matches!(self.mode, Mode::Visual { .. }) {
                self.mode = Mode::Normal;
            }
            self.cursor = self.clamp_to_mode(active);
            self.anchor = self.cursor;
            self.insert_moved(self.cursor);
        } else if anchor <= active {
            self.anchor = self.clamp_normal(anchor);
            self.cursor = self.clamp_normal(char_before(&self.buf, active));
            self.mode = Mode::Visual { linewise: false };
        } else {
            self.anchor = self.drag_start(anchor);
            self.cursor = self.clamp_normal(active);
            self.mode = Mode::Visual { linewise: false };
            widened = self.anchor != self.clamp_normal(char_before(&self.buf, anchor));
        }
        self.desired_col = self.cursor.col;
        Effects {
            mode: self.mode.label(),
            selections: if widened {
                self.current_selections()
            } else {
                Vec::new() // don't fight the host for its selection
            },
            edits: Vec::new(),
            commands: Vec::new(),
            pending: String::new(),
            message: None,
            search: aborted_search.then_some(SearchUi::Cancelled),
            easy: self.easy_ui.take(),
            failed: false,
        }
    }

    /// The editor's whole selection set changed: extra cursors were added
    /// (`cmd+alt+arrow`, `cmd+d`), removed, or moved as a group. Selections
    /// are VSCode-style (end-exclusive) with the primary first; one of them
    /// takes the single-cursor paths above. `by_hand` says the user drew the
    /// selection rather than a command leaving it behind; see `set_selection`.
    /// Several selections *with* bodies are `cmd+d` and friends, which are
    /// visual mode at every cursor however they were made.
    pub fn set_cursors(&mut self, sels: &[(Pos, Pos)], by_hand: bool) -> Effects {
        match sels {
            [] => self.set_position(self.cursor.line, self.cursor.col),
            [(anchor, active)] if anchor == active => self.set_position(active.line, active.col),
            [(anchor, active)] => self.set_selection(*anchor, *active, by_hand),
            _ => self.set_multi_cursors(sels),
        }
    }

    fn set_multi_cursors(&mut self, sels: &[(Pos, Pos)]) -> Effects {
        self.remember_visual();
        let aborted_search = matches!(self.pending.awaiting, Awaiting::Search { .. });
        self.cancel_easy();
        self.pending = Pending::default();
        // Several selections with a body (`cmd+d` on a word, a multi-cursor
        // drag) are visual mode at every cursor; bare cursors are not. Insert
        // mode stays insert either way — see `set_selection`.
        let insert = self.mode == Mode::Insert;
        let bodied = sels.iter().any(|(a, c)| a != c);
        let visual = !insert && bodied;
        self.mode = match self.mode {
            _ if visual => Mode::Visual { linewise: false },
            Mode::Insert => Mode::Insert,
            _ => Mode::Normal,
        };
        let mut states = Vec::with_capacity(sels.len());
        let mut clamped = false;
        // See `set_selection`: a drag anchored on a line's last character
        // covers it, and the host's highlight has to be widened to match.
        let mut widened = false;
        for &(anchor, active) in sels {
            let (a, c) = if insert {
                // No visual anchor while inserting: every cursor is its
                // selection's head, and the host keeps the bodies.
                let p = self.clamp_insert(active);
                clamped |= p != active;
                (p, p)
            } else if anchor == active {
                let p = self.clamp_to_mode(active);
                clamped |= p != active;
                (p, p)
            } else if anchor <= active {
                (
                    self.clamp_normal(anchor),
                    self.clamp_normal(char_before(&self.buf, active)),
                )
            } else {
                let start = self.drag_start(anchor);
                widened |= start != self.clamp_normal(char_before(&self.buf, anchor));
                (start, self.clamp_normal(active))
            };
            states.push(CursorState {
                pos: c,
                anchor: a,
                desired_col: c.col,
                pending: Pending::default(),
                ..CursorState::default()
            });
        }
        // The cursors are new to the engine, but the primary keeps what it
        // remembers for `gv` and `gi`.
        states[0].last_area = self.last_area;
        states[0].last_insert = self.last_insert;
        if insert {
            self.insert_moved(states[0].pos);
        }
        self.put_cursor(states.remove(0));
        self.secondaries = states;
        Effects {
            mode: self.mode.label(),
            // Only a bare cursor sitting past what the mode allows, or a body
            // the engine widened, needs correcting; otherwise don't fight the
            // editor for its selection — collapsing it is what dropping a body
            // would do here.
            selections: if widened || (clamped && !bodied) {
                self.current_selections()
            } else {
                Vec::new()
            },
            edits: Vec::new(),
            commands: Vec::new(),
            pending: String::new(),
            message: None,
            search: aborted_search.then_some(SearchUi::Cancelled),
            easy: self.easy_ui.take(),
            failed: false,
        }
    }

    pub fn key(&mut self, key: Key) -> Effects {
        let start = self.key_start(key);
        if start.dot {
            self.step_redo();
        }
        self.esc_typed = start.typed.as_ref().map(|(typed, _)| typed.clone());
        self.was_recording = start.recording;
        self.failed = false;
        let mut fx = self.run_key(key);
        self.esc_typed = None;
        fx.failed = std::mem::take(&mut self.failed);
        self.after_key(key, start, &fx);
        fx
    }

    /// A key from a register being run (see `Command::Execute`): `key` in
    /// every way but one — a recording in progress leaves it out, as Vim's
    /// takes in what is typed and not what a register replays. `@a` typed
    /// while recording records the `@a`.
    pub fn feed(&mut self, key: Key) -> Effects {
        self.fed = true;
        let fx = self.key(key);
        self.fed = false;
        fx
    }

    /// The register a recording is going into, for the status bar's
    /// "recording @a".
    pub fn recording(&self) -> Option<char> {
        record::recording()
    }

    /// Text a register being run types in insert mode (see
    /// `Command::Execute`), put in at every cursor as the engine's own edit.
    /// It counts as typed: `".`, `.` and the end of the insert see it as
    /// they see what the host types.
    pub fn insert_text(&mut self, text: &str) -> Effects {
        let mut edits = Vec::new();
        if self.mode == Mode::Insert && !text.is_empty() {
            let mut spots: Vec<Pos> =
                std::iter::once(self.cursor).chain(self.secondaries.iter().map(|c| c.pos)).collect();
            let mut order: Vec<usize> = (0..spots.len()).collect();
            order.sort_by(|&a, &b| spots[b].cmp(&spots[a]).then(b.cmp(&a)));
            // Bottom-up, as `key_multi` goes, so every edit is still in the
            // coordinates of the document the host holds.
            for &i in &order {
                let at = spots[i];
                let edit = Edit { start: at, end: at, text: text.to_string() };
                self.quiet = i != 0;
                self.edit_mirror(Shift::new(&self.buf, at, at, text), &edit);
                for (j, spot) in spots.iter_mut().enumerate() {
                    *spot = if j == i { text_end(at, text) } else { shifted(*spot, &edit) };
                }
                if let Some(rec) = &mut self.insert {
                    rec.start = shifted(rec.start, &edit);
                    rec.reach = shifted(rec.reach, &edit);
                    rec.at = if i == 0 { spots[0] } else { shifted(rec.at, &edit) };
                    if i == 0 {
                        rec.reach = rec.reach.max(rec.at);
                    }
                }
                edits.push(edit);
            }
            self.quiet = false;
            self.cursor = spots[0];
            self.desired_col = self.cursor.col;
            for (state, &spot) in self.secondaries.iter_mut().zip(&spots[1..]) {
                (state.pos, state.anchor, state.desired_col) = (spot, spot, spot.col);
            }
        }
        Effects {
            mode: self.mode.label(),
            selections: self.current_selections(),
            edits,
            commands: Vec::new(),
            pending: self.pending.display(),
            message: None,
            search: None,
            easy: self.easy_ui.take(),
            failed: false,
        }
    }

    fn run_key(&mut self, key: Key) -> Effects {
        // `ctrl-o` / `ctrl-i` walk the one jumplist there is, to one place:
        // with several cursors they would walk it once per cursor and pile
        // them all up there anyway, so they drop back to the primary first.
        let walk = matches!(key, Key::Ctrl('o' | 'i'));
        if walk && self.mode != Mode::Insert && self.pending.op.is_none() && self.pending.awaiting == Awaiting::None {
            self.secondaries.clear();
        }
        if !self.secondaries.is_empty() {
            self.multi = true;
            let fx = self.key_multi(key);
            self.multi = false;
            return fx;
        }
        let mut edits = Vec::new();
        let mut commands = Vec::new();
        self.key_changed = false;
        match self.mode {
            Mode::Insert => self.key_insert(key, &mut edits),
            _ => self.key_noting_visual(key, &mut edits, &mut commands),
        }
        self.marks.check_context(self.cursor);
        self.shifts.clear();
        commands.extend(register::take_clipboard().map(|text| Command::SetClipboard { text }));
        Effects {
            mode: self.mode.label(),
            selections: self.current_selections(),
            edits,
            commands,
            pending: self.pending.display(),
            message: self.message.take(),
            search: self.search_ui.take(),
            easy: self.easy_ui.take(),
            failed: false,
        }
    }

    /// One key at every cursor.
    ///
    /// Cursors run bottom-up, because an edit never moves the text above it:
    /// every edit the pass emits still refers to the document the host is
    /// holding, so they all fit in one `editor.edit` transaction. The reverse
    /// is not free — an edit *does* move everything below it — so each
    /// cursor's edits shift the positions already recorded under them.
    ///
    /// Modal state other than the cursors is shared: one mode, one set of
    /// registers, one set of marks and the primary's report, which is the one
    /// the status bar shows. Marks are set from the primary cursor alone —
    /// `ma` at three cursors is still one mark a — though every cursor's
    /// edits carry them.
    ///
    /// Registers follow VSCode's copy and paste rather than letting the
    /// cursors overwrite each other's yanks: a key's register writes are held
    /// until every cursor has run, then land as *one* write — one `"1`
    /// shift, one `"A` append — of a register holding a piece per cursor,
    /// top to bottom. A put at the same number of cursors gives each cursor
    /// its own piece, so `yiw` then `p` doubles every cursor's word; a put at
    /// any other number of cursors, a single one included, puts all of the
    /// pieces, joined by line breaks. Holding the writes back also means a
    /// cursor reads the registers as the key found them, so visual `p` at
    /// every cursor puts what was yanked, not what the cursor next door just
    /// deleted.
    fn key_multi(&mut self, key: Key) -> Effects {
        // Esc with nothing pending leaves multi-cursor editing, the way it
        // drops the extra cursors in VSCode. From insert or visual mode it
        // returns to normal mode first, keeping them.
        if key == Key::Esc && self.mode == Mode::Normal && self.pending.is_empty() {
            self.secondaries.clear();
            return Effects {
                mode: self.mode.label(),
                selections: self.current_selections(),
                edits: Vec::new(),
                commands: Vec::new(),
                pending: String::new(),
                message: None,
                search: None,
                easy: self.easy_ui.take(),
                failed: false,
            };
        }

        let n = 1 + self.secondaries.len();
        let mut states = Vec::with_capacity(n);
        states.push(self.take_cursor());
        states.append(&mut self.secondaries);
        let mut order: Vec<usize> = (0..n).collect();
        order.sort_by(|&a, &b| states[b].pos.cmp(&states[a].pos).then(b.cmp(&a)));

        let start_mode = self.mode;
        let mut end_mode = start_mode;
        let mut edits: Vec<Edit> = Vec::new();
        let mut commands: Vec<Command> = Vec::new();
        let mut done: Vec<Option<CursorState>> = vec![None; n];
        let (mut message, mut search) = (None, None);
        let mut writes: Vec<Vec<Staged>> = vec![Vec::new(); n];
        // Each cursor's place in document order, top first — `order` runs
        // bottom-up.
        let mut slot = vec![0; n];
        for (rank, &i) in order.iter().rev().enumerate() {
            slot[i] = rank;
        }
        self.shifts.clear();

        // An ex command carries its own range, so running it once is running
        // it everywhere; N cursors would just repeat the same edits. The rest
        // sit the key out — recorded up front, so that the one command's
        // edits carry them along wherever in the document they land.
        if key == Key::Enter && matches!(states[0].pending.awaiting, Awaiting::Ex) {
            for i in 1..n {
                let state = std::mem::take(&mut states[i]);
                done[i] = Some(CursorState { pending: Pending::default(), ..state });
            }
            order.retain(|&i| i == 0);
        }

        for &i in &order {
            let state = std::mem::take(&mut states[i]);
            self.mode = start_mode;
            // `quiet` also says "not the primary": marks, `".` and the
            // context mark are the primary cursor's to set.
            self.quiet = i != 0;
            self.put_cursor(state);
            self.staged = Some(Vec::new());
            self.put_slot = Some((slot[i], order.len()));
            self.key_changed = false;
            let (first_edit, first_command) = (edits.len(), commands.len());
            match self.mode {
                Mode::Insert => self.key_insert(key, &mut edits),
                _ => self.key_noting_visual(key, &mut edits, &mut commands),
            }
            end_mode = self.mode;
            writes[i] = self.staged.take().unwrap_or_default();
            if i == 0 {
                self.marks.check_context(self.cursor);
                message = self.message.take();
                search = self.search_ui.take();
            } else {
                self.message = None;
                self.search_ui = None;
                // Undo, redo, scrolling and opening a line are commands the
                // host runs over the whole selection set: one cursor's worth
                // is the whole story, and N undos would undo N times.
                let mine: Vec<Command> = commands.split_off(first_command);
                commands.extend(
                    mine.into_iter()
                        .filter(|c| matches!(c, Command::IndentLines { .. })),
                );
            }
            for edit in &edits[first_edit..] {
                for other in done.iter_mut().flatten() {
                    other.pos = shifted(other.pos, edit);
                    other.anchor = shifted(other.anchor, edit);
                }
            }
            // What a parked cursor remembers for `gi` and `gv` is a mark
            // like any other, and follows the text the same way.
            for shift in self.shifts.drain(..) {
                for other in done.iter_mut().flatten() {
                    other.last_insert = other.last_insert.and_then(|p| shift.mark(p));
                    if let Some(area) = &mut other.last_area {
                        area.anchor = shift.kept(area.anchor);
                        area.cursor = shift.kept(area.cursor);
                    }
                }
            }
            done[i] = Some(self.take_cursor());
        }

        self.mode = end_mode;
        self.quiet = false;
        self.put_slot = None;
        self.land_staged(writes, &slot);
        commands.extend(register::take_clipboard().map(|text| Command::SetClipboard { text }));
        let mut states: Vec<CursorState> = done.into_iter().flatten().collect();
        for state in &mut states {
            // Another cursor's edit may have shortened the line under this one.
            state.pos = self.clamp_to_mode(state.pos);
            state.anchor = self.clamp_to_mode(state.anchor);
        }
        // Cursors that met are one cursor, as they are in VSCode. What counts
        // is the selection they produce: outside visual mode two cursors on
        // the same char are the same cursor whatever their anchors say.
        let mut seen: Vec<(Pos, Pos)> = Vec::with_capacity(states.len());
        states.retain(|s| {
            let sel = self.selection_at(s.anchor, s.pos);
            let key = (sel.anchor, sel.active);
            !seen.contains(&key) && {
                seen.push(key);
                true
            }
        });
        self.put_cursor(states.remove(0));
        self.secondaries = states;
        Effects {
            mode: self.mode.label(),
            selections: self.current_selections(),
            edits,
            commands,
            pending: self.pending.display(),
            message,
            search,
            easy: self.easy_ui.take(),
            failed: false,
        }
    }

    // ---- mode helpers -----------------------------------------------------

    fn clamp_normal(&self, p: Pos) -> Pos {
        let line = p.line.min(self.buf.last_line());
        let len = self.buf.line_len(line);
        let max_col = self.last_char_col(line, len);
        // Snap into a char boundary.
        let col = p.col.min(max_col);
        let byte = crate::buffer::utf16_to_byte(self.buf.line(line), col);
        Pos::new(line, crate::buffer::byte_to_utf16(self.buf.line(line), byte))
    }

    fn clamp_insert(&self, p: Pos) -> Pos {
        let line = p.line.min(self.buf.last_line());
        Pos::new(line, p.col.min(self.buf.line_len(line)))
    }

    /// Clamp for the current mode: insert may sit past the last char, the
    /// others may not.
    fn clamp_to_mode(&self, p: Pos) -> Pos {
        match self.mode {
            Mode::Insert => self.clamp_insert(p),
            _ => self.clamp_normal(p),
        }
    }

    /// Swap the primary cursor out of / into the fields the single-cursor
    /// code works on.
    fn take_cursor(&mut self) -> CursorState {
        CursorState {
            pos: self.cursor,
            anchor: self.anchor,
            desired_col: self.desired_col,
            pending: std::mem::take(&mut self.pending),
            last_area: self.last_area.take(),
            last_insert: self.last_insert.take(),
        }
    }

    fn put_cursor(&mut self, state: CursorState) {
        self.cursor = state.pos;
        self.anchor = state.anchor;
        self.desired_col = state.desired_col;
        self.pending = state.pending;
        self.last_area = state.last_area;
        self.last_insert = state.last_insert;
    }

    /// The selection in play, when visual mode is.
    fn visual_area(&self) -> Option<VisualArea> {
        match self.mode {
            Mode::Visual { linewise } => {
                Some(VisualArea { anchor: self.anchor, cursor: self.cursor, linewise })
            }
            _ => None,
        }
    }

    /// Note the selection visual mode is about to end on, for `gv`: the
    /// host is moving the cursor or replacing the selection.
    fn remember_visual(&mut self) {
        if let Some(area) = self.visual_area() {
            self.last_area = Some(area);
        }
    }

    /// `key_normal`, remembering the selection when the key ends visual
    /// mode. The selection as the key found it is the one to keep: every key
    /// that leaves visual mode — `<esc>`, an operator, `J`, `p`, the `<cr>`
    /// of a `:'<,'>` line — acts on the selection rather than moving it.
    fn key_noting_visual(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let before = self.visual_area();
        self.key_normal(key, edits, commands);
        if before.is_some() && !matches!(self.mode, Mode::Visual { .. }) {
            self.last_area = before;
        }
    }

    /// The character a right-to-left drag started on, inclusive.
    ///
    /// The host's anchor is the far end of such a selection, one *past* the
    /// last character it covers — except at the end of a line. Normal mode
    /// keeps the caret off the position past the last character, so the click
    /// that began the drag was pulled back onto that character, and the anchor
    /// the host reports sits *on* it rather than after it. Stepping back from
    /// there is what drops the last character of a line dragged end-to-start.
    fn drag_start(&self, anchor: Pos) -> Pos {
        let len = self.buf.line_len(anchor.line);
        let inclusive = len > 0 && anchor.col >= self.last_char_col(anchor.line, len);
        self.clamp_normal(if inclusive {
            anchor
        } else {
            char_before(&self.buf, anchor)
        })
    }

    /// Column of the last char on a line (0 when empty).
    fn last_char_col(&self, line: usize, len: usize) -> usize {
        if len == 0 {
            return 0;
        }
        chars_with_cols(self.buf.line(line))
            .last()
            .map_or(0, |&(c, _)| c)
    }

    fn enter_insert(&mut self, at: Pos) {
        self.mode = Mode::Insert;
        self.cursor = self.clamp_insert(at);
        self.desired_col = self.cursor.col;
        self.insert_count = 1;
        self.insert_opened = false;
        self.start_insert(false);
    }

    /// Start following what the primary cursor's insert types, for `".`.
    fn start_insert(&mut self, opening: bool) {
        if !self.quiet {
            let at = self.cursor;
            self.insert = Some(InsertRecord { start: at, at, typed: None, opening, reach: at, restarted: false });
        }
    }

    /// The host put the insert cursor at `pos`. Unless that is where the
    /// typing just took it, the insert starts over from there (see
    /// `InsertRecord`).
    fn insert_moved(&mut self, pos: Pos) {
        if self.mode != Mode::Insert {
            return;
        }
        let Some(rec) = &mut self.insert else {
            return;
        };
        let typing = rec.typed.is_some_and(|(a, b)| (a..=b).contains(&pos));
        if rec.opening || pos != rec.at && !typing {
            // A recording cannot say where the cursor went, but keeps what
            // was typed before it did.
            if !rec.opening {
                if rec.start < rec.at.max(rec.reach) {
                    record::push(&record::spell(&self.buf.slice(rec.start, rec.at.max(rec.reach))));
                }
                rec.restarted = true;
            }
            rec.start = pos;
            rec.reach = pos;
            rec.opening = false;
        }
        rec.at = pos;
        rec.typed = None;
    }

    /// The primary cursor's insert ended at the cursor: `".` gets what it
    /// typed, and `'[ ']` bracket it.
    fn finish_insert(&mut self) {
        if self.quiet {
            return;
        }
        let (typed, _) = self.typed();
        let Some(rec) = self.insert.take() else {
            return;
        };
        register::set_inserted(typed.text());
        let end = self.cursor;
        if rec.opening || rec.start >= end {
            return;
        }
        self.marks.op_start = Some(rec.start);
        self.marks.op_end = Some(char_before(&self.buf, end));
    }

    /// What the primary cursor's insert has typed so far, and whether it
    /// started over on the way (see `InsertRecord`).
    fn typed(&self) -> (Typed, bool) {
        let Some(rec) = self.insert.filter(|rec| !rec.opening) else {
            return (Typed::default(), false);
        };
        let end = self.cursor;
        let slice = |a: Pos, b: Pos| if a < b { self.buf.slice(a, b) } else { String::new() };
        (Typed { before: slice(rec.start, end), after: slice(end, rec.reach) }, rec.restarted)
    }

    fn clear_pending(&mut self) {
        self.pending = Pending::default();
    }

    /// The editor selection of every cursor, primary first.
    fn current_selections(&self) -> Vec<Selection> {
        std::iter::once(self.selection_at(self.anchor, self.cursor))
            .chain(
                self.secondaries
                    .iter()
                    .map(|c| self.selection_at(c.anchor, c.pos)),
            )
            .collect()
    }

    /// One cursor's editor selection in the current mode.
    fn selection_at(&self, anchor: Pos, cursor: Pos) -> Selection {
        match self.mode {
            Mode::Normal | Mode::Insert => Selection {
                anchor: cursor,
                active: cursor,
            },
            Mode::Visual { linewise: false } => {
                if anchor <= cursor {
                    Selection {
                        anchor,
                        active: motion::right(&self.buf, cursor, 1),
                    }
                } else {
                    Selection {
                        anchor: motion::right(&self.buf, anchor, 1),
                        active: cursor,
                    }
                }
            }
            Mode::Visual { linewise: true } => {
                let (top, bottom, forward) = if anchor.line <= cursor.line {
                    (anchor.line, cursor.line, true)
                } else {
                    (cursor.line, anchor.line, false)
                };
                let start = Pos::new(top, 0);
                let end = if bottom + 1 < self.buf.line_count() {
                    Pos::new(bottom + 1, 0)
                } else {
                    Pos::new(bottom, self.buf.line_len(bottom))
                };
                if forward {
                    Selection { anchor: start, active: end }
                } else {
                    Selection { anchor: end, active: start }
                }
            }
        }
    }

    // ---- insert mode ------------------------------------------------------

    fn key_insert(&mut self, key: Key, edits: &mut Vec<Edit>) {
        if key == Key::Esc {
            self.mode = Mode::Normal;
            self.finish_insert();
            self.repeat_insert(edits);
            // `'^` is where the cursor was in insert mode, before the step
            // back: `gi` picks up typing exactly there.
            self.last_insert = Some(self.cursor);
            let back = if self.cursor.col > 0 {
                char_before(&self.buf, self.cursor)
            } else {
                self.cursor
            };
            self.cursor = self.clamp_normal(back);
            self.desired_col = self.cursor.col;
        }
        // Everything else in insert mode is handled natively by the editor.
    }

    /// `3ix<esc>`: a counted insert puts what it typed in `count - 1` more
    /// times as it ends, at the cursor; after `o` / `O`, each time on a line
    /// of its own below, indented as the line typed was (edit.c `ins_esc`,
    /// which repeats the insert the way `.` would). The text is the primary
    /// cursor's, which every cursor typed alike.
    fn repeat_insert(&mut self, edits: &mut Vec<Edit>) {
        let count = self.insert_count;
        let Some(typed) = self.esc_typed.as_ref().filter(|_| count > 1) else {
            return;
        };
        let text = typed.text();
        if text.is_empty() {
            return;
        }
        let at = self.cursor;
        let unit = if self.insert_opened {
            // The indent the line got before the typing started on it.
            let breaks = typed.before.matches('\n').count();
            let first = at.line.saturating_sub(breaks);
            let line = self.buf.line(first);
            let mut blank = line.len() - line.trim_start_matches([' ', '\t']).len();
            if breaks == 0 {
                let from = at.col.saturating_sub(utf16_len(&typed.before));
                blank = blank.min(crate::buffer::utf16_to_byte(line, from));
            }
            format!("\n{}{text}", &line[..blank])
        } else {
            text
        };
        let all = unit.repeat(count - 1);
        self.emit_edit(at, at, &all, edits);
        self.cursor = text_end(at, &all);
    }

    // ---- repeating and recording --------------------------------------------

    /// What a key's bookkeeping needs to know from before it runs.
    fn key_start(&self, key: Key) -> KeyStart {
        let p = &self.pending;
        let insert = self.mode == Mode::Insert;
        let prefix = !insert
            && match (p.awaiting, key) {
                (Awaiting::Register, _) => true,
                (Awaiting::None, Key::Char('"')) => p.op.is_none(),
                (Awaiting::None, Key::Char(c @ '0'..='9')) => c != '0' || p.has_count(),
                _ => false,
            };
        let idle = !insert && p.awaiting == Awaiting::None && p.op.is_none();
        KeyStart {
            fresh: p.is_empty(),
            prefix,
            count: p.has_count().then(|| p.count()),
            register: p.register,
            visual: self.visual_area().filter(|_| p.is_empty()).map(|area| self.visual_size(area)),
            dot: idle && self.mode == Mode::Normal && key == Key::Char('.'),
            easy: matches!(p.awaiting, Awaiting::Easy(_)) || self.lead > 0,
            insert,
            typed: (insert && key == Key::Esc).then(|| self.typed()),
            recording: record::recording().is_some(),
        }
    }

    /// After every key: keep the recording and the redo record.
    ///
    /// Vim builds its redo buffer as a command runs, each part appending the
    /// keys it took (getchar.c `AppendToRedobuff`). Here the keys of the
    /// command under way are collected as they arrive, and when it finishes
    /// having changed something — an edit, an indent, a line opened, insert
    /// mode entered — they become the redo record. Motions, yanks, searches,
    /// undo and the like change nothing and leave it be. So does a command
    /// line, which `.` does not repeat (`@:` does).
    fn after_key(&mut self, key: Key, start: KeyStart, fx: &Effects) {
        if start.recording && !self.fed && record::recording().is_some() && (!start.insert || key == Key::Esc) {
            if let Some((typed, _)) = &start.typed {
                record::push(&record::spell(&typed.text()));
            }
            record::push(&key.notation());
        }
        if start.insert {
            if let Some((typed, restarted)) = start.typed
                && self.mode != Mode::Insert
            {
                self.close_redo(typed, restarted);
            }
            return;
        }
        if start.fresh {
            self.cmd.clear();
            self.cmd_visual = start.visual;
        }
        if !start.prefix {
            self.cmd.push(key);
        }
        if !self.pending.is_empty() {
            return;
        }
        let keys = std::mem::take(&mut self.cmd);
        let visual = self.cmd_visual.take();
        let changed = !fx.edits.is_empty()
            || self.mode == Mode::Insert
            || fx.commands.iter().any(|c| matches!(c, Command::IndentLines { .. } | Command::OpenLine { .. }));
        // `.`, `@` and `q` change nothing themselves: what `.` repeats stays
        // what it was, and the keys a register runs record their own changes.
        let runs_others = start.dot || matches!(keys.first(), Some(Key::Char(':' | '@' | 'q')));
        if changed && !runs_others && !start.easy {
            let (count, register) = (start.count, start.register);
            self.redo = Some(Redo { keys, count, register, visual, insert: None });
            self.redo_inserting = self.mode == Mode::Insert;
        }
    }

    /// The insert a change went on into has ended: what it typed is the rest
    /// of the change. An insert the cursor was moved in started over, as
    /// Vim's does (edit.c `stop_arrow`), and repeats as an `i` of what was
    /// typed since.
    fn close_redo(&mut self, typed: Typed, restarted: bool) {
        let inserting = std::mem::take(&mut self.redo_inserting);
        if restarted && typed != Typed::default() {
            let keys = vec![Key::Char('i')];
            self.redo = Some(Redo { keys, count: None, register: None, visual: None, insert: Some(typed) });
        } else if inserting && let Some(redo) = &mut self.redo {
            redo.insert = Some(typed);
        }
    }

    /// Before `.` runs: a count given to it replaces the change's own, and
    /// a numbered register steps on to the next (getchar.c `start_redo`).
    /// Done once for the key, however many cursors repeat the change.
    fn step_redo(&mut self) {
        let count = self.pending.has_count().then(|| self.pending.count());
        let Some(redo) = &mut self.redo else {
            return;
        };
        if count.is_some() {
            redo.count = count;
        }
        if let Some(r @ '1'..='8') = redo.register {
            redo.register = char::from_digit(r as u32 - '0' as u32 + 1, 10);
        }
    }

    /// `.`: the last change again, at the cursor (`:help .`) — its keys run
    /// once more, with the count and register they had (or the ones `.` was
    /// given), on a selection the size of the first one if it was made in
    /// visual mode. What its insert typed then goes in as an edit, with the
    /// cursor left where `<esc>` would leave it.
    fn dot_repeat(&mut self, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let Some(redo) = self.redo.clone() else {
            return self.fail();
        };
        self.clear_pending();
        let typed = redo.insert.clone().unwrap_or_default();
        if let [Key::Char(c @ ('o' | 'O'))] = redo.keys[..] {
            return self.redo_open_line(c, redo.count, &typed, commands);
        }
        if let Some(size) = redo.visual {
            self.reselect(size);
        }
        self.pending.count1 = redo.count.unwrap_or(0);
        self.pending.register = redo.register;
        let first = edits.len();
        // `replay`: the keys are the change's, not a new EasyMotion trigger.
        self.replay = true;
        for &key in &redo.keys {
            self.key_noting_visual(key, edits, commands);
        }
        self.replay = false;
        if self.mode == Mode::Insert {
            self.replay_typing(&typed, first, edits);
        }
    }

    /// The text a repeated change's insert typed, put in where its keys left
    /// the insert cursor — as many times over as a counted insert puts it,
    /// the repeats going in at the cursor as `<esc>` puts them — and the
    /// insert ended as `<esc>` ends it. It is one edit, joined to the
    /// change's own when it starts where that one ends: the delete of `cw`
    /// and the word typed in its place go to the host together, in the
    /// coordinates of the document it holds, and are one undo step.
    fn replay_typing(&mut self, typed: &Typed, first: usize, edits: &mut Vec<Edit>) {
        let at = self.cursor;
        let repeats = typed.text().repeat(self.insert_count.saturating_sub(1));
        let text = format!("{}{repeats}{}", typed.before, typed.after);
        if !text.is_empty() {
            let edit = Edit { start: at, end: at, text: text.clone() };
            let joins = edits.len() > first && edits.last().is_some_and(|e| text_end(e.start, &e.text) == at);
            self.edit_mirror(Shift::new(&self.buf, at, at, &text), &edit);
            match edits.last_mut() {
                Some(last) if joins => last.text.push_str(&text),
                _ => edits.push(edit),
            }
        }
        self.cursor = text_end(at, &format!("{}{repeats}", typed.before));
        // The insert is over already: `".` is what it typed, once.
        if !self.quiet {
            self.insert = None;
            register::set_inserted(typed.text());
        }
        self.key_insert(Key::Esc, edits);
    }

    /// `.` after `o` / `O`. The line is opened the way they opened it, by
    /// the host's line-insert — the indent the language wants is the host's
    /// to know — so the repeat goes back to the host as keys: the count, the
    /// `o`, what was typed and the `<esc>`, for it to run as it runs a
    /// register. Emitted once; the host's line-insert opens a line at every
    /// cursor, and the text then goes in at each.
    fn redo_open_line(&mut self, key: char, count: Option<usize>, typed: &Typed, commands: &mut Vec<Command>) {
        if self.quiet {
            return;
        }
        let mut keys: Vec<String> = count.map_or(String::new(), |n| n.to_string()).chars().map(String::from).collect();
        keys.push(key.to_string());
        keys.extend(typed.text().chars().map(|c| if c == '\n' { Key::Enter } else { Key::Char(c) }.label()));
        keys.push(Key::Esc.label());
        commands.push(Command::Execute { keys, count: 1 });
    }

    /// The size of a selection, as a visual change repeats it.
    fn visual_size(&self, area: VisualArea) -> VisualSize {
        let (s, e) = ordered(area.anchor, area.cursor);
        let lines = e.line - s.line + 1;
        let cols = if area.linewise {
            0
        } else if self.desired_col == STICKY_EOL {
            STICKY_EOL
        } else if lines == 1 {
            self.buf.slice(s, motion::right(&self.buf, e, 1)).chars().count()
        } else {
            e.col
        };
        VisualSize { linewise: area.linewise, lines, cols }
    }

    /// Select from the cursor what a repeated visual change works on: as
    /// many lines as the first, and charwise as many characters on the line,
    /// or up to the same column on the last of several lines (normal.c
    /// `do_pending_operator`, `redo_VIsual_busy`).
    fn reselect(&mut self, size: VisualSize) {
        let start = self.cursor;
        let line = (start.line + size.lines - 1).min(self.buf.last_line());
        let end = if size.linewise {
            Pos::new(line, start.col)
        } else if size.cols == STICKY_EOL {
            Pos::new(line, self.buf.line_len(line))
        } else if size.lines == 1 {
            motion::right(&self.buf, start, size.cols.saturating_sub(1))
        } else {
            Pos::new(line, size.cols)
        };
        self.anchor = start;
        self.cursor = self.clamp_normal(end);
        self.mode = Mode::Visual { linewise: size.linewise };
    }

    /// `q`: stop the recording in progress, or start one with the register
    /// named next. Decided by whether one was going on when the key came,
    /// so every cursor reads the `q` alike.
    fn key_record(&mut self) {
        if !self.was_recording {
            self.pending.awaiting = Awaiting::Record;
            self.pending.keys.push('q');
            return;
        }
        self.clear_pending();
        // The recording is one, whatever the cursors: the primary's to end.
        if !self.quiet
            && let Some((name, keys)) = record::stop()
        {
            register::set(name, Register::new(keys, false));
        }
    }

    /// `q{0-9a-zA-Z"}`: record what is typed from here into that register;
    /// an uppercase name appends to it (`:help q`).
    fn resolve_record(&mut self, key: Key) {
        self.clear_pending();
        match key {
            Key::Char(c) if record::recordable(c) => {
                if !self.quiet {
                    record::start(c);
                }
            }
            Key::Esc => {}
            _ => self.fail(),
        }
    }

    /// `@{reg}`: run the register's text as keys, `count` times over (`:help
    /// @`); `@@` runs the register run last and `@:` the last command line.
    /// The engine reads the register and the host runs it (see
    /// `Command::Execute`).
    fn resolve_execute(&mut self, key: Key, commands: &mut Vec<Command>) {
        let count = self.pending.count();
        self.clear_pending();
        let name = match key {
            Key::Char('@') => record::last_run(),
            Key::Char(c) => Some(c),
            Key::Esc => return,
            _ => return self.fail(),
        };
        let Some(name) = name else {
            self.message = Some("E748: No previously used register".into());
            return self.fail();
        };
        let keys: Vec<Key> = match name {
            // The line as it was typed: a `<b>` in it is three characters.
            ':' => match register::get(':') {
                Some(line) => {
                    let typed = std::iter::once(':').chain(line.text.chars()).map(Key::Char);
                    typed.chain([Key::Enter]).collect()
                }
                None => {
                    self.message = Some("E30: No previous command line".into());
                    return self.fail();
                }
            },
            '_' => Vec::new(),
            c if register::readable(c) => {
                self.register(c).map_or(Vec::new(), |reg| record::keys(&reg.text, reg.linewise))
            }
            _ => return self.fail(),
        };
        if keys.is_empty() {
            return self.fail();
        }
        if !self.quiet {
            record::set_last_run(name);
            commands.push(Command::Execute { keys: keys.iter().map(|k| k.label()).collect(), count });
        }
    }

    /// A command that could not do what it was asked: drop what is pending,
    /// and tell a register being run, which stops there as Vim's does.
    fn fail(&mut self) {
        self.failed = true;
        self.clear_pending();
    }

    // ---- normal / visual dispatch ------------------------------------------

    fn key_normal(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        match self.pending.awaiting {
            Awaiting::Find(kind) => return self.resolve_find(kind, key, edits, commands),
            Awaiting::Replace => return self.resolve_replace(key, edits),
            Awaiting::G => return self.resolve_g(key, edits, commands),
            Awaiting::Z => return self.resolve_z(key, commands),
            Awaiting::Quit => return self.resolve_quit(key, commands),
            Awaiting::Bracket { forward } => {
                return self.resolve_bracket(forward, key, edits, commands);
            }
            Awaiting::Object { around } => return self.resolve_object(around, key, edits, commands),
            Awaiting::Search { backward } => return self.resolve_search(backward, key, edits, commands),
            Awaiting::Ex => return self.resolve_ex(key, edits, commands),
            Awaiting::Easy(stage) => return self.resolve_easy(stage, key, edits, commands),
            Awaiting::Register => return self.resolve_register(key),
            Awaiting::Mark { linewise } => return self.resolve_mark(linewise, key, edits, commands),
            Awaiting::SetMark => return self.resolve_set_mark(key),
            Awaiting::Record => return self.resolve_record(key),
            Awaiting::Execute => return self.resolve_execute(key, commands),
            Awaiting::None => {}
        }

        // The EasyMotion trigger sits in front of every other normal-mode key,
        // the way a `<Leader>` mapping does.
        if self.easy.enabled && !self.replay && !self.multi && self.easy_lead(key, edits, commands) {
            return;
        }

        let visual = matches!(self.mode, Mode::Visual { .. });
        match key {
            Key::Esc => {
                if visual && self.pending.is_empty() {
                    self.mode = Mode::Normal;
                }
                self.clear_pending();
            }
            Key::Char(c @ '0'..='9') if c != '0' || self.pending.has_count() => {
                self.pending.add_digit(c as usize - '0' as usize);
                self.pending.keys.push(c);
            }
            // A command that would fill a read-only register (`".`, `":`,
            // `"/`) does nothing at all, as in Vim.
            Key::Char('d' | 'c' | 'y' | 'x' | 'X' | 's' | 'S' | 'D' | 'C' | 'Y')
                if self.pending.register.is_some_and(|r| !register::writable(r)) =>
            {
                self.clear_pending()
            }
            // `"x` names the register for the command after it, and comes
            // before any operator (normal.c nv_regname).
            Key::Char('"') if self.pending.op.is_none() => {
                if self.pending.count1 > 0 {
                    self.pending.count0 = self.pending.count0.max(1).saturating_mul(self.pending.count1);
                    self.pending.count1 = 0;
                }
                self.pending.awaiting = Awaiting::Register;
                self.pending.keys.push('"');
            }
            Key::Char(c @ ('\'' | '`')) => {
                self.pending.awaiting = Awaiting::Mark { linewise: c == '\'' };
                self.pending.keys.push(c);
            }
            // `q` records and `@` runs; neither is a motion, so a pending
            // operator does not take them.
            Key::Char('q') if self.pending.op.is_none() => self.key_record(),
            Key::Char('@') if self.pending.op.is_none() => {
                self.pending.awaiting = Awaiting::Execute;
                self.pending.keys.push('@');
            }
            Key::Char(c) if Op::from_char(c).is_some() && !visual => {
                let op = Op::from_char(c).unwrap();
                match self.pending.op {
                    Some(prev) if prev == op => self.doubled_operator(op, edits, commands),
                    Some(_) => self.fail(),
                    None => {
                        self.pending.op = Some(op);
                        self.pending.keys.push(c);
                    }
                }
            }
            Key::Char(c) if Op::from_char(c).is_some() && visual => {
                self.visual_operator(Op::from_char(c).unwrap(), edits, commands)
            }
            Key::Char('g') => {
                self.pending.awaiting = Awaiting::G;
                self.pending.keys.push('g');
            }
            Key::Char('z') if self.pending.op.is_none() && !visual => {
                self.pending.awaiting = Awaiting::Z;
                self.pending.keys.push('z');
            }
            Key::Char('Z') if self.pending.op.is_none() && !visual => {
                self.pending.awaiting = Awaiting::Quit;
                self.pending.keys.push('Z');
            }
            Key::Char(c @ ('[' | ']')) => {
                self.pending.awaiting = Awaiting::Bracket { forward: c == ']' };
                self.pending.keys.push(c);
            }
            Key::Char(c @ ('i' | 'a')) if self.pending.op.is_some() || visual => {
                self.pending.awaiting = Awaiting::Object { around: c == 'a' };
                self.pending.keys.push(c);
            }
            Key::Char(c @ ('f' | 'F' | 't' | 'T')) => {
                self.pending.awaiting = Awaiting::Find(match c {
                    'f' => FindKind::To,
                    'F' => FindKind::ToBack,
                    't' => FindKind::Till,
                    _ => FindKind::TillBack,
                });
                self.pending.keys.push(c);
            }
            Key::Char(c @ (';' | ',')) => {
                if let Some((kind, target)) = self.last_find {
                    let kind = if c == ',' { kind.reversed() } else { kind };
                    self.finish_find(kind, target, edits, commands);
                } else {
                    self.fail();
                }
            }
            Key::Char('r') => {
                self.pending.awaiting = Awaiting::Replace;
                self.pending.keys.push('r');
            }
            Key::Char(c @ ('/' | '?')) => {
                self.pending.awaiting = Awaiting::Search { backward: c == '?' };
                self.pending.keys.push(c);
                // An empty Active opens the session: the host snapshots the
                // viewport it will restore if the search is abandoned.
                self.search_ui = Some(SearchUi::Active { matches: Vec::new(), current: None });
            }
            Key::Char(':') => {
                self.pending.awaiting = Awaiting::Ex;
                self.pending.keys.push(':');
                // `:` in visual mode prefills the selection's range, as Vim's does.
                if visual {
                    self.pending.prompt.push_str("'<,'>");
                }
            }
            _ => self.simple_key(key, edits, commands),
        }
    }

    /// Keys that resolve immediately: motions and standalone actions.
    fn simple_key(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let count = self.pending.count();
        let visual = matches!(self.mode, Mode::Visual { .. });
        let cur = self.cursor;
        match key {
            // -- horizontal motions
            // Both fail where they cannot move — `h` in the first column, `l`
            // on the last character unless an operator takes it — which is
            // what stops a register being run (`:help @`).
            Key::Char('h') | Key::Backspace => {
                let target = motion::left(cur, count);
                if target == cur {
                    return self.fail();
                }
                self.do_motion(target, MotionKind::Exclusive, edits, commands)
            }
            Key::Char('l') | Key::Char(' ') => {
                let target = motion::right(&self.buf, cur, count);
                let stays = if self.pending.op.is_some() { target } else { self.clamp_normal(target) };
                if stays == cur {
                    return self.fail();
                }
                self.do_motion(target, MotionKind::Exclusive, edits, commands)
            }
            Key::Char('0') => {
                self.do_motion(Pos::new(cur.line, 0), MotionKind::Exclusive, edits, commands)
            }
            Key::Char('^') => self.do_motion(
                Pos::new(cur.line, self.buf.first_non_blank(cur.line)),
                MotionKind::Exclusive,
                edits,
                commands,
            ),
            Key::Char('$') => {
                let line = (cur.line + count - 1).min(self.buf.last_line());
                let target = Pos::new(line, self.buf.line_len(line));
                self.do_motion(target, MotionKind::Exclusive, edits, commands);
                self.desired_col = STICKY_EOL;
            }
            // -- word motions
            Key::Char(c @ ('w' | 'W')) => self.motion_word_forward(c == 'W', count, edits, commands),
            // `b` at the start of the buffer and `e` at its end fail.
            Key::Char(c @ ('b' | 'B')) => match motion::word_back(&self.buf, cur, c == 'B', count) {
                target if target == cur => self.fail(),
                target => self.do_motion(target, MotionKind::Exclusive, edits, commands),
            },
            Key::Char(c @ ('e' | 'E')) => match motion::word_end(&self.buf, cur, c == 'E', count) {
                target if self.clamp_normal(target) == cur => self.fail(),
                target => self.do_motion(target, MotionKind::Inclusive, edits, commands),
            },
            // -- vertical motions
            // CTRL-N / CTRL-P are j / k (`:help CTRL-N`)
            Key::Char('j') | Key::Ctrl('n') => self.vertical(count as isize, false, edits, commands),
            Key::Char('k') | Key::Ctrl('p') => self.vertical(-(count as isize), false, edits, commands),
            Key::Enter | Key::Char('+') => self.vertical(count as isize, true, edits, commands),
            Key::Char('-') => self.vertical(-(count as isize), true, edits, commands),
            // The jumps (`:help jump-motions`) leave `''` and a jumplist entry
            // behind, operator or not; `{ } ( ) %` are also among the motions
            // whose deletes always go to `"1`.
            Key::Char('G') => {
                let line = if self.pending.has_count() {
                    (count - 1).min(self.buf.last_line())
                } else {
                    self.buf.last_line()
                };
                let target = Pos::new(line, self.buf.first_non_blank(line));
                self.set_pcmark();
                self.do_motion(target, MotionKind::Linewise, edits, commands);
            }
            Key::Char(c @ ('{' | '}')) => {
                let target = motion::paragraph(&self.buf, cur, c == '}', count);
                if target == cur {
                    return self.fail();
                }
                self.set_pcmark();
                self.pending.reg_one = true;
                self.do_motion(target, MotionKind::Exclusive, edits, commands)
            }
            Key::Char(c @ ('(' | ')')) => match motion::sentence(&self.buf, cur, c == ')', count) {
                Some(target) => {
                    self.set_pcmark();
                    self.pending.reg_one = true;
                    self.sentence_motion(target, edits, commands)
                }
                None => self.fail(),
            },
            Key::Char('%') => match motion::matching_pair(&self.buf, cur) {
                Some(target) => {
                    self.set_pcmark();
                    self.pending.reg_one = true;
                    self.do_motion(target, MotionKind::Inclusive, edits, commands)
                }
                None => self.fail(),
            },
            // -- search motions
            Key::Char(c @ ('n' | 'N')) => match self.last_search.clone() {
                Some(last) => {
                    let backward = last.backward != (c == 'N');
                    self.run_search(&last.pattern, backward, edits, commands);
                }
                None => self.fail(),
            },
            Key::Char(c @ ('*' | '#')) => self.search_word(c == '#', edits, commands),
            // -- line, column and screen motions
            // `_`: `count - 1` lines down, onto the first non-blank. Linewise,
            // which is what makes `d_` the same as `dd` (vim :help _).
            Key::Char('_') => {
                let line = (cur.line + count - 1).min(self.buf.last_line());
                let target = Pos::new(line, self.buf.first_non_blank(line));
                self.do_motion(target, MotionKind::Linewise, edits, commands);
            }
            Key::Char('|') => self.motion_column(edits, commands),
            Key::Char(c @ ('H' | 'M' | 'L')) => self.motion_screen(c, edits, commands),
            // `guu` / `gUU` / `g~~`: a case operator doubled by its own key.
            Key::Char(c) if self.pending.op.is_some_and(|op| Op::case_from_char(c) == Some(op)) => {
                self.doubled_case_operator(edits, commands)
            }

            // A pending operator combines only with the motions above; any
            // other key aborts it, like Vim.
            _ if self.pending.op.is_some() => self.fail(),

            // -- operatorless actions (normal mode)
            Key::Char('.') if !visual => self.dot_repeat(edits, commands),
            Key::Char('x') if !visual => self.delete_chars(count, edits, false),
            Key::Char('X') if !visual => self.delete_back(count, edits),
            Key::Char('s') if !visual => self.delete_chars(count, edits, true),
            Key::Char('D') if !visual => self.delete_to_eol(edits, false),
            Key::Char('C') if !visual => self.delete_to_eol(edits, true),
            Key::Char('Y') if !visual => self.linewise_yank(count),
            Key::Char('S') if !visual => self.linewise_change(cur.line, count, edits),
            Key::Char('~') if !visual => self.toggle_case(count, edits),
            // `&`: the last `:s` again, on this line, without its flags.
            Key::Char('m') => {
                self.pending.awaiting = Awaiting::SetMark;
                self.pending.keys.push('m');
            }
            // `ctrl-o` / `ctrl-i`: back and forth through the jumplist. Not
            // motions — an operator does not take them — and not jumps
            // either: walking the list leaves `''` alone.
            Key::Ctrl(c @ ('o' | 'i')) => {
                let step = if c == 'o' { -(count as isize) } else { count as isize };
                let Some(to) = self.marks.step_jumps(step, cur) else {
                    return self.fail();
                };
                self.cursor = self.clamp_normal(to);
                self.desired_col = self.cursor.col;
                self.clear_pending();
            }
            Key::Char('&') if !visual => {
                let line = cur.line;
                self.substitute(
                    &ex::Substitute { first: line, last: line, ..ex::Substitute::default() },
                    edits,
                );
            }
            Key::Char('J') => self.join_lines(count, edits),
            Key::Char('p') => self.paste(false, count, edits),
            Key::Char('P') => self.paste(true, count, edits),
            Key::Char('u') if !visual => {
                for _ in 0..count {
                    commands.push(Command::Undo);
                }
                self.clear_pending();
            }
            Key::Ctrl('r') => {
                for _ in 0..count {
                    commands.push(Command::Redo);
                }
                self.clear_pending();
            }

            // -- insert entries
            Key::Char('i') if !visual => self.enter_insert_cleared(cur),
            Key::Char('a') if !visual => {
                let at = motion::right(&self.buf, cur, 1);
                self.enter_insert_cleared(at)
            }
            Key::Char('I') if !visual => {
                self.enter_insert_cleared(Pos::new(cur.line, self.buf.first_non_blank(cur.line)))
            }
            Key::Char('A') if !visual => {
                self.enter_insert_cleared(Pos::new(cur.line, self.buf.line_len(cur.line)))
            }
            Key::Char('o') if !visual => self.open_line(false, commands),
            Key::Char('O') if !visual => self.open_line(true, commands),

            // -- visual mode entry / manipulation
            Key::Char('v') => self.toggle_visual(false),
            Key::Char('V') => self.toggle_visual(true),
            Key::Char('o') if visual => {
                std::mem::swap(&mut self.anchor, &mut self.cursor);
                self.desired_col = self.cursor.col;
                self.clear_pending();
            }

            // -- visual operator synonyms
            Key::Char('x') if visual => self.visual_operator(Op::Delete, edits, commands),
            Key::Char('s') if visual => self.visual_operator(Op::Change, edits, commands),
            Key::Char('D' | 'X') if visual => {
                self.force_linewise_visual();
                self.visual_operator(Op::Delete, edits, commands)
            }
            Key::Char('C' | 'S') if visual => {
                self.force_linewise_visual();
                self.visual_operator(Op::Change, edits, commands)
            }
            Key::Char('Y') if visual => {
                self.force_linewise_visual();
                self.visual_operator(Op::Yank, edits, commands)
            }
            Key::Char('~') if visual => self.visual_toggle_case(edits),
            Key::Char('u') if visual => self.visual_case(Op::Lower, edits),
            Key::Char('U') if visual => self.visual_case(Op::Upper, edits),
            Key::Char(c @ ('I' | 'A')) if visual => self.visual_insert(c == 'A'),

            // -- numbers and scrolling
            Key::Ctrl(c @ ('a' | 'x')) => self.add_sub(c == 'x', count, false, edits),
            Key::Ctrl(c @ ('e' | 'y')) => {
                commands.push(Command::ScrollLines {
                    down: c == 'e',
                    lines: count,
                    move_cursor: !visual && !self.multi,
                });
                self.clear_pending();
            }

            _ => self.fail(),
        }
    }

    /// `i a I A gi gI`: the insert entries a count repeats, where the
    /// change operators' counts go to their motions instead.
    fn enter_insert_cleared(&mut self, at: Pos) {
        let count = self.pending.count();
        self.clear_pending();
        self.enter_insert(at);
        self.insert_count = count;
    }

    /// `o` / `O`: hand the line split to the host so the new line is indented
    /// the way the language wants it. The cursor stays put — the host's
    /// document change and cursor move come back through `apply_change` and
    /// `set_cursors` and land it — but the mode switches now, so the keys
    /// after this one go straight to the editor as insert-mode typing.
    fn open_line(&mut self, above: bool, commands: &mut Vec<Command>) {
        self.insert_count = self.pending.count();
        self.insert_opened = true;
        self.clear_pending();
        self.mode = Mode::Insert;
        self.start_insert(true);
        commands.push(Command::OpenLine { above });
    }

    fn toggle_visual(&mut self, linewise: bool) {
        self.clear_pending();
        match self.mode {
            Mode::Visual { linewise: cur } if cur == linewise => self.mode = Mode::Normal,
            Mode::Visual { .. } => self.mode = Mode::Visual { linewise },
            _ => {
                self.anchor = self.cursor;
                self.mode = Mode::Visual { linewise };
            }
        }
    }

    fn force_linewise_visual(&mut self) {
        if let Mode::Visual { linewise } = &mut self.mode {
            *linewise = true;
        }
    }

    // ---- motion plumbing ---------------------------------------------------

    fn motion_word_forward(
        &mut self,
        big: bool,
        count: usize,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let cur = self.cursor;
        // `cw`/`cW` on a non-blank acts like `ce` (vim :help cw).
        if self.pending.op == Some(Op::Change)
            && self.buf.char_at(cur).is_some_and(|ch| !ch.is_whitespace())
        {
            let target = motion::word_end(&self.buf, cur, big, count);
            return self.do_motion(target, MotionKind::Inclusive, edits, commands);
        }
        let mut target = motion::word_forward(&self.buf, cur, big, count);
        // Operator + w stops at end of line when the last word moved over
        // ends there (vim :help word), instead of eating the newline.
        if self.pending.op.is_some() && target.line > cur.line {
            let eol = Pos::new(cur.line, self.buf.line_len(cur.line));
            let crossed = self.buf.slice(eol, target);
            let line_had_word = self
                .buf
                .slice(cur, eol)
                .chars()
                .any(|ch| !ch.is_whitespace());
            if line_had_word && crossed.chars().all(char::is_whitespace) {
                target = eol;
            }
        }
        self.do_motion(target, MotionKind::Exclusive, edits, commands);
    }

    fn vertical(
        &mut self,
        dy: isize,
        first_non_blank: bool,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let desired = self.desired_col;
        let had_op = self.pending.op.is_some();
        let line = self
            .cursor
            .line
            .saturating_add_signed(dy)
            .min(self.buf.last_line());
        // Vim moves as far as it can, and fails only where it cannot move
        // at all: `j` on the last line, `k` on the first.
        if line == self.cursor.line {
            return self.fail();
        }
        let col = if first_non_blank {
            self.buf.first_non_blank(line)
        } else {
            let len = self.buf.line_len(line);
            desired.min(self.last_char_col(line, len))
        };
        self.do_motion(Pos::new(line, col), MotionKind::Linewise, edits, commands);
        if !first_non_blank && !had_op {
            self.desired_col = desired;
        }
    }

    /// `|`: to column `count` (the first without one), exclusive. Vim keeps
    /// wanting that column when the line is too short for it, so a `j`
    /// afterwards reaches for it again (vim :help bar). Columns are chars
    /// here, not screen cells: a tab is one.
    fn motion_column(&mut self, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let want = if self.pending.has_count() { self.pending.count() - 1 } else { 0 };
        let had_op = self.pending.op.is_some();
        let line = self.cursor.line;
        let cols = chars_with_cols(self.buf.line(line));
        let col = cols.get(want.min(cols.len().saturating_sub(1))).map_or(0, |&(c, _)| c);
        self.do_motion(Pos::new(line, col), MotionKind::Exclusive, edits, commands);
        if !had_op {
            self.desired_col = cols.get(want).map_or(want, |&(c, _)| c);
        }
    }

    /// `H` / `M` / `L`: the top, middle or bottom line on screen, onto its
    /// first non-blank; linewise. A count is lines in from the top (`H`) or
    /// up from the bottom (`L`), and like Vim's it is not held to the screen
    /// — `L` stops at the first line of the buffer, `H` at the last. The
    /// screen is the one the host last reported (`set_view`), the whole
    /// buffer before it has; `'scrolloff'` is not applied, as the engine
    /// cannot know VSCode's.
    fn motion_screen(&mut self, which: char, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let (top, bottom) = self.view.unwrap_or((0, self.buf.last_line()));
        let bottom = bottom.min(self.buf.last_line());
        let n = self.pending.count() - 1;
        let line = match which {
            'H' => top.saturating_add(n),
            'L' => bottom.saturating_sub(n),
            _ => top + bottom.saturating_sub(top) / 2,
        }
        .min(self.buf.last_line());
        let target = Pos::new(line, self.buf.first_non_blank(line));
        self.set_pcmark();
        self.do_motion(target, MotionKind::Linewise, edits, commands);
    }

    /// Column of the last non-blank char of a line (0 for a blank line),
    /// where `g_` lands.
    fn last_non_blank(&self, line: usize) -> usize {
        chars_with_cols(self.buf.line(line))
            .iter()
            .rev()
            .find(|(_, ch)| !ch.is_whitespace())
            .map_or(0, |&(c, _)| c)
    }

    /// Route a resolved motion: either move the cursor or feed the pending
    /// operator. `target` may sit one past the last char (exclusive end).
    fn do_motion(
        &mut self,
        target: Pos,
        kind: MotionKind,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        if let Some(op) = self.pending.op {
            let (s, e) = ordered(self.cursor, target);
            let (s, e) = match kind {
                MotionKind::Exclusive => (s, e),
                MotionKind::Inclusive => (s, motion::right(&self.buf, e, 1)),
                MotionKind::Linewise => {
                    self.apply_operator_linewise(op, s.line, e.line, edits, commands);
                    self.clear_pending();
                    self.desired_col = self.cursor.col;
                    return;
                }
            };
            self.apply_operator_charwise(op, s, e, edits, commands);
            self.clear_pending();
            self.desired_col = self.cursor.col;
        } else {
            self.cursor = match self.mode {
                Mode::Insert => self.clamp_insert(target),
                _ => self.clamp_normal(target),
            };
            self.desired_col = self.cursor.col;
            self.clear_pending();
        }
    }

    // ---- awaiting resolutions ----------------------------------------------

    fn resolve_find(
        &mut self,
        kind: FindKind,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        match key {
            Key::Char(c) => {
                self.last_find = Some((kind, c));
                self.finish_find(kind, c, edits, commands);
            }
            Key::Esc => self.clear_pending(),
            _ => self.fail(),
        }
    }

    fn finish_find(
        &mut self,
        kind: FindKind,
        target: char,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let count = self.pending.count();
        match motion::find_char(&self.buf, self.cursor, kind, target, count) {
            Some(pos) => {
                let mk = if kind.forward() {
                    MotionKind::Inclusive
                } else {
                    MotionKind::Exclusive
                };
                self.pending.awaiting = Awaiting::None;
                self.do_motion(pos, mk, edits, commands);
            }
            None => self.fail(),
        }
    }

    /// Collect the `/` or `?` pattern until `<cr>` runs it. `<esc>`, and
    /// `<bs>` past the start of the pattern, abort the search like Vim.
    /// Every edit of the pattern refreshes the incremental preview.
    fn resolve_search(
        &mut self,
        backward: bool,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        match key {
            Key::Char(c) => {
                self.pending.prompt.push(c);
                self.search_preview(backward);
            }
            Key::Backspace => {
                if self.pending.prompt.pop().is_none() {
                    self.search_ui = Some(SearchUi::Cancelled);
                    self.clear_pending();
                } else {
                    self.search_preview(backward);
                }
            }
            Key::Enter => {
                // An empty pattern reuses the last one.
                let pattern = match (self.pending.prompt.as_str(), &self.last_search) {
                    ("", None) => {
                        self.search_ui = Some(SearchUi::Cancelled);
                        return self.clear_pending();
                    }
                    ("", Some(last)) => last.pattern.clone(),
                    (typed, _) => typed.to_string(),
                };
                self.search_ui = Some(SearchUi::Committed);
                self.pending.awaiting = Awaiting::None;
                self.pending.prompt.clear();
                self.last_search = Some(Search {
                    pattern: pattern.clone(),
                    backward,
                });
                self.run_search(&pattern, backward, edits, commands);
            }
            Key::Esc => {
                self.search_ui = Some(SearchUi::Cancelled);
                self.clear_pending();
            }
            Key::Ctrl(_) => {}
        }
    }

    /// Matches of the pattern typed so far, for the host to paint and peek
    /// at. An empty or unparseable pattern previews as no matches — the
    /// half-typed `\(` is on its way somewhere, not an error yet.
    fn search_preview(&mut self, backward: bool) {
        // One preview per key, painted from the primary cursor; none for a
        // pattern `.` is typing again, which nobody is watching being typed.
        if self.quiet || self.replay {
            return;
        }
        let (matches, current) = search::Pattern::parse(&self.pending.prompt)
            .map(|pat| {
                pat.budget_for(&self.buf);
                search::preview(&self.buf, self.cursor, &pat, backward)
            })
            .unwrap_or_default();
        self.search_ui = Some(SearchUi::Active { matches, current });
    }

    /// `*` / `#`: search for the keyword under the cursor, whole-word. Vim
    /// first parks the cursor on the keyword's first char, which is what
    /// keeps the search from matching the keyword the cursor sits in.
    fn search_word(&mut self, backward: bool, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let Some((start, word)) = search::word_under_cursor(&self.buf, self.cursor) else {
            return self.fail();
        };
        self.cursor = start;
        self.desired_col = start.col;
        let pattern = format!("\\<{}\\>", search::escape_literal(&word));
        self.last_search = Some(Search {
            pattern: pattern.clone(),
            backward,
        });
        self.run_search(&pattern, backward, edits, commands);
    }

    /// Move to the count-th match of `pattern`. Search is an exclusive
    /// motion, so it composes with a pending operator (`d/foo<cr>`, `dn`).
    /// A plain search also reports "match N of M", like Vim's search count.
    fn run_search(
        &mut self,
        pattern: &str,
        backward: bool,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let count = self.pending.count();
        let pat = match search::Pattern::parse(pattern) {
            Ok(p) => p,
            Err(msg) => {
                self.message = Some(msg);
                return self.fail();
            }
        };
        pat.budget_for(&self.buf);
        // A search is a jump, and one of the motions whose deletes always go
        // to `"1`.
        self.pending.reg_one = true;
        if self.pending.op.is_some() || self.quiet {
            // Feeding an operator: the early-exit find, and no report — the
            // cursor doesn't land on the match, the operator eats up to it.
            // A secondary cursor takes the same path: it moves, but nothing
            // shows its count.
            match search::find(&self.buf, self.cursor, &pat, backward, count) {
                Some(pos) => {
                    self.pending.awaiting = Awaiting::None;
                    self.set_pcmark();
                    self.do_motion(pos, MotionKind::Exclusive, edits, commands);
                }
                None => {
                    self.message = Some(gave_up_or_missing(&pat, pattern));
                    self.fail();
                }
            }
            return;
        }
        match search::find_ranked(&self.buf, self.cursor, &pat, backward, count) {
            Some((pos, idx, total)) => {
                self.pending.awaiting = Awaiting::None;
                self.set_pcmark();
                self.do_motion(pos, MotionKind::Exclusive, edits, commands);
                self.message = Some(if pat.gave_up() {
                    // The scan was cut short; the numbers would lie.
                    format!("gave up counting matches of: {pattern}")
                } else {
                    format!("match {} of {total}", idx + 1)
                });
            }
            None => {
                self.message = Some(gave_up_or_missing(&pat, pattern));
                self.fail();
            }
        }
    }

    // ---- easymotion ----------------------------------------------------------

    /// Match the keys that open a jump. Returns whether the key was spent on
    /// the trigger.
    ///
    /// Vim resolves an ambiguous mapping by holding the keys it has and
    /// running them when the next one doesn't complete it; so does this. With
    /// the default `<space><space>`, `<space>l` still moves two characters
    /// right — the first space is replayed as the motion it otherwise is, and
    /// the key that broke the match runs after it.
    fn easy_lead(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) -> bool {
        if self.easy.trigger.get(self.lead) == Some(&key) {
            self.lead += 1;
            self.pending.keys.push_str(&key.label());
            if self.lead == self.easy.trigger.len() {
                self.lead = 0;
                self.pending.awaiting = Awaiting::Easy(EasyStage::Motion {
                    count: 0,
                    after_g: false,
                });
            }
            return true;
        }
        if self.lead == 0 {
            return false;
        }
        let buffered: Vec<Key> = self.easy.trigger[..self.lead].to_vec();
        let shown: usize = buffered.iter().map(|k| k.label().len()).sum();
        let keep = self.pending.keys.len().saturating_sub(shown);
        self.pending.keys.truncate(keep);
        self.lead = 0;
        self.replay = true;
        for buffered_key in buffered {
            self.key_normal(buffered_key, edits, commands);
        }
        self.replay = false;
        // A replayed key may have opened a wait of its own (`f`, `:`); the key
        // that ended the trigger belongs to that, not to the general dispatch.
        if self.pending.awaiting != Awaiting::None {
            self.key_normal(key, edits, commands);
            return true;
        }
        false
    }

    fn resolve_easy(
        &mut self,
        stage: EasyStage,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        match stage {
            EasyStage::Motion { count, after_g } => {
                self.easy_motion_key(count, after_g, key, edits, commands)
            }
            EasyStage::Chars { dir, before, want } => {
                self.easy_chars_key(dir, before, want, key, edits, commands)
            }
            EasyStage::Pick => self.easy_pick_key(key, edits, commands),
        }
    }

    /// The key naming the jump: `w W b B e E ge gE j k`, or one of the char
    /// jumps `f F t T s /`. A count in front of a char jump is how many
    /// characters it asks for, which is easymotion's `2s`.
    fn easy_motion_key(
        &mut self,
        count: usize,
        after_g: bool,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let Key::Char(ch) = key else {
            return self.cancel_easy();
        };
        if !after_g && ch.is_ascii_digit() && !(ch == '0' && count == 0) {
            self.pending.keys.push(ch);
            self.pending.awaiting = Awaiting::Easy(EasyStage::Motion {
                count: count.saturating_mul(10) + (ch as usize - '0' as usize),
                after_g,
            });
            return;
        }
        self.pending.keys.push(ch);
        let big = ch.is_uppercase();
        let want = count.max(1);
        let exclusive = JumpKind::Fixed(MotionKind::Exclusive);
        let inclusive = JumpKind::Fixed(MotionKind::Inclusive);
        let linewise = JumpKind::Fixed(MotionKind::Linewise);
        match (after_g, ch) {
            // `ge` / `gE`: word ends behind the cursor.
            (false, 'g') => {
                self.pending.awaiting = Awaiting::Easy(EasyStage::Motion { count, after_g: true });
            }
            (true, 'e' | 'E') => {
                let kind = easymotion::Kind::WordEnd { big };
                self.start_jump(kind, Dir::Backward, inclusive, edits, commands);
            }
            (true, _) => self.cancel_easy(),
            (false, 'w' | 'W') => {
                let kind = easymotion::Kind::WordStart { big };
                self.start_jump(kind, Dir::Forward, exclusive, edits, commands);
            }
            (false, 'b' | 'B') => {
                let kind = easymotion::Kind::WordStart { big };
                self.start_jump(kind, Dir::Backward, exclusive, edits, commands);
            }
            (false, 'e' | 'E') => {
                let kind = easymotion::Kind::WordEnd { big };
                self.start_jump(kind, Dir::Forward, inclusive, edits, commands);
            }
            (false, 'j') => self.start_jump(easymotion::Kind::Line, Dir::Forward, linewise, edits, commands),
            (false, 'k') => self.start_jump(easymotion::Kind::Line, Dir::Backward, linewise, edits, commands),
            (false, 'f') => self.easy_await_chars(Dir::Forward, false, want),
            (false, 'F') => self.easy_await_chars(Dir::Backward, false, want),
            (false, 't') => self.easy_await_chars(Dir::Forward, true, want),
            (false, 'T') => self.easy_await_chars(Dir::Backward, true, want),
            (false, 's') => self.easy_await_chars(Dir::Both, false, want),
            // `/`: as many characters as you care to type, `<cr>` to label.
            (false, '/') => self.easy_await_chars(Dir::Both, false, 0),
            _ => self.cancel_easy(),
        }
    }

    fn easy_await_chars(&mut self, dir: Dir, before: bool, want: usize) {
        self.pending.awaiting = Awaiting::Easy(EasyStage::Chars { dir, before, want });
    }

    /// Collect what a char jump looks for. A fixed-length one labels as soon
    /// as it has its characters; the `/` jump waits for `<cr>`.
    fn easy_chars_key(
        &mut self,
        dir: Dir,
        before: bool,
        want: usize,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        match key {
            Key::Char(ch) => {
                self.pending.prompt.push(ch);
                if want > 0 && self.pending.prompt.chars().count() >= want {
                    self.start_char_jump(dir, before, edits, commands);
                }
            }
            Key::Enter if want == 0 => {
                if self.pending.prompt.is_empty() {
                    return self.cancel_easy();
                }
                self.start_char_jump(dir, before, edits, commands);
            }
            Key::Backspace => {
                if self.pending.prompt.pop().is_none() {
                    self.cancel_easy();
                }
            }
            _ => self.cancel_easy(),
        }
    }

    fn start_char_jump(
        &mut self,
        dir: Dir,
        before: bool,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let needle = std::mem::take(&mut self.pending.prompt);
        let kind = easymotion::Kind::Chars { needle: &needle, before };
        self.start_jump(kind, dir, JumpKind::Char, edits, commands);
    }

    /// Collect the targets and label them. One target needs no label — the
    /// jump is already decided, so it runs; none at all says so and stops.
    fn start_jump(
        &mut self,
        kind: easymotion::Kind,
        dir: Dir,
        jump_kind: JumpKind,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let view = self.view.unwrap_or((0, self.buf.last_line()));
        let targets = easymotion::collect(&self.buf, view, self.cursor, kind, dir);
        if targets.is_empty() {
            self.message = Some("no targets on screen".to_string());
            return self.cancel_easy();
        }
        if targets.len() == 1 {
            let target = targets[0];
            self.take_jump(target, jump_kind, edits, commands);
            return;
        }
        let labels = easymotion::labels(targets.len(), &self.easy.keys);
        if labels.is_empty() {
            return self.cancel_easy();
        }
        let jump = Jump {
            targets: targets.into_iter().zip(labels).collect(),
            typed: String::new(),
            kind: jump_kind,
            view,
        };
        self.easy_ui = Some(labels_ui(&jump));
        self.jump = Some(jump);
        self.pending.awaiting = Awaiting::Easy(EasyStage::Pick);
    }

    /// A label key: it either names a target, narrows the labels still on
    /// screen, or belongs to no label at all and ends the jump.
    fn easy_pick_key(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let (Some(mut jump), Key::Char(ch)) = (self.jump.take(), key) else {
            return self.cancel_easy();
        };
        jump.typed.push(ch);
        if let Some(&(target, _)) = jump.targets.iter().find(|(_, label)| *label == jump.typed) {
            return self.take_jump(target, jump.kind, edits, commands);
        }
        if !jump.targets.iter().any(|(_, l)| l.starts_with(&jump.typed)) {
            return self.cancel_easy();
        }
        self.pending.keys.push(ch);
        self.easy_ui = Some(labels_ui(&jump));
        self.jump = Some(jump);
    }

    /// Land on a chosen target: an ordinary motion from here on, so it moves
    /// the cursor, extends a visual selection or feeds a pending operator.
    fn take_jump(
        &mut self,
        target: Pos,
        kind: JumpKind,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let motion_kind = kind.motion(self.cursor, target);
        self.jump = None;
        self.lead = 0;
        self.easy_ui = Some(EasyUi::Done);
        self.pending.awaiting = Awaiting::None;
        self.pending.prompt.clear();
        // vim-easymotion records the jump, so `''` and `ctrl-o` come back.
        self.set_pcmark();
        self.do_motion(target, motion_kind, edits, commands);
    }

    /// Drop a jump in progress and tell the host to take its paint down.
    fn cancel_easy(&mut self) {
        let painted = self.jump.is_some()
            || self.lead > 0
            || matches!(self.pending.awaiting, Awaiting::Easy(_));
        self.jump = None;
        self.lead = 0;
        if painted {
            self.easy_ui = Some(EasyUi::Done);
        }
        self.clear_pending();
    }

    // ---- registers and marks --------------------------------------------------

    /// `"x`: the register the command after it uses. A count before it stays
    /// a count, and digits after it are one more, multiplying the first.
    fn resolve_register(&mut self, key: Key) {
        match key {
            Key::Char(c) if register::readable(c) => {
                self.pending.awaiting = Awaiting::None;
                self.pending.register = Some(c);
                self.pending.keys.push(c);
            }
            Key::Esc => self.clear_pending(),
            _ => self.fail(),
        }
    }

    /// `'x` and `` `x ``: to a mark — a jump, so `''` is set first (normal.c
    /// nv_gomark). `'` lands on the line's first non-blank and is linewise;
    /// `` ` `` lands on the mark itself and is exclusive, one of the motions
    /// whose deletes always go to `"1`. A file mark set in another document
    /// is the host's to open.
    fn resolve_mark(&mut self, linewise: bool, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let Key::Char(c) = key else {
            return self.clear_pending();
        };
        self.pending.awaiting = Awaiting::None;
        if let Some((doc, at)) = mark::file_mark(c).filter(|(doc, _)| self.doc.as_ref() != Some(doc)) {
            // Only a plain jump can leave the document: an operator or a
            // selection cannot reach into another one.
            if self.pending.op.is_some() || self.mode != Mode::Normal {
                self.message = Some("E20: Mark not set".into());
                return self.fail();
            }
            self.set_pcmark();
            commands.push(Command::GotoMark { uri: doc, line: at.line, col: at.col, first_non_blank: linewise });
            return self.clear_pending();
        }
        let at = match self.mark_pos(c) {
            Ok(at) if at.line > self.buf.last_line() => Err("E19: Mark has invalid line number"),
            other => other,
        };
        let at = match at {
            Ok(at) => at,
            Err(msg) => {
                self.message = Some(msg.into());
                return self.fail();
            }
        };
        self.set_pcmark();
        if linewise {
            let target = Pos::new(at.line, self.buf.first_non_blank(at.line));
            self.do_motion(target, MotionKind::Linewise, edits, commands);
        } else {
            self.pending.reg_one = true;
            let target = Pos::new(at.line, at.col.min(self.buf.line_len(at.line)));
            self.do_motion(target, MotionKind::Exclusive, edits, commands);
        }
    }

    /// `m{a-zA-Z'`[]<>}` sets a mark at the cursor; the primary cursor's
    /// alone. Any other key drops the `m`, as Vim's does.
    fn resolve_set_mark(&mut self, key: Key) {
        self.clear_pending();
        if let Key::Char(c) = key
            && !self.quiet
        {
            self.set_mark(c, self.cursor);
        }
    }

    /// Set mark `c` at `at`; false for a name that cannot be set.
    fn set_mark(&mut self, c: char, at: Pos) -> bool {
        match c {
            'a'..='z' => self.marks.named[c as usize - 'a' as usize] = Some(at),
            // A file mark needs the document's name, which the host gives
            // every session it makes.
            'A'..='Z' => match &self.doc {
                Some(doc) => mark::set_file_mark(c, Some((doc.clone(), at))),
                None => return false,
            },
            // `m'` is a jump that goes nowhere: `''` and a jumplist entry.
            '\'' | '`' => {
                if !self.in_global {
                    self.marks.set_context(at);
                }
            }
            '[' => self.marks.op_start = Some(at),
            ']' => self.marks.op_end = Some(at),
            '<' | '>' => {
                let area = self.last_area.get_or_insert(VisualArea { anchor: at, cursor: at, linewise: false });
                let (start, end) = ordered(area.anchor, area.cursor);
                let (start, end) = if c == '<' { (at, end) } else { (start, at) };
                (area.anchor, area.cursor) = ordered(start, end);
            }
            _ => return false,
        }
        true
    }

    /// A jump is leaving the cursor: it becomes `''` and goes on the
    /// jumplist (mark.c `setpcmark`). The primary cursor's alone, and only
    /// once for a whole `:g`, as Vim has it.
    fn set_pcmark(&mut self) {
        if !self.quiet && !self.in_global {
            self.marks.set_context(self.cursor);
        }
    }

    /// Where mark `c` is in this document, or Vim's complaint about it.
    /// `'<` and `'>` are the ends of the last selection — the one before the
    /// current, while one is up, as Vim sets them when visual mode ends.
    fn mark_pos(&self, c: char) -> Result<Pos, &'static str> {
        let not_set = "E20: Mark not set";
        let found = match c {
            'a'..='z' => self.marks.named[c as usize - 'a' as usize],
            'A'..='Z' => mark::file_mark(c).filter(|(doc, _)| self.doc.as_ref() == Some(doc)).map(|(_, p)| p),
            '\'' | '`' => self.marks.context,
            '.' => self.marks.change,
            '^' => self.last_insert,
            '[' => self.marks.op_start,
            ']' => self.marks.op_end,
            '<' | '>' => self.last_area.map(|area| self.area_end(area, c == '>')),
            '0'..='9' | '"' => None,
            _ => return Err("E78: Unknown mark"),
        };
        found.ok_or(not_set)
    }

    /// One end of a selection as `'<` / `'>` have it: the line's ends for a
    /// linewise one.
    fn area_end(&self, area: VisualArea, end: bool) -> Pos {
        let (first, last) = ordered(area.anchor, area.cursor);
        match (end, area.linewise) {
            (false, false) => first,
            (false, true) => Pos::new(first.line, 0),
            (true, false) => last,
            (true, true) => Pos::new(last.line, self.buf.line_len(last.line.min(self.buf.last_line()))),
        }
    }

    /// What ex addresses know: the cursor line, the last line, and the line
    /// of every mark that is set here. A `:` line typed over a selection —
    /// the `'<,'>` it starts with — is about that selection, which visual
    /// mode hands over as it ends.
    fn ex_context(&self, current: usize) -> ex::Context {
        let last = self.buf.last_line();
        let names = ('a'..='z').chain('A'..='Z').chain("'.^[]".chars());
        let mut marks: Vec<(char, usize)> =
            names.filter_map(|c| self.mark_pos(c).ok().map(|p| (c, p.line.min(last)))).collect();
        if let Some(area) = self.visual_area().or(self.last_area) {
            marks.push(('<', self.area_end(area, false).line.min(last)));
            marks.push(('>', self.area_end(area, true).line.min(last)));
        }
        ex::Context { current, last, marks }
    }

    /// `:pu[t][!] [x]`: the register's text as lines of their own, whatever
    /// its type, below 1-based line `below`; the cursor lands on the first
    /// non-blank of the last of them (ex_docmd.c ex_put).
    fn put_lines(&mut self, below: usize, name: Option<char>, edits: &mut Vec<Edit>) -> Result<(), String> {
        let name = name.unwrap_or('"');
        let reg = self.register(name).ok_or_else(|| format!("E353: Nothing in register {name}"))?;
        let text = reg.part(self.put_slot).to_string();
        let put = self.lines_insertion(below, &text);
        self.emit_edit(put.start, put.end, &put.text, edits);
        let landing = (below + text.matches('\n').count()).min(self.buf.last_line());
        self.cursor = Pos::new(landing, self.buf.first_non_blank(landing));
        Ok(())
    }

    /// `:reg` / `:di`: every register with something in it, or only the ones
    /// named, in Vim's order (register.c ex_display). `"*` asks for the
    /// clipboard as `"+` does, there being one clipboard.
    fn list_registers(&mut self, names: Option<&str>, commands: &mut Vec<Command>) {
        let mut regs = register::listing();
        if let Some(last) = &self.last_search {
            regs.push(('/', Register::new(last.pattern.clone(), false)));
        }
        let wanted = |c: char| names.is_none_or(|n| n.contains(c) || c == '+' && n.contains('*'));
        let rows: Vec<ListRow> = regs
            .into_iter()
            .filter(|&(c, _)| wanted(c))
            .map(|(c, reg)| ListRow {
                label: format!("\"{c}"),
                description: if reg.linewise { "line" } else { "char" }.to_string(),
                detail: printable(&reg.text, reg.linewise),
                uri: None,
            })
            .collect();
        if rows.is_empty() {
            self.message = Some("the registers are empty".into());
        } else {
            commands.push(Command::ShowList { title: ":registers".into(), rows });
        }
    }

    /// `:marks [names]`, in Vim's order (mark.c ex_marks): `''`, the
    /// lowercase marks, the file marks — wherever they are — then `'[ ']`,
    /// `'^ '.` and `'< '>`.
    fn list_marks(&mut self, names: Option<&str>, commands: &mut Vec<Command>) -> Result<(), String> {
        let wanted = |c: char| names.is_none_or(|n| n.contains(c));
        let mut rows = Vec::new();
        for c in std::iter::once('\'').chain('a'..='z').chain('A'..='Z').chain("[]^.<>".chars()) {
            if !wanted(c) {
                continue;
            }
            let (at, uri) = match mark::file_mark(c) {
                Some((doc, at)) if self.doc.as_ref() != Some(&doc) => (at, Some(doc)),
                _ => match self.mark_pos(c) {
                    Ok(at) => (at, None),
                    Err(_) => continue,
                },
            };
            let detail = match &uri {
                Some(doc) => doc.clone(),
                None => self.buf.line(at.line.min(self.buf.last_line())).trim_start().to_string(),
            };
            let description = format!("line {}, col {}", at.line + 1, at.col);
            rows.push(ListRow { label: c.to_string(), description, detail, uri });
        }
        if rows.is_empty() {
            return Err(match names {
                None => "No marks set".into(),
                Some(n) => format!("E283: No marks matching \"{n}\""),
            });
        }
        commands.push(Command::ShowList { title: ":marks".into(), rows });
        Ok(())
    }

    /// `:jumps`: the jumplist, oldest first, each entry with how many
    /// `ctrl-o` (above) or `ctrl-i` (below) reach it from where the list
    /// stands, which `>` marks.
    fn list_jumps(&mut self, commands: &mut Vec<Command>) {
        let (entries, idx) = self.marks.jumps.listing();
        if entries.is_empty() {
            self.message = Some("the jumplist is empty".into());
            return;
        }
        let last = self.buf.last_line();
        let mut rows: Vec<ListRow> = entries
            .iter()
            .enumerate()
            .map(|(i, &(dist, at))| ListRow {
                label: format!("{}{dist}", if i == idx { "> " } else { "" }),
                description: format!("line {}, col {}", at.line + 1, at.col),
                detail: self.buf.line(at.line.min(last)).trim_start().to_string(),
                uri: None,
            })
            .collect();
        if idx == entries.len() {
            rows.push(ListRow { label: ">".into(), description: String::new(), detail: String::new(), uri: None });
        }
        commands.push(Command::ShowList { title: ":jumps".into(), rows });
    }

    /// `:delm {marks}` and `:delm!` (mark.c ex_delmarks). `!` clears what
    /// Vim's `clrallmarks` does: the lowercase marks, `'[ ']`, `'^` and `'.`.
    /// The `'< '>` pair is one remembered selection here, so deleting either
    /// end forgets it.
    fn delete_marks(&mut self, all: bool, names: &[char]) {
        let everything: Vec<char> = ('a'..='z').chain("[]^.".chars()).collect();
        for &c in if all { &everything } else { names } {
            match c {
                'a'..='z' => self.marks.named[c as usize - 'a' as usize] = None,
                'A'..='Z' => mark::set_file_mark(c, None),
                '[' => self.marks.op_start = None,
                ']' => self.marks.op_end = None,
                '^' => self.last_insert = None,
                '.' => self.marks.change = None,
                '<' | '>' => self.last_area = None,
                _ => {}
            }
        }
    }

    /// What register `name` holds: the way the host and `@` read a
    /// register. `"/` is this session's last search pattern; `"_` and an
    /// empty register are `None`.
    pub fn register(&self, name: char) -> Option<Register> {
        match name {
            '/' => self.last_search.as_ref().map(|s| Register::new(s.pattern.clone(), false)),
            '_' => None,
            _ => register::get(name),
        }
    }

    /// Write register `name` the way a recording or `:let @a = …` does: an
    /// uppercase name appends, `""` keeps pointing where it did, and the
    /// numbered registers do not shift. `"/` sets the pattern `n` repeats.
    /// False for a name that cannot be written: `"_`, `".`, `":`.
    pub fn set_register(&mut self, name: char, text: &str, linewise: bool) -> bool {
        if name == '/' {
            self.last_search = Some(Search { pattern: text.to_string(), backward: false });
            return true;
        }
        register::set(name, Register::new(text, linewise))
    }

    /// The system clipboard's text, from the host, ahead of a key that reads
    /// `"+` / `"*` (see `Command::SetClipboard`).
    pub fn set_clipboard(&mut self, text: &str) {
        register::set_clipboard(text);
    }

    /// Whether `key` would read the system clipboard, so the host has to
    /// supply it first with `set_clipboard` (see `Command::SetClipboard`):
    /// the key comes after a `"+` or `"*` prefix, or it is the `<cr>` of a
    /// `:put` or `:registers` line that names — or, listing everything,
    /// takes in — the clipboard.
    pub fn wants_clipboard(&self, key: Key) -> bool {
        match self.pending.awaiting {
            Awaiting::Ex => {
                key == Key::Enter
                    && match ex::parse(&self.pending.prompt, &self.ex_context(self.cursor.line)) {
                        Ok(ex::Command::Put { register: Some(r), .. }) => register::is_clipboard(r),
                        Ok(ex::Command::Registers { names }) => names.is_none_or(|n| n.contains(['+', '*'])),
                        _ => false,
                    }
            }
            Awaiting::Execute => matches!(key, Key::Char(c) if register::is_clipboard(c)),
            Awaiting::Register | Awaiting::Search { .. } => false,
            _ => self.mode != Mode::Insert && self.pending.register.is_some_and(register::is_clipboard),
        }
    }

    /// Where mark `name` is in this document, if it is set.
    pub fn mark(&self, name: char) -> Option<Pos> {
        self.mark_pos(name).ok()
    }

    // ---- ex command line -----------------------------------------------------

    /// Collect the `:` command line until `<cr>` runs it; `<esc>`, and `<bs>`
    /// past the start of the line, abandon it like Vim.
    fn resolve_ex(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        match key {
            Key::Char(c) => self.pending.prompt.push(c),
            Key::Backspace => {
                if self.pending.prompt.pop().is_none() {
                    self.clear_pending();
                }
            }
            Key::Enter => {
                let line = std::mem::take(&mut self.pending.prompt);
                self.clear_pending();
                self.run_ex(&line, edits, commands);
                // `":` is the line just run, once it has run: a `:reg` shows
                // the one before it.
                if !line.trim().is_empty() {
                    register::set_command(&line);
                }
            }
            Key::Esc => self.clear_pending(),
            Key::Ctrl(_) => {}
        }
    }

    fn run_ex(&mut self, line: &str, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        // A selection in play becomes `'<`/`'>`, and running the command
        // leaves visual mode, as `:'<,'>s/…` does in Vim.
        if let Some(area) = self.visual_area() {
            let (s, _) = ordered(self.anchor, self.cursor);
            self.last_area = Some(area);
            self.mode = Mode::Normal;
            self.cursor = self.clamp_normal(s);
            self.desired_col = self.cursor.col;
        }
        let ctx = self.ex_context(self.cursor.line);
        let done = ex::parse(line, &ctx).and_then(|cmd| self.ex_command(cmd, false, edits, commands));
        if let Err(msg) = done {
            self.message = Some(msg);
            self.failed = true;
        }
    }

    /// Carry out one parsed command line. `in_global` is set for the ones
    /// `:g` runs, which stay inside the buffer.
    fn ex_command(
        &mut self,
        cmd: ex::Command,
        in_global: bool,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) -> Result<(), String> {
        match cmd {
            ex::Command::Nothing => {}
            ex::Command::Goto(target) => {
                let target = target.min(self.buf.last_line());
                self.set_pcmark();
                self.cursor = Pos::new(target, self.buf.first_non_blank(target));
            }
            ex::Command::Substitute(sub) => {
                self.substitute(&sub, edits);
            }
            // `:d` and `:y` fill the registers `dd` and `yy` do, `:d a` the
            // one named as well; `:d _` is the black hole, which keeps them
            // all as they were.
            ex::Command::Delete(lines) => {
                self.yank_lines(lines.register, lines.first, lines.last, true);
                self.delete_lines(lines.first, lines.last, edits);
                let count = lines.last - lines.first + 1;
                self.report(count, format!("{count} fewer lines"));
            }
            ex::Command::Yank(lines) => {
                self.yank_lines(lines.register, lines.first, lines.last, false);
                let count = lines.last - lines.first + 1;
                self.report(count, format!("{count} lines yanked"));
            }
            ex::Command::Put { below, register } => self.put_lines(below, register, edits)?,
            ex::Command::Registers { .. } | ex::Command::Marks { .. } | ex::Command::Jumps if in_global => {
                return Err(":global cannot run listing commands".into());
            }
            ex::Command::Registers { names } => self.list_registers(names.as_deref(), commands),
            ex::Command::Marks { names } => self.list_marks(names.as_deref(), commands)?,
            ex::Command::Jumps => self.list_jumps(commands),
            ex::Command::DeleteMarks { all, names } => self.delete_marks(all, &names),
            // Vim's `:mark` puts the mark on the line's first non-blank.
            ex::Command::SetMark { mark, line } => {
                let at = Pos::new(line, self.buf.first_non_blank(line));
                if !self.set_mark(mark, at) {
                    return Err("E191: Argument must be a letter or forward/backward quote".into());
                }
            }
            ex::Command::Move { first, last, below } => self.move_lines(first, last, below, edits)?,
            ex::Command::Copy { first, last, below } => self.copy_lines(first, last, below, edits),
            ex::Command::Join { first, last, spaces } => self.join_range(first, last, spaces, edits),
            ex::Command::Shift { first, last, dedent, times } => {
                commands.push(Command::IndentLines { start_line: first, end_line: last, dedent, times });
                // The host puts the cursor on the first line's first
                // non-blank once the lines are shifted, as it does after
                // `>>`; Vim's `:>` would leave it on the last line.
                self.cursor = Pos::new(first, self.buf.first_non_blank(first));
                let count = last - first + 1;
                let op = if dedent { '<' } else { '>' };
                self.report(count, format!("{count} lines {op}ed {times} time{}", plural(times)));
            }
            ex::Command::Sort(sort) => self.sort_lines(&sort, edits)?,
            ex::Command::Global(global) => self.global(&global, edits, commands)?,
            ex::Command::File(_) | ex::Command::Reload { .. } if in_global => {
                return Err(":global cannot run file commands".into());
            }
            ex::Command::File(file) => commands.push(Command::File(file)),
            ex::Command::Reload { force } => commands.push(Command::Reload { force }),
        }
        self.desired_col = self.cursor.col;
        Ok(())
    }

    /// Say how many lines a command touched, when there are more of them than
    /// Vim's `'report'` lets pass in silence.
    fn report(&mut self, lines: usize, what: String) {
        if lines > REPORT {
            self.message = Some(what);
        }
    }

    /// `:m`: the lines go below 1-based line `below`, 0 being the top. The
    /// edit is an insertion and a deletion rather than a rewrite of all the
    /// lines in between, so the lines the move passes over keep their
    /// identity — which is how `:g/x/m$` tells the lines it has yet to visit.
    fn move_lines(
        &mut self,
        first: usize,
        last: usize,
        below: usize,
        edits: &mut Vec<Edit>,
    ) -> Result<(), String> {
        if below > first && below <= last {
            return Err("E134: Cannot move a range of lines into itself".into());
        }
        let count = last - first + 1;
        // Lines already in place stay put, but the cursor lands as though
        // they had moved: on the last of them.
        let landing = if below > last { below - 1 } else { below + count - 1 };
        if below != first && below != last + 1 {
            let block = self.lines_text(first, last);
            let (put, take) = (self.lines_insertion(below, &block), self.lines_removal(first, last));
            // The marks on the moved lines go with them, as Vim's do; the
            // removal would otherwise take them.
            let carried = self.marks.take_lines(first, last);
            // Bottom-up, so each edit's coordinates still hold when it lands.
            let removal = Shift::new(&self.buf, take.start, take.end, "").removing(first, last);
            if below > last {
                self.edit_mirror(Shift::new(&self.buf, put.start, put.end, &put.text), &put);
                self.edit_mirror(removal, &take);
            } else {
                self.edit_mirror(removal, &take);
                self.edit_mirror(Shift::new(&self.buf, put.start, put.end, &put.text), &put);
            }
            self.marks.put_lines(carried, landing + 1 - count);
            edits.extend([put, take]);
            self.report(count, format!("{count} lines moved"));
        }
        self.cursor = Pos::new(landing, self.buf.first_non_blank(landing));
        Ok(())
    }

    /// `:t` / `:co`: a copy of the lines below 1-based line `below`; the
    /// cursor lands on the last line of the copy.
    fn copy_lines(&mut self, first: usize, last: usize, below: usize, edits: &mut Vec<Edit>) {
        let copy = self.lines_insertion(below, &self.lines_text(first, last));
        self.edit_mirror(Shift::new(&self.buf, copy.start, copy.end, &copy.text), &copy);
        edits.push(copy);
        let count = last - first + 1;
        let landing = below + count - 1;
        self.cursor = Pos::new(landing, self.buf.first_non_blank(landing));
        self.report(count, format!("{count} more lines"));
    }

    /// `:j`: lines `first..=last` into one, the cursor on its first non-blank.
    /// With `spaces` the join is Vim's (`:help J`): each line's leading white
    /// space goes and one space takes its place — none before a `)`, after a
    /// tab or a space, or at the start of the line — and an empty line adds
    /// nothing. Without it (`:j!`) the lines run together as they are.
    fn join_range(&mut self, first: usize, last: usize, spaces: bool, edits: &mut Vec<Edit>) {
        let mut joins = Vec::with_capacity(last - first);
        let mut empty = self.buf.line_len(first) == 0;
        // The last char of the line before; an empty line has none, so a
        // space does go in after one.
        let mut end = self.buf.line(first).chars().last();
        for line in first + 1..=last {
            let full = self.buf.line(line);
            if !spaces {
                joins.push((0, ""));
                continue;
            }
            let next = full.trim_start_matches([' ', '\t']);
            let sep = !next.is_empty() && !next.starts_with(')') && !empty && !matches!(end, Some(' ' | '\t'));
            joins.push((utf16_len(&full[..full.len() - next.len()]), if sep { " " } else { "" }));
            empty &= next.is_empty();
            end = next.chars().last();
        }
        self.emit_joins(first, &joins, edits);
        self.cursor = Pos::new(first, self.buf.first_non_blank(first));
    }

    /// Join line `first` with the ones after it, one edit per line break:
    /// the break, and the first `strip` columns of the line after it, give
    /// way to `sep`. Edits that small carry every mark on a joined line
    /// along with its text, the way Vim moves them (ops.c do_join). They are
    /// made bottom-up, which leaves each in the coordinates the host holds.
    fn emit_joins(&mut self, first: usize, joins: &[(usize, &str)], edits: &mut Vec<Edit>) {
        for (k, &(strip, sep)) in joins.iter().enumerate().rev() {
            let line = first + k;
            let start = Pos::new(line, self.buf.line_len(line));
            self.emit_edit(start, Pos::new(line + 1, strip), sep, edits);
        }
    }

    /// `:sort` (`:help :sort`). The sort is stable, so lines with equal keys
    /// keep their order and `!` reverses all of it. `u` drops a line equal
    /// to the one kept before it, comparing whole lines whatever the key.
    fn sort_lines(&mut self, sort: &ex::Sort, edits: &mut Vec<Edit>) -> Result<(), String> {
        // Sorting one line is quick, and leaves even the cursor alone.
        if sort.last <= sort.first {
            return Ok(());
        }
        let pattern = match sort.pattern.as_deref() {
            None => None,
            Some(src) => {
                let source = match (src, &self.last_search) {
                    ("", Some(last)) => last.pattern.clone(),
                    ("", None) => return Err("E35: No previous regular expression".into()),
                    (src, _) => src.to_string(),
                };
                let pattern = search::Pattern::parse(&source)?;
                pattern.budget_for(&self.buf);
                Some((pattern, source))
            }
        };
        let lines: Vec<String> = (sort.first..=sort.last).map(|l| self.buf.line(l).to_string()).collect();
        // Each line's key, as a byte span: the line, what follows the
        // pattern's match (or the match itself, with `r`), or nothing at
        // all on a line the pattern misses.
        let mut found = Vec::new();
        let keys: Vec<(usize, usize)> = lines
            .iter()
            .map(|text| {
                let Some((pattern, _)) = &pattern else {
                    return (0, text.len());
                };
                pattern.find_all_into(text, false, &mut found);
                match found.first().map(search::LineMatch::bytes) {
                    Some((from, to)) if sort.on_match => (from, to),
                    Some((_, to)) => (to, text.len()),
                    None => (0, 0),
                }
            })
            .collect();
        if let Some((pattern, source)) = &pattern
            && pattern.gave_up()
        {
            return Err(gave_up_or_missing(pattern, source));
        }
        let key = |i: usize| &lines[i][keys[i].0..keys[i].1];
        let mut order: Vec<usize> = (0..lines.len()).collect();
        if sort.numeric {
            let numbers: Vec<Option<i64>> = (0..lines.len()).map(|i| first_number(key(i))).collect();
            // A line without a number sorts before every line with one.
            order.sort_by_key(|&i| numbers[i]);
        } else if sort.ignore_case {
            // ASCII folding, as Vim's `STRICMP` does.
            let folded = |i: usize| key(i).bytes().map(|c| c.to_ascii_lowercase());
            order.sort_by(|&a, &b| folded(a).cmp(folded(b)));
        } else {
            order.sort_by(|&a, &b| key(a).cmp(key(b)));
        }
        if sort.reverse {
            order.reverse();
        }
        let mut kept: Vec<&str> = Vec::with_capacity(lines.len());
        for &i in &order {
            let line = lines[i].as_str();
            let same = |prev: &&str| {
                if sort.ignore_case { prev.eq_ignore_ascii_case(line) } else { *prev == line }
            };
            if !(sort.unique && kept.last().is_some_and(same)) {
                kept.push(line);
            }
        }
        let sorted = kept.join("\n");
        if kept.len() != lines.len() || kept.iter().zip(&lines).any(|(a, b)| a != b) {
            let end = Pos::new(sort.last, self.buf.line_len(sort.last));
            self.emit_edit(Pos::new(sort.first, 0), end, &sorted, edits);
        }
        self.cursor = Pos::new(sort.first, self.buf.first_non_blank(sort.first));
        let gone = lines.len() - kept.len();
        self.report(gone, format!("{gone} fewer lines"));
        Ok(())
    }

    /// `:g/pattern/cmd` and `:v` (`:help :global`), done Vim's way: mark the
    /// lines that match (or, for `:v`, do not), then visit the marked lines
    /// top-down, running the command with the cursor on each. A mark
    /// travels with its line through every edit the commands make, and goes
    /// when the line does, so `:g/x/m0` reverses the matches and `:g/^$/d`
    /// clears every blank line however many adjoin.
    ///
    /// The commands edit the mirror one after another; the host is handed a
    /// single edit spanning everything that changed, which keeps the whole
    /// `:g` one undo step and its coordinates in the document the host has.
    fn global(
        &mut self,
        g: &ex::Global,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) -> Result<(), String> {
        let (pattern, source) = self.global_pattern(g)?;
        // Vim sets `''` once for the whole of a `:g`, before it starts.
        self.set_pcmark();
        self.in_global = true;
        let done = self.global_run(g, &pattern, &source, edits, commands);
        self.in_global = false;
        done
    }

    fn global_run(
        &mut self,
        g: &ex::Global,
        pattern: &search::Pattern,
        source: &str,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) -> Result<(), String> {
        let mut found = Vec::new();
        let mut marks: Vec<bool> = (0..self.buf.line_count())
            .map(|line| {
                (g.first..=g.last).contains(&line) && {
                    pattern.find_all_into(self.buf.line(line), false, &mut found);
                    found.is_empty() == g.invert
                }
            })
            .collect();
        if pattern.gave_up() {
            return Err(gave_up_or_missing(pattern, source));
        }
        let matched = marks.iter().filter(|&&m| m).count();
        if matched == 0 {
            return Err(if g.invert {
                format!("Pattern found in every line: {source}")
            } else {
                format!("Pattern not found: {source}")
            });
        }
        if g.command.trim().is_empty() {
            // Vim's default is `:p`, printing the lines; there is nowhere to
            // print them, so this counts them instead.
            self.message = Some(format!("{matched} matching line{}", plural(matched)));
            return Ok(());
        }

        let before = self.buf.clone();
        let mut tally = Tally::default();
        let mut line_edits = Vec::new();
        let mut from = 0;
        let mut failed = None;
        while let Some(line) = (from..marks.len()).find(|&l| marks[l]) {
            marks[line] = false;
            self.cursor = Pos::new(line, 0);
            let done = self.global_line(&g.command, line, &mut line_edits, commands, &mut tally);
            // The lines above the first edit kept their (cleared) marks, so
            // the next search starts there.
            from = line;
            line_edits.sort_by_key(|e| std::cmp::Reverse((e.start, e.end)));
            for edit in line_edits.drain(..) {
                carry_marks(&mut marks, &edit);
                from = from.min(edit.start.line);
            }
            // Vim stops at the first command that fails.
            if let Err(msg) = done {
                failed = Some(msg);
                break;
            }
        }
        edits.extend(lines_diff(&before, &self.buf));
        self.cursor = self.clamp_normal(self.cursor);
        self.desired_col = self.cursor.col;
        if let Some(msg) = failed {
            return Err(msg);
        }
        // Reported once for the lot, as Vim does: what `:s` did if it ran,
        // otherwise how many lines came or went.
        if tally.hits > 0 {
            let (hits, lines) = (tally.hits, tally.lines);
            self.message = Some(if tally.count_only {
                format!("{hits} match{} on {lines} line{}", if hits == 1 { "" } else { "es" }, plural(lines))
            } else {
                format!("{hits} substitution{} on {lines} line{}", plural(hits), plural(lines))
            });
        } else {
            let (was, is) = (before.line_count(), self.buf.line_count());
            if is < was {
                self.report(was - is, format!("{} fewer lines", was - is));
            } else {
                self.report(is - was, format!("{} more lines", is - was));
            }
        }
        Ok(())
    }

    /// A `:g` pattern compiled, and made the last search like Vim makes it.
    fn global_pattern(&mut self, g: &ex::Global) -> Result<(search::Pattern, String), String> {
        let last_used = self.last_search.as_ref().map(|s| s.pattern.clone());
        let source = g.pattern.clone().or(last_used).ok_or("E35: No previous regular expression")?;
        let pattern = search::Pattern::parse(&source)?;
        pattern.budget_for(&self.buf);
        self.last_search = Some(Search { pattern: source.clone(), backward: false });
        Ok((pattern, source))
    }

    /// One marked line's turn under `:g`: the command is parsed afresh, since
    /// its addresses count from this line.
    fn global_line(
        &mut self,
        command: &str,
        line: usize,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
        tally: &mut Tally,
    ) -> Result<(), String> {
        let ctx = self.ex_context(line);
        match ex::parse(command, &ctx)? {
            ex::Command::Substitute(mut sub) => {
                // A line `:s` finds nothing on is no news under `:g`; any
                // other complaint is a real error and stops it.
                sub.quiet = true;
                let (hits, lines) = self.substitute(&sub, edits);
                let complaint = self.message.take();
                if hits == 0
                    && let Some(msg) = complaint
                {
                    return Err(msg);
                }
                tally.hits += hits;
                tally.lines += lines;
                tally.count_only |= sub.count_only;
            }
            // Nested, `:g` and `:v` look at the one line they were given
            // (`:g/a/v/b/d` deletes the lines with an a and no b).
            ex::Command::Global(inner) => {
                if (inner.first, inner.last) != (0, self.buf.last_line()) {
                    return Err("E147: Cannot do :global recursive with a range".into());
                }
                let (pattern, _) = self.global_pattern(&inner)?;
                let found = pattern.find_all(self.buf.line(line), false);
                if found.is_empty() == inner.invert && !inner.command.trim().is_empty() {
                    return self.global_line(&inner.command, line, edits, commands, tally);
                }
            }
            cmd => {
                self.ex_command(cmd, true, edits, commands)?;
                // Per-line reports would only bury the one at the end.
                self.message = None;
            }
        }
        Ok(())
    }

    /// Lines `l1..=l2` as one string, the way a linewise register holds them.
    fn lines_text(&self, l1: usize, l2: usize) -> String {
        (l1..=l2).map(|l| self.buf.line(l)).collect::<Vec<_>>().join("\n")
    }

    /// An edit putting whole lines (`text`, without a final line break)
    /// below 1-based line `below`, 0 being the top.
    fn lines_insertion(&self, below: usize, text: &str) -> Edit {
        let last = self.buf.last_line();
        if below <= last {
            let at = Pos::new(below, 0);
            Edit { start: at, end: at, text: format!("{text}\n") }
        } else {
            let at = Pos::new(last, self.buf.line_len(last));
            Edit { start: at, end: at, text: format!("\n{text}") }
        }
    }

    /// An edit removing lines `l1..=l2` and one line break beside them, the
    /// span `delete_lines` takes.
    fn lines_removal(&self, l1: usize, l2: usize) -> Edit {
        let last = self.buf.last_line();
        let (start, end) = if l2 < last {
            (Pos::new(l1, 0), Pos::new(l2 + 1, 0))
        } else if l1 > 0 {
            (Pos::new(l1 - 1, self.buf.line_len(l1 - 1)), Pos::new(l2, self.buf.line_len(l2)))
        } else {
            (Pos::new(0, 0), Pos::new(l2, self.buf.line_len(l2)))
        };
        Edit { start, end, text: String::new() }
    }

    /// Run `:s` over its line range: at most one edit per line, spanning that
    /// line's first match start to its last match end. Returns how many
    /// matches it replaced (or, with `n`, counted) on how many lines, which
    /// `:g` adds up.
    fn substitute(&mut self, sub: &ex::Substitute, edits: &mut Vec<Edit>) -> (usize, usize) {
        self.clear_pending();
        let last_used = self.last_search.as_ref().map(|s| s.pattern.clone());
        let Some(source) = sub.pattern.clone().or(last_used) else {
            self.message = Some("no previous regular expression".into());
            return (0, 0);
        };
        let pattern = match search::Pattern::parse_case(&source, sub.ignore_case.unwrap_or(false)) {
            Ok(p) => p,
            Err(msg) => {
                self.message = Some(msg);
                return (0, 0);
            }
        };
        pattern.budget_for(&self.buf);
        let previous = self.last_replacement.clone().unwrap_or_default();
        let replacement = ex::expand_tilde(
            sub.replacement.as_deref().unwrap_or(&previous),
            &previous,
        );
        self.last_search = Some(Search { pattern: source.clone(), backward: false });
        if !sub.count_only {
            self.last_replacement = Some(replacement.clone());
        }

        let mut line_edits: Vec<Edit> = Vec::new();
        let (mut hits, mut lines) = (0usize, 0usize);
        // One vector of matches for the whole range: `find_all_into` refills
        // it per line rather than allocating one each time.
        let mut found: Vec<search::LineMatch> = Vec::new();
        for line in sub.first..=sub.last.min(self.buf.last_line()) {
            let text = self.buf.line(line);
            pattern.find_all_into(text, sub.global, &mut found);
            let (Some(first), Some(last)) = (found.first(), found.last()) else {
                continue;
            };
            let (start, end) = (first.start, last.end);
            hits += found.len();
            lines += 1;
            if sub.count_only {
                continue;
            }
            let mut out = String::new();
            // Byte offsets, so the text between matches is a slice and not a
            // column-to-byte walk of the line per match.
            let mut at = first.bytes().0;
            for m in &found {
                let (from, to) = m.bytes();
                out.push_str(&text[at..from.max(at)]);
                out.push_str(&ex::expand(&replacement, text, m));
                at = to;
            }
            line_edits.push(Edit {
                start: Pos::new(line, start),
                end: Pos::new(line, end),
                text: out,
            });
        }

        if hits == 0 {
            if !sub.quiet || pattern.gave_up() {
                self.message = Some(gave_up_or_missing(&pattern, &source));
                self.failed = true;
            }
            return (0, 0);
        }
        if sub.count_only {
            self.message = Some(format!(
                "{hits} match{} on {lines} line{}",
                if hits == 1 { "" } else { "es" },
                plural(lines)
            ));
            return (hits, lines);
        }
        // Bottom-up, so each edit's pre-state coordinates still hold when it
        // reaches the mirror. The host applies them all to one snapshot.
        for e in line_edits.iter().rev() {
            self.edit_mirror(Shift::new(&self.buf, e.start, e.end, &e.text), e);
        }
        // Vim leaves the cursor on the last line it changed; a replacement
        // that broke lines pushes that line further down.
        let added: usize = line_edits.iter().map(|e| e.text.matches('\n').count()).sum();
        let target = line_edits.last().map_or(self.cursor.line, |e| e.start.line) + added;
        // The mirror took the edits bottom-up, so `'.` is on the first; Vim's
        // is on the last, where its top-down run ended.
        if let Some(last) = line_edits.last()
            && !self.quiet
        {
            let above = added - last.text.matches('\n').count();
            self.marks.change = Some(Pos::new(last.start.line + above, last.start.col));
        }
        let target = target.min(self.buf.last_line());
        self.cursor = Pos::new(target, self.buf.first_non_blank(target));
        self.desired_col = self.cursor.col;
        edits.extend(line_edits);
        self.message = Some(format!(
            "{hits} substitution{} on {lines} line{}",
            plural(hits),
            plural(lines)
        ));
        (hits, lines)
    }

    fn resolve_replace(&mut self, key: Key, edits: &mut Vec<Edit>) {
        let count = self.pending.count();
        let ch = match key {
            Key::Char(c) => c,
            // `r<cr>` is Vim's line-splitting special case: the characters go
            // away and *one* line break takes their place, however many the
            // count asked for. Visual `r` fills the selection with the char
            // instead, which a line break cannot do, so it cancels there.
            Key::Enter if !matches!(self.mode, Mode::Visual { .. }) => '\n',
            _ => return self.clear_pending(),
        };
        if let Mode::Visual { linewise } = self.mode {
            self.visual_replace(ch, linewise, edits);
            return;
        }
        let cur = self.cursor;
        let cols = chars_with_cols(self.buf.line(cur.line));
        let idx = cols.partition_point(|&(c, _)| c <= cur.col).saturating_sub(1);
        if cols.is_empty() || idx + count > cols.len() {
            return self.fail(); // not enough chars: r fails
        }
        let end = cols
            .get(idx + count)
            .map_or(Pos::new(cur.line, self.buf.line_len(cur.line)), |&(c, _)| {
                Pos::new(cur.line, c)
            });
        let start = Pos::new(cur.line, cols[idx].0);
        let replacement: String = if ch == '\n' {
            "\n".to_string()
        } else {
            std::iter::repeat_n(ch, count).collect()
        };
        self.emit_edit(start, end, &replacement, edits);
        self.cursor = if ch == '\n' {
            // The rest of the line became the next one; land on its first char.
            self.clamp_normal(Pos::new(cur.line + 1, 0))
        } else {
            self.clamp_normal(Pos::new(
                cur.line,
                cols[idx].0 + ch.len_utf16() * (count - 1),
            ))
        };
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    fn visual_replace(&mut self, ch: char, linewise: bool, edits: &mut Vec<Edit>) {
        let (s, e) = self.visual_range(linewise);
        // One edit per line, none of them changing the line count, so each
        // can go to the mirror as it is made.
        for line in s.line..=e.line.min(self.buf.last_line()) {
            let len = self.buf.line_len(line);
            let from = if line == s.line && !linewise { s.col } else { 0 };
            let to = if line == e.line && !linewise { e.col.min(len) } else { len };
            if from >= to {
                continue;
            }
            let n = self.buf.slice(Pos::new(line, from), Pos::new(line, to)).chars().count();
            let replacement: String = std::iter::repeat_n(ch, n).collect();
            self.emit_edit(Pos::new(line, from), Pos::new(line, to), &replacement, edits);
        }
        self.mode = Mode::Normal;
        self.cursor = self.clamp_normal(s);
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    fn resolve_g(&mut self, key: Key, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        match key {
            Key::Char('g') => {
                let line = if self.pending.has_count() {
                    (self.pending.count() - 1).min(self.buf.last_line())
                } else {
                    0
                };
                let target = Pos::new(line, self.buf.first_non_blank(line));
                self.pending.awaiting = Awaiting::None;
                self.set_pcmark();
                self.do_motion(target, MotionKind::Linewise, edits, commands);
            }
            // Not vim's `gh` (select mode); the editor's hover, as vscode-vim
            // spells it. Leaves mode and cursor alone.
            Key::Char('h') => {
                commands.push(Command::ShowHover);
                self.clear_pending();
            }
            // -- motions
            Key::Char(c @ ('e' | 'E')) => {
                let count = self.pending.count();
                let (target, ok) = motion::word_end_back(&self.buf, self.cursor, c == 'E', count);
                if ok || self.pending.op.is_none() {
                    self.pending.awaiting = Awaiting::None;
                    self.do_motion(target, MotionKind::Inclusive, edits, commands);
                } else {
                    self.fail(); // ran out of buffer: the operator is dropped
                }
            }
            // `g_`: the last non-blank, `count - 1` lines down; inclusive.
            Key::Char('_') => {
                let line = (self.cursor.line + self.pending.count() - 1).min(self.buf.last_line());
                let target = Pos::new(line, self.last_non_blank(line));
                self.pending.awaiting = Awaiting::None;
                self.do_motion(target, MotionKind::Inclusive, edits, commands);
            }
            // -- case operators: `gu{motion}`, `gUU`, visual `gu`
            Key::Char(c @ ('u' | 'U' | '~')) => {
                let op = Op::case_from_char(c).unwrap();
                match self.pending.op {
                    _ if matches!(self.mode, Mode::Visual { .. }) => self.visual_case(op, edits),
                    None => {
                        self.pending.op = Some(op);
                        self.pending.awaiting = Awaiting::None;
                        self.pending.keys.push(c);
                    }
                    Some(prev) if prev == op => self.doubled_case_operator(edits, commands),
                    Some(_) => self.fail(),
                }
            }
            // Everything below is an action, which a pending operator does
            // not take.
            _ if self.pending.op.is_some() => self.fail(),
            Key::Char('J') => self.join_lines_bare(self.pending.count(), edits),
            Key::Char('v') => self.reselect_visual(),
            Key::Char('i') => {
                let at = self.last_insert.unwrap_or(self.cursor);
                self.enter_insert_cleared(at);
            }
            Key::Char('I') => self.enter_insert_cleared(Pos::new(self.cursor.line, 0)),
            Key::Char(c @ ('p' | 'P')) => self.paste_past(c == 'P', self.pending.count(), edits),
            // Not Vim's `gd` (a search for the first use in the function);
            // the language server's definition, as vscode-vim binds it.
            Key::Char('d') => {
                commands.push(Command::GotoDefinition);
                self.clear_pending();
            }
            // `g ctrl-a` / `g ctrl-x`: visual only, each line's number moved
            // one step further than the line above's — `1 1 1` becomes
            // `2 3 4`.
            Key::Ctrl(c @ ('a' | 'x')) if matches!(self.mode, Mode::Visual { .. }) => {
                self.add_sub(c == 'x', self.pending.count(), true, edits)
            }
            Key::Esc => self.clear_pending(),
            _ => self.fail(),
        }
    }

    fn resolve_z(&mut self, key: Key, commands: &mut Vec<Command>) {
        match key {
            Key::Char('z') => commands.push(Command::Scroll { to: "center" }),
            Key::Char('t') => commands.push(Command::Scroll { to: "top" }),
            Key::Char('b') => commands.push(Command::Scroll { to: "bottom" }),
            Key::Esc => {}
            _ => return self.fail(),
        }
        self.clear_pending();
    }

    /// `ZZ` is `:x` and `ZQ` is `:q!` (`:help ZZ`); any other key after `Z`
    /// drops it.
    fn resolve_quit(&mut self, key: Key, commands: &mut Vec<Command>) {
        let file = match key {
            Key::Char('Z') => ex::file_command("xit", false),
            Key::Char('Q') => ex::file_command("quit", true),
            _ => None,
        };
        commands.extend(file.map(Command::File));
        self.clear_pending();
    }

    /// `[(` `[{` `])` `]}`: to the unmatched bracket that opens or closes the
    /// block the cursor is in. The prefix picks the direction and the second
    /// key must be the bracket facing that way — `[` takes an opener, `]` a
    /// closer; anything else (`[}`, `]]`, a letter) drops the pending keys.
    /// Exclusive, like Vim, so `d]}` stops short of the brace.
    fn resolve_bracket(
        &mut self,
        forward: bool,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let Key::Char(c) = key else {
            return self.clear_pending();
        };
        let (open, close) = match c {
            '(' | ')' => ('(', ')'),
            '{' | '}' => ('{', '}'),
            _ => return self.fail(),
        };
        if forward != (c == close) {
            return self.fail();
        }
        let count = self.pending.count();
        match motion::unmatched_bracket(&self.buf, self.cursor, open, close, forward, count) {
            Some(target) => {
                self.pending.awaiting = Awaiting::None;
                self.do_motion(target, MotionKind::Exclusive, edits, commands);
            }
            None => self.fail(),
        }
    }

    fn resolve_object(
        &mut self,
        around: bool,
        key: Key,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let Key::Char(c) = key else {
            return self.clear_pending();
        };
        let Some(obj) = TextObject::parse(c) else {
            return self.fail();
        };
        let visual = match self.mode {
            Mode::Visual { linewise } => Some(textobj::Visual { anchor: self.anchor, linewise }),
            _ => None,
        };
        let count = self.pending.count();
        let Some(sel) = textobj::select(&self.buf, self.cursor, visual, obj, around, count) else {
            return self.fail();
        };
        if let Some(op) = self.pending.op {
            if sel.linewise {
                self.apply_operator_linewise(op, sel.anchor.line, sel.cursor.line, edits, commands);
            } else {
                let end = if sel.inclusive {
                    motion::right(&self.buf, sel.cursor, 1)
                } else {
                    sel.cursor
                };
                // Vim exempts only the tag objects from the end adjustment, so
                // `dit` takes the same text whether or not the end tag is
                // indented.
                let adjust = obj != TextObject::Tag;
                let end = end.max(sel.anchor);
                self.operate_exclusive(op, sel.anchor, end, adjust, edits, commands);
            }
            if op == Op::Yank {
                // Vim leaves the cursor on the object's start, which may lie
                // after it (`yi(` ahead on the line, `yit` from the tag).
                self.cursor = self.clamp_normal(sel.anchor);
                self.desired_col = self.cursor.col;
            }
            self.clear_pending();
        } else if visual.is_some() {
            // Paragraphs make the selection linewise, the rest charwise.
            self.mode = Mode::Visual { linewise: sel.linewise };
            self.anchor = sel.anchor;
            self.cursor = self.clamp_normal(sel.cursor);
            self.desired_col = self.cursor.col;
            self.clear_pending();
        } else {
            self.clear_pending();
        }
    }

    /// `(` / `)`: exclusive, like Vim's, and through `operate_exclusive` with
    /// an operator so `d)` up to a sentence that opens the next line leaves
    /// the line break alone.
    fn sentence_motion(
        &mut self,
        target: Pos,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        match self.pending.op {
            Some(op) => {
                let (s, e) = ordered(self.cursor, target);
                self.operate_exclusive(op, s, e, true, edits, commands);
                self.clear_pending();
            }
            None => self.do_motion(target, MotionKind::Exclusive, edits, commands),
        }
    }

    /// Run `op` over the charwise `[s, e)` of an exclusive motion or object,
    /// applying Vim's rule for an end in column 0 of a later line (`:help
    /// exclusive-linewise`, ops.c `do_pending_operator`) when `adjust` is set:
    /// the end moves back to the end of the line before, and if `s` is at or
    /// before its line's first non-blank the operator turns linewise. That is
    /// what makes `das` on a line's last sentence keep the line break, and
    /// `di{` on a block of whole lines delete those lines.
    ///
    /// A delete gets Vi's other rule too (`:help d`, `cpo-z`): spanning lines
    /// from the indent to the end of a line, it deletes the lines whole.
    fn operate_exclusive(
        &mut self,
        op: Op,
        s: Pos,
        e: Pos,
        adjust: bool,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let mut e = e;
        let blank = |c: char| c == ' ' || c == '\t';
        let in_indent = self.buf.line(s.line).chars().take_while(|&c| blank(c)).count() >= s.col;
        if adjust && e.col == 0 && e.line > s.line {
            let line = e.line - 1;
            if in_indent {
                return self.apply_operator_linewise(op, s.line, line, edits, commands);
            }
            e = Pos::new(line, self.buf.line_len(line));
        }
        if op == Op::Delete && e.line > s.line && in_indent {
            let last = self.buf.line(e.line);
            let rest = &last[crate::buffer::utf16_to_byte(last, e.col)..];
            if rest.chars().all(blank) {
                return self.apply_operator_linewise(op, s.line, e.line, edits, commands);
            }
        }
        self.apply_operator_charwise(op, s, e, edits, commands);
    }

    // ---- operators ----------------------------------------------------------

    fn doubled_operator(&mut self, op: Op, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let count = self.pending.count();
        let l1 = self.cursor.line;
        let l2 = (l1 + count - 1).min(self.buf.last_line());
        self.apply_operator_linewise(op, l1, l2, edits, commands);
        self.clear_pending();
    }

    fn visual_operator(&mut self, op: Op, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let Mode::Visual { linewise } = self.mode else {
            return;
        };
        self.mode = Mode::Normal;
        if linewise || matches!(op, Op::Indent | Op::Dedent) {
            let (s, e) = self.visual_range(true);
            self.apply_operator_linewise(op, s.line, e.line, edits, commands);
        } else {
            let (s, e) = self.visual_range(false);
            self.apply_operator_charwise(op, s, e, edits, commands);
        }
        self.clear_pending();
    }

    /// Visual range. Charwise: `[start, end)` with `end` one past the cursor
    /// char. Linewise: positions carry the line span only.
    fn visual_range(&self, linewise: bool) -> (Pos, Pos) {
        let (s, e) = ordered(self.anchor, self.cursor);
        if linewise {
            (Pos::new(s.line, 0), Pos::new(e.line, self.buf.line_len(e.line)))
        } else {
            (s, motion::right(&self.buf, e, 1))
        }
    }

    fn apply_operator_charwise(
        &mut self,
        op: Op,
        s: Pos,
        e: Pos,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let name = self.pending.register;
        match op {
            Op::Delete => {
                self.yank_range(name, s, e, true);
                self.emit_edit(s, e, "", edits);
                self.cursor = self.clamp_normal(s);
            }
            Op::Change => {
                self.yank_range(name, s, e, true);
                self.emit_edit(s, e, "", edits);
                self.enter_insert(s);
            }
            Op::Yank => {
                self.yank_range(name, s, e, false);
                if s < self.cursor {
                    self.cursor = self.clamp_normal(s);
                }
            }
            Op::Indent | Op::Dedent => {
                commands.push(Command::IndentLines {
                    start_line: s.line,
                    end_line: e.line,
                    dedent: op == Op::Dedent,
                    times: 1,
                });
                self.cursor = self.clamp_normal(Pos::new(s.line, self.cursor.col));
            }
            Op::Lower | Op::Upper | Op::ToggleCase => {
                self.recase_range(op, s, e, false, edits);
                self.cursor = self.clamp_normal(s);
            }
        }
        self.desired_col = self.cursor.col;
    }

    fn apply_operator_linewise(
        &mut self,
        op: Op,
        l1: usize,
        l2: usize,
        edits: &mut Vec<Edit>,
        commands: &mut Vec<Command>,
    ) {
        let l2 = l2.min(self.buf.last_line());
        let name = self.pending.register;
        match op {
            Op::Delete => {
                self.yank_lines(name, l1, l2, true);
                self.delete_lines(l1, l2, edits);
            }
            // `linewise_change` files the lines itself, being `S` too.
            Op::Change => self.linewise_change(l1, l2 - l1 + 1, edits),
            Op::Yank => {
                self.yank_lines(name, l1, l2, false);
                if l1 < self.cursor.line {
                    self.cursor = self.clamp_normal(Pos::new(l1, self.cursor.col));
                }
            }
            Op::Indent | Op::Dedent => {
                commands.push(Command::IndentLines {
                    start_line: l1,
                    end_line: l2,
                    dedent: op == Op::Dedent,
                    times: 1,
                });
                self.cursor = self.clamp_normal(Pos::new(l1, self.cursor.col));
            }
            // The cursor goes to the start of what was changed, which for a
            // motion upward is the line it went to (`gUk`).
            Op::Lower | Op::Upper | Op::ToggleCase => {
                let end = Pos::new(l2, self.buf.line_len(l2));
                self.recase_range(op, Pos::new(l1, 0), end, true, edits);
                if l1 < self.cursor.line {
                    self.cursor = self.clamp_normal(Pos::new(l1, self.cursor.col));
                }
            }
        }
        self.desired_col = self.cursor.col;
    }

    /// File the charwise `[s, e)` into the registers: for a yank, or
    /// (`deleting`) for the delete or change about to take it. `name` is
    /// the `"x` typed, if any; see `register::store` for where it all goes.
    /// An empty span files nothing, as Vim's empty-region rule has it.
    fn yank_range(&mut self, name: Option<char>, s: Pos, e: Pos, deleting: bool) {
        let text = self.buf.slice(s, e);
        if text.is_empty() {
            return;
        }
        let kind = if deleting {
            Kind::Delete { lines: text.contains('\n'), reg_one: self.pending.reg_one }
        } else {
            self.set_op_marks(s, char_before(&self.buf, e));
            Kind::Yank
        };
        self.store_register(name, Register::new(text, false), kind);
    }

    /// File lines `l1..=l2`, linewise; see `yank_range`.
    fn yank_lines(&mut self, name: Option<char>, l1: usize, l2: usize, deleting: bool) {
        let text = self.lines_text(l1, l2);
        let kind = if deleting {
            Kind::Delete { lines: true, reg_one: false }
        } else {
            let len = self.buf.line_len(l2);
            self.set_op_marks(Pos::new(l1, 0), Pos::new(l2, self.last_char_col(l2, len)));
            Kind::Yank
        };
        self.store_register(name, Register::new(text, true), kind);
    }

    /// A register write, held back while `key_multi` runs (see there).
    fn store_register(&mut self, name: Option<char>, reg: Register, kind: Kind) {
        match &mut self.staged {
            Some(staged) => staged.push(Staged { name, reg, kind }),
            None => register::store(name, reg, kind),
        }
    }

    /// Land the register writes `key_multi` held back. The cursors' first
    /// writes go together, then their second ones, and so on: writes of one
    /// kind into one register become a single write of a register with a
    /// piece per cursor, top to bottom, its text the pieces joined by line
    /// breaks — linewise if any piece is. Writes that do not pair up land
    /// one by one, top to bottom.
    fn land_staged(&mut self, writes: Vec<Vec<Staged>>, slot: &[usize]) {
        let mut queues: Vec<(usize, std::vec::IntoIter<Staged>)> =
            writes.into_iter().enumerate().map(|(i, w)| (slot[i], w.into_iter())).collect();
        queues.sort_by_key(|&(rank, _)| rank);
        loop {
            let round: Vec<Staged> = queues.iter_mut().filter_map(|(_, q)| q.next()).collect();
            if round.is_empty() {
                return;
            }
            let alike = round.windows(2).all(|w| {
                w[0].name == w[1].name
                    && matches!((w[0].kind, w[1].kind), (Kind::Yank, Kind::Yank) | (Kind::Delete { .. }, Kind::Delete { .. }))
            });
            if round.len() < 2 || !alike {
                for w in round {
                    register::store(w.name, w.reg, w.kind);
                }
                continue;
            }
            let kind = match round[0].kind {
                Kind::Yank => Kind::Yank,
                Kind::Delete { .. } => Kind::Delete {
                    lines: round.iter().any(|w| matches!(w.kind, Kind::Delete { lines: true, .. })),
                    reg_one: round.iter().any(|w| matches!(w.kind, Kind::Delete { reg_one: true, .. })),
                },
            };
            let linewise = round.iter().any(|w| w.reg.linewise);
            let parts: Vec<String> = round.iter().map(|w| w.reg.text.clone()).collect();
            let reg = Register { text: parts.join("\n"), linewise, parts };
            register::store(round[0].name, reg, kind);
        }
    }

    fn linewise_yank(&mut self, count: usize) {
        let l1 = self.cursor.line;
        let l2 = (l1 + count - 1).min(self.buf.last_line());
        self.yank_lines(self.pending.register, l1, l2, false);
        self.clear_pending();
    }

    /// Delete whole lines including one adjoining newline.
    fn delete_lines(&mut self, l1: usize, l2: usize, edits: &mut Vec<Edit>) {
        let last = self.buf.last_line();
        let (s, e) = if l2 < last {
            (Pos::new(l1, 0), Pos::new(l2 + 1, 0))
        } else if l1 > 0 {
            (
                Pos::new(l1 - 1, self.buf.line_len(l1 - 1)),
                Pos::new(l2, self.buf.line_len(l2)),
            )
        } else {
            (Pos::new(0, 0), Pos::new(l2, self.buf.line_len(l2)))
        };
        // The marks on the lines go with them; the edit alone cannot say
        // which lines those were when an empty line sits beside them.
        let shift = Shift::new(&self.buf, s, e, "").removing(l1, l2);
        let edit = Edit { start: s, end: e, text: String::new() };
        self.edit_mirror(shift, &edit);
        edits.push(edit);
        let line = l1.min(self.buf.last_line());
        self.cursor = Pos::new(line, self.buf.first_non_blank(line));
        self.desired_col = self.cursor.col;
    }

    /// `cc`/`S`: blank the lines (keeping one) and enter insert.
    fn linewise_change(&mut self, l1: usize, count: usize, edits: &mut Vec<Edit>) {
        let l2 = (l1 + count - 1).min(self.buf.last_line());
        self.yank_lines(self.pending.register, l1, l2, true);
        // The first line stays, emptied but for its indent, as Vim's
        // 'autoindent' keeps it (`:help cc`); the marks on the others go
        // with them, as Vim deletes those lines.
        let line = self.buf.line(l1);
        let indent = utf16_len(line) - utf16_len(line.trim_start_matches([' ', '\t']));
        let (s, e) = (Pos::new(l1, indent), Pos::new(l2, self.buf.line_len(l2)));
        let edit = Edit { start: s, end: e, text: String::new() };
        let shift = Shift::new(&self.buf, s, e, "");
        let shift = if l2 > l1 { shift.removing(l1 + 1, l2) } else { shift };
        self.edit_mirror(shift, &edit);
        edits.push(edit);
        self.clear_pending();
        self.enter_insert(s);
    }

    // ---- standalone edit actions ---------------------------------------------

    fn emit_edit(&mut self, s: Pos, e: Pos, text: &str, edits: &mut Vec<Edit>) {
        let edit = Edit { start: s, end: e, text: text.to_string() };
        self.edit_mirror(Shift::new(&self.buf, s, e, text), &edit);
        edits.push(edit);
    }

    /// Apply one of the engine's own edits to the mirror, as `shift` reads
    /// it: the marks follow it, `'.` moves to it, and `'[ ']` take it in.
    fn edit_mirror(&mut self, shift: Shift, e: &Edit) {
        self.buf.apply_change(e.start, e.end, &e.text);
        self.shift_marks(&shift);
        self.note_change(e.start, &e.text);
    }

    /// Apply a change the engine did not make to the mirror; the marks
    /// follow it.
    fn mirror(&mut self, s: Pos, e: Pos, text: &str) {
        let shift = Shift::new(&self.buf, s, e, text);
        self.buf.apply_change(s, e, text);
        self.shift_marks(&shift);
    }

    /// Carry every mark through one edit: the session's, the cursors' `'^`
    /// and `'< '>`, and this document's file marks.
    fn shift_marks(&mut self, shift: &Shift) {
        if shift.is_inline() {
            return; // Vim leaves marks alone for an edit inside one line
        }
        self.marks.shift(shift);
        let cursors = std::iter::once((&mut self.last_insert, &mut self.last_area))
            .chain(self.secondaries.iter_mut().map(|c| (&mut c.last_insert, &mut c.last_area)));
        for (insert, area) in cursors {
            *insert = insert.and_then(|p| shift.mark(p));
            if let Some(area) = area {
                area.anchor = shift.kept(area.anchor);
                area.cursor = shift.kept(area.cursor);
            }
        }
        if let Some(doc) = &self.doc {
            mark::shift_file_marks(doc, shift);
        }
        if self.multi {
            self.shifts.push(shift.clone());
        }
    }

    /// An edit of the engine's own put `text` at `s`: `'.` moves there, and
    /// `'[ ']` take in what it put — or, for the key's first edit, become
    /// just that. The primary cursor's alone.
    fn note_change(&mut self, s: Pos, text: &str) {
        if self.quiet {
            return;
        }
        self.marks.change = Some(s);
        let breaks = text.matches('\n').count();
        let last = match text.rsplit('\n').next() {
            // Whole lines: Vim's `']` is the last of them, column 0.
            _ if text.is_empty() => s,
            Some("") => Pos::new(s.line + breaks - 1, 0),
            Some(tail) => {
                let col = if breaks == 0 { s.col } else { 0 } + utf16_len(tail);
                Pos::new(s.line + breaks, col - tail.chars().last().map_or(0, char::len_utf16))
            }
            None => s,
        };
        let (first, last) = match (self.key_changed, self.marks.op_start, self.marks.op_end) {
            (true, Some(a), Some(b)) => (a.min(s), b.max(last)),
            _ => (s, last),
        };
        self.key_changed = true;
        self.marks.op_start = Some(first);
        self.marks.op_end = Some(last);
    }

    /// `'[ ']` for a yank, which changes nothing; the primary cursor's alone.
    fn set_op_marks(&mut self, first: Pos, last: Pos) {
        if !self.quiet {
            self.marks.op_start = Some(first);
            self.marks.op_end = Some(last);
        }
    }

    /// `x` / `s`: delete `count` chars under/after the cursor.
    fn delete_chars(&mut self, count: usize, edits: &mut Vec<Edit>, then_insert: bool) {
        let cur = self.cursor;
        if self.buf.line_len(cur.line) == 0 {
            return self.clear_pending();
        }
        let e = motion::right(&self.buf, cur, count);
        self.yank_range(self.pending.register, cur, e, true);
        self.emit_edit(cur, e, "", edits);
        self.clear_pending();
        if then_insert {
            self.enter_insert(cur);
        } else {
            self.cursor = self.clamp_normal(cur);
            self.desired_col = self.cursor.col;
        }
    }

    /// `X`: delete `count` chars before the cursor (line-local).
    fn delete_back(&mut self, count: usize, edits: &mut Vec<Edit>) {
        let cur = self.cursor;
        let cols = chars_with_cols(self.buf.line(cur.line));
        let idx = cols.partition_point(|&(c, _)| c < cur.col);
        let from = idx.saturating_sub(count);
        if idx == 0 {
            return self.clear_pending();
        }
        let s = Pos::new(cur.line, cols[from].0);
        let e = Pos::new(cur.line, cur.col);
        self.yank_range(self.pending.register, s, e, true);
        self.emit_edit(s, e, "", edits);
        self.cursor = self.clamp_normal(s);
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    /// `D` / `C`.
    fn delete_to_eol(&mut self, edits: &mut Vec<Edit>, then_insert: bool) {
        let cur = self.cursor;
        let e = Pos::new(cur.line, self.buf.line_len(cur.line));
        self.yank_range(self.pending.register, cur, e, true);
        self.emit_edit(cur, e, "", edits);
        self.clear_pending();
        if then_insert {
            self.enter_insert(cur);
        } else {
            self.cursor = self.clamp_normal(cur);
            self.desired_col = self.cursor.col;
        }
    }

    /// `~`: toggle case of `count` chars, cursor lands after the last one.
    fn toggle_case(&mut self, count: usize, edits: &mut Vec<Edit>) {
        let cur = self.cursor;
        let e = motion::right(&self.buf, cur, count);
        if e == cur {
            return self.clear_pending();
        }
        let toggled: String = self
            .buf
            .slice(cur, e)
            .chars()
            .map(|ch| Op::ToggleCase.recase(ch))
            .collect();
        self.emit_edit(cur, e, &toggled, edits);
        self.cursor = self.clamp_normal(e);
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    fn visual_toggle_case(&mut self, edits: &mut Vec<Edit>) {
        self.visual_case(Op::ToggleCase, edits);
    }

    /// Visual `~`, `u`, `U` (and `g~`, `gu`, `gU`): change the selection's
    /// case and leave the cursor at its start.
    fn visual_case(&mut self, op: Op, edits: &mut Vec<Edit>) {
        let Mode::Visual { linewise } = self.mode else {
            return;
        };
        let (s, e) = self.visual_range(linewise);
        self.recase_range(op, s, e, linewise, edits);
        self.mode = Mode::Normal;
        self.cursor = self.clamp_normal(s);
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    /// Apply a case operator to `[s, e)` (whole lines when `linewise`), one
    /// edit per line it changes. Each edit stays on its own line, so they
    /// can go to the mirror one by one and still be in pre-state terms.
    fn recase_range(&mut self, op: Op, s: Pos, e: Pos, linewise: bool, edits: &mut Vec<Edit>) {
        for line in s.line..=e.line.min(self.buf.last_line()) {
            let len = self.buf.line_len(line);
            let from = if line == s.line && !linewise { s.col } else { 0 };
            let to = if line == e.line && !linewise { e.col.min(len) } else { len };
            if from >= to {
                continue;
            }
            let (start, end) = (Pos::new(line, from), Pos::new(line, to));
            let text = self.buf.slice(start, end);
            let changed: String = text.chars().map(|ch| op.recase(ch)).collect();
            if changed != text {
                self.emit_edit(start, end, &changed, edits);
            }
        }
    }

    /// `guu`, `gUU`, `g~~` and their spelled-out twins (`gugu`). Vim runs a
    /// doubled operator over the `_` motion, whose first-non-blank landing
    /// is where a lone line leaves the cursor — when it was past it. With a
    /// count the operated text starts where the cursor already is, and it
    /// stays (normal.c nv_lineop).
    fn doubled_case_operator(&mut self, edits: &mut Vec<Edit>, commands: &mut Vec<Command>) {
        let Some(op) = self.pending.op else {
            return self.clear_pending();
        };
        if self.pending.count() == 1 {
            let first = self.buf.first_non_blank(self.cursor.line);
            self.cursor.col = self.cursor.col.min(first);
        }
        self.doubled_operator(op, edits, commands);
    }

    /// `gv`: the last visual selection again, in the mode it had. From
    /// visual mode the current selection and the last one trade places, so
    /// a second `gv` comes back (vim :help gv). Without one, nothing.
    fn reselect_visual(&mut self) {
        self.clear_pending();
        let Some(area) = self.last_area else {
            return;
        };
        if let Some(current) = self.visual_area() {
            self.last_area = Some(current);
        }
        self.mode = Mode::Visual { linewise: area.linewise };
        self.anchor = self.clamp_normal(area.anchor);
        self.cursor = self.clamp_normal(area.cursor);
        self.desired_col = self.cursor.col;
    }

    /// Visual `I` / `A`. Outside block mode Vim turns them into linewise
    /// operators that insert *once*, not on every line — the per-line kind
    /// is block visual's (`:help v_b_I`), which multiple cursors do here.
    /// Going linewise pins the anchor's end of the selection to column 0,
    /// so `I` inserts at the start of the selection's first line when it was
    /// made downward and at the cursor when it was made upward, and `A`
    /// appends after the char the selection ends on — before it when the
    /// selection is no wider than column 0 (normal.c v_visop, ops.c
    /// op_insert).
    fn visual_insert(&mut self, append: bool) {
        let pinned = Pos::new(self.anchor.line, 0);
        let (start, end) = if pinned < self.cursor {
            (pinned, self.cursor)
        } else {
            (self.cursor, pinned)
        };
        self.mode = Mode::Normal;
        let at = if append {
            let end = self.clamp_normal(end);
            let narrow = if start.line == end.line { start.col == end.col } else { end.col == 0 };
            if narrow || self.buf.line_len(end.line) == 0 {
                end
            } else {
                motion::right(&self.buf, end, 1)
            }
        } else {
            start
        };
        self.enter_insert_cleared(at);
    }

    /// `J`: join lines with a single space, collapsing leading whitespace.
    fn join_lines(&mut self, count: usize, edits: &mut Vec<Edit>) {
        // In visual mode J joins the selected lines.
        let (l1, joins) = if let Mode::Visual { .. } = self.mode {
            let (s, e) = ordered(self.anchor, self.cursor);
            self.mode = Mode::Normal;
            (s.line, (e.line - s.line).max(1))
        } else {
            (self.cursor.line, count.max(2) - 1)
        };
        let last_join = (l1 + joins).min(self.buf.last_line());
        if last_join == l1 {
            return self.fail();
        }
        let mut joins = Vec::with_capacity(last_join - l1);
        let mut len = self.buf.line_len(l1);
        let mut ends_space = self.buf.line(l1).ends_with(' ');
        let mut cursor_col = len;
        for line in l1 + 1..=last_join {
            let full = self.buf.line(line);
            let next = full.trim_start();
            cursor_col = len;
            let sep = !next.is_empty() && len > 0 && !ends_space && !next.starts_with(')');
            let sep = if sep { " " } else { "" };
            joins.push((utf16_len(&full[..full.len() - next.len()]), sep));
            if !next.is_empty() {
                len += sep.len() + utf16_len(next);
                ends_space = next.ends_with(' ');
            }
        }
        self.emit_joins(l1, &joins, edits);
        self.cursor = self.clamp_normal(Pos::new(l1, cursor_col));
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    /// `gJ`: join lines exactly as they are — no space put in, no indent
    /// taken out. Counts and visual mode as for `J`; the cursor lands where
    /// the last line joined begins.
    fn join_lines_bare(&mut self, count: usize, edits: &mut Vec<Edit>) {
        let (l1, joins) = if let Mode::Visual { .. } = self.mode {
            let (s, e) = ordered(self.anchor, self.cursor);
            self.mode = Mode::Normal;
            (s.line, (e.line - s.line).max(1))
        } else {
            (self.cursor.line, count.max(2) - 1)
        };
        let last_join = (l1 + joins).min(self.buf.last_line());
        if last_join == l1 {
            return self.fail();
        }
        let cursor_col: usize = (l1..last_join).map(|l| self.buf.line_len(l)).sum();
        self.emit_joins(l1, &vec![(0, ""); last_join - l1], edits);
        self.cursor = self.clamp_normal(Pos::new(l1, cursor_col));
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    /// `ctrl-a` / `ctrl-x`: add `count` to the number at or after the
    /// cursor, or take it away, and land on its last digit (vim :help
    /// CTRL-A; number.rs has the scan). In visual mode the first number in
    /// each line's share of the selection changes and the cursor goes back
    /// to the selection's start. `progressive` is `g ctrl-a`: every line that
    /// had a number adds `count` more than the one before it.
    fn add_sub(&mut self, subtract: bool, count: usize, progressive: bool, edits: &mut Vec<Edit>) {
        let amount = count as u64;
        let Mode::Visual { linewise } = self.mode else {
            let line = self.cursor.line;
            let Some(b) = number::bump_at(self.buf.line(line), self.cursor.col, amount, subtract) else {
                return self.fail();
            };
            self.emit_edit(Pos::new(line, b.start), Pos::new(line, b.end), &b.text, edits);
            self.cursor = Pos::new(line, b.last_col());
            self.desired_col = self.cursor.col;
            return self.clear_pending();
        };
        let (s, e) = self.visual_range(linewise);
        let mut step = amount;
        // One edit per line, none of them changing the line count, so each
        // can go to the mirror as it is made.
        for line in s.line..=e.line.min(self.buf.last_line()) {
            let len = self.buf.line_len(line);
            let from = if line == s.line && !linewise { s.col } else { 0 };
            let to = if line == e.line && !linewise { e.col.min(len) } else { len };
            let Some(b) = number::bump_in(self.buf.line(line), from, to, step, subtract) else {
                continue;
            };
            self.emit_edit(Pos::new(line, b.start), Pos::new(line, b.end), &b.text, edits);
            if progressive {
                step = step.saturating_add(amount);
            }
        }
        self.mode = Mode::Normal;
        self.cursor = self.clamp_normal(s);
        self.desired_col = self.cursor.col;
        self.clear_pending();
    }

    /// What a put by this cursor puts: the register `"x` named, or `""`,
    /// and its piece for this cursor when the put runs at several (see
    /// `key_multi`). An empty register says so, as Vim's E353 does.
    fn put_register(&mut self) -> Option<(String, bool)> {
        let name = self.pending.register.unwrap_or('"');
        match self.register(name) {
            Some(reg) => Some((reg.part(self.put_slot).to_string(), reg.linewise)),
            None => {
                self.message = Some(format!("E353: Nothing in register {name}"));
                None
            }
        }
    }

    /// `gp` / `gP`: `p` / `P`, but the cursor ends up just past what was
    /// put — on the line after it, for lines — where the next paste can go.
    fn paste_past(&mut self, before: bool, count: usize, edits: &mut Vec<Edit>) {
        let Some((text, linewise)) = self.put_register() else {
            return self.fail();
        };
        let first = edits.len();
        self.paste_text(&text, linewise, before, count, edits);
        self.clear_pending();
        if edits.len() == first {
            return;
        }
        // The put itself is the last edit `p` makes; in the mirror's terms
        // it ends where its text does.
        let put = &edits[edits.len() - 1];
        let breaks = put.text.matches('\n').count();
        let tail = utf16_len(put.text.rsplit('\n').next().unwrap_or(""));
        let end = if breaks == 0 {
            Pos::new(put.start.line, put.start.col + tail)
        } else {
            Pos::new(put.start.line + breaks, tail)
        };
        self.cursor = if linewise {
            Pos::new(end.line.min(self.buf.last_line()), 0)
        } else {
            self.clamp_normal(end)
        };
        self.desired_col = self.cursor.col;
    }

    /// `p` / `P`.
    fn paste(&mut self, before: bool, count: usize, edits: &mut Vec<Edit>) {
        let Some((text, linewise)) = self.put_register() else {
            return self.fail();
        };
        self.paste_text(&text, linewise, before, count, edits);
        self.clear_pending();
    }

    /// Put `text` from a register at the cursor, or over the selection.
    fn paste_text(&mut self, text: &str, linewise: bool, before: bool, count: usize, edits: &mut Vec<Edit>) {
        let Mode::Visual { linewise: by_lines } = self.mode else {
            return self.paste_at(text, linewise, before, count, edits);
        };
        // Visual put: delete the selection, then put at the gap. The text was
        // read first, so the delete cannot overwrite it. `p` files the
        // selection the way `d` would — under the default name, the one typed
        // being the register to put from — and `P` leaves every register as
        // it was (normal.c nv_put_opt).
        let (s, e) = self.visual_range(by_lines);
        self.mode = Mode::Normal;
        let name = if before { Some('_') } else { None };
        if by_lines {
            self.yank_lines(name, s.line, e.line, true);
            self.delete_lines(s.line, e.line, edits);
        } else {
            self.yank_range(name, s, e, true);
            self.emit_edit(s, e, "", edits);
            self.cursor = self.clamp_normal(s);
        }
        if linewise || by_lines {
            self.paste_at(text, linewise, true, count, edits);
        } else {
            // Insert exactly at the gap (which may sit at end of line,
            // past where a normal-mode cursor can rest).
            self.paste_charwise_at(text, s, count, edits);
        }
    }

    fn paste_at(&mut self, text: &str, linewise: bool, before: bool, count: usize, edits: &mut Vec<Edit>) {
        let cur = self.cursor;
        if linewise {
            let unit = format!("{text}\n");
            let block = unit.repeat(count.max(1));
            if before {
                self.emit_edit(Pos::new(cur.line, 0), Pos::new(cur.line, 0), &block, edits);
                self.cursor = Pos::new(cur.line, self.buf.first_non_blank(cur.line));
            } else if cur.line < self.buf.last_line() {
                let at = Pos::new(cur.line + 1, 0);
                self.emit_edit(at, at, &block, edits);
                self.cursor = Pos::new(cur.line + 1, self.buf.first_non_blank(cur.line + 1));
            } else {
                let at = Pos::new(cur.line, self.buf.line_len(cur.line));
                let text = format!("\n{}", &block[..block.len() - 1]);
                self.emit_edit(at, at, &text, edits);
                let line = (cur.line + 1).min(self.buf.last_line());
                self.cursor = Pos::new(line, self.buf.first_non_blank(line));
            }
        } else {
            let at = if before {
                cur
            } else {
                motion::right(&self.buf, cur, 1)
            };
            self.paste_charwise_at(text, at, count, edits);
        }
        self.desired_col = self.cursor.col;
    }

    fn paste_charwise_at(&mut self, text: &str, at: Pos, count: usize, edits: &mut Vec<Edit>) {
        let text = text.repeat(count.max(1));
        if text.is_empty() {
            return;
        }
        self.emit_edit(at, at, &text, edits);
        // Cursor lands on the last pasted char.
        let newlines = text.matches('\n').count();
        let last_seg = text.rsplit('\n').next().unwrap_or("");
        let end_line = at.line + newlines;
        let end_col = if newlines == 0 {
            at.col + utf16_len(last_seg)
        } else {
            utf16_len(last_seg)
        };
        self.cursor = self.clamp_normal(char_before(&self.buf, Pos::new(end_line, end_col)));
        self.desired_col = self.cursor.col;
    }
}

/// The overlay for a jump: every label still in the running, showing only
/// what is left to type for it.
fn labels_ui(jump: &Jump) -> EasyUi {
    let typed = jump.typed.len();
    EasyUi::Labels {
        labels: jump
            .targets
            .iter()
            .filter(|(_, label)| label.starts_with(&jump.typed))
            .map(|(pos, label)| EasyLabel {
                line: pos.line,
                col: pos.col,
                text: label[typed..].to_string(),
            })
            .collect(),
        first_line: jump.view.0,
        last_line: jump.view.1,
    }
}

fn ordered(a: Pos, b: Pos) -> (Pos, Pos) {
    if a <= b { (a, b) } else { (b, a) }
}

/// Where a position recorded before `edit` ends up once it is applied — the
/// transform VSCode runs on its own selections, used by `key_multi` to carry
/// the cursors below an edit along with it. A position inside the replaced
/// range lands at its end.
fn shifted(p: Pos, edit: &Edit) -> Pos {
    if p <= edit.start {
        return p;
    }
    let breaks = edit.text.matches('\n').count();
    let tail = edit.text.rsplit('\n').next().unwrap_or("");
    let new_end = if breaks == 0 {
        Pos::new(edit.start.line, edit.start.col + utf16_len(tail))
    } else {
        Pos::new(edit.start.line + breaks, utf16_len(tail))
    };
    if p <= edit.end {
        return new_end;
    }
    if p.line == edit.end.line {
        Pos::new(new_end.line, new_end.col + (p.col - edit.end.col))
    } else {
        Pos::new(p.line - edit.end.line + new_end.line, p.col)
    }
}

/// Where `text` ends once it is put in at `at`.
fn text_end(at: Pos, text: &str) -> Pos {
    let breaks = text.matches('\n').count();
    let tail = utf16_len(text.rsplit('\n').next().unwrap_or(""));
    if breaks == 0 { Pos::new(at.line, at.col + tail) } else { Pos::new(at.line + breaks, tail) }
}

fn plural(n: usize) -> &'static str {
    if n == 1 { "" } else { "s" }
}

/// Vim's `'report'` default: an ex command touching more lines than this
/// says how many.
const REPORT: usize = 2;

/// What the `:s` runs under one `:g` add up to, reported once at the end.
#[derive(Default)]
struct Tally {
    hits: usize,
    lines: usize,
    count_only: bool,
}

/// `:sort n`'s key: the first decimal number in `text`, a `-` right before it
/// included. Too long a number saturates rather than wrapping.
fn first_number(text: &str) -> Option<i64> {
    let start = text.find(|c: char| c.is_ascii_digit())?;
    let digits = text[start..].bytes().take_while(u8::is_ascii_digit);
    let value = digits.fold(0i64, |n, d| n.saturating_mul(10).saturating_add(i64::from(d - b'0')));
    Some(if text[..start].ends_with('-') { -value } else { value })
}

/// Carry `:g`'s marks through one edit, made in the coordinates `marks` are
/// in, the way Vim's travel with their lines.
///
/// An edit from the start of one line to the start of another removes the
/// lines in between and leaves the last one whole, mark and all — a deleted
/// line and a line pasted above another both look like that. Any other edit
/// rewrites the lines it spans into the first of them, which keeps that
/// line's mark, the way a join or a `:s` does. Lines an edit adds are
/// unmarked.
fn carry_marks(marks: &mut Vec<bool>, edit: &Edit) {
    let last = marks.len() - 1;
    let (start, end) = (edit.start.line.min(last), edit.end.line.min(last));
    let added = edit.text.matches('\n').count();
    let mut lines = vec![false; added + 1];
    if edit.start.col == 0 && edit.end.col == 0 {
        lines[added] = marks[end];
    } else {
        lines[0] = marks[start];
    }
    marks.splice(start..=end, lines);
}

/// One edit turning `before` into `after`: the lines between what the two
/// have in common at the top and at the bottom, rewritten. `None` when they
/// are the same.
fn lines_diff(before: &Buffer, after: &Buffer) -> Option<Edit> {
    let (old, new) = (before.line_count(), after.line_count());
    let common = old.min(new);
    let head = (0..common).take_while(|&i| before.line(i) == after.line(i)).count();
    let tail = (0..common - head)
        .take_while(|&i| before.line(old - 1 - i) == after.line(new - 1 - i))
        .count();
    let (old_end, new_end) = (old - tail, new - tail);
    let text = (head..new_end).map(|i| after.line(i)).collect::<Vec<_>>().join("\n");
    let edit = |start: Pos, end: Pos, text: String| Some(Edit { start, end, text });
    let eol = |line: usize| Pos::new(line, before.line_len(line));
    match (head == old_end, head == new_end) {
        (true, true) => None,
        // Only lines added: above the first line kept below them, or after
        // the last line when nothing is kept below.
        (true, false) if head < old => edit(Pos::new(head, 0), Pos::new(head, 0), text + "\n"),
        (true, false) => edit(eol(old - 1), eol(old - 1), format!("\n{text}")),
        // Only lines gone: with the line break after them, or the one before
        // them at the end of the buffer. `after` has a line, so they cannot
        // be all of it.
        (false, true) if old_end < old => edit(Pos::new(head, 0), Pos::new(old_end, 0), String::new()),
        (false, true) => edit(eol(head - 1), eol(old - 1), String::new()),
        (false, false) => edit(Pos::new(head, 0), eol(old_end - 1), text),
    }
}

/// Why a scan came back empty: no match, or a pattern so expensive the engine
/// stopped rather than block the editor. Saying which keeps a partial answer
/// from reading as a definitive one.
fn gave_up_or_missing(pat: &search::Pattern, source: &str) -> String {
    if pat.gave_up() {
        format!("gave up: pattern is too slow to run here: {source}")
    } else {
        format!("pattern not found: {source}")
    }
}

/// A register's text as `:registers` shows it: a line break is `^J` (and
/// linewise text ends in one), other control characters `^X`, and a long
/// register is cut short — the list shows one line per register anyway.
fn printable(text: &str, linewise: bool) -> String {
    const LIMIT: usize = 200;
    let mut out = String::new();
    for (i, ch) in text.chars().enumerate() {
        if i == LIMIT {
            out.push('…');
            return out;
        }
        match ch {
            '\n' => out.push_str("^J"),
            c if (c as u32) < 0x20 => {
                out.push('^');
                out.push((c as u8 + b'@') as char);
            }
            '\u{7f}' => out.push_str("^?"),
            c => out.push(c),
        }
    }
    if linewise {
        out.push_str("^J");
    }
    out
}
