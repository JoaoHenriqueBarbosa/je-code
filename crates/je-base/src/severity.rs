//! Porte de `src/vs/base/common/severity.ts`.

use crate::strings::equals_ignore_case;

/// `Severity`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Ignore = 0,
    Info = 1,
    Warning = 2,
    Error = 3,
}

const ERROR: &str = "error";
const WARNING: &str = "warning";
const WARN: &str = "warn";
const INFO: &str = "info";
const IGNORE: &str = "ignore";

fn equals_ci(a: &str, b: &str) -> bool {
    let (a, b): (Vec<u16>, Vec<u16>) = (a.encode_utf16().collect(), b.encode_utf16().collect());
    equals_ignore_case(&a, &b)
}

impl Severity {
    /// `Severity.fromValue`: lê `error`, `warning`, `warn` e `info` em qualquer caixa; o resto é `Ignore`.
    pub fn from_value(value: &str) -> Self {
        if value.is_empty() {
            return Self::Ignore;
        }
        if equals_ci(ERROR, value) {
            return Self::Error;
        }
        if equals_ci(WARNING, value) || equals_ci(WARN, value) {
            return Self::Warning;
        }
        if equals_ci(INFO, value) {
            return Self::Info;
        }
        Self::Ignore
    }

    /// `Severity.toString`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Error => ERROR,
            Self::Warning => WARNING,
            Self::Info => INFO,
            Self::Ignore => IGNORE,
        }
    }
}
