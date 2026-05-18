//! Shared diagnostic shape.
//!
//! Per-language analyzers define their own [`DiagnosticCode`]-implementing
//! enum and use [`Diagnostic`] as the carrier. `Severity` is fixed at the
//! four LSP-equivalent levels.

use crate::spans::ByteSpan;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Info,
    Hint,
}

impl Severity {
    /// Stable lowercase identifier — matches `serde(rename_all = "lowercase")`
    /// so it can be embedded in JSON payloads without re-serializing.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
            Severity::Info => "info",
            Severity::Hint => "hint",
        }
    }
}

/// Implemented by per-language `DiagnosticCode` enums so the WASM boundary
/// can convert any code into the stable `LANG####` identifier without
/// caring which language emitted it.
pub trait DiagnosticCode: Copy + Eq + std::hash::Hash + 'static {
    fn as_str(self) -> &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diagnostic<C> {
    pub code: C,
    pub severity: Severity,
    pub message: String,
    pub span: ByteSpan,
}

impl<C> Diagnostic<C> {
    pub fn new(code: C, severity: Severity, message: impl Into<String>, span: ByteSpan) -> Self {
        Diagnostic { code, severity, message: message.into(), span }
    }

    pub fn error(code: C, message: impl Into<String>, span: ByteSpan) -> Self {
        Self::new(code, Severity::Error, message, span)
    }

    pub fn warning(code: C, message: impl Into<String>, span: ByteSpan) -> Self {
        Self::new(code, Severity::Warning, message, span)
    }

    pub fn info(code: C, message: impl Into<String>, span: ByteSpan) -> Self {
        Self::new(code, Severity::Info, message, span)
    }

    pub fn hint(code: C, message: impl Into<String>, span: ByteSpan) -> Self {
        Self::new(code, Severity::Hint, message, span)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum TestCode {
        One,
        Two,
    }

    impl DiagnosticCode for TestCode {
        fn as_str(self) -> &'static str {
            match self {
                TestCode::One => "T001",
                TestCode::Two => "T002",
            }
        }
    }

    #[test]
    fn severity_lowercase_serde_roundtrip() {
        for s in [Severity::Error, Severity::Warning, Severity::Info, Severity::Hint] {
            let json = serde_json::to_string(&s).unwrap();
            assert_eq!(json, format!("\"{}\"", s.as_str()));
            let back: Severity = serde_json::from_str(&json).unwrap();
            assert_eq!(back, s);
        }
    }

    #[test]
    fn constructors_set_severity() {
        let span = ByteSpan::new(0, 1);
        assert_eq!(Diagnostic::error(TestCode::One, "x", span).severity, Severity::Error);
        assert_eq!(Diagnostic::warning(TestCode::One, "x", span).severity, Severity::Warning);
        assert_eq!(Diagnostic::info(TestCode::One, "x", span).severity, Severity::Info);
        assert_eq!(Diagnostic::hint(TestCode::One, "x", span).severity, Severity::Hint);
    }

    #[test]
    fn diagnostic_code_trait_returns_stable_strings() {
        assert_eq!(TestCode::One.as_str(), "T001");
        assert_eq!(TestCode::Two.as_str(), "T002");
    }
}
