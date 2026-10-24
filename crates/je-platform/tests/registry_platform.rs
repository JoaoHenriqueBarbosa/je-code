//! Porte de `src/vs/platform/registry/test/common/platform.test.ts` (suíte `Platform / Registry`), caso a caso e na
//! mesma ordem.
//!
//! Divergências: `isFunction(Registry.add)` (e `as`, `knows`) é garantido pelo tipo: o caso confere que os três métodos
//! existem com a assinatura do original. As contribuições são structs (`{ bar: true }` é [`Foo`], `{}` é [`Empty`]), e o
//! `Registry.as<any>('foo').bar` é `as_::<Foo>`. A identidade `Registry.as('knows,as') === ext` é `Rc::ptr_eq`, e o `null`
//! do id desconhecido é `None`. O `try { add(...); assert.ok(false) } catch { assert.ok(true) }` do último caso é conferir
//! que `add` com id repetido lança.

mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::errors;
use je_platform::registry::{Registry, registry};

struct Foo {
    bar: bool,
}

struct Empty;

#[test]
fn registry_api() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let _add: fn(&Registry, &str, Rc<Foo>) = Registry::add::<Foo>;
    let _as: fn(&Registry, &str) -> Option<Rc<Foo>> = Registry::as_::<Foo>;
    let _knows: fn(&Registry, &str) -> bool = Registry::knows;
}

#[test]
fn registry_mixin() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = registry();

    registry.add("foo", Rc::new(Foo { bar: true }));

    assert!(registry.knows("foo"));
    assert!(registry.as_::<Foo>("foo").expect("foo").bar);
    assert!(registry.as_::<Foo>("foo").expect("foo").bar);
}

#[test]
fn registry_knows_as() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = registry();

    let ext = Rc::new(Empty);

    registry.add("knows,as", ext.clone());

    assert!(registry.knows("knows,as"));
    assert!(!registry.knows("knows,as1234"));

    assert!(Rc::ptr_eq(&registry.as_::<Empty>("knows,as").expect("knows,as"), &ext));
    assert!(registry.as_::<Empty>("knows,as1234").is_none());
}

#[test]
fn registry_mixin_fails_on_duplicate_ids() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let registry = registry();

    registry.add("foo-dup", Rc::new(Foo { bar: true }));

    assert!(errors::catch(|| registry.add("foo-dup", Rc::new(Foo { bar: false }))).is_err());
}
