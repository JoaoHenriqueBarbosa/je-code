//! Porte de `src/vs/workbench/contrib/multiDiffEditor/test/browser/actions.test.ts` (suíte `MultiDiffEditor Actions`), caso a
//! caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no início de cada caso. O registro dos submenus que o
//!   `actions.ts` faz ao carregar é [`register_multi_diff_editor_menus`], chamado no começo do primeiro caso (o menu registry
//!   é por thread, e cada caso roda na sua).
//! - `new ViewFileListAsListAction().desc` é `Action2::desc`; `action.desc.menu` (objeto único, não lista) é o primeiro e único
//!   item de `menu`; `hasKey(toggled, { condition: true }) ? toggled.condition : toggled` é a variante de `Toggled`.
//!   `keys().sort()` é o `keys()` ordenado. `{ id: 'list-flat' }` é o `ThemeIcon` de id `list-flat`.
//! - **Recorte do segundo caso.** `getLayoutDebugTargetGroup` roda `OpenMultiDiffEditorLayoutDebugAction.run` com um
//!   `IEditorService` falso cujo painel ativo expõe `getLayoutDebugState`, e lê o grupo que chegou a `openEditor`. O `run` não
//!   entra no porte (depende do `createMultiDiffEditorLayoutDebugModel` e do `MultiDiffEditor`, ver `actions.rs`), então o
//!   caso confere a decisão que o `run` toma, [`OpenMultiDiffEditorLayoutDebugAction::target_group`], para a janela comum e
//!   para a janela de agentes (`isSessionsWindow`). Os dublês de `IModelService`, `ILanguageService`, `ITextModel` e do
//!   `ServicesAccessor` só serviam ao `run` e não existem aqui.
//! - `PreferredGroup` não tem igualdade (pode guardar um grupo), então a comparação é pelo número do grupo (`as_number`):
//!   `SIDE_GROUP` e `AUX_WINDOW_GROUP`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::themables::ThemeIcon;
use je_platform::action::{Icon, Toggled};
use je_platform::actions::actions::{Action2, AnyMenuItem, menu_registry};
use je_platform::actions::menu_ids;
use je_workbench::contrib::multi_diff_editor::actions::{
    OpenMultiDiffEditorLayoutDebugAction, PositionFileListLeftAction, PositionFileListRightAction, ToggleFileListAction, ViewFileListAsListAction,
    ViewFileListAsTreeAction, register_multi_diff_editor_menus,
};
use je_workbench::services::editor::editor_groups_service::{AUX_WINDOW_GROUP, SIDE_GROUP};

/// `{ id, submenu, group, order, toggled }` de uma das quatro ações do submenu.
#[derive(Debug, PartialEq)]
struct Entry {
    id: String,
    submenu: String,
    group: Option<String>,
    order: Option<f64>,
    toggled: Option<String>,
}

fn entry_of(action: &dyn Action2) -> Entry {
    let desc = action.desc();
    assert!(desc.menu.len() == 1, "a ação tem um único item de menu");
    let menu = &desc.menu[0];
    let toggled = desc.command.toggled.as_ref().map(|toggled| match toggled {
        Toggled::Expression(expression) => expression.serialize(),
        Toggled::Info(info) => info.condition.serialize(),
    });
    Entry { id: desc.command.id.clone(), submenu: menu.id.id.clone(), group: menu.group.clone(), order: menu.order, toggled }
}

#[test]
fn groups_the_view_modes_and_positions_in_the_editor_title_overflow_submenu() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    guard.add(register_multi_diff_editor_menus());

    let submenu = menu_registry()
        .get_menu_items(&menu_ids::editor_title())
        .into_iter()
        .find_map(|item| match item {
            AnyMenuItem::Submenu(submenu) if submenu.submenu.id == "MultiDiffEditorFilesViewAndPosition" => Some(submenu),
            _ => None,
        })
        .expect("o submenu está registrado");
    let actions: Vec<Box<dyn Action2>> = vec![
        Box::new(ViewFileListAsListAction::new()),
        Box::new(ViewFileListAsTreeAction::new()),
        Box::new(PositionFileListLeftAction::new()),
        Box::new(PositionFileListRightAction::new()),
    ];
    let entries: Vec<Entry> = actions.iter().map(|action| entry_of(action.as_ref())).collect();
    let toggle = ToggleFileListAction.desc();
    assert!(toggle.menu.len() == 1, "a ação de alternar tem um único item de menu");
    let mut toggle_precondition_keys = toggle.command.precondition.as_ref().map(|precondition| precondition.keys()).unwrap_or_default();
    toggle_precondition_keys.sort();

    assert_eq!(submenu.group.as_deref(), Some("z_files"));
    assert_eq!(toggle.command.icon, Some(Icon::Theme(ThemeIcon::from_id("list-flat"))));
    assert_eq!(toggle.menu[0].order, Some(100001.0));
    assert_eq!(
        toggle_precondition_keys,
        vec!["activeEditor", "config.multiDiffEditor.experimental.fileList", "isSessionsWindow", "multiDiffEditorFileListAvailable"]
    );
    let submenu_id = submenu.submenu.id.clone();
    assert_eq!(
        entries,
        vec![
            Entry {
                id: "multiDiffEditor.viewFileListAsList".to_string(),
                submenu: submenu_id.clone(),
                group: Some("1_view".to_string()),
                order: Some(1.0),
                toggled: Some("multiDiffEditorFileListViewMode == 'list'".to_string()),
            },
            Entry {
                id: "multiDiffEditor.viewFileListAsTree".to_string(),
                submenu: submenu_id.clone(),
                group: Some("1_view".to_string()),
                order: Some(2.0),
                toggled: Some("multiDiffEditorFileListViewMode == 'tree'".to_string()),
            },
            Entry {
                id: "multiDiffEditor.positionFileListLeft".to_string(),
                submenu: submenu_id.clone(),
                group: Some("2_position".to_string()),
                order: Some(1.0),
                toggled: Some("multiDiffEditorFileListPosition == 'left'".to_string()),
            },
            Entry {
                id: "multiDiffEditor.positionFileListRight".to_string(),
                submenu: submenu_id,
                group: Some("2_position".to_string()),
                order: Some(2.0),
                toggled: Some("multiDiffEditorFileListPosition == 'right'".to_string()),
            },
        ]
    );
}

#[test]
fn opens_layout_debug_in_an_auxiliary_window_from_the_agents_window() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let workbench = OpenMultiDiffEditorLayoutDebugAction::target_group(false).as_number();
    let agents_window = OpenMultiDiffEditorLayoutDebugAction::target_group(true).as_number();

    assert_eq!((workbench, agents_window), (Some(SIDE_GROUP), Some(AUX_WINDOW_GROUP)));
}
