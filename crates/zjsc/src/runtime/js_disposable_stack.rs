//! Porte de `runtime/JSDisposableStack.{h,cpp}` e `JSDisposableStackInlines.h`: a célula do
//! `DisposableStack` (`JSInternalFieldObjectImpl<2>`: o estado e o array de recursos da capability).

use crate::runtime::js_internal_field_object_impl::define_internal_field_cell;
use crate::runtime::js_value::{js_null, js_number_i32};

/// `JSDisposableStackNumberOfInternalFields`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Pending = 0,
    Disposed = 1,
}

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Capability = 1,
}

define_internal_field_cell!(
    JSDisposableStack,
    JSDisposableStackRef,
    DisposableStack,
    DisposableStackType,
    JS_DISPOSABLE_STACK_S_INFO,
    "DisposableStack",
    NUMBER_OF_INTERNAL_FIELDS as usize,
    [js_number_i32(State::Pending as i32), js_null()]
);

impl JSDisposableStack {
    /// `disposed()`: o campo de estado é `Disposed`.
    pub fn disposed(&self) -> bool {
        self.internal_field_as_int32(Field::State as u32) == State::Disposed as i32
    }
}
