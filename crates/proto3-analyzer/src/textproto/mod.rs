//! Protocol Buffers **text format** support.
//!
//! Where the sibling [`crate::parse`] pipeline handles `.proto` schema files,
//! this module handles their *data* counterparts (`.txtpb`, `.textproto`,
//! `.pbtxt`, …). The entry point is [`parse::parse`], which produces a
//! [`parse::ParsedTextproto`] containing:
//!
//! * the AST of fields and values,
//! * header annotations (`# proto-file:`, `# proto-message:`) extracted from
//!   leading `#` comments,
//! * parse-time diagnostics.
//!
//! When a document carries a `# proto-message:` header, the [`schema`] module
//! cross-references the workspace's `.proto` sources to validate field names,
//! value kinds, enum members, singular duplication, and oneof conflicts.

pub mod ast;
pub mod features;
pub mod header;
pub mod lexer;
pub mod parse;
pub mod parser;
pub mod schema;

pub use parse::{parse, ParsedTextproto};
pub use schema::validate;
