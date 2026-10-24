//! Porte de `src/vs/workbench/contrib/searchEditor/browser/constants.ts`.
//!
//! Divergência: `InSearchEditor` (um `RawContextKey<boolean>`) fica de fora, porque nenhum consumidor do recorte o lê.

pub const SEARCH_EDITOR_SCHEME: &str = "search-editor";

pub const SEARCH_EDITOR_WORKING_COPY_TYPE_ID: &str = "search/editor";

pub const SEARCH_EDITOR_FIND_MATCH_CLASS: &str = "searchEditorFindMatch";

pub const SEARCH_EDITOR_ID: &str = "workbench.editor.searchEditor";

pub const OPEN_NEW_EDITOR_COMMAND_ID: &str = "search.action.openNewEditor";
pub const OPEN_EDITOR_COMMAND_ID: &str = "search.action.openEditor";
pub const TOGGLE_SEARCH_EDITOR_CONTEXT_LINES_COMMAND_ID: &str = "toggleSearchEditorContextLines";

pub const SEARCH_EDITOR_INPUT_TYPE_ID: &str = "workbench.editorinputs.searchEditorInput";

/// `SearchConfiguration.notebookSearchConfig`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotebookSearchConfig {
    pub include_markup_input: bool,
    pub include_markup_preview: bool,
    pub include_code_input: bool,
    pub include_output: bool,
}

/// `SearchConfiguration`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SearchConfiguration {
    pub query: String,
    pub files_to_include: String,
    pub files_to_exclude: String,
    pub context_lines: i32,
    pub match_whole_word: bool,
    pub is_case_sensitive: bool,
    pub is_regexp: bool,
    pub use_exclude_settings_and_ignore_files: bool,
    pub show_includes_excludes: bool,
    pub only_open_editors: bool,
    pub notebook_search_config: NotebookSearchConfig,
}
