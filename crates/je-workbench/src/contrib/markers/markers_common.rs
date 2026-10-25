//! Porte de `src/vs/workbench/contrib/markers/common/markers.ts`: o modo de exibição do painel de problemas
//! (`MarkersViewMode`), os identificadores de ação e de visão (`Markers`) e as chaves de contexto
//! (`MarkersContextKeys`).
//!
//! Notas de tradução:
//! - **`const enum MarkersViewMode`** de texto é o enum [`MarkersViewMode`] com [`MarkersViewMode::as_str`] (o valor que a
//!   chave de contexto `problemsViewMode` guarda) e [`MarkersViewMode::from_value`].
//! - **`namespace Markers`** é a struct sem campos [`Markers`], com as constantes como associadas.
//! - **`namespace MarkersContextKeys`** são funções que devolvem o [`RawContextKey`] do `thread_local` (o `RawContextKey`
//!   registra a chave na lista central ao nascer, então cada uma nasce uma vez por thread).

use je_platform::contextkey::{ContextKeyMeta, RawContextKey};

/// `MarkersViewMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkersViewMode {
    Table,
    Tree,
}

impl MarkersViewMode {
    /// O valor de texto do original (`'table'`, `'tree'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Table => "table",
            Self::Tree => "tree",
        }
    }

    /// O modo de um valor de texto (configuração `problems.defaultViewMode`, chave de contexto).
    pub fn from_value(value: &str) -> Option<Self> {
        match value {
            "table" => Some(Self::Table),
            "tree" => Some(Self::Tree),
            _ => None,
        }
    }
}

/// `namespace Markers`.
pub struct Markers;

impl Markers {
    pub const MARKERS_CONTAINER_ID: &'static str = "workbench.panel.markers";
    pub const MARKERS_VIEW_ID: &'static str = "workbench.panel.markers.view";
    pub const MARKERS_VIEW_STORAGE_ID: &'static str = "workbench.panel.markers";
    pub const MARKER_COPY_ACTION_ID: &'static str = "problems.action.copy";
    pub const MARKER_COPY_MESSAGE_ACTION_ID: &'static str = "problems.action.copyMessage";
    pub const RELATED_INFORMATION_COPY_MESSAGE_ACTION_ID: &'static str = "problems.action.copyRelatedInformationMessage";
    pub const FOCUS_PROBLEMS_FROM_FILTER: &'static str = "problems.action.focusProblemsFromFilter";
    pub const MARKERS_VIEW_FOCUS_FILTER: &'static str = "problems.action.focusFilter";
    pub const MARKERS_VIEW_CLEAR_FILTER_TEXT: &'static str = "problems.action.clearFilterText";
    pub const MARKERS_VIEW_SHOW_MULTILINE_MESSAGE: &'static str = "problems.action.showMultilineMessage";
    pub const MARKERS_VIEW_SHOW_SINGLELINE_MESSAGE: &'static str = "problems.action.showSinglelineMessage";
    pub const MARKER_OPEN_ACTION_ID: &'static str = "problems.action.open";
    pub const MARKER_OPEN_SIDE_ACTION_ID: &'static str = "problems.action.openToSide";
    pub const MARKER_SHOW_PANEL_ID: &'static str = "workbench.action.showErrorsWarnings";
    pub const MARKER_SHOW_QUICK_FIX: &'static str = "problems.action.showQuickFixes";
    pub const TOGGLE_MARKERS_VIEW_ACTION_ID: &'static str = "workbench.actions.view.toggleProblems";
}

thread_local! {
    static MARKERS_VIEW_MODE: RawContextKey = RawContextKey::new("problemsViewMode", MarkersViewMode::Tree.as_str(), ContextKeyMeta::None);
    static MARKERS_TREE_VISIBILITY: RawContextKey = RawContextKey::new("problemsVisibility", false, ContextKeyMeta::None);
    static MARKER_FOCUS: RawContextKey = RawContextKey::new("problemFocus", false, ContextKeyMeta::None);
    static MARKER_VIEW_FILTER_FOCUS: RawContextKey = RawContextKey::new("problemsFilterFocus", false, ContextKeyMeta::None);
    static RELATED_INFORMATION_FOCUS: RawContextKey = RawContextKey::new("relatedInformationFocus", false, ContextKeyMeta::None);
    static SHOW_ERRORS_FILTER: RawContextKey = RawContextKey::new("problems.filter.errors", true, ContextKeyMeta::None);
    static SHOW_WARNINGS_FILTER: RawContextKey = RawContextKey::new("problems.filter.warnings", true, ContextKeyMeta::None);
    static SHOW_INFO_FILTER: RawContextKey = RawContextKey::new("problems.filter.info", true, ContextKeyMeta::None);
    static SHOW_ACTIVE_FILE_FILTER: RawContextKey = RawContextKey::new("problems.filter.activeFile", false, ContextKeyMeta::None);
    static SHOW_EXCLUDED_FILES_FILTER: RawContextKey = RawContextKey::new("problems.filter.excludedFiles", true, ContextKeyMeta::None);
}

/// `MarkersContextKeys.MarkersViewModeContextKey`.
pub fn markers_view_mode_context_key() -> RawContextKey {
    MARKERS_VIEW_MODE.with(Clone::clone)
}

/// `MarkersContextKeys.MarkersTreeVisibilityContextKey`.
pub fn markers_tree_visibility_context_key() -> RawContextKey {
    MARKERS_TREE_VISIBILITY.with(Clone::clone)
}

/// `MarkersContextKeys.MarkerFocusContextKey`.
pub fn marker_focus_context_key() -> RawContextKey {
    MARKER_FOCUS.with(Clone::clone)
}

/// `MarkersContextKeys.MarkerViewFilterFocusContextKey`.
pub fn marker_view_filter_focus_context_key() -> RawContextKey {
    MARKER_VIEW_FILTER_FOCUS.with(Clone::clone)
}

/// `MarkersContextKeys.RelatedInformationFocusContextKey`.
pub fn related_information_focus_context_key() -> RawContextKey {
    RELATED_INFORMATION_FOCUS.with(Clone::clone)
}

/// `MarkersContextKeys.ShowErrorsFilterContextKey`.
pub fn show_errors_filter_context_key() -> RawContextKey {
    SHOW_ERRORS_FILTER.with(Clone::clone)
}

/// `MarkersContextKeys.ShowWarningsFilterContextKey`.
pub fn show_warnings_filter_context_key() -> RawContextKey {
    SHOW_WARNINGS_FILTER.with(Clone::clone)
}

/// `MarkersContextKeys.ShowInfoFilterContextKey`.
pub fn show_info_filter_context_key() -> RawContextKey {
    SHOW_INFO_FILTER.with(Clone::clone)
}

/// `MarkersContextKeys.ShowActiveFileFilterContextKey`.
pub fn show_active_file_filter_context_key() -> RawContextKey {
    SHOW_ACTIVE_FILE_FILTER.with(Clone::clone)
}

/// `MarkersContextKeys.ShowExcludedFilesFilterContextKey`.
pub fn show_excluded_files_filter_context_key() -> RawContextKey {
    SHOW_EXCLUDED_FILES_FILTER.with(Clone::clone)
}
