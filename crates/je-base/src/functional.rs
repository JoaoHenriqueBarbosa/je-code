//! Porte de `src/vs/base/common/functional.ts`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

/// `createSingleCallFunction(fn, fnDidRunCallback?)`: uma função que só chama `fn` uma vez; as chamadas
/// seguintes devolvem o resultado da primeira. `fn_did_run_callback` roda depois de `fn`, mesmo que
/// `fn` lance (o `finally` do original); nesse caso nada fica guardado, como no original, e a
/// próxima chamada não repete `fn` (`didCall` já é verdadeiro) e devolve `undefined`.
pub fn create_single_call_function<A, R: Clone + Default>(
    f: impl Fn(A) -> R + 'static,
    fn_did_run_callback: Option<Box<dyn Fn()>>,
) -> impl Fn(A) -> R {
    let did_call = Cell::new(false);
    let result: RefCell<R> = RefCell::new(R::default());
    let callback = fn_did_run_callback.map(Rc::new);

    move |args| {
        if did_call.get() {
            return result.borrow().clone();
        }

        did_call.set(true);
        if let Some(callback) = &callback {
            struct RunOnDrop<'a>(&'a dyn Fn());
            impl Drop for RunOnDrop<'_> {
                fn drop(&mut self) {
                    (self.0)();
                }
            }
            let _finally = RunOnDrop(callback.as_ref());
            *result.borrow_mut() = f(args);
        } else {
            *result.borrow_mut() = f(args);
        }

        result.borrow().clone()
    }
}
