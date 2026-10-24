//! Tradução de `runtime/JSGenerator.{h,cpp}`: constantes, enums e a célula `JSGenerator`
//! (`CellEntry::Generator`), um `JSInternalFieldObjectImpl<4>`.

use crate::runtime::js_internal_field_object_impl::define_internal_field_cell;
use crate::runtime::js_value::{js_number_i32, js_undefined, JSValue};

/// `JSInternalFieldObjectImpl<4>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 4;

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResumeMode {
    NormalMode = 0,
    ReturnMode = 1,
    ThrowMode = 2,
}

#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Completed = -1,
    Executing = -2,
    Init = 0,
}

/// `[this], @generator, @generatorState, @generatorValue, @generatorResumeMode, @generatorFrame.`
#[repr(i32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Argument {
    ThisValue = 0,
    Generator = 1,
    State = 2,
    Value = 3,
    ResumeMode = 4,
    Frame = 5,
}

impl Argument {
    /// `NumberOfArguments = Frame`.
    pub const NUMBER_OF_ARGUMENTS: usize = Argument::Frame as usize;
}

#[repr(u32)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    State = 0,
    Next = 1,
    This = 2,
    Frame = 3,
}

/// `JSGenerator::initialValues()`.
fn initial_values() -> [JSValue; NUMBER_OF_INTERNAL_FIELDS as usize] {
    [js_number_i32(State::Init as i32), js_undefined(), js_undefined(), js_undefined()]
}

define_internal_field_cell!(
    JSGenerator,
    JSGeneratorRef,
    Generator,
    JSGeneratorType,
    JS_GENERATOR_S_INFO,
    "Generator",
    NUMBER_OF_INTERNAL_FIELDS as usize,
    initial_values()
);

impl JSGenerator {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::cell_registry::{self, CellEntry};
    use crate::runtime::js_type::JSType;
    use crate::runtime::vm::VM;

    #[test]
    fn cell_starts_in_init_and_registers() {
        let vm = VM::new();
        let structure = JSGenerator::create_structure(&vm, None, crate::runtime::js_value::js_null());
        let generator = JSGenerator::create(&vm, &structure);
        assert_eq!(generator.state(), State::Init as i32);
        assert_eq!(generator.next(), js_undefined());
        generator.set_state(State::Executing as i32);
        assert_eq!(generator.state(), -2);
        let id = generator.as_value();
        assert!(JSGenerator::from_value(&id).is_some());
        assert_eq!(cell_registry::cell_type(generator.cell_id()), Some(JSType::JSGeneratorType));
        assert!(matches!(cell_registry::get(generator.cell_id()), Some(CellEntry::Generator(_))));
    }

    #[test]
    fn values() {
        assert_eq!(Argument::NUMBER_OF_ARGUMENTS, 5);
        assert_eq!(Field::Frame as u32 + 1, NUMBER_OF_INTERNAL_FIELDS);
        assert_eq!(State::Executing as i32, -2);
    }
}
