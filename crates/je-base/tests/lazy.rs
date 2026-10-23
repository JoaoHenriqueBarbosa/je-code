//! Porte de `src/vs/base/test/common/lazy.test.ts` (suíte `Lazy`), caso a caso e na mesma ordem.

use std::cell::Cell;
use std::rc::Rc;

use je_base::errors::{self, Error};
use je_base::lazy::Lazy;

#[test]
fn lazy_values_should_only_be_resolved_once() {
    let counter = Rc::new(Cell::new(0));
    let c = counter.clone();
    let value = Lazy::new(move || {
        c.set(c.get() + 1);
        c.get()
    });

    assert!(!value.has_value());
    assert_eq!(value.value(), 1);
    assert!(value.has_value());
    assert_eq!(value.value(), 1); // make sure we did not evaluate again
}

#[test]
fn lazy_values_handle_error_case() {
    let counter = Rc::new(Cell::new(0));
    let c = counter.clone();
    let value: Lazy<i32> = Lazy::new(move || {
        c.set(c.get() + 1);
        errors::throw(Error::new(c.get().to_string()))
    });

    assert!(!value.has_value());
    let error = errors::catch(|| value.value()).expect_err("deveria lançar");
    assert_eq!(error.message, "1");
    assert!(value.has_value());
    let error = errors::catch(|| value.value()).expect_err("deveria lançar");
    assert_eq!(error.message, "1");
}

#[test]
fn should_throw_when_accessing_lazy_value_in_initializer() {
    let value: Rc<Lazy<String>> = Rc::new_cyclic(|weak: &std::rc::Weak<Lazy<String>>| {
        let weak = weak.clone();
        Lazy::new(move || weak.upgrade().expect("lazy vivo").value())
    });

    let error = errors::catch(|| value.value()).expect_err("deveria lançar");
    assert!(error.message.contains("Cannot read the value of a lazy that is being initialized"));
}
