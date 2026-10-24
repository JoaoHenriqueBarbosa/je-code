//! Porte de `src/vs/workbench/services/search/common/searchExtTypesInternal.ts`: as opções de busca que o motor
//! do ripgrep recebe, as da API de extensões mais o número de threads.
//!
//! Diferenças de forma em relação ao original, nenhuma de comportamento:
//! - `Omit<Partial<TextSearchProviderOptions>, 'folderOptions'> & { folderOptions }` vira uma struct com
//!   `folder_options` (uma pasta só, não a lista) e os demais campos como `Option` (o `Partial`).
//! - `RipgrepFileSearchOptions` estende `FileSearchProviderOptionsRipgrep`, que troca `folderOptions` por uma
//!   pasta só e mantém o resto de `FileSearchProviderOptions`.
//! - `interface ... extends` de `RipgrepSearchOptionsCommon` vira o campo `num_threads`.

use std::any::Any;
use std::rc::Rc;

use super::search::ITextSearchPreviewOptions;
use super::search_ext_types::{FileSearchProviderFolderOptions, TextSearchProviderFolderOptions};

/// `RipgrepTextSearchOptions` (`TextSearchProviderOptionsRipgrep` e `RipgrepSearchOptionsCommon`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RipgrepTextSearchOptions {
    pub folder_options: TextSearchProviderFolderOptions,
    pub max_results: Option<i64>,
    pub preview_options: Option<ITextSearchPreviewOptions>,
    pub max_file_size: Option<i64>,
    pub surrounding_context: Option<i64>,
    pub num_threads: Option<i64>,
}

impl RipgrepTextSearchOptions {
    /// Só com a pasta, sem nenhuma outra opção.
    pub fn new(folder_options: TextSearchProviderFolderOptions) -> Self {
        Self {
            folder_options,
            max_results: None,
            preview_options: None,
            max_file_size: None,
            surrounding_context: None,
            num_threads: None,
        }
    }
}

/// `RipgrepFileSearchOptions` (`FileSearchProviderOptionsRipgrep` e `RipgrepSearchOptionsCommon`).
#[derive(Clone)]
pub struct RipgrepFileSearchOptions {
    pub folder_options: FileSearchProviderFolderOptions,
    pub session: Rc<dyn Any>,
    pub max_results: i64,
    pub num_threads: Option<i64>,
}
