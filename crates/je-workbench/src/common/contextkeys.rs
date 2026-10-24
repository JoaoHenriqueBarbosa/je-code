//! Porte de `src/vs/workbench/common/contextkeys.ts`: as chaves de contexto do workbench (estado do workbench, janela,
//! editor, grupo e parte de editor, barras, painéis, visões e recurso) e as duas expressões de barra de menu.
//!
//! Divergências:
//! - **Registro preguiçoso.** No original as constantes são criadas (e entram em `RawContextKey.all()`) quando o módulo
//!   carrega. Aqui cada chave é criada, e registrada, na primeira chamada da sua função (o mesmo modelo de
//!   `je_platform::contextkey::contextkeys`), e a função devolve sempre a mesma chave.
//! - **Nomes.** `ActiveEditorDirtyContext` é [`active_editor_dirty_context`], e assim por diante. As chaves estáticas de
//!   `AbstractResourceContextKey` (`Scheme`, `Filename`, `Dirname`, `Path`, `LangId`, `Resource`, `Extension`,
//!   `HasResource`, `IsFileSystemResource`) viram `resource_*` abaixo. `SingleEditorGroupsContext` é uma expressão
//!   ([`single_editor_groups_context`]), como `MultipleEditorGroupsContext.toNegated()`.
//! - **Mensagens.** `localize(...)` é o texto padrão (sem tradução ativa os dois coincidem).
//!
//! Fora deste módulo (e por quê): `ResourceContextKey` e `StaticResourceContextKey` (a lógica que liga as chaves de
//! recurso a um `URI`) e `applyAvailableEditorIds`, que dependem do `ILanguageService`, do `IModelService`, do
//! `IFileService` e do `IEditorResolverService` lidos pelo contexto escopado de cada grupo, e que só o título e os
//! context keys do grupo (`handleGroupContextKeys`, fora do `EditorGroupView` lógico) usam.

use je_base::dynamic::Dynamic;
use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression, ContextKeyMeta, RawContextKey};
use je_platform::contextkey::contextkeys::is_mac_native_context;

macro_rules! raw_keys {
    ($($(#[$doc:meta])* $name:ident: $key:literal, $default:expr, $meta:expr;)*) => {$(
        $(#[$doc])*
        pub fn $name() -> RawContextKey {
            thread_local! { static KEY: RawContextKey = RawContextKey::new($key, $default, $meta); }
            KEY.with(Clone::clone)
        }
    )*};
}

/// `{ type, description }`, o terceiro argumento do `RawContextKey` que descreve o tipo.
fn typed(ty: &str, description: &str) -> ContextKeyMeta {
    ContextKeyMeta::Info { ty: ty.to_string(), description: description.to_string() }
}

// region: Workbench

raw_keys! {
    workbench_state_context: "workbenchState", Dynamic::Undefined, typed("string", "The kind of workspace opened in the window, either 'empty' (no workspace), 'folder' (single folder) or 'workspace' (multi-root workspace)");
    workspace_folder_count_context: "workspaceFolderCount", 0.0, "The number of root folders in the workspace";

    open_folder_workspace_support_context: "openFolderWorkspaceSupport", true, ContextKeyMeta::Hide;
    enter_multi_root_workspace_support_context: "enterMultiRootWorkspaceSupport", true, ContextKeyMeta::Hide;
    empty_workspace_support_context: "emptyWorkspaceSupport", true, ContextKeyMeta::Hide;

    dirty_working_copies_context: "dirtyWorkingCopies", false, "Whether there are any working copies with unsaved changes";

    remote_name_context: "remoteName", "", "The name of the remote the window is connected to or an empty string if not connected to any remote";

    virtual_workspace_context: "virtualWorkspace", "", "The scheme of the current workspace is from a virtual file system or an empty string.";
    temporary_workspace_context: "temporaryWorkspace", false, "The scheme of the current workspace is from a temporary file system.";

    is_sessions_window_context: "isSessionsWindow", false, "Whether the current window is a agent sessions window.";

    /// Support for FileSystemAccess web APIs (<https://wicg.github.io/file-system-access>).
    has_web_file_system_access: "hasWebFileSystemAccess", false, ContextKeyMeta::Hide;

    embedder_identifier_context: "embedderIdentifier", Dynamic::Undefined, "The identifier of the embedder according to the product service, if one is defined";

    in_automation_context: "inAutomation", false, "Whether VS Code is running under automation/smoke test";
}

// endregion

// region: Window

raw_keys! {
    is_main_window_fullscreen_context: "isFullscreen", false, "Whether the main window is in fullscreen mode";
    is_auxiliary_window_focused_context: "isAuxiliaryWindowFocusedContext", false, "Whether an auxiliary window is focused";

    is_window_always_on_top_context: "isWindowAlwaysOnTop", false, "Whether the window is always on top";

    is_auxiliary_window_context: "isAuxiliaryWindow", false, "Window is an auxiliary window";
}

/// `MenuSettings.MenuBarVisibility`.
const MENU_BAR_VISIBILITY_SETTING: &str = "window.menuBarVisibility";

/// `MenuBarVisibleContext`.
pub fn menu_bar_visible_context() -> ContextKeyExpression {
    let setting = format!("config.{MENU_BAR_VISIBILITY_SETTING}");

    ContextKeyExpr::or([
        is_mac_native_context().expr(),
        ContextKeyExpr::and([
            ContextKeyExpr::not_equals(&setting, "hidden"),
            ContextKeyExpr::not_equals(&setting, "toggle"),
            ContextKeyExpr::not_equals(&setting, "compact"),
        ])
        .expect("a conjunção tem termos"),
    ])
    .expect("a disjunção tem termos")
}

/// `CustomMenuBarVisibleContext`.
pub fn custom_menu_bar_visible_context() -> ContextKeyExpression {
    ContextKeyExpr::and([is_mac_native_context().expr().negate(), menu_bar_visible_context()]).expect("a conjunção tem termos")
}

// endregion

// region: Editor

raw_keys! {
    // Editor State Context Keys
    active_editor_dirty_context: "activeEditorIsDirty", false, "Whether the active editor has unsaved changes";
    active_editor_pinned_context: "activeEditorIsNotPreview", false, "Whether the active editor is not in preview mode";
    active_editor_first_in_group_context: "activeEditorIsFirstInGroup", false, "Whether the active editor is the first one in its group";
    active_editor_last_in_group_context: "activeEditorIsLastInGroup", false, "Whether the active editor is the last one in its group";
    active_editor_sticky_context: "activeEditorIsPinned", false, "Whether the active editor is pinned";
    active_editor_readonly_context: "activeEditorIsReadonly", false, "Whether the active editor is read-only";
    active_compare_editor_can_swap_context: "activeCompareEditorCanSwap", false, "Whether the active compare editor can swap sides";
    active_editor_can_toggle_readonly_context: "activeEditorCanToggleReadonly", true, "Whether the active editor can toggle between being read-only or writeable";
    active_editor_can_revert_context: "activeEditorCanRevert", false, "Whether the active editor can revert";
    active_editor_can_split_in_group_context: "activeEditorCanSplitInGroup", true, ContextKeyMeta::None;
    active_editor_cannot_close_context: "activeEditorCannotClose", false, "Whether the active editor cannot be closed through standard user actions";

    // Editor Kind Context Keys
    active_editor_context: "activeEditor", Dynamic::Null, typed("string", "The identifier of the active editor");
    active_editor_available_editor_ids_context: "activeEditorAvailableEditorIds", "", "The available editor identifiers that are usable for the active editor";
    text_compare_editor_visible_context: "textCompareEditorVisible", false, "Whether a text compare editor is visible";
    text_compare_editor_active_context: "textCompareEditorActive", false, "Whether a text compare editor is active";
    side_by_side_editor_active_context: "sideBySideEditorActive", false, "Whether a side by side editor is active";
    active_custom_editor_diff_can_toggle_layout_context: "activeCustomEditorDiffCanToggleLayout", false, "Whether the active custom editor diff can toggle between inline and side by side layout";
    active_custom_editor_text_diff_context: "activeCustomEditorTextDiff", false, "Whether the active custom editor diff is backed by text documents";

    // Editor Group Context Keys
    editor_group_editors_count_context: "groupEditorsCount", 0.0, "The number of opened editor groups";
    is_top_right_editor_group_context: "isTopRightEditorGroup", false, "Whether the editor group is the top right editor group in the editor part";
    active_editor_group_empty_context: "activeEditorGroupEmpty", false, "Whether the active editor group is empty";
    active_editor_group_index_context: "activeEditorGroupIndex", 0.0, "The index of the active editor group";
    active_editor_group_last_context: "activeEditorGroupLast", false, "Whether the active editor group is the last group";
    active_editor_group_locked_context: "activeEditorGroupLocked", false, "Whether the active editor group is locked";
    multiple_editor_groups_context: "multipleEditorGroups", false, "Whether there are multiple editor groups opened";
    multiple_editors_selected_in_group_context: "multipleEditorsSelectedInGroup", false, "Whether multiple editors have been selected in an editor group";
    two_editors_selected_in_group_context: "twoEditorsSelectedInGroup", false, "Whether exactly two editors have been selected in an editor group";
    selected_editors_in_group_file_or_untitled_resource_context_key: "SelectedEditorsInGroupFileOrUntitledResourceContextKey", true, "Whether all selected editors in a group have a file or untitled resource associated";

    // Editor Part Context Keys
    editor_part_multiple_editor_groups_context: "editorPartMultipleEditorGroups", false, "Whether there are multiple editor groups opened in an editor part";
    editor_part_maximized_editor_group_context: "editorPartMaximizedEditorGroup", false, "Editor Part has a maximized group";

    editor_part_modal_context: "editorPartModal", false, "Whether focus is in a modal editor part";
    editor_part_modal_visible_context: "editorPartModalVisible", false, "Whether a modal editor part is visible";
    editor_part_modal_can_move_to_editor_area_context: "editorPartModalCanMoveToEditorArea", true, "Whether the modal editor can move into an editor area";
    editor_part_modal_maximized_context: "editorPartModalMaximized", false, "Whether the modal editor part is maximized";
    editor_part_modal_navigation_context: "editorPartModalNavigation", false, "Whether the modal editor part has navigation context";
    editor_part_modal_sidebar_context: "editorPartModalSidebar", false, "Whether the modal editor part has a sidebar";
    editor_part_modal_sidebar_visible_context: "editorPartModalSidebarVisible", false, "Whether the modal editor part sidebar is visible";

    // Editor Layout Context Keys
    editors_visible_context: "editorIsOpen", false, "Whether an editor is open";
    editor_area_focus_context: "editorAreaFocus", false, "Whether the editor area (any editor part) has keyboard focus";
    in_editor_zen_mode_context: "inZenMode", false, "Whether Zen mode is enabled";
    is_main_editor_centered_layout_context: "isCenteredLayout", false, "Whether centered layout is enabled for the main editor";
    split_editors_vertically: "splitEditorsVertically", false, "Whether editors split vertically";
    main_editor_area_visible_context: "mainEditorAreaVisible", true, "Whether the editor area in the main window is visible";
    editor_tabs_visible_context: "editorTabsVisible", true, "Whether editor tabs are visible";
}

/// `SingleEditorGroupsContext`: `MultipleEditorGroupsContext.toNegated()`.
pub fn single_editor_groups_context() -> ContextKeyExpression {
    multiple_editor_groups_context().to_negated()
}

// endregion

// region: Side Bar

raw_keys! {
    side_bar_visible_context: "sideBarVisible", false, "Whether the sidebar is visible";
    sidebar_focus_context: "sideBarFocus", false, "Whether the sidebar has keyboard focus";
    active_viewlet_context: "activeViewlet", "", "The identifier of the active viewlet";
}

// endregion

// region: Status Bar

raw_keys! {
    status_bar_focused: "statusBarFocused", false, "Whether the status bar has keyboard focus";
}

// endregion

// region: Title Bar

raw_keys! {
    title_bar_style_context: "titleBarStyle", "custom", "Style of the window title bar";
    title_bar_visible_context: "titleBarVisible", false, "Whether the title bar is visible";
    is_compact_title_bar_context: "isCompactTitleBar", false, "Title bar is in compact mode";
}

// endregion

// region: Banner

raw_keys! {
    banner_focused: "bannerFocused", false, "Whether the banner has keyboard focus";
}

// endregion

// region: Notifications

raw_keys! {
    notification_focused_context: "notificationFocus", true, "Whether a notification has keyboard focus";
    notifications_center_visible_context: "notificationCenterVisible", false, "Whether the notifications center is visible";
    notifications_toasts_visible_context: "notificationToastsVisible", false, "Whether a notification toast is visible";
}

// endregion

// region: Auxiliary Bar

raw_keys! {
    active_auxiliary_context: "activeAuxiliary", "", "The identifier of the active auxiliary panel";
    auxiliary_bar_focus_context: "auxiliaryBarFocus", false, "Whether the auxiliary bar has keyboard focus";
    auxiliary_bar_visible_context: "auxiliaryBarVisible", false, "Whether the auxiliary bar is visible";
    secondary_side_bar_visible_context: "secondarySideBarVisible", false, "Whether the layout surface representing the secondary side bar is visible";
    auxiliary_bar_maximized_context: "auxiliaryBarMaximized", false, "Whether the auxiliary bar is maximized";
}

// endregion

// region: Panel

raw_keys! {
    active_panel_context: "activePanel", "", "The identifier of the active panel";
    panel_focus_context: "panelFocus", false, "Whether the panel has keyboard focus";
    panel_position_context: "panelPosition", "bottom", "The position of the panel, always 'bottom'";
    panel_alignment_context: "panelAlignment", "center", "The alignment of the panel, either 'center', 'left', 'right' or 'justify'";
    panel_visible_context: "panelVisible", false, "Whether the panel is visible";
    panel_maximized_context: "panelMaximized", false, "Whether the panel is maximized";
}

// endregion

// region: Views

raw_keys! {
    focused_view_context: "focusedView", "", "The identifier of the view that has keyboard focus";
}

/// `getVisbileViewContextKey(viewId)`.
pub fn get_visible_view_context_key(view_id: &str) -> String {
    format!("view.{view_id}.visible")
}

// endregion

// region: Resources

raw_keys! {
    // NOTE: o valor padrão tem de ser `undefined`: só ele herda o do contexto pai.
    resource_scheme_context: "resourceScheme", Dynamic::Undefined, typed("string", "The scheme of the resource");
    resource_filename_context: "resourceFilename", Dynamic::Undefined, typed("string", "The file name of the resource");
    resource_dirname_context: "resourceDirname", Dynamic::Undefined, typed("string", "The folder name the resource is contained in");
    resource_path_context: "resourcePath", Dynamic::Undefined, typed("string", "The full path of the resource");
    resource_lang_id_context: "resourceLangId", Dynamic::Undefined, typed("string", "The language identifier of the resource");
    resource_context: "resource", Dynamic::Undefined, typed("URI", "The full value of the resource including scheme and path");
    resource_extension_context: "resourceExtname", Dynamic::Undefined, typed("string", "The extension name of the resource");
    has_resource_context: "resourceSet", Dynamic::Undefined, typed("boolean", "Whether a resource is present or not");
    is_file_system_resource_context: "isFileSystemResource", Dynamic::Undefined, typed("boolean", "Whether the resource is backed by a file system provider");
}

// endregion
