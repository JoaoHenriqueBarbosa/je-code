//! Porte de `src/vs/workbench/test/browser/parts/paneCompositePart.test.ts` (suíte `Pane composite part layout`).
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo do caso; o `store.add(...)` do original é
//!   `guard.add_rc`. O `toDisposable(() => root.remove())` não tem estado a descartar: a raiz é um [`ViewElement`] solto.
//! - **DOM vira estado nativo.** `append(mainWindow.document.body, $('.monaco-workbench'))` e `append(root, $('.part.panel'))` são
//!   `ViewElement::with_classes` ligados por `set_parent`; `part.element.style.height` e `classList.contains(...)` são
//!   `style_get_property` e `class_list_contains`. A vista é a [`StubPaneView`] (nenhum nó nativo: o caso só lê classes, estilo e a
//!   dimensão do conteúdo).
//! - **`TestPaneCompositeLayoutService`** é o [`PaneTestLayoutService`] com as `Cell` que o caso reatribui (`panelPosition`,
//!   `floatingPanelsEnabled` e `modernUICompact`) e as partes visíveis do `visibleParts`; **`TestViewDescriptorService`** é o
//!   [`TestViewDescriptorService`] com `canMoveViews() { return false }`.
//! - **`TestPanelPart extends PanelPart`**: o `getFloatingBorderWidth` sobrescrito é o [`TestPanelBehavior`], que repassa tudo ao
//!   [`PanelBehavior`] e troca só a largura da borda (entra por [`PanelPart::extend`]); `floatingContentDimension` é
//!   `content_dimension` com o `assertReturnsDefined`.
//! - **O serviço de instanciação** é um [`InstantiationService`] vazio (o `createInstance(PanelPart)` do original é o construtor
//!   com os serviços injetados) e a barra de composites é a [`StubPaneCompositeBar`] (o `createInstance(PaneCompositeBar)`).
//! - **`'calc(100% + 0px)'`** é o texto que o CSSOM devolve para `calc(100% - 0px)`; a parte escreve o texto já normalizado.
//! - **`deepStrictEqual` de objetos literais** é `assert_eq!` de estruturas com os mesmos campos.

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

use je_workbench::browser::pane_composite::{extensions, pane_composite_registry};
use std::cell::Cell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::dynamic::Dynamic;
use je_editor::services::theme_service::IThemeService;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextview::view_element::ViewElement;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_workbench::browser::parts::pane_composite_bar::PaneCompositeBarOptions;
use je_workbench::browser::parts::pane_composite_part::{AbstractPaneCompositePart, CompositeBarPosition, PaneCompositePartBehavior};
use je_workbench::browser::parts::panel::panel_part::{PanelBehavior, PanelPart, PanelPartServices};
use je_workbench::services::layout::layout_service::{Parts, Position};
use je_workbench::test::browser::workbench_test_services::TestExtensionService;
use notification_doubles::{NoCommandService, NoKeybindingService, NoMenuService};
use pane_composite_part_support::{
    PaneTestDeps, PaneTestLayoutService, StubPaneView, TestViewDescriptorService, empty_instantiation_service, pane_part_services, test_storage_service,
};
use part_doubles::TestContextMenuService;
use test_color_theme_service::TestThemeService;
use test_notification_service::TestNotificationService;

/// `class TestPanelPart extends PanelPart { floatingBorderWidth = 1; protected override getFloatingBorderWidth() {...} }`.
struct TestPanelBehavior {
    inner: Rc<PanelBehavior>,
    floating_border_width: Rc<Cell<f64>>,
}

impl PaneCompositePartBehavior for TestPanelBehavior {
    fn minimum_width(&self) -> f64 {
        self.inner.minimum_width()
    }

    fn maximum_width(&self) -> f64 {
        self.inner.maximum_width()
    }

    fn minimum_height(&self) -> f64 {
        self.inner.minimum_height()
    }

    fn maximum_height(&self) -> f64 {
        self.inner.maximum_height()
    }

    fn to_json(&self) -> Dynamic {
        self.inner.to_json()
    }

    fn update_styles(&self, part: &AbstractPaneCompositePart) {
        self.inner.update_styles(part);
    }

    fn layout(&self, part: &AbstractPaneCompositePart, width: f64, height: f64, top: f64, left: f64) {
        self.inner.layout(part, width, height, top, left);
    }

    fn get_floating_border_width(&self, _part: &AbstractPaneCompositePart) -> f64 {
        self.floating_border_width.get()
    }

    fn should_show_composite_bar(&self, part: &AbstractPaneCompositePart) -> bool {
        self.inner.should_show_composite_bar(part)
    }

    fn get_composite_bar_options(&self, part: &AbstractPaneCompositePart) -> PaneCompositeBarOptions {
        self.inner.get_composite_bar_options(part)
    }

    fn get_composite_bar_position(&self, part: &AbstractPaneCompositePart) -> CompositeBarPosition {
        self.inner.get_composite_bar_position(part)
    }
}

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

/// O `captureState` do caso.
#[derive(Debug, PartialEq)]
struct State {
    height: String,
    content_dimension: (f64, f64),
    outer_edges: (bool, bool, bool, bool),
}

#[test]
fn preserves_panel_layout_across_classic_and_dpr_2_5_floating_modes() {
    // Os registros de `Registry.as<PaneCompositeRegistry>(...)` nascem na importação do módulo no original, antes do
    // rastreio de vazamentos do caso.
    for id in [extensions::VIEWLETS, extensions::PANELS, extensions::AUXILIARY] {
        pane_composite_registry(id);
    }
    let guard = ensure_no_disposables_are_leaked_in_test_suite();

    let root = ViewElement::with_classes(&["monaco-workbench"]);
    let container = ViewElement::with_classes(&["part", "panel"]);
    container.set_parent(Some(root.clone()));
    let layout_service = PaneTestLayoutService::new(&[Parts::EditorPart, Parts::PanelPart, Parts::StatusbarPart]);
    let deps = deps(&guard);
    let floating_border_width = Rc::new(Cell::new(1.0));

    let services = PanelPartServices {
        pane: pane_part_services(&deps, layout_service.clone(), StubPaneView::new(), empty_instantiation_service()),
        command_service: deps.command_service.clone(),
    };
    let border_width = floating_border_width.clone();
    let part = guard.add_rc(PanelPart::extend(services, move |inner| Rc::new(TestPanelBehavior { inner, floating_border_width: border_width })));
    part.create(&container);
    let device_pixel_ratio = 2.5_f64;
    floating_border_width.set(device_pixel_ratio.floor() / device_pixel_ratio);

    let capture_state = || {
        let content_dimension = part.content_dimension().expect("Assertion Failed: argument is undefined or null");
        State {
            height: container.style_get_property("height").unwrap_or_default(),
            content_dimension: (content_dimension.width, content_dimension.height),
            outer_edges: (
                container.class_list_contains("floating-part-outer-left"),
                container.class_list_contains("floating-part-outer-right"),
                container.class_list_contains("floating-part-outer-top"),
                container.class_list_contains("floating-part-outer-bottom"),
            ),
        }
    };

    layout_service.panel_position.set(Position::Top);
    part.layout(300.0, 200.0, 0.0, 0.0);
    let classic_top = capture_state();

    layout_service.panel_position.set(Position::Right);
    part.layout(300.0, 200.0, 0.0, 0.0);
    let classic_right = capture_state();

    layout_service.panel_position.set(Position::Bottom);
    part.layout(300.0, 200.0, 0.0, 0.0);
    let classic_bottom = capture_state();

    layout_service.floating_panels_enabled.set(true);
    part.layout(300.0, 200.0, 0.0, 0.0);
    let floating_default = capture_state();

    layout_service.modern_ui_compact.set(true);
    part.layout(300.0, 200.0, 0.0, 0.0);
    let floating_compact = capture_state();

    layout_service.floating_panels_enabled.set(false);
    part.layout(300.0, 200.0, 0.0, 0.0);
    let restored_classic = capture_state();

    assert_eq!(
        [classic_top, classic_right, classic_bottom, floating_default, floating_compact, restored_classic],
        [
            State { height: String::new(), content_dimension: (300.0, 199.0), outer_edges: (false, false, false, false) },
            State { height: String::new(), content_dimension: (299.0, 200.0), outer_edges: (false, false, false, false) },
            State { height: String::new(), content_dimension: (300.0, 200.0), outer_edges: (false, false, false, false) },
            State { height: "calc(100% - 8px)".to_owned(), content_dimension: (290.4, 190.4), outer_edges: (true, true, false, true) },
            State { height: "calc(100% + 0px)".to_owned(), content_dimension: (298.4, 198.4), outer_edges: (true, true, false, true) },
            State { height: String::new(), content_dimension: (300.0, 200.0), outer_edges: (false, false, false, false) },
        ]
    );
}
