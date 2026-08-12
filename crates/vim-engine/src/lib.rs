//! vim-engine: a modal Vim editing engine designed to run behind a thin
//! editor host (VSCode via WASM, or natively for tests).
//!
//! The host owns the real document and keyboard; the engine owns modal state
//! and a line mirror of the document. Keys go in, `Effects` come out: edits
//! to apply, the selection to set, host commands (undo/scroll/indent), and
//! status info. See state.rs for the protocol details.

pub mod buffer;
pub mod ex;
pub mod keys;
pub mod motion;
pub mod regex;
pub mod search;
pub mod state;
pub mod textobj;
pub mod wasm_api;

pub use buffer::{Buffer, Pos};
pub use keys::Key;
pub use search::{Pattern, Search};
pub use state::{Command, Edit, Effects, Mode, SearchUi, Selection, Session};
