//! Tradução de `runtime/JSAsyncFunctionGenerator.{h,cpp}`: campos internos e a célula
//! `JSAsyncFunctionGenerator` (`CellEntry::AsyncFunctionGenerator`), um `JSInternalFieldObjectImpl<5>`.

use crate::runtime::js_internal_field_object_impl::define_internal_field_cell;
use crate::runtime::js_value::{js_number_i32, js_undefined, JSValue};

/// `JSInternalFieldObjectImpl<5>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 5;

pub use crate::runtime::js_generator::{Argument, ResumeMode, State};

/// `enum class Field : uint32_t`.
#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Next = 1,
    This = 2,
    Frame = 3,
    Context = 4,
}

/// `JSAsyncFunctionGenerator::initialValues()`.
fn initial_values() -> [JSValue; NUMBER_OF_INTERNAL_FIELDS as usize] {
    [js_number_i32(State::Init as i32), js_undefined(), js_undefined(), js_undefined(), js_undefined()]
}

define_internal_field_cell!(
    JSAsyncFunctionGenerator,
    JSAsyncFunctionGeneratorRef,
    AsyncFunctionGenerator,
    JSAsyncFunctionGeneratorType,
    JS_ASYNC_FUNCTION_GENERATOR_S_INFO,
    "AsyncFunctionGenerator",
    NUMBER_OF_INTERNAL_FIELDS as usize,
    initial_values()
);

impl JSAsyncFunctionGenerator {
    /// `state()`.
    pub fn state(&self) -> i32 {
        self.internal_field_as_int32(Field::State as u32)
    }

    /// `setState(state)`.
    pub fn set_state(&self, state: i32) {
        self.set_internal_field(Field::State as u32, js_number_i32(state));
    }

    /// `next()`.
    pub fn next(&self) -> JSValue {
        self.internal_field(Field::Next as u32)
    }

    /// `thisValue()`.
    pub fn this_value(&self) -> JSValue {
        self.internal_field(Field::This as u32)
    }

    /// `frame()`.
    pub fn frame(&self) -> JSValue {
        self.internal_field(Field::Frame as u32)
    }

    /// `context()`.
    pub fn context(&self) -> JSValue {
        self.internal_field(Field::Context as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::js_generator;

    #[test]
    fn values() {
        assert_eq!(Field::Context as u32 + 1, NUMBER_OF_INTERNAL_FIELDS);
        assert_eq!(Field::State as u32, js_generator::Field::State as u32);
        assert_eq!(Field::Next as u32, js_generator::Field::Next as u32);
        assert_eq!(Field::This as u32, js_generator::Field::This as u32);
        assert_eq!(Field::Frame as u32, js_generator::Field::Frame as u32);
    }
}
