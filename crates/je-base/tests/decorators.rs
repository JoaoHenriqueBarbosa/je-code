//! Porte de `src/vs/base/test/common/decorators.test.ts` (suíte `Decorators`), caso a caso e na mesma ordem.
//!
//! O Rust não tem decorator, então `@memoize` e `@throttle` são os tipos `Memoize` e `Throttle`.
//!
//! Pendente: `memoized_property_should_not_be_enumerable` e `memoized_property_should_not_be_writable`
//! verificam que a propriedade `$memoize$answer` que o decorator grava no objeto não aparece em
//! `Object.keys` e não aceita atribuição. Em Rust o valor guardado é um campo privado do `Memoize`, sem
//! nome de propriedade dinâmico, então os dois comportamentos valem por construção e não há o que
//! afirmar.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_base::decorators::{Memoize, Throttle};
use je_base::runtime;

struct MemoizeFoo<T: Clone> {
    count: Cell<u32>,
    answer: T,
    memo: Memoize<T>,
}

impl<T: Clone> MemoizeFoo<T> {
    fn new(answer: T) -> Self {
        Self { count: Cell::new(0), answer, memo: Memoize::new() }
    }

    // `@memoize answer()` e `@memoize get answer()`: o mesmo mecanismo.
    fn answer(&self) -> T {
        self.memo
            .get(|| {
                self.count.set(self.count.get() + 1);
                self.answer.clone()
            })
            .clone()
    }
}

#[test]
fn memoize_should_memoize_methods() {
    let foo = MemoizeFoo::new(Some(42));
    assert_eq!(foo.count.get(), 0);
    assert_eq!(foo.answer(), Some(42));
    assert_eq!(foo.count.get(), 1);
    assert_eq!(foo.answer(), Some(42));
    assert_eq!(foo.count.get(), 1);

    let foo2 = MemoizeFoo::new(Some(1337));
    assert_eq!(foo2.count.get(), 0);
    assert_eq!(foo2.answer(), Some(1337));
    assert_eq!(foo2.count.get(), 1);
    assert_eq!(foo2.answer(), Some(1337));
    assert_eq!(foo2.count.get(), 1);

    assert_eq!(foo.answer(), Some(42));
    assert_eq!(foo.count.get(), 1);

    // `null` e `undefined` também ficam guardados: ambos são `None` aqui, então o segundo `Foo`
    // com `None` cobre os dois.
    let foo3: MemoizeFoo<Option<i32>> = MemoizeFoo::new(None);
    assert_eq!(foo3.count.get(), 0);
    assert_eq!(foo3.answer(), None);
    assert_eq!(foo3.count.get(), 1);
    assert_eq!(foo3.answer(), None);
    assert_eq!(foo3.count.get(), 1);

    let foo4: MemoizeFoo<Option<i32>> = MemoizeFoo::new(None);
    assert_eq!(foo4.count.get(), 0);
    assert_eq!(foo4.answer(), None);
    assert_eq!(foo4.count.get(), 1);
    assert_eq!(foo4.answer(), None);
    assert_eq!(foo4.count.get(), 1);
}

#[test]
fn memoize_should_memoize_getters() {
    let foo = MemoizeFoo::new(Some(42));
    assert_eq!(foo.count.get(), 0);
    assert_eq!(foo.answer(), Some(42));
    assert_eq!(foo.count.get(), 1);
    assert_eq!(foo.answer(), Some(42));
    assert_eq!(foo.count.get(), 1);

    let foo2 = MemoizeFoo::new(Some(1337));
    assert_eq!(foo2.count.get(), 0);
    assert_eq!(foo2.answer(), Some(1337));
    assert_eq!(foo2.count.get(), 1);
    assert_eq!(foo2.answer(), Some(1337));
    assert_eq!(foo2.count.get(), 1);

    assert_eq!(foo.answer(), Some(42));
    assert_eq!(foo.count.get(), 1);

    let foo3: MemoizeFoo<Option<i32>> = MemoizeFoo::new(None);
    assert_eq!(foo3.count.get(), 0);
    assert_eq!(foo3.answer(), None);
    assert_eq!(foo3.count.get(), 1);
    assert_eq!(foo3.answer(), None);
    assert_eq!(foo3.count.get(), 1);

    let foo4: MemoizeFoo<Option<i32>> = MemoizeFoo::new(None);
    assert_eq!(foo4.count.get(), 0);
    assert_eq!(foo4.answer(), None);
    assert_eq!(foo4.count.get(), 1);
    assert_eq!(foo4.answer(), None);
    assert_eq!(foo4.count.get(), 1);
}

#[test]
fn throttle() {
    runtime::with_faked_timers(|| {
        let spy: Rc<RefCell<Vec<i32>>> = Rc::default();
        let s = spy.clone();
        // `@throttle(100, (a, b) => a + b, () => 0) report(p)`
        let report = Throttle::new(
            100,
            |previous: Option<i32>, p: i32| previous.unwrap_or(0) + p,
            Some(Rc::new(|| 0)),
            move |p| s.borrow_mut().push(p),
        );

        report.call(1);
        report.call(2);
        report.call(3);
        assert_eq!(*spy.borrow(), vec![1]);

        runtime::advance_fake_time(200);
        assert_eq!(*spy.borrow(), vec![1, 5]);
        spy.borrow_mut().clear();

        report.call(4);
        report.call(5);
        runtime::advance_fake_time(50);
        report.call(6);

        assert_eq!(*spy.borrow(), vec![4]);
        runtime::advance_fake_time(60);
        assert_eq!(*spy.borrow(), vec![4, 11]);
    });
}
