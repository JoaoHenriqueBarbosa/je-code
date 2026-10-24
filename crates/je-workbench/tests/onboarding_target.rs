//! Porte de `src/vs/workbench/contrib/onboarding/test/browser/onboardingTarget.test.ts` (suíte `Onboarding target
//! providers`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** É o [`LeakGuard`] criado no começo de cada caso; `disposables.add`
//!   vira `guard.add`.
//! - **DOM.** Não há DOM no porte: `$('button')` com `position: fixed; width: 100px; height: 30px` e
//!   `mainWindow.document.body.appendChild` viram [`OnboardingElement::new`], `set_size(100, 30)` e `append_to_body`;
//!   `mainWindow` é uma [`OnboardingWindow`] por caso. `element.style.display = 'none'` é `set_display_none`,
//!   `element.remove()` é `remove`, `element.focus()` e `document.activeElement` são `focus` e `active_element`.
//!   `hasAttribute` é `has_attribute`. `a === b` entre elementos é a igualdade por identidade do [`OnboardingElement`].
//! - **`await target?.open?.()`.** O `open` devolve `Option<Promise<()>>`; [`open_target`] roda e, se vier promessa,
//!   esvazia as microtarefas e confere que resolveu.
//! - **Identidade de `Event`.** O `Event` não tem identidade comparável (`===`), então
//!   `target?.onDidSelect === selected.event` vira a conferência do comportamento: o evento do alvo entrega o disparo do
//!   emissor original (e `assert.ok(target?.onDidActivate)` é o `Option` ser `Some`).
//! - **`upcastPartial<Window>({ document: ...createHTMLDocument() })`.** É uma segunda [`OnboardingWindow`], com outro
//!   documento.
//! - **`assert.throws(..., /already registered/)`.** É `errors::catch` com a conferência da mensagem; o `Event` e o
//!   alvo não têm `Debug`, então os `deepStrictEqual` de objetos de booleanos viram tuplas de booleanos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::errors;
use je_base::event::Emitter;
use je_base::lifecycle::to_disposable;
use je_base::promise::Promise;
use je_base::runtime;
use je_workbench::contrib::onboarding::onboarding_target::{
    ONBOARDING_TARGET_ATTR, OnboardingElement, OnboardingFocusTarget, OnboardingTarget, OnboardingTargetOptions,
    OnboardingTargetScope, OnboardingWindow, mark_onboarding_target, register_onboarding_target_provider,
    resolve_onboarding_target,
};

fn create_target(window: &OnboardingWindow, guard: &LeakGuard) -> OnboardingElement {
    let target = OnboardingElement::new(window);
    target.set_size(100.0, 30.0);
    target.append_to_body();
    let attached = target.clone();
    guard.add(to_disposable(move || attached.remove()));
    target
}

/// `await target?.open?.()`.
fn open_target(target: Option<&OnboardingTarget>) {
    if let Some(promise) = target.and_then(|target| target.open.as_ref()).and_then(|open| open()) {
        runtime::run_microtasks();
        assert!(matches!(promise.result(), Some(Ok(_))), "o open resolveu");
    }
}

fn counting_open(opened: &Rc<Cell<u32>>) -> Rc<dyn Fn() -> Option<Promise<()>>> {
    let opened = opened.clone();
    Rc::new(move || {
        opened.set(opened.get() + 1);
        None
    })
}

#[derive(Clone, Copy)]
enum Owner {
    Registration,
    Provider,
}

#[test]
fn resolves_an_owner_provided_control_without_marking_or_opening_it() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = create_target(&window, &guard);
    let opened = Rc::new(Cell::new(0));
    let scopes: Rc<RefCell<Vec<Option<String>>>> = Rc::default();
    guard.add(register_onboarding_target_provider("test.control", {
        let (element, opened, scopes) = (element.clone(), opened.clone(), scopes.clone());
        move |scope| {
            scopes.borrow_mut().push(scope.map(str::to_string));
            let mut target = OnboardingTarget::new(element.clone());
            target.open = Some(counting_open(&opened));
            Some(target)
        }
    }));
    let target = resolve_onboarding_target(&window, "test.control", Some("prepared"));
    let opened_before = opened.get();
    open_target(target.as_ref());

    assert_eq!(
        (
            target.as_ref().map(|target| target.element == element),
            element.has_attribute(ONBOARDING_TARGET_ATTR),
            scopes.borrow().clone(),
            opened_before,
            opened.get(),
        ),
        (
            Some(true),
            false,
            vec![Some("prepared".to_string()), Some("prepared".to_string())],
            0,
            1,
        )
    );
}

#[test]
fn a_provider_that_cannot_resolve_the_requested_scope_does_not_fall_back_to_a_marked_control() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = create_target(&window, &guard);
    guard.add(mark_onboarding_target(
        &element,
        "test.control",
        OnboardingTargetOptions { scope: Some(OnboardingTargetScope::Fixed("stale".to_string())), ..Default::default() },
    ));
    guard.add(register_onboarding_target_provider("test.control", |_| None));

    assert!(resolve_onboarding_target(&window, "test.control", Some("stale")).is_none());
}

fn exposes_a_separate_keyboard_focus_target(owner: Owner) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = create_target(&window, &guard);
    let focus_element = create_target(&window, &guard);
    let focus_target = OnboardingFocusTarget {
        element: focus_element.clone(),
        focus: {
            let focus_element = focus_element.clone();
            Rc::new(move || focus_element.focus())
        },
    };
    match owner {
        Owner::Registration => {
            guard.add(mark_onboarding_target(
                &element,
                "test.focus",
                OnboardingTargetOptions { focus_target: Some(focus_target.clone()), ..Default::default() },
            ));
        }
        Owner::Provider => {
            let (element, focus_target) = (element.clone(), focus_target.clone());
            guard.add(register_onboarding_target_provider("test.focus", move |_| {
                let mut target = OnboardingTarget::new(element.clone());
                target.focus_target = Some(focus_target.clone());
                Some(target)
            }));
        }
    }
    let target = resolve_onboarding_target(&window, "test.focus", None);
    if let Some(focus) = target.as_ref().and_then(|target| target.focus_target.as_ref()) {
        (focus.focus)();
    }

    assert_eq!(
        (
            target.as_ref().map(|target| target.element == element),
            target.as_ref().map(|target| target.focus_target.as_ref() == Some(&focus_target)),
            window.document().active_element() == Some(focus_element),
        ),
        (Some(true), Some(true), true)
    );
}

fn exposes_the_activation_event(owner: Owner) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = create_target(&window, &guard);
    let activated = guard.add_rc(Emitter::<()>::new());
    match owner {
        Owner::Registration => {
            guard.add(mark_onboarding_target(
                &element,
                "test.activation",
                OnboardingTargetOptions { on_did_activate: Some(activated.event()), ..Default::default() },
            ));
        }
        Owner::Provider => {
            let (element, event) = (element.clone(), activated.event());
            guard.add(register_onboarding_target_provider("test.activation", move |_| {
                let mut target = OnboardingTarget::new(element.clone());
                target.on_did_activate = Some(event.clone());
                Some(target)
            }));
        }
    }
    let target = resolve_onboarding_target(&window, "test.activation", None);
    let on_did_activate = target.and_then(|target| target.on_did_activate).expect("o alvo expõe o evento de ativação");
    let activations = Rc::new(Cell::new(0));
    guard.add(on_did_activate.listen({
        let activations = activations.clone();
        move |_| activations.set(activations.get() + 1)
    }));
    activated.fire(());
    assert_eq!(activations.get(), 1);
}

#[test]
fn exposes_a_separate_keyboard_focus_target_from_the_registration() {
    exposes_a_separate_keyboard_focus_target(Owner::Registration);
}

#[test]
fn exposes_the_activation_event_from_the_registration() {
    exposes_the_activation_event(Owner::Registration);
}

#[test]
fn exposes_a_separate_keyboard_focus_target_from_the_provider() {
    exposes_a_separate_keyboard_focus_target(Owner::Provider);
}

#[test]
fn exposes_the_activation_event_from_the_provider() {
    exposes_the_activation_event(Owner::Provider);
}

#[test]
fn preserves_provider_selection_and_activation_events_together() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = create_target(&window, &guard);
    let selected = guard.add_rc(Emitter::<Promise<bool>>::new());
    let activated = guard.add_rc(Emitter::<()>::new());
    guard.add(register_onboarding_target_provider("test.control", {
        let (element, select_event, activate_event) = (element.clone(), selected.event(), activated.event());
        move |_| {
            let mut target = OnboardingTarget::new(element.clone());
            target.on_did_select = Some(select_event.clone());
            target.on_did_activate = Some(activate_event.clone());
            Some(target)
        }
    }));
    let target = resolve_onboarding_target(&window, "test.control", None).expect("o alvo resolve");

    let selections = Rc::new(Cell::new(0));
    let activations = Rc::new(Cell::new(0));
    guard.add(target.on_did_select.as_ref().expect("expõe a seleção").listen({
        let selections = selections.clone();
        move |_| selections.set(selections.get() + 1)
    }));
    guard.add(target.on_did_activate.as_ref().expect("expõe a ativação").listen({
        let activations = activations.clone();
        move |_| activations.set(activations.get() + 1)
    }));
    selected.fire(Promise::resolved(true));
    activated.fire(());

    assert_eq!((selections.get(), activations.get()), (1, 1));
}

#[test]
fn ignores_hidden_detached_and_other_window_controls() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = create_target(&window, &guard);
    guard.add(register_onboarding_target_provider("test.control", {
        let element = element.clone();
        move |_| Some(OnboardingTarget::new(element.clone()))
    }));
    element.set_display_none(true);
    let hidden = resolve_onboarding_target(&window, "test.control", None);
    element.set_display_none(false);
    let other_window = OnboardingWindow::new();
    let wrong_window = resolve_onboarding_target(&other_window, "test.control", None);
    element.remove();
    let detached = resolve_onboarding_target(&window, "test.control", None);

    assert_eq!((hidden.is_none(), wrong_window.is_none(), detached.is_none()), (true, true, true));
}

#[test]
fn restores_marked_target_behavior_when_the_provider_is_disposed() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = create_target(&window, &guard);
    let opened = Rc::new(Cell::new(0));
    guard.add(mark_onboarding_target(
        &element,
        "test.control",
        OnboardingTargetOptions { open: Some(counting_open(&opened)), ..Default::default() },
    ));
    let provider = guard.add(register_onboarding_target_provider("test.control", |_| None));
    provider.dispose();
    let target = resolve_onboarding_target(&window, "test.control", None);
    open_target(target.as_ref());

    assert_eq!((target.as_ref().map(|target| target.element == element), opened.get()), (Some(true), 1));
}

#[test]
fn does_not_invoke_a_resolved_control_after_the_same_provider_callback_is_re_registered() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = create_target(&window, &guard);
    let opened = Rc::new(Cell::new(0));
    let resolve: Rc<dyn Fn(Option<&str>) -> Option<OnboardingTarget>> = Rc::new({
        let (element, opened) = (element.clone(), opened.clone());
        move |_| {
            let mut target = OnboardingTarget::new(element.clone());
            target.open = Some(counting_open(&opened));
            Some(target)
        }
    });
    let provider = guard.add(register_onboarding_target_provider("test.control", {
        let resolve = resolve.clone();
        move |scope| resolve(scope)
    }));
    let target = resolve_onboarding_target(&window, "test.control", None);
    provider.dispose();
    guard.add(register_onboarding_target_provider("test.control", {
        let resolve = resolve.clone();
        move |scope| resolve(scope)
    }));
    open_target(target.as_ref());

    assert_eq!(opened.get(), 0);
}

#[test]
fn does_not_open_a_control_that_its_owner_has_replaced() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = Rc::new(RefCell::new(create_target(&window, &guard)));
    let opened = Rc::new(Cell::new(0));
    guard.add(register_onboarding_target_provider("test.control", {
        let (element, opened) = (element.clone(), opened.clone());
        move |_| {
            let mut target = OnboardingTarget::new(element.borrow().clone());
            target.open = Some(counting_open(&opened));
            Some(target)
        }
    }));
    let old_target = resolve_onboarding_target(&window, "test.control", None);
    *element.borrow_mut() = create_target(&window, &guard);
    open_target(old_target.as_ref());
    let opened_before = opened.get();
    open_target(resolve_onboarding_target(&window, "test.control", None).as_ref());

    assert_eq!((opened_before, opened.get()), (0, 1));
}

#[test]
fn rejects_duplicate_providers_without_replacing_the_owner() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let window = OnboardingWindow::new();
    let element = create_target(&window, &guard);
    guard.add(register_onboarding_target_provider("test.control", {
        let element = element.clone();
        move |_| Some(OnboardingTarget::new(element.clone()))
    }));
    let error = match errors::catch(|| register_onboarding_target_provider("test.control", |_| None)) {
        Ok(_) => panic!("o provedor duplicado deveria lançar"),
        Err(error) => error,
    };
    assert!(error.message.contains("already registered"), "mensagem: {}", error.message);
    assert_eq!(resolve_onboarding_target(&window, "test.control", None).map(|target| target.element), Some(element));
}
