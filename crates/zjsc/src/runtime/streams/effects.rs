//! A arena de promessas e a drenagem de efeitos que `writable_js` e `transform_js` dividem.
//!
//! O `Core` de cada stream ([`super::writable`], [`super::transform`]) é uma máquina de estados sem motor: as
//! promessas viram ids e o que o C++ faz no mundo JS sai como `Effect`, em ordem. [`drain`] é a ÚNICA função que
//! aplica esses efeitos. Os quatro efeitos de promessa (criar, resolver, rejeitar, marcar tratada) são os mesmos nos
//! dois núcleos e rodam aqui; o resto do que cada `Effect` carrega (`SinkWrite`, `CallTransform`...) fica com o
//! estado, em [`EffectState::apply_other`]. Nenhum empréstimo atravessa uma chamada a JS: os efeitos saem do estado
//! num bloco curto e só então rodam.

use std::cell::RefCell;
use std::rc::Rc;

use super::readable::{mark_handled, new_promise, reject, resolve};
use super::trace::Traced;
use crate::runtime::host_call::Thrown;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;

/// O id de uma promessa do `Core` (o índice na arena).
pub(super) type P = u32;

/// As promessas JS dos ids do `Core` (o id é o índice).
pub(super) struct Promises(Vec<JSValue>);

impl Traced for Promises {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.0.trace(out);
    }
}

impl Promises {
    pub(super) fn new() -> Self {
        Promises(Vec::new())
    }

    fn get(&self, id: P) -> JSValue {
        self.0[id as usize]
    }

    fn push(&mut self, id: P, promise: JSValue) {
        debug_assert_eq!(self.0.len(), id as usize);
        self.0.push(promise);
    }
}

/// O que um efeito de promessa faz com a promessa do id.
pub(super) enum PromiseOp {
    New,
    Resolve,
    Reject(JSValue),
    MarkHandled,
}

/// Um stream cujo `Core` deixa efeitos para drenar.
pub(super) trait EffectState: Sized + 'static {
    type Effect;

    fn promises(&mut self) -> &mut Promises;

    /// Retira os efeitos acumulados (vazio se o `Core` está emprestado a uma operação em curso).
    fn take_effects(&mut self) -> Vec<Self::Effect>;

    /// `Ok` quando o efeito é de promessa, `Err` devolve o efeito para [`EffectState::apply_other`].
    fn classify(effect: Self::Effect) -> Result<(P, PromiseOp), Self::Effect>;

    fn apply_other(global_object: &JSGlobalObject, state: &Rc<RefCell<Self>>, effect: Self::Effect) -> Result<(), Thrown>;
}

/// A promessa JS do id.
pub(super) fn promise_of<S: EffectState>(state: &Rc<RefCell<S>>, id: P) -> JSValue {
    state.borrow_mut().promises().get(id)
}

fn apply_promise<S: EffectState>(global_object: &JSGlobalObject, state: &Rc<RefCell<S>>, id: P, op: PromiseOp) {
    match op {
        PromiseOp::New => {
            let promise = new_promise(global_object);
            state.borrow_mut().promises().push(id, promise);
        }
        PromiseOp::Resolve => resolve(global_object, promise_of(state, id), JSValue::undefined()),
        PromiseOp::Reject(error) => reject(global_object, promise_of(state, id), error),
        PromiseOp::MarkHandled => mark_handled(promise_of(state, id)),
    }
}

/// Aplica, em ordem, os efeitos que o `Core` deixou. É a única função que faz isso.
pub(super) fn drain<S: EffectState>(global_object: &JSGlobalObject, state: &Rc<RefCell<S>>) -> Result<(), Thrown> {
    loop {
        let effects = state.borrow_mut().take_effects();
        if effects.is_empty() {
            return Ok(());
        }
        for effect in effects {
            match S::classify(effect) {
                Ok((id, op)) => apply_promise(global_object, state, id, op),
                Err(other) => S::apply_other(global_object, state, other)?,
            }
        }
    }
}
