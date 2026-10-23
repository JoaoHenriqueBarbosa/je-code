//! Porte de `src/vs/base/test/node/id.test.ts` (suíte `ID`, uma `flakySuite`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - `flakySuite` repete a suíte em caso de falha intermitente; aqui roda uma vez (nenhum caso depende de
//!   tempo ou de rede, só do estado da máquina);
//! - as funções `async` devolvem uma `Promise` já resolvida, então o `await` é ler o resultado;
//! - `getMac` lê `/sys/class/net`: só o Linux tem interfaces (ver `node/network_interfaces.rs`), e uma
//!   máquina sem interface de rede reprova `getMac` e cai no UUID em `getMachineId`, como no original;
//! - a regex `/^([0-9A-Fa-f]{2}[:-]){5}([0-9A-Fa-f]{2})$/` é conferida à mão.

mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors::Error;
use je_base::node::id::{get_dev_device_id, get_machine_id, get_sqm_machine_id};
use je_base::node::mac_address::get_mac;
use je_base::promise::Promise;

/// O resultado da `Promise` já resolvida e os erros que o `errorLogger` recebeu.
fn run(get: impl FnOnce(&dyn Fn(&Error)) -> Promise<String>) -> (String, Vec<Error>) {
    let errors = Rc::new(RefCell::new(Vec::new()));
    let logger = {
        let errors = errors.clone();
        move |err: &Error| errors.borrow_mut().push(err.clone())
    };
    let id = get(&logger).result().expect("a promessa está resolvida").expect("sem erro");
    let logged = errors.borrow().clone();
    (id, logged)
}

/// `/^([0-9A-Fa-f]{2}[:-]){5}([0-9A-Fa-f]{2})$/`.
fn is_mac_address(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 17
        && bytes.iter().enumerate().all(|(i, b)| if i % 3 == 2 { matches!(b, b':' | b'-') } else { b.is_ascii_hexdigit() })
}

#[test]
fn get_machine_id_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let (id, errors) = run(get_machine_id);
    assert!(!id.is_empty());
    assert_eq!(errors.len(), 0);
}

#[test]
fn get_sqm_id() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let (_id, errors) = run(get_sqm_machine_id);
    assert_eq!(errors.len(), 0);
}

#[test]
fn get_dev_device_id_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let (_id, errors) = run(get_dev_device_id);
    assert_eq!(errors.len(), 0);
}

#[test]
fn get_mac_case() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    let mac_address = get_mac();
    assert!(is_mac_address(&mac_address), "Expected a MAC address, got: {mac_address}");
}
