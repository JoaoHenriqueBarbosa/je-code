//! Porte de `src/vs/workbench/contrib/terminal/test/browser/xterm/promptTypeDetectionCapability.test.ts` (suíte
//! `PromptTypeDetectionCapability`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()`.** Cada caso cria o [`LeakGuard`] antes de tudo e registra nele a
//!   capacidade e a inscrição, como o `store.add` do original.
//! - **`capability.type`.** É `capability_type()`.
//! - **`promptType` e `lastEventValue`.** São `Option<String>`: o `undefined` do original é `None`. O evento entrega
//!   `Option<String>`, como o `string | undefined` do `Emitter` do original.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_platform::terminal::capabilities::capabilities::{IPromptTypeDetectionCapability, TerminalCapability};
use je_platform::terminal::capabilities::prompt_type_detection_capability::PromptTypeDetectionCapability;

#[test]
fn should_have_correct_capability_type() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capability = store.add_rc(PromptTypeDetectionCapability::new());
    assert_eq!(capability.capability_type(), TerminalCapability::PromptTypeDetection);
}

#[test]
fn should_initialize_with_undefined_prompt_type() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capability = store.add_rc(PromptTypeDetectionCapability::new());
    assert_eq!(capability.prompt_type(), None);
}

#[test]
fn should_set_and_get_prompt_type() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capability = store.add_rc(PromptTypeDetectionCapability::new());

    capability.set_prompt_type("p10k");
    assert_eq!(capability.prompt_type().as_deref(), Some("p10k"));

    capability.set_prompt_type("posh-git");
    assert_eq!(capability.prompt_type().as_deref(), Some("posh-git"));
}

#[test]
fn should_fire_event_when_prompt_type_changes() {
    let store = ensure_no_disposables_are_leaked_in_test_suite();
    let capability = store.add_rc(PromptTypeDetectionCapability::new());
    let event_fired_count = Rc::new(Cell::new(0));
    let last_event_value: Rc<RefCell<Option<String>>> = Rc::default();

    let (count, last) = (event_fired_count.clone(), last_event_value.clone());
    let disposable = capability.on_prompt_type_changed().listen(move |value| {
        count.set(count.get() + 1);
        *last.borrow_mut() = value.clone();
    });
    store.add(disposable);

    capability.set_prompt_type("starship");
    assert_eq!(event_fired_count.get(), 1);
    assert_eq!(last_event_value.borrow().as_deref(), Some("starship"));

    capability.set_prompt_type("oh-my-zsh");
    assert_eq!(event_fired_count.get(), 2);
    assert_eq!(last_event_value.borrow().as_deref(), Some("oh-my-zsh"));
}
