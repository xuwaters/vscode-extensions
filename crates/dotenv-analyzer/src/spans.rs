//! Re-export of the shared spans module so existing `crate::spans::*`
//! imports keep working. The implementation lives in `analyzer-core`.

pub use analyzer_core::spans::*;
