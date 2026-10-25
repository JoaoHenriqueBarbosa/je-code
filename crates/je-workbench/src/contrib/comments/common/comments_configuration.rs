//! Porte de `src/vs/workbench/contrib/comments/common/commentsConfiguration.ts`: a seção `comments` das configurações e o
//! objeto que ela forma (`ICommentsConfiguration`).
//!
//! Divergências:
//! - O `getValue<ICommentsConfiguration>('comments')` do original devolve o objeto cru; aqui [`ICommentsConfiguration::from_dynamic`]
//!   o lê do [`Dynamic`] que o serviço de configuração devolve, com a veracidade do JavaScript nos booleanos e [`OpenView::Never`]
//!   quando `openView` não é um dos quatro valores (o original deixaria o texto como veio).

use je_base::dynamic::{Dynamic, Truthy};

/// `COMMENTS_SECTION`.
pub const COMMENTS_SECTION: &str = "comments";

/// `'never' | 'file' | 'firstFile' | 'firstFileUnresolved'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OpenView {
    Never,
    File,
    FirstFile,
    FirstFileUnresolved,
}

impl OpenView {
    /// O valor de texto da configuração.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::File => "file",
            Self::FirstFile => "firstFile",
            Self::FirstFileUnresolved => "firstFileUnresolved",
        }
    }

    /// O modo de um valor de texto da configuração.
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "never" => Some(Self::Never),
            "file" => Some(Self::File),
            "firstFile" => Some(Self::FirstFile),
            "firstFileUnresolved" => Some(Self::FirstFileUnresolved),
            _ => None,
        }
    }
}

/// `ICommentsConfiguration`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ICommentsConfiguration {
    pub open_view: OpenView,
    pub use_relative_time: bool,
    pub visible: bool,
    pub max_height: bool,
    pub collapse_on_resolve: bool,
}

impl ICommentsConfiguration {
    /// O objeto da seção `comments`; `None` quando a seção não existe (`undefined`).
    pub fn from_dynamic(value: &Dynamic) -> Option<Self> {
        if value.is_nullish() {
            return None;
        }
        let open_view = match value.get_str("openView") {
            Dynamic::String(text) => OpenView::from_value(&text.to_string_lossy()).unwrap_or(OpenView::Never),
            _ => OpenView::Never,
        };
        Some(Self {
            open_view,
            use_relative_time: value.get_str("useRelativeTime").is_truthy(),
            visible: value.get_str("visible").is_truthy(),
            max_height: value.get_str("maxHeight").is_truthy(),
            collapse_on_resolve: value.get_str("collapseOnResolve").is_truthy(),
        })
    }
}
