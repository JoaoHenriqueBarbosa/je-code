//! Porte de `src/vs/workbench/contrib/notebook/common/notebookProvider.ts`: a descrição de um provedor de notebook contribuído
//! (`NotebookEditorDescriptor`) e o `NotebookProviderInfo`, que sabe dizer se um recurso é dele pelos seletores de nome de arquivo.
//!
//! Divergências:
//! - `basename(resource.fsPath)` é o texto depois da última barra (`/` ou `\`) de `fsPath`, sem o ambiente de `path`.
//! - `NotebookSelector` (`string | IRelativePattern | INotebookExclusiveDocumentFilter`) é o [`NotebookFilenamePattern`].
//! - `descriptor.selectors?.map(...) || descriptor._selectors || []` é o construtor [`NotebookProviderInfo::new`] (a forma com
//!   `filenamePattern`/`excludeFileNamePattern`) e [`NotebookProviderInfo::from_selectors`] (a forma já convertida, `_selectors`, que o
//!   `Memento` devolve).
//! - `RegExp` de [`NotebookProviderInfo::possible_file_ending`] é o `zyarr::RegExp`.

use std::cell::RefCell;

use je_base::glob::{GlobOptions, PatternArg, PatternItem, match_pattern};
use je_base::platform::Platform;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;
use zyarr::RegExp;

use super::notebook_common::{INotebookExclusiveDocumentFilter, NotebookFilenamePattern, TransientOptions, is_document_exclude_pattern};
use crate::services::editor::editor_resolver_service::RegisteredEditorPriority;

/// `{ filenamePattern?: string; excludeFileNamePattern?: string }`.
#[derive(Clone, Debug, Default)]
pub struct NotebookEditorDescriptorSelector {
    pub filename_pattern: Option<String>,
    pub exclude_file_name_pattern: Option<String>,
}

/// `NotebookEditorDescriptor`.
#[derive(Clone, Debug)]
pub struct NotebookEditorDescriptor {
    pub extension: Option<ExtensionIdentifier>,
    pub id: String,
    pub display_name: String,
    pub selectors: Vec<NotebookEditorDescriptorSelector>,
    pub priority: RegisteredEditorPriority,
    pub provider_display_name: String,
}

/// `{ selectors?: NotebookSelector[]; options?: TransientOptions }`.
#[derive(Clone, Debug, Default)]
pub struct NotebookProviderInfoUpdate {
    pub selectors: Option<Vec<NotebookFilenamePattern>>,
    pub options: Option<TransientOptions>,
}

/// `NotebookProviderInfo`.
#[derive(Debug)]
pub struct NotebookProviderInfo {
    pub extension: Option<ExtensionIdentifier>,
    pub id: String,
    pub display_name: String,
    pub priority: RegisteredEditorPriority,
    pub provider_display_name: String,
    selectors: RefCell<Vec<NotebookFilenamePattern>>,
    options: RefCell<TransientOptions>,
}

fn basename(resource: &URI) -> String {
    let fs_path = je_platform::resources::fs_path(&resource);
    fs_path.rsplit(['/', '\\']).next().unwrap_or("").to_string()
}

fn glob_match(pattern: &PatternItem, name: &str) -> bool {
    let options = GlobOptions { ignore_case: Some(true), ..GlobOptions::default() };
    match pattern {
        PatternItem::Str(pattern) => match_pattern(PatternArg::Str(pattern), name, &options),
        PatternItem::Relative(pattern) => match_pattern(PatternArg::Relative(pattern), name, &options),
    }
}

impl NotebookProviderInfo {
    pub fn new(descriptor: NotebookEditorDescriptor) -> Self {
        let selectors = descriptor
            .selectors
            .iter()
            .map(|selector| {
                NotebookFilenamePattern::Exclusive(INotebookExclusiveDocumentFilter {
                    include: selector.filename_pattern.as_deref().map(PatternItem::from),
                    exclude: Some(PatternItem::Str(selector.exclude_file_name_pattern.clone().unwrap_or_default())),
                })
            })
            .collect();
        Self::from_selectors(descriptor, selectors)
    }

    /// A forma que o `Memento` guarda (`_selectors`), com os seletores já convertidos.
    pub fn from_selectors(descriptor: NotebookEditorDescriptor, selectors: Vec<NotebookFilenamePattern>) -> Self {
        Self {
            extension: descriptor.extension,
            id: descriptor.id,
            display_name: descriptor.display_name,
            priority: descriptor.priority,
            provider_display_name: descriptor.provider_display_name,
            selectors: RefCell::new(selectors),
            options: RefCell::new(TransientOptions::default()),
        }
    }

    pub fn selectors(&self) -> Vec<NotebookFilenamePattern> {
        self.selectors.borrow().clone()
    }

    pub fn options(&self) -> TransientOptions {
        self.options.borrow().clone()
    }

    pub fn update(&self, args: NotebookProviderInfoUpdate) {
        if let Some(selectors) = args.selectors {
            *self.selectors.borrow_mut() = selectors;
        }

        if let Some(options) = args.options {
            *self.options.borrow_mut() = options;
        }
    }

    pub fn matches(&self, resource: &URI) -> bool {
        self.selectors.borrow().iter().any(|selector| Self::selector_matches(selector, resource))
    }

    pub fn selector_matches(selector: &NotebookFilenamePattern, resource: &URI) -> bool {
        let name = basename(resource);
        match selector {
            NotebookFilenamePattern::Pattern(pattern) => {
                if glob_match(&PatternItem::Str(pattern.clone()), &name) {
                    return true;
                }
            }
            NotebookFilenamePattern::Relative(pattern) => {
                if glob_match(&PatternItem::Relative(pattern.clone()), &name) {
                    return true;
                }
            }
            NotebookFilenamePattern::Exclusive(_) => {}
        }

        if !is_document_exclude_pattern(selector) {
            return false;
        }

        let NotebookFilenamePattern::Exclusive(filter) = selector else {
            return false;
        };
        let (Some(filename_pattern), exclude_filename_pattern) = (&filter.include, &filter.exclude) else {
            return false;
        };

        if glob_match(filename_pattern, &name) {
            if let Some(exclude_filename_pattern) = exclude_filename_pattern
                && !matches!(exclude_filename_pattern, PatternItem::Str(text) if text.is_empty())
                && glob_match(exclude_filename_pattern, &name)
            {
                return false;
            }
            return true;
        }

        false
    }

    pub fn possible_file_ending(selectors: &[NotebookFilenamePattern]) -> Option<String> {
        selectors.iter().find_map(Self::possible_file_ending_of)
    }

    fn possible_file_ending_of(selector: &NotebookFilenamePattern) -> Option<String> {
        let candidate: Option<String> = match selector {
            NotebookFilenamePattern::Pattern(pattern) => Some(pattern.clone()),
            NotebookFilenamePattern::Relative(pattern) => Some(pattern.pattern.clone()),
            NotebookFilenamePattern::Exclusive(filter) => match &filter.include {
                Some(PatternItem::Str(pattern)) if !pattern.is_empty() => Some(pattern.clone()),
                Some(PatternItem::Relative(pattern)) => Some(pattern.pattern.clone()),
                _ => return None,
            },
        };

        let candidate = candidate?;
        let pattern: Vec<u16> = r"^.*(\.[a-zA-Z0-9_-]+)$".encode_utf16().collect();
        let regexp = RegExp::new(&pattern, &[]).expect("o padrão fixo é válido");
        let input: Vec<u16> = candidate.encode_utf16().collect();
        let found = regexp.exec(&input, 0)?;
        let group = found.captures.get(1)?.clone()?;
        Some(String::from_utf16_lossy(&input[group]))
    }
}
