//! Porte parcial de `src/vs/workbench/contrib/files/common/files.ts`: o `UndoConfirmLevel` e os valores de
//! `explorer.incrementalNaming`, que o recortar, copiar e colar do explorador leem da configuração.
//!
//! O resto do módulo (`VIEWLET_ID`, `IFilesConfiguration`, `SortOrder`, `OpenEditor` e as chaves de contexto) entra com
//! os consumidores que o leem.

/// `UndoConfirmLevel` (`explorer.confirmUndo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoConfirmLevel {
    Verbose,
    Default,
    Light,
}

impl UndoConfirmLevel {
    /// O valor de texto do `const enum` (`'verbose' | 'default' | 'light'`).
    pub const fn as_str(self) -> &'static str {
        match self {
            UndoConfirmLevel::Verbose => "verbose",
            UndoConfirmLevel::Default => "default",
            UndoConfirmLevel::Light => "light",
        }
    }

    /// `value === UndoConfirmLevel.X` para o valor que a configuração devolveu.
    pub fn is(self, value: Option<&str>) -> bool {
        value == Some(self.as_str())
    }
}

/// `explorer.incrementalNaming`: `'simple' | 'smart' | 'disabled'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncrementalNamingSetting {
    Simple,
    Smart,
    Disabled,
}

impl IncrementalNamingSetting {
    /// O valor que a configuração devolveu. O original compara o texto com `'disabled'` e com `'simple'` e trata todo o
    /// resto como o ramo inteligente (`incrementFileName` só testa `=== 'simple'`), então um valor desconhecido ou ausente
    /// é [`IncrementalNamingSetting::Smart`].
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("simple") => IncrementalNamingSetting::Simple,
            Some("disabled") => IncrementalNamingSetting::Disabled,
            _ => IncrementalNamingSetting::Smart,
        }
    }

    /// O `'simple' | 'smart'` que o `incrementFileName` aceita, ou `None` quando é `'disabled'`.
    pub fn enabled(self) -> Option<IncrementalNaming> {
        match self {
            IncrementalNamingSetting::Simple => Some(IncrementalNaming::Simple),
            IncrementalNamingSetting::Smart => Some(IncrementalNaming::Smart),
            IncrementalNamingSetting::Disabled => None,
        }
    }
}

/// `'simple' | 'smart'`: o modo de numeração que `incrementFileName` recebe.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IncrementalNaming {
    Simple,
    Smart,
}
