//! Testes próprios de `ime.rs` e `ports.rs`, que o VS Code não testa. Não é porte de teste do VS Code.

use std::cell::Cell;
use std::rc::Rc;

use je_base::ime::IMEImpl;
use je_base::lifecycle::IDisposable;
use je_base::ports::random_port;

#[test]
fn ime_fires_on_every_toggle() {
    let ime = IMEImpl::new();
    let count = Rc::new(Cell::new(0));
    let counter = count.clone();
    let listener = ime.on_did_change().listen(move |()| counter.set(counter.get() + 1));
    assert!(ime.enabled());
    ime.disable();
    assert!(!ime.enabled());
    ime.enable();
    assert!(ime.enabled());
    assert_eq!(count.get(), 2);
    listener.dispose();
}

#[test]
fn random_port_stays_in_range() {
    for _ in 0..10_000 {
        let port = random_port();
        assert!((1025..65535).contains(&port), "{port}");
    }
}
