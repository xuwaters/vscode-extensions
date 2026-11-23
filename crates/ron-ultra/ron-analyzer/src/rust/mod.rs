//! Reading the Rust code a RON file deserializes into.
//!
//! [`parser`] skims item declarations out of a `.rs` file, [`cargo`] reads
//! the manifests that say where crates start, and [`world`] ties them
//! together into module and name resolution over files the host loads on
//! demand.

pub mod cargo;
pub mod lexer;
pub mod parser;
pub mod syntax;
pub mod world;
