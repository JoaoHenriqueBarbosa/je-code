//! O `visitChildren` do estado de stream que mora em Rust.
//!
//! No C++ cada objeto de stream é uma célula com os valores como membros, e o `visitChildren` de cada classe os
//! marca. Aqui o estado é `Rc<RefCell<...>>` ao lado do objeto JS, e o coletor o percorre por [`Traced`]: cada tipo de
//! estado põe em `out` os valores JS que segura (só os dele, sem seguir para outro estado: o que está registrado num
//! `OBJECTS` é visitado pelo `visit_gc_roots` do módulo dele, e os objetos JS das chaves são raízes enquanto o programa
//! existir, como as instâncias de `INSTANCES`).
//!
//! O que não mora num `OBJECTS` (as reações, os sorvedouros de `read_all`, o estado do `tee` e do `pipeTo`) é
//! declarado a quem o segura: uma reação leva a lista do que a closure dela captura (ver `readable::reaction`) e a
//! entrada some quando a função da reação morre.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;

use crate::runtime::gc::Marker;
use crate::runtime::js_value::JSValue;

thread_local! {
    /// Um `RefCell` estava emprestado para escrita no momento da coleta: ela desiste (ver [`mark_all`]).
    static CONTENDED: Cell<bool> = const { Cell::new(false) };
}

/// Os valores JS que um estado do hospedeiro segura.
pub(super) trait Traced {
    fn trace(&self, out: &mut Vec<JSValue>);
}

impl Traced for JSValue {
    fn trace(&self, out: &mut Vec<JSValue>) {
        out.push(*self);
    }
}

impl<T: Traced> Traced for Option<T> {
    fn trace(&self, out: &mut Vec<JSValue>) {
        if let Some(inner) = self {
            inner.trace(out);
        }
    }
}

impl<T: Traced> Traced for Vec<T> {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.iter().for_each(|item| item.trace(out));
    }
}

impl<T: Traced> Traced for VecDeque<T> {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.iter().for_each(|item| item.trace(out));
    }
}

impl<T: Traced, const N: usize> Traced for [T; N] {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.iter().for_each(|item| item.trace(out));
    }
}

impl<A: Traced, B: Traced> Traced for (A, B) {
    fn trace(&self, out: &mut Vec<JSValue>) {
        self.0.trace(out);
        self.1.trace(out);
    }
}

impl<T: Traced + ?Sized> Traced for Rc<T> {
    fn trace(&self, out: &mut Vec<JSValue>) {
        (**self).trace(out);
    }
}

impl<T: Traced + ?Sized> Traced for RefCell<T> {
    fn trace(&self, out: &mut Vec<JSValue>) {
        match self.try_borrow() {
            Ok(inner) => inner.trace(out),
            Err(_) => CONTENDED.with(|flag| flag.set(true)),
        }
    }
}

/// Um valor que não segura nada JS (um estado só de bytes e números).
impl Traced for () {
    fn trace(&self, _out: &mut Vec<JSValue>) {}
}

/// O que uma closure do hospedeiro captura, para `readable::reaction` e os sorvedouros.
pub(super) fn hold<T: Traced + 'static>(held: T) -> Rc<dyn Traced> {
    Rc::new(held)
}

/// `true` (e zera) se algum `RefCell` estava emprestado durante o último percurso.
pub(super) fn take_contended() -> bool {
    CONTENDED.with(Cell::take)
}

/// Marca como raiz os valores que `visit` põe em `out`; desiste da coleta se um estado estava emprestado.
pub(super) fn mark_all(marker: &mut Marker, visit: impl FnOnce(&mut Vec<JSValue>)) {
    let mut values = Vec::new();
    visit(&mut values);
    values.into_iter().for_each(|value| marker.value(value));
    if take_contended() {
        marker.abort();
    }
}
