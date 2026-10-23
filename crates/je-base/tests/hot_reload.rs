//! Testes próprios de `hot_reload.rs` e `hot_reload_helpers.rs`, que o VS Code não testa. Não é porte de teste
//! do VS Code. O estado é por thread, e cada teste roda no seu.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::run_with_faked_timers;
use je_base::dynamic::Dynamic;
use je_base::hot_reload::{AcceptNewExportsHandler, Exports, HotReloadArgs, HotReloadConfig, apply_new_exports, enable_hot_reload, is_hot_reload_enabled, register_hot_reload_handler};
use je_base::hot_reload_helpers::create_hot_class;
use je_base::lifecycle::IDisposable;

fn args(old: &[(&str, f64)]) -> HotReloadArgs {
    HotReloadArgs { old_exports: old.iter().map(|(k, v)| ((*k).to_owned(), Dynamic::Number(*v))).collect(), new_src: String::new(), config: HotReloadConfig::default() }
}

#[test]
fn nothing_is_registered_while_hot_reload_is_disabled() {
    assert!(!is_hot_reload_enabled());
    let called = Rc::new(Cell::new(false));
    let flag = called.clone();
    let disposable = register_hot_reload_handler(Rc::new(move |_| {
        flag.set(true);
        None
    }));
    assert!(apply_new_exports(args(&[])).is_none());
    assert!(!called.get());
    disposable.dispose();
}

#[test]
fn handlers_combine_and_unregister() {
    enable_hot_reload();
    let accepted = Rc::new(Cell::new(0));
    type Handler = Rc<dyn Fn(&HotReloadArgs) -> Option<AcceptNewExportsHandler>>;
    let handler = |only: &'static str, accepted: Rc<Cell<i32>>| -> Handler {
        Rc::new(move |a: &HotReloadArgs| {
            if !a.old_exports.contains_key(only) {
                return None;
            }
            let accepted = accepted.clone();
            Some(Rc::new(move |new_exports: &mut Exports| {
                accepted.set(accepted.get() + 1);
                new_exports.insert(only.to_owned(), Dynamic::Bool(true));
                true
            }))
        })
    };
    let a = register_hot_reload_handler(handler("a", accepted.clone()));
    let _b = register_hot_reload_handler(handler("b", accepted.clone()));

    assert!(apply_new_exports(args(&[("c", 1.0)])).is_none());
    let apply = apply_new_exports(args(&[("a", 1.0), ("b", 2.0)])).expect("dois tratadores sabem tratar");
    let mut new_exports = Exports::new();
    assert!(apply(&mut new_exports));
    assert_eq!(accepted.get(), 2);
    assert_eq!(new_exports.len(), 2);

    a.dispose();
    let apply = apply_new_exports(args(&[("a", 1.0), ("b", 2.0)])).expect("o b continua registrado");
    assert!(apply(&mut Exports::new()));
    assert_eq!(accepted.get(), 3);
}

#[test]
fn hot_class_is_constant_when_disabled() {
    let class = create_hot_class(5i32, "Widget");
    assert_eq!(class.get(), 5);
}

#[test]
fn hot_class_observable_is_shared_and_updated_by_name_when_enabled() {
    enable_hot_reload();
    run_with_faked_timers(async {
        let first = create_hot_class(1i32, "Widget");
        assert_eq!(first.get(), 1);
        let second = create_hot_class(2i32, "Widget");
        // O novo valor entra no próximo tick, como no original.
        assert_eq!(second.get(), 1);
        je_base::runtime::advance_fake_time(1);
        assert_eq!(first.get(), 2);
        assert_eq!(second.get(), 2);
    });
}
