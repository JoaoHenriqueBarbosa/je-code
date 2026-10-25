//! Porte de `src/vs/workbench/test/browser/parts/sidebar/sidebarPart.test.ts` (suíte `SidebarPart`).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo do caso; o `store.add(...)` do original é
//!   `guard.add_rc`.
//! - **`TestSidebarLayoutService`** é o [`PaneTestLayoutService`] com a `Cell` `side_bar_position` que o caso reatribui;
//!   **`TestViewDescriptorService`** é o [`TestViewDescriptorService`] (sem contêineres e com o contêiner padrão `undefined`).
//! - **DOM vira estado nativo.** O `document.createElement('div')` do contêiner e os dois `Sash` são nós da árvore de widgets
//!   (`Ui`): o `container.children.item(0)` e o `item(1)` são os dois primeiros filhos do nó, e `classList.contains` é `has_class`.
//!   O `store.add(new Sash(...))` é `guard.add_rc`.
//! - **O serviço de instanciação** é a [`CompositeBarFactory`] dos serviços (nenhum caso mostra a barra de atividades, então ela
//!   nunca é chamada), o `IHostService` é o `TestHostService` das partes e a configuração é a `TestConfigurationService` vazia.
//! - **`part.dispose()`** é o `dispose` da [`SidebarPart`], que descarta a classe do sash.
//! - **`deepStrictEqual` de arrays** é `assert_eq!`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
#[path = "support/pane_composite_part_support.rs"]
mod pane_composite_part_support;
#[path = "support/part_doubles.rs"]
mod part_doubles;
#[path = "../../je-editor/tests/support/test_color_theme_service.rs"]
mod test_color_theme_service;
#[path = "../../je-editor/tests/support/test_notification_service.rs"]
mod test_notification_service;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::lifecycle::IDisposable;
use je_editor::services::theme_service::IThemeService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_ui::{Role, Ui};
use je_widgets::sash::{BoundarySashes, Sash, SashOptions};
use je_workbench::browser::parts::sidebar::sidebar_part::{SidebarPart, SidebarPartServices};
use je_workbench::services::layout::layout_service::{Parts, Position};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use notification_doubles::{NoCommandService, NoKeybindingService, NoMenuService};
use pane_composite_part_support::{
    PaneTestDeps, PaneTestLayoutService, StubPaneView, TestViewDescriptorService, empty_instantiation_service, pane_part_services, test_storage_service,
};
use part_doubles::{TestContextMenuService, TestHostService};
use test_color_theme_service::TestThemeService;
use test_notification_service::TestNotificationService;

/// O `workbenchInstantiationService(undefined, store)` do caso.
fn deps(guard: &LeakGuard) -> PaneTestDeps {
    let theme_service = guard.add_rc(TestThemeService::new_default());
    let theme_service: Rc<dyn IThemeService> = theme_service;
    PaneTestDeps {
        notification_service: Rc::new(TestNotificationService),
        theme_service,
        keybinding_service: Rc::new(NoKeybindingService),
        context_key_service: MockContextKeyService::new(),
        menu_service: Rc::new(NoMenuService),
        context_menu_service: Rc::new(TestContextMenuService),
        command_service: Rc::new(NoCommandService),
        configuration_service: TestConfigurationService::new(None),
        view_descriptor_service: Rc::new(TestViewDescriptorService { can_move_views: false }),
        extension_service: TestExtensionService::new(),
        storage_service: test_storage_service(guard),
    }
}

#[test]
fn moves_the_primary_side_bar_class_to_the_inward_boundary_sash() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let deps = deps(&guard);
    let layout_service = PaneTestLayoutService::new(&[Parts::SidebarPart, Parts::ActivitybarPart, Parts::EditorPart]);
    let part = guard.add_rc(SidebarPart::new(SidebarPartServices {
        pane: pane_part_services(&deps, layout_service.clone(), StubPaneView::new(), empty_instantiation_service()),
        host_service: { let service = guard.add_rc(TestHostService::new()); service },
    }));

    let ui = Ui::new();
    let root = ui.root();
    let container = ui.tree_mut().create(root, Role::Generic);
    let left_sash = guard.add_rc(Sash::new_vertical(&ui, container, Rc::new(|_: &Sash| 0.0), SashOptions::default()));
    let right_sash = guard.add_rc(Sash::new_vertical(&ui, container, Rc::new(|_: &Sash| 0.0), SashOptions::default()));
    let (left_sash_element, right_sash_element) = {
        let tree = ui.tree();
        (tree.children(container).first().copied(), tree.children(container).get(1).copied())
    };
    let mut states: Vec<[bool; 2]> = Vec::new();
    let mut capture_state = || {
        let tree = ui.tree();
        states.push([
            left_sash_element.is_some_and(|node| tree.has_class(node, "primary-sidebar-sash")),
            right_sash_element.is_some_and(|node| tree.has_class(node, "primary-sidebar-sash")),
        ]);
    };

    let sashes = BoundarySashes { left: Some(left_sash.clone()), right: Some(right_sash.clone()), ..BoundarySashes::default() };
    part.set_boundary_sashes(&sashes);
    capture_state();

    layout_service.side_bar_position.set(Position::Right);
    part.set_boundary_sashes(&sashes);
    capture_state();

    part.dispose();
    capture_state();

    assert_eq!(states, [[false, true], [true, false], [false, false]]);
}
