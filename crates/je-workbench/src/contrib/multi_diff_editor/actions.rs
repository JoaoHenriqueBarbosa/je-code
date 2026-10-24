//! Porte de `src/vs/workbench/contrib/multiDiffEditor/browser/actions.ts`: os submenus do título do editor e as ações do editor
//! de diff de vários arquivos (ir ao arquivo, mudança anterior e próxima, lista de arquivos, recolher e expandir tudo).
//!
//! Divergências:
//! - **Registro explícito.** Os dois `MenuRegistry.appendMenuItem` do início do arquivo rodam no carregamento do módulo; aqui
//!   são [`register_multi_diff_editor_menus`]. O `registerAction2` de cada ação fica com quem monta o workbench.
//! - **O que não entra (e por quê).** O corpo de `run` de [`GoToFileAction`], [`GoToNextChangeAction`],
//!   [`GoToPreviousChangeAction`], [`FocusFileListAction`], [`CollapseAllAction`] e [`ExpandAllAction`] depende do painel
//!   `MultiDiffEditor`, do `MultiDiffEditorInput` e do `resolveCommandsContext`, que ainda não foram portados, e o de
//!   [`OpenMultiDiffEditorLayoutDebugAction`] ainda do `createMultiDiffEditorLayoutDebugModel` (ver
//!   `multi_diff_editor_layout_debug.rs`). Essas seis ações têm só a descrição (`super({...})`), como `layout_actions.rs`; a
//!   decisão de `run` que não depende do painel, o grupo de destino do editor de depuração, é
//!   [`OpenMultiDiffEditorLayoutDebugAction::target_group`]. As ações que só gravam no armazenamento
//!   ([`ToggleFileListAction`], [`SetFileListViewModeAction`], [`SetFileListPositionAction`]) implementam `Action2` inteiras.
//! - `Codicon.goToFile`, `listFlat`, `arrowDown`, `arrowUp`, `collapseAll` e `expandAll` são o `ThemeIcon` com o id do codicon
//!   (`Codicon` não está portado).
//! - `EditorContextKeys.inMultiDiffEditor` (do editor, não portado) é `ContextKeyExpr.has('inMultiDiffEditor')`, a mesma
//!   expressão `Defined` que a chave daria.
//! - `ViewFileListAsTreeAction` e as irmãs são construtores de [`SetFileListViewModeAction`] e [`SetFileListPositionAction`]
//!   (subclasses sem comportamento próprio). `CommandAction.title: string | ILocalizedString` de `localize2` é
//!   `Title::Localized`.
//! - `KeyMod.Alt | KeyCode.F5` é o `u32` das duas partes.

use je_base::key_codes::{KeyCode, KeyMod};
use je_base::lifecycle::{DisposableRef, to_disposable};
use je_base::nls::localize;
use je_base::themables::ThemeIcon;
use je_instantiation::ServicesAccessor;
use je_platform::action::action_common_categories::Categories;
use je_platform::action::{Category, CommandAction, CommandActionTitle, Icon, Title, Toggled, localize2};
use je_platform::actions::actions::{Action2, Action2Keybinding, Action2MenuItem, Action2Options, AnyMenuItem, MenuId, SubmenuItem, menu_registry};
use je_platform::actions::menu_ids;
use je_platform::contextkey::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::keybinding::keybindings_registry::{KeybindingWeight, Keybindings};
use je_platform::storage::I_STORAGE_SERVICE;

use crate::common::contextkeys::{active_editor_context, is_sessions_window_context};
use crate::contrib::multi_diff_editor::common::multi_diff_editor::{
    MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING, MultiDiffEditorFileListPosition, MultiDiffEditorFileListViewMode,
    multi_diff_editor_file_list_available_context, multi_diff_editor_file_list_enabled_context, multi_diff_editor_file_list_position_context,
    multi_diff_editor_file_list_view_mode_context, multi_diff_editor_file_list_visible_context, set_multi_diff_editor_file_list_position,
    set_multi_diff_editor_file_list_view_mode, toggle_multi_diff_editor_file_list,
};
use crate::services::editor::editor_groups_service::PreferredGroup;

/// `MultiDiffEditor.ID`.
const MULTI_DIFF_EDITOR_ID: &str = "multiDiffEditor";

/// `localize2(key, message)` como o `title` de um `ICommandAction`.
fn title(key: &str, message: &str) -> Title {
    let localized = localize2(key, message);
    Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None })
}

fn theme_icon(id: &str) -> Option<Icon> {
    Some(Icon::Theme(ThemeIcon::from_id(id)))
}

/// `ContextKeyExpr.equals('activeEditor', MultiDiffEditor.ID)`.
fn active_editor_is_multi_diff() -> ContextKeyExpression {
    ContextKeyExpr::equals("activeEditor", MULTI_DIFF_EDITOR_ID)
}

/// `ActiveEditorContext.isEqualTo(MultiDiffEditor.ID)`.
fn active_editor_context_is_multi_diff() -> ContextKeyExpression {
    active_editor_context().is_equal_to(MULTI_DIFF_EDITOR_ID)
}

fn and_of(items: impl IntoIterator<Item = ContextKeyExpression>) -> ContextKeyExpression {
    ContextKeyExpr::and(items).expect("a conjunção tem termos")
}

/// `new MenuId('MultiDiffEditorFilesViewAndPosition')`.
pub fn files_view_and_position_menu() -> MenuId {
    MenuId::for_id("MultiDiffEditorFilesViewAndPosition")
}

/// `filesViewWhen`.
fn files_view_when() -> ContextKeyExpression {
    and_of([
        active_editor_context_is_multi_diff(),
        ContextKeyExpr::has(&format!("config.{MULTI_DIFF_EDITOR_EXPERIMENTAL_FILE_LIST_SETTING}")),
        multi_diff_editor_file_list_available_context().expr(),
        is_sessions_window_context().to_negated(),
    ])
}

/// Os dois `MenuRegistry.appendMenuItem(MenuId.EditorTitle, { submenu, ... })` do início de `actions.ts`.
pub fn register_multi_diff_editor_menus() -> DisposableRef {
    let registry = menu_registry();
    let diff_view = registry.append_menu_item(
        &menu_ids::editor_title(),
        AnyMenuItem::Submenu(SubmenuItem {
            title: Title::Plain(localize("diffView", "Diff View", &[])),
            submenu: menu_ids::diff_editor_view_submenu(),
            icon: None,
            when: Some(and_of([active_editor_context_is_multi_diff(), is_sessions_window_context().to_negated()])),
            group: Some("1_diff".to_string()),
            order: Some(10.0),
            is_selection: None,
            is_split_button: None,
        }),
    );
    let files_view = registry.append_menu_item(
        &menu_ids::editor_title(),
        AnyMenuItem::Submenu(SubmenuItem {
            title: Title::Plain(localize("filesViewAndPosition", "Files View & Position", &[])),
            submenu: files_view_and_position_menu(),
            icon: None,
            when: Some(files_view_when()),
            group: Some("z_files".to_string()),
            order: None,
            is_selection: None,
            is_split_button: None,
        }),
    );
    to_disposable(move || {
        diff_view.dispose();
        files_view.dispose();
    })
}

/// Um item de menu da ação, sem `alt` nem `isHiddenByDefault`.
fn menu_item(id: MenuId, when: Option<ContextKeyExpression>, group: &str, order: f64) -> Action2MenuItem {
    Action2MenuItem { id, null_precondition: false, alt: None, when, group: Some(group.to_string()), order: Some(order), is_hidden_by_default: None }
}

/// `{ primary, weight: KeybindingWeight.EditorContrib, when }`.
fn editor_contrib_keybinding(primary: u32, when: ContextKeyExpression) -> Action2Keybinding {
    Action2Keybinding {
        keybindings: Keybindings { primary: Some(primary), ..Keybindings::default() },
        weight: KeybindingWeight::EDITOR_CONTRIB,
        args: je_base::dynamic::Dynamic::Undefined,
        when: Some(when),
    }
}

fn action_options(command: CommandAction, menu: Vec<Action2MenuItem>, keybinding: Vec<Action2Keybinding>, f1: bool) -> Action2Options {
    Action2Options { command, menu, keybinding, f1 }
}

/// `GoToFileAction`.
pub struct GoToFileAction;

impl GoToFileAction {
    /// O argumento do `super({...})`.
    pub fn desc(&self) -> Action2Options {
        action_options(
            CommandAction {
                icon: theme_icon("go-to-file"),
                precondition: Some(active_editor_context_is_multi_diff()),
                ..CommandAction::new("multiDiffEditor.goToFile", title("goToFile", "Open File"))
            },
            vec![menu_item(menu_ids::multi_diff_editor_file_toolbar(), Some(active_editor_context_is_multi_diff()), "navigation", 22.0)],
            Vec::new(),
            false,
        )
    }
}

/// `OpenMultiDiffEditorLayoutDebugAction`.
pub struct OpenMultiDiffEditorLayoutDebugAction;

impl OpenMultiDiffEditorLayoutDebugAction {
    pub const ID: &'static str = "multiDiffEditor.openLayoutDebug";

    /// O argumento do `super({...})`.
    pub fn desc(&self) -> Action2Options {
        action_options(
            CommandAction {
                category: Some(Category::Localized(Categories::new().developer)),
                precondition: ContextKeyExpr::or([active_editor_context_is_multi_diff(), ContextKeyExpr::has("inMultiDiffEditor")]),
                ..CommandAction::new(Self::ID, title("openMultiDiffEditorLayoutDebug", "Open Multi Diff Editor Layout Debug State"))
            },
            Vec::new(),
            Vec::new(),
            true,
        )
    }

    /// O grupo de `openEditor(..., environmentService.isSessionsWindow ? AUX_WINDOW_GROUP : SIDE_GROUP)`.
    pub fn target_group(is_sessions_window: bool) -> PreferredGroup {
        if is_sessions_window { PreferredGroup::AuxWindow } else { PreferredGroup::Side }
    }
}

/// `GoToNextChangeAction`.
pub struct GoToNextChangeAction;

impl GoToNextChangeAction {
    /// O argumento do `super({...})`.
    pub fn desc(&self) -> Action2Options {
        action_options(
            CommandAction {
                icon: theme_icon("arrow-down"),
                precondition: Some(active_editor_is_multi_diff()),
                ..CommandAction::new("multiDiffEditor.goToNextChange", title("goToNextChange", "Go to Next Change"))
            },
            [menu_ids::editor_title(), menu_ids::compact_window_editor_title()]
                .into_iter()
                .map(|id| menu_item(id, Some(active_editor_is_multi_diff()), "navigation", 2.0))
                .collect(),
            vec![editor_contrib_keybinding(KeyMod::ALT | KeyCode::F5.0 as u32, active_editor_is_multi_diff())],
            true,
        )
    }
}

/// `GoToPreviousChangeAction`.
pub struct GoToPreviousChangeAction;

impl GoToPreviousChangeAction {
    /// O argumento do `super({...})`.
    pub fn desc(&self) -> Action2Options {
        action_options(
            CommandAction {
                icon: theme_icon("arrow-up"),
                precondition: Some(active_editor_is_multi_diff()),
                ..CommandAction::new("multiDiffEditor.goToPreviousChange", title("goToPreviousChange", "Go to Previous Change"))
            },
            [menu_ids::editor_title(), menu_ids::compact_window_editor_title()]
                .into_iter()
                .map(|id| menu_item(id, Some(active_editor_is_multi_diff()), "navigation", 1.0))
                .collect(),
            vec![editor_contrib_keybinding(KeyMod::ALT | KeyMod::SHIFT | KeyCode::F5.0 as u32, active_editor_is_multi_diff())],
            true,
        )
    }
}

/// `FocusFileListAction`.
pub struct FocusFileListAction;

impl FocusFileListAction {
    /// O argumento do `super({...})`.
    pub fn desc(&self) -> Action2Options {
        action_options(
            CommandAction {
                precondition: ContextKeyExpr::and([active_editor_context_is_multi_diff(), multi_diff_editor_file_list_visible_context().expr()]),
                ..CommandAction::new("multiDiffEditor.focusFileList", title("focusFileList", "Focus File List"))
            },
            Vec::new(),
            Vec::new(),
            true,
        )
    }
}

/// `ToggleFileListAction`.
pub struct ToggleFileListAction;

impl Action2 for ToggleFileListAction {
    fn desc(&self) -> Action2Options {
        action_options(
            CommandAction {
                icon: theme_icon("list-flat"),
                toggled: Some(Toggled::Expression(multi_diff_editor_file_list_enabled_context().expr())),
                precondition: Some(files_view_when()),
                ..CommandAction::new("multiDiffEditor.toggleFileList", title("toggleFileList", "Toggle Files"))
            },
            vec![menu_item(menu_ids::editor_title(), Some(files_view_when()), "navigation", 100001.0)],
            Vec::new(),
            true,
        )
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[je_base::dynamic::Dynamic]) {
        toggle_multi_diff_editor_file_list(accessor.get(I_STORAGE_SERVICE).as_ref());
    }
}

/// `SetFileListViewModeAction`.
pub struct SetFileListViewModeAction {
    id: &'static str,
    title: Title,
    view_mode: MultiDiffEditorFileListViewMode,
}

impl Action2 for SetFileListViewModeAction {
    fn desc(&self) -> Action2Options {
        let order = if self.view_mode == MultiDiffEditorFileListViewMode::List { 1.0 } else { 2.0 };
        action_options(
            CommandAction {
                toggled: Some(Toggled::Expression(multi_diff_editor_file_list_view_mode_context().is_equal_to(self.view_mode.as_str()))),
                precondition: Some(files_view_when()),
                ..CommandAction::new(self.id, self.title.clone())
            },
            vec![menu_item(files_view_and_position_menu(), None, "1_view", order)],
            Vec::new(),
            false,
        )
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[je_base::dynamic::Dynamic]) {
        set_multi_diff_editor_file_list_view_mode(accessor.get(I_STORAGE_SERVICE).as_ref(), self.view_mode);
    }
}

/// `ViewFileListAsTreeAction`.
pub struct ViewFileListAsTreeAction;

impl ViewFileListAsTreeAction {
    #[allow(clippy::new_ret_no_self)] // o construtor devolve a ação-base, como `new ViewFileListAsTreeAction()` no original
    pub fn new() -> SetFileListViewModeAction {
        SetFileListViewModeAction {
            id: "multiDiffEditor.viewFileListAsTree",
            title: title("viewFileListAsTree", "View as Tree"),
            view_mode: MultiDiffEditorFileListViewMode::Tree,
        }
    }
}

/// `ViewFileListAsListAction`.
pub struct ViewFileListAsListAction;

impl ViewFileListAsListAction {
    #[allow(clippy::new_ret_no_self)] // o construtor devolve a ação-base, como `new ViewFileListAsListAction()` no original
    pub fn new() -> SetFileListViewModeAction {
        SetFileListViewModeAction {
            id: "multiDiffEditor.viewFileListAsList",
            title: title("viewFileListAsList", "View as List"),
            view_mode: MultiDiffEditorFileListViewMode::List,
        }
    }
}

/// `SetFileListPositionAction`.
pub struct SetFileListPositionAction {
    id: &'static str,
    title: Title,
    position: MultiDiffEditorFileListPosition,
}

impl Action2 for SetFileListPositionAction {
    fn desc(&self) -> Action2Options {
        let order = if self.position == MultiDiffEditorFileListPosition::Left { 1.0 } else { 2.0 };
        action_options(
            CommandAction {
                toggled: Some(Toggled::Expression(multi_diff_editor_file_list_position_context().is_equal_to(self.position.as_str()))),
                precondition: Some(files_view_when()),
                ..CommandAction::new(self.id, self.title.clone())
            },
            vec![menu_item(files_view_and_position_menu(), None, "2_position", order)],
            Vec::new(),
            false,
        )
    }

    fn run(&self, accessor: &ServicesAccessor, _args: &[je_base::dynamic::Dynamic]) {
        set_multi_diff_editor_file_list_position(accessor.get(I_STORAGE_SERVICE).as_ref(), self.position);
    }
}

/// `PositionFileListLeftAction`.
pub struct PositionFileListLeftAction;

impl PositionFileListLeftAction {
    #[allow(clippy::new_ret_no_self)] // o construtor devolve a ação-base, como `new PositionFileListLeftAction()` no original
    pub fn new() -> SetFileListPositionAction {
        SetFileListPositionAction {
            id: "multiDiffEditor.positionFileListLeft",
            title: title("positionFileListLeft", "On the Left"),
            position: MultiDiffEditorFileListPosition::Left,
        }
    }
}

/// `PositionFileListRightAction`.
pub struct PositionFileListRightAction;

impl PositionFileListRightAction {
    #[allow(clippy::new_ret_no_self)] // o construtor devolve a ação-base, como `new PositionFileListRightAction()` no original
    pub fn new() -> SetFileListPositionAction {
        SetFileListPositionAction {
            id: "multiDiffEditor.positionFileListRight",
            title: title("positionFileListRight", "On the Right"),
            position: MultiDiffEditorFileListPosition::Right,
        }
    }
}

/// Os três itens de menu de `CollapseAllAction` e `ExpandAllAction`, que só diferem na condição de colapso.
fn collapse_menu(collapsed: ContextKeyExpression) -> Vec<Action2MenuItem> {
    let in_multi_diff_and_state = || and_of([active_editor_is_multi_diff(), collapsed.clone()]);
    vec![
        // In the agents window this action lives in the editor header overflow (...) menu instead of as a primary toolbar icon.
        menu_item(
            menu_ids::editor_title(),
            Some(and_of([active_editor_is_multi_diff(), collapsed.clone(), is_sessions_window_context().to_negated()])),
            "navigation",
            100.0,
        ),
        // The compact window editor title has no overflow menu, so keep the primary toolbar icon there.
        menu_item(menu_ids::compact_window_editor_title(), Some(in_multi_diff_and_state()), "navigation", 100.0),
        menu_item(
            menu_ids::editor_title(),
            Some(and_of([active_editor_is_multi_diff(), collapsed.clone(), is_sessions_window_context().expr()])),
            "4_collapse",
            10.0,
        ),
    ]
}

/// `CollapseAllAction`.
pub struct CollapseAllAction;

impl CollapseAllAction {
    /// O argumento do `super({...})`.
    pub fn desc(&self) -> Action2Options {
        let not_all_collapsed = ContextKeyExpr::not("multiDiffEditorAllCollapsed");
        action_options(
            CommandAction {
                icon: theme_icon("collapse-all"),
                precondition: ContextKeyExpr::and([active_editor_is_multi_diff(), not_all_collapsed.clone()]),
                ..CommandAction::new("multiDiffEditor.collapseAll", title("collapseAllDiffs", "Collapse All Diffs"))
            },
            collapse_menu(not_all_collapsed),
            Vec::new(),
            true,
        )
    }
}

/// `ExpandAllAction`.
pub struct ExpandAllAction;

impl ExpandAllAction {
    /// O argumento do `super({...})`.
    pub fn desc(&self) -> Action2Options {
        let all_collapsed = ContextKeyExpr::has("multiDiffEditorAllCollapsed");
        action_options(
            CommandAction {
                icon: theme_icon("expand-all"),
                precondition: ContextKeyExpr::and([active_editor_is_multi_diff(), all_collapsed.clone()]),
                ..CommandAction::new("multiDiffEditor.expandAll", title("ExpandAllDiffs", "Expand All Diffs"))
            },
            collapse_menu(all_collapsed),
            Vec::new(),
            true,
        )
    }
}
