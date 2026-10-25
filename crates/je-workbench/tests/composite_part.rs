//! Porte de `src/vs/workbench/test/browser/parts/compositePart.test.ts`.
//!
//! Suíte `CompositePart`, caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`** é o [`LeakGuard`] criado no começo do caso; o `disposables.add(...)` do
//!   original é `guard.add_rc`. O `teardown` (`fixture.remove()` e `disposables.clear()`) é o `Drop` do [`Suite`] seguido do `Drop`
//!   do guarda, nessa ordem.
//! - **DOM vira estado nativo.** O `fixture` é um [`ViewElement`] solto. O que a `CompositePart` desenha com o DOM e com widgets
//!   passa pela [`ICompositePartView`]; o [`TestCompositePartView`] cria elementos sem nó nativo e a `ProgressBar` é o
//!   [`TestProgressBar`], que nada pinta. A parte do caso não tem título (`hasTitle: false`), então o rótulo e a barra de
//!   ferramentas nunca são pedidos à vista.
//! - **`TestComposite`, `TestCompositeDescriptor` e `TestCompositeRegistry`.** O `TestComposite` é um [`Composite`] com o
//!   [`TestCompositeBehavior`] (`layout` e `setBoundarySashes` vazios). O construtor com `@ITelemetryService`, `@IThemeService` e
//!   `@IStorageService` é a [`CompositeFactory`] do descritor, que captura os três serviços que o `TestInstantiationService` do
//!   original entregaria. O `TestCompositeRegistry.register` é o `register_composite`, que o porte deixa público.
//! - **`TestCompositePart`.** Os limites e o `toJSON` são o [`TestCompositePartBehavior`]; o `createTestComposite`, o
//!   `removeTestComposite` e o `hasCompositeOpenListeners` são `create_composite`, `remove_composite` e o `has_listeners` do
//!   emissor `on_did_composite_open_emitter`. `new TestNotificationService()` e `new MockKeybindingService()` são os dublês de
//!   `je-editor` e de `notification_doubles`; `{} as IContextMenuService` e o `NullHoverService` pertencem à vista.
//! - **`part.create(fixture)`** é o `create` da [`Part`](je_workbench::browser::part::Part) da parte.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;
#[path = "support/notification_doubles.rs"]
mod notification_doubles;
#[path = "../../je-editor/tests/support/test_color_theme_service.rs"]
mod test_color_theme_service;
#[path = "../../je-editor/tests/support/test_notification_service.rs"]
mod test_notification_service;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::actions::ActionRef;
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_editor::core_2d::dimension::Dimension;
use je_editor::services::theme_service::IThemeService;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::contextview::view_element::ViewElement;
use je_platform::storage::StorageService;
use je_platform::telemetry::{ITelemetryService, null_telemetry_service};
use je_widgets::action_view_items::{IActionViewItem, IActionViewItemOptions};
use je_widgets::sash::BoundarySashes;
use je_workbench::browser::composite::{Composite, CompositeBehavior, CompositeDescriptor, CompositeFactory, CompositeRegistry};
use je_workbench::browser::part::{DomPosition, PartOptions};
use je_workbench::browser::parts::composite_part::{
    CompositePart, CompositePartBehavior, CompositePartServices, CompositeToolBarArgs, ICompositePartOptions, ICompositePartView, ICompositeProgressBar, ICompositeToolBar,
    ITitleLabelView,
};
use je_workbench::services::progress::progress_indicator::IProgressBar;
use je_workbench::test::common::workbench_test_services::TestStorageService;
use notification_doubles::{NoKeybindingService, TestLayoutService};
use test_color_theme_service::TestThemeService;
use test_notification_service::TestNotificationService;

// region: dublês

/// `TestComposite.ID`.
const TEST_COMPOSITE_ID: &str = "testComposite";

/// O corpo de `TestComposite`: `layout` e `setBoundarySashes` vazios.
struct TestCompositeBehavior;

impl CompositeBehavior for TestCompositeBehavior {
    fn layout(&self, _dimension: Dimension, _position: Option<DomPosition>) {}

    fn set_boundary_sashes(&self, _sashes: &BoundarySashes) {}
}

/// `new TestCompositeDescriptor()`: `super(TestComposite as IConstructorSignature<TestComposite>, TestComposite.ID,
/// 'Test Composite')`, com os três serviços que a instanciação injetaria.
fn test_composite_descriptor(
    telemetry_service: Rc<dyn ITelemetryService>,
    theme_service: Rc<dyn IThemeService>,
    storage_service: Rc<dyn StorageService>,
) -> CompositeDescriptor<Composite> {
    let ctor: CompositeFactory<Composite> = Rc::new(move |_instantiation_service: &Rc<InstantiationService>| {
        Composite::new(TEST_COMPOSITE_ID, telemetry_service.clone(), theme_service.clone(), storage_service.clone(), None, Rc::new(TestCompositeBehavior))
    });

    CompositeDescriptor::new(ctor, TEST_COMPOSITE_ID, "Test Composite", None, None, None)
}

/// A `ProgressBar` do caso: nada é pintado.
struct TestProgressBar;

impl IProgressBar for TestProgressBar {
    fn infinite(&self) {}

    fn total(&self, _value: f64) {}

    fn has_total(&self) -> bool {
        false
    }

    fn worked(&self, _value: f64) {}

    fn stop(&self) {}

    fn show(&self, _delay: Option<u64>) {}

    fn hide(&self) {}
}

impl IDisposable for TestProgressBar {
    fn dispose(&self) {}
}

/// A vista da parte: elementos sem nó nativo e a barra de progresso vazia.
struct TestCompositePartView;

impl ICompositePartView for TestCompositePartView {
    fn create_element(&self, parent: Option<&Rc<ViewElement>>, _tag: &str, class_name: &str) -> Rc<ViewElement> {
        let element = ViewElement::new();
        element.set_class_name(class_name);
        if let Some(parent) = parent {
            element.set_parent(Some(parent.clone()));
        }
        element
    }

    fn prepend_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }

    fn append_child(&self, parent: &Rc<ViewElement>, child: &Rc<ViewElement>) {
        child.set_parent(Some(parent.clone()));
    }

    fn remove_child(&self, child: &Rc<ViewElement>) {
        child.set_parent(None);
    }

    fn create_title_label_view(&self, _title_label: &Rc<ViewElement>) -> Rc<dyn ITitleLabelView> {
        unreachable!("a parte do caso não tem título")
    }

    fn create_progress_bar(&self, _container: &Rc<ViewElement>) -> Rc<dyn ICompositeProgressBar> {
        Rc::new(TestProgressBar)
    }

    fn create_tool_bar(&self, _args: CompositeToolBarArgs) -> Rc<dyn ICompositeToolBar> {
        unreachable!("a parte do caso não tem título")
    }

    fn create_action_view_item(&self, _action: &ActionRef, _options: &IActionViewItemOptions) -> Option<Rc<dyn IActionViewItem>> {
        None
    }
}

/// O corpo de `TestCompositePart`: `minimumWidth = 0`, `maximumWidth = Number.POSITIVE_INFINITY`, `minimumHeight = 0`,
/// `maximumHeight = Number.POSITIVE_INFINITY` e `toJSON(): {}`.
struct TestCompositePartBehavior;

impl CompositePartBehavior for TestCompositePartBehavior {
    fn minimum_width(&self) -> f64 {
        0.0
    }

    fn maximum_width(&self) -> f64 {
        f64::INFINITY
    }

    fn minimum_height(&self) -> f64 {
        0.0
    }

    fn maximum_height(&self) -> f64 {
        f64::INFINITY
    }

    fn to_json(&self) -> Dynamic {
        Dynamic::object(Vec::<(&str, Dynamic)>::new())
    }
}

/// O `setup` (o `fixture` solto) e o `teardown` (`fixture.remove()`).
struct Suite {
    fixture: Rc<ViewElement>,
}

fn setup() -> Suite {
    Suite { fixture: ViewElement::new() }
}

impl Drop for Suite {
    fn drop(&mut self) {
        self.fixture.set_parent(None);
    }
}

// endregion

#[test]
fn removing_a_composite_disposes_its_progress_scope_listeners() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let suite = setup();

    let storage = guard.add_rc(TestStorageService::new());
    let storage_service: Rc<dyn StorageService> = Rc::new(storage);
    let theme_service = guard.add_rc(TestThemeService::new_default());
    let theme_service: Rc<dyn IThemeService> = theme_service;
    let instantiation_service = guard.add_rc(InstantiationService::new(ServiceCollection::new()));
    let registry = guard.add_rc(CompositeRegistry::<Composite>::new());
    registry.register_composite(test_composite_descriptor(null_telemetry_service(), theme_service.clone(), storage_service.clone()));
    let part = guard.add_rc(CompositePart::new(
        CompositePartServices {
            notification_service: Rc::new(TestNotificationService),
            storage_service,
            keybinding_service: Rc::new(NoKeybindingService),
            instantiation_service,
            theme_service,
            layout_service: Rc::new(TestLayoutService),
            view: Rc::new(TestCompositePartView),
        },
        registry,
        "test.activeComposite",
        TEST_COMPOSITE_ID,
        "test",
        "test-composite",
        None,
        None,
        "testCompositePart",
        ICompositePartOptions { part: PartOptions { has_title: false, ..PartOptions::default() }, ..ICompositePartOptions::default() },
        Rc::new(TestCompositePartBehavior),
    ));
    part.part().create(&suite.fixture, None);

    part.create_composite(TEST_COMPOSITE_ID, false);
    assert!(part.on_did_composite_open_emitter().has_listeners());

    assert!(part.remove_composite(TEST_COMPOSITE_ID));
    assert!(!part.on_did_composite_open_emitter().has_listeners());
}
