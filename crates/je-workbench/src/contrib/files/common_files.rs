//! Porte parcial de `src/vs/workbench/contrib/files/common/files.ts`: o `UndoConfirmLevel`, os valores de
//! `explorer.incrementalNaming`, `VIEW_ID`, `SESSIONS_FILES_VIEW_ID`, `SortOrder`, `LexicographicOptions` e
//! `ISortOrderConfiguration`, que o explorador lê da configuração.
//!
//! O resto do módulo (`VIEWLET_ID`, `IFilesConfiguration`, `OpenEditor` e as chaves de contexto) entra com
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

/// `VIEW_ID`: explorer file view id.
pub const VIEW_ID: &str = "workbench.explorer.fileView";

/// `SESSIONS_FILES_VIEW_ID`.
pub const SESSIONS_FILES_VIEW_ID: &str = "sessions.files.explorer";

/// `SortOrder` (`explorer.sortOrder`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortOrder {
    Default,
    Mixed,
    FilesFirst,
    Type,
    Modified,
    FoldersNestsFiles,
}

impl SortOrder {
    /// O valor de texto do `const enum`.
    pub const fn as_str(self) -> &'static str {
        match self {
            SortOrder::Default => "default",
            SortOrder::Mixed => "mixed",
            SortOrder::FilesFirst => "filesFirst",
            SortOrder::Type => "type",
            SortOrder::Modified => "modified",
            SortOrder::FoldersNestsFiles => "foldersNestsFiles",
        }
    }

    /// O valor que a configuração devolveu; um texto fora do enum (que o esquema da configuração não admite) vale
    /// [`SortOrder::Default`].
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("mixed") => SortOrder::Mixed,
            Some("filesFirst") => SortOrder::FilesFirst,
            Some("type") => SortOrder::Type,
            Some("modified") => SortOrder::Modified,
            Some("foldersNestsFiles") => SortOrder::FoldersNestsFiles,
            _ => SortOrder::Default,
        }
    }
}

/// `LexicographicOptions` (`explorer.sortOrderLexicographicOptions`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexicographicOptions {
    Default,
    Upper,
    Lower,
    Unicode,
}

impl LexicographicOptions {
    /// O valor de texto do `const enum`.
    pub const fn as_str(self) -> &'static str {
        match self {
            LexicographicOptions::Default => "default",
            LexicographicOptions::Upper => "upper",
            LexicographicOptions::Lower => "lower",
            LexicographicOptions::Unicode => "unicode",
        }
    }

    /// O valor que a configuração devolveu; um texto fora do enum vale [`LexicographicOptions::Default`].
    pub fn from_setting(value: Option<&str>) -> Self {
        match value {
            Some("upper") => LexicographicOptions::Upper,
            Some("lower") => LexicographicOptions::Lower,
            Some("unicode") => LexicographicOptions::Unicode,
            _ => LexicographicOptions::Default,
        }
    }
}

/// `ISortOrderConfiguration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SortOrderConfiguration {
    pub sort_order: SortOrder,
    pub lexicographic_options: LexicographicOptions,
    pub reverse: bool,
}
