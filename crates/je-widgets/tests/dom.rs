//! Porte do caso da suíte `ModifierKeyEmitter` de `src/vs/base/test/browser/dom.test.ts`. Os outros casos da suíte `dom`
//! estão em `crates/je-base/tests/dom.rs`; este mora aqui porque a produção equivalente (`ModifierKeyEmitter`) é do `je-widgets`.
//!
//! Notas de tradução:
//! - `mainWindow.dispatchEvent(new KeyboardEvent('keydown', { key: 'Control', ctrlKey: true }))` é despachar um evento de
//!   teclado na raiz da árvore do `Ui` (a "janela"), com a tecla Ctrl e o modificador ligado.
//! - `status.event?.type` é o `kind` do evento exposto no status (`keydown` e `keyup`).

mod support;

use std::cell::RefCell;
use std::rc::Rc;

use je_base::key_codes::KeyCode;
use je_base::lifecycle::IDisposable;
use je_ui::{Event, EventKind, KeyData, Modifiers, Ui};
use je_widgets::modifier_key_emitter::ModifierKeyEmitter;
use support::ensure_no_disposables_are_leaked_in_test_suite;

#[test]
fn exposes_the_keyboard_event_only_while_notifying_listeners() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ui = Ui::new();
    let root = ui.root();
    let emitter = ModifierKeyEmitter::get_instance(&ui);
    let received: Rc<RefCell<Vec<Option<EventKind>>>> = Rc::new(RefCell::new(Vec::new()));
    let sink = received.clone();
    let listener = emitter.event().listen(move |status| sink.borrow_mut().push(status.event.as_ref().map(|event| event.kind)));

    let control = Modifiers { ctrl: true, ..Modifiers::default() };
    ui.dispatch(&mut Event::new(EventKind::KeyDown, root).with_key(KeyData { key_code: KeyCode::Ctrl, modifiers: control }));
    ui.dispatch(&mut Event::new(EventKind::KeyUp, root).with_key(KeyData { key_code: KeyCode::Ctrl, modifiers: Modifiers::default() }));
    listener.dispose();

    let actual = (received.borrow().clone(), emitter.key_status().event.is_none());

    assert_eq!(actual, (vec![Some(EventKind::KeyDown), Some(EventKind::KeyUp)], true));
}
