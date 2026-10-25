//! Porte de `src/vs/platform/actions/test/browser/toolbar.test.ts` (suíte `WorkbenchToolBar`), caso a caso e na mesma
//! ordem.
//!
//! Este teste vive no `je-workbench` (e não no `je-platform`) porque o `WorkbenchToolBar` estende a `ToolBar` de
//! `je-widgets`, e a plataforma não depende de widgets (ver `je_workbench::platform`).
//!
//! Divergências de tradução:
//! - `$('div')` é um nó da `Ui`. O `TestWorkbenchToolBar.runAction` (que lê o `protected actionBar`) é o `action_bar()`
//!   da `ToolBar` de dentro, que o `Deref` do `WorkbenchToolBar` expõe.
//! - `new class extends mock<...>() { }()` são os dublês de `platform_actions_doubles` (qualquer chamada não prevista é
//!   um erro, como o `mock` do original); `lookupKeybinding() { return undefined; }` é o `MockKeybindingService`, que não
//!   tem atalho nenhum.
//! - `await toolBar.runAction(action)` é esperar a promessa dentro de `run_with_faked_timers`.
//! - O `toolBar.dispose()` que a ação chama de dentro do `run` aponta para o próprio toolbar por referência fraca: o
//!   original o captura por fecho, e um `Rc` aqui seria um ciclo com a ação.

#[path = "../../je-platform/tests/common/mod.rs"]
mod common;
mod platform_actions_doubles;

use std::cell::RefCell;
use std::rc::Rc;

use common::keybinding::MockKeybindingService;
use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::actions::{IActionRunner, ToActionProps, to_action};
use je_base::dynamic::Dynamic;
use je_base::lifecycle::IDisposable;
use je_base::promise::MaybePromise;
use je_platform::keybinding::mock_keybinding_service::MockContextKeyService;
use je_render::kurbo::Rect;
use je_ui::{Role, Ui};
use je_workbench::platform::actions::{IWorkbenchToolBarOptions, WorkbenchToolBar};
use platform_actions_doubles::{NoopContextMenuService, TelemetryEvent, TestTelemetryService, UnusedCommandService, UnusedMenuService};

#[test]
fn logs_telemetry_before_an_action_disposes_the_toolbar() {
    let disposables = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let container = {
        let mut tree = ui.tree_mut();
        let root = tree.root();
        tree.set_bounds(root, Rect::new(0.0, 0.0, 800.0, 600.0));
        let container = tree.create(root, Role::Generic);
        tree.set_bounds(container, Rect::new(0.0, 0.0, 273.0, 30.0));
        container
    };

    let telemetry_service = Rc::new(TestTelemetryService::default());
    let tool_bar = disposables.add_rc(WorkbenchToolBar::new(
        &ui,
        container,
        Some(IWorkbenchToolBarOptions { telemetry_source: Some("testToolBar".to_owned()), ..IWorkbenchToolBarOptions::default() }),
        Rc::new(UnusedMenuService),
        disposables.add_rc(MockContextKeyService::new()),
        Rc::new(NoopContextMenuService),
        Rc::new(MockKeybindingService),
        Rc::new(UnusedCommandService),
        telemetry_service.clone(),
    ));

    let events_during_run: Rc<RefCell<Vec<TelemetryEvent>>> = Rc::new(RefCell::new(Vec::new()));
    let self_disposing_action = to_action(ToActionProps {
        id: "selfDisposing".to_owned(),
        label: "selfDisposing".to_owned(),
        tooltip: None,
        enabled: None,
        checked: None,
        class: None,
        alt: None,
        run: Rc::new({
            let (telemetry_service, events_during_run, tool_bar) = (telemetry_service.clone(), events_during_run.clone(), Rc::downgrade(&tool_bar));
            move |_: &[Dynamic]| {
                *events_during_run.borrow_mut() = telemetry_service.events.borrow().clone();
                if let Some(tool_bar) = tool_bar.upgrade() {
                    tool_bar.dispose();
                }
                MaybePromise::Value(Dynamic::Undefined)
            }
        }),
    });
    tool_bar.set_actions(&[self_disposing_action.clone()], &[], None);

    let run = tool_bar.action_bar().action_runner().run(&self_disposing_action, None);
    run_with_faked_timers(async move {
        run.await.expect("a ação roda sem erro");
    });

    let expected_events = vec![TelemetryEvent::workbench_action_executed("selfDisposing", "testToolBar")];
    assert_eq!(*events_during_run.borrow(), expected_events);
    assert_eq!(*telemetry_service.events.borrow(), expected_events);
}
