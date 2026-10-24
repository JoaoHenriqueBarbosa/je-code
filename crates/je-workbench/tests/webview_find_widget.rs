//! Porte de `src/vs/workbench/contrib/webview/test/browser/webviewFindWidget.test.ts` (suíte `WebviewFindWidget`), caso a caso e
//! na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite`.** É o [`LeakGuard`] criado no começo do caso; o emissor e o widget vão para
//!   o `store` do guarda (`store.add_rc`).
//! - **Sem DOM.** O `container` `.monaco-workbench` e o `toDisposable(() => container.remove())` não têm efeito no modelo (não
//!   há documento). `input` e `nextButton` são os [`FocusTarget`] `Input` e `NextButton` de `webview_find_widget`;
//!   `input.value = ...` é `set_input_value`, e os `dispatchEvent` de `CompositionEvent` e `InputEvent` são
//!   `dispatch_composition_start`, `dispatch_composition_end` e `dispatch_input`.
//! - **`workbenchInstantiationService` + `createInstance(WebviewFindWidget, delegate)`.** Os serviços (`IContextViewService`,
//!   `IHoverService`, `IKeybindingService` e os demais) só servem à base visual, fora do recorte; o widget recebe só o delegate.
//! - **`await timeout(0)`.** É `run_timers`, a entrega dos `setTimeout` de `show`.
//! - **`sinon.spy(input, 'focus')` e `sinon.spy(nextButton, 'focus')`.** Os espiões são o histórico de chamadas de `focus()` do
//!   [`FocusModel`]: `focus_call_count`, `reset_focus_history` (`resetHistory`) e `focus_called_after` (`calledAfter`). Criar o
//!   espião depois do `input.focus()` inicial é `reset_focus_history`. `sinon.restore()` no `teardown` não tem efeito aqui.
//! - **`assert.deepStrictEqual({ ... }, { ... })`.** É um `assert_eq!` sobre as duas estruturas.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::event::{Emitter, Event};
use je_workbench::contrib::webview::webview_find_widget::{FocusTarget, WebviewFindDelegate, WebviewFindWidget};

struct TestDelegate {
    has_find_result: Rc<Emitter<bool>>,
    searches: Rc<RefCell<Vec<String>>>,
}

impl WebviewFindDelegate for TestDelegate {
    fn has_find_result(&self) -> Event<bool> {
        self.has_find_result.event()
    }

    fn on_did_stop_find(&self) -> Event<()> {
        Event::none()
    }

    fn check_ime_completion_state(&self) -> bool {
        true
    }

    fn find(&self, _value: &str, _previous: bool) {}

    fn update_find(&self, value: &str) {
        self.searches.borrow_mut().push(value.to_string());
    }

    fn stop_find(&self, _keep_selection: Option<bool>) {}

    fn focus(&self) {}
}

struct Fixture {
    widget: Rc<WebviewFindWidget>,
    has_find_result: Rc<Emitter<bool>>,
    searches: Rc<RefCell<Vec<String>>>,
}

impl Fixture {
    fn searches(&self) -> Vec<String> {
        self.searches.borrow().clone()
    }

    fn input_focus_calls(&self) -> usize {
        self.widget.focus_call_count(FocusTarget::Input)
    }

    fn next_button_focus_calls(&self) -> usize {
        self.widget.focus_call_count(FocusTarget::NextButton)
    }

    fn input_has_focus(&self) -> bool {
        self.widget.has_focus(FocusTarget::Input)
    }
}

/// `setup`.
fn setup(store: &LeakGuard) -> Fixture {
    let searches = Rc::new(RefCell::new(Vec::new()));
    let has_find_result = store.add_rc(Emitter::<bool>::new());
    let delegate = Rc::new(TestDelegate { has_find_result: has_find_result.clone(), searches: searches.clone() });

    let widget = store.add_rc(WebviewFindWidget::new(delegate));
    widget.show();
    widget.run_timers();

    widget.focus_input();
    assert!(widget.has_focus(FocusTarget::Input));

    // O `sinon.spy` nasce depois do foco inicial: as contagens partem de zero.
    widget.reset_focus_history();

    Fixture { widget, has_find_result, searches }
}

#[derive(Debug, PartialEq)]
struct CompositionResult {
    searches_during_composition: Vec<String>,
    searches: Vec<String>,
    input_focus_calls: usize,
    next_button_focus_calls: usize,
    input_has_focus: bool,
}

#[test]
fn compositionend_updates_search_without_moving_focus() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);

    f.widget.dispatch_composition_start();
    f.widget.set_input_value("ㅇ");
    f.widget.dispatch_input();
    let searches_during_composition = f.searches();

    // O Chromium entrega o texto confirmado antes de `compositionend`, ainda compondo.
    f.widget.set_input_value("안");
    f.widget.dispatch_input();
    f.widget.dispatch_composition_end();

    assert_eq!(
        CompositionResult {
            searches_during_composition,
            searches: f.searches(),
            input_focus_calls: f.input_focus_calls(),
            next_button_focus_calls: f.next_button_focus_calls(),
            input_has_focus: f.input_has_focus(),
        },
        CompositionResult {
            searches_during_composition: vec![],
            searches: vec!["안".to_string()],
            input_focus_calls: 0,
            next_button_focus_calls: 0,
            input_has_focus: true,
        }
    );
}

#[derive(Debug, PartialEq)]
struct ResultAfterComposition {
    searches: Vec<String>,
    next_button_enabled: bool,
    input_focus_calls: usize,
    next_button_focus_calls: usize,
    input_has_focus: bool,
}

#[test]
fn find_results_do_not_move_focus_after_a_new_composition_starts() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);

    f.widget.dispatch_composition_start();
    f.widget.set_input_value("안");
    f.widget.dispatch_composition_end();

    f.widget.dispatch_composition_start();
    f.widget.set_input_value("안ㄴ");
    f.widget.dispatch_input();
    f.widget.reset_focus_history();

    // Entrega o resultado da busca anterior depois de a próxima sílaba começar a compor.
    f.has_find_result.fire(true);

    assert_eq!(
        ResultAfterComposition {
            searches: f.searches(),
            next_button_enabled: f.widget.next_button_aria_disabled() == "false",
            input_focus_calls: f.input_focus_calls(),
            next_button_focus_calls: f.next_button_focus_calls(),
            input_has_focus: f.input_has_focus(),
        },
        ResultAfterComposition {
            searches: vec!["안".to_string()],
            next_button_enabled: true,
            input_focus_calls: 0,
            next_button_focus_calls: 0,
            input_has_focus: true,
        }
    );
}

#[derive(Debug, PartialEq)]
struct NormalInputResult {
    searches: Vec<String>,
    input_focus_calls: usize,
    next_button_focus_calls: usize,
    input_focused_after_next_button: bool,
    input_has_focus: bool,
}

#[test]
fn normal_input_still_updates_search_and_restores_focus() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let f = setup(&store);

    f.widget.set_input_value("hello");
    f.widget.dispatch_input();

    assert_eq!(
        NormalInputResult {
            searches: f.searches(),
            input_focus_calls: f.input_focus_calls(),
            next_button_focus_calls: f.next_button_focus_calls(),
            input_focused_after_next_button: f.widget.focus_called_after(FocusTarget::Input, FocusTarget::NextButton),
            input_has_focus: f.input_has_focus(),
        },
        NormalInputResult {
            searches: vec!["hello".to_string()],
            input_focus_calls: 1,
            next_button_focus_calls: 1,
            input_focused_after_next_button: true,
            input_has_focus: true,
        }
    );
}
