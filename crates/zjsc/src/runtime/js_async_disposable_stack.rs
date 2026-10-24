//! Porte de `runtime/JSAsyncDisposableStack.{h,cpp}` e `JSAsyncDisposableStackInlines.h`: a célula do
//! `AsyncDisposableStack` (`JSInternalFieldObjectImpl<2>`: o estado e o array de recursos da capability).

use crate::runtime::js_internal_field_object_impl::define_internal_field_cell;
use crate::runtime::js_value::{js_null, js_number_i32};

/// `JSAsyncDisposableStackNumberOfInternalFields`.
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
    JSAsyncDisposableStack,
    JSAsyncDisposableStackRef,
    AsyncDisposableStack,
    AsyncDisposableStackType,
    JS_ASYNC_DISPOSABLE_STACK_S_INFO,
    "AsyncDisposableStack",
    NUMBER_OF_INTERNAL_FIELDS as usize,
    [js_number_i32(State::Pending as i32), js_null()]
);

impl JSAsyncDisposableStack {
    /// `disposed()`: o campo de estado é `Disposed`.
    pub fn disposed(&self) -> bool {
        self.internal_field_as_int32(Field::State as u32) == State::Disposed as i32
    }
}
