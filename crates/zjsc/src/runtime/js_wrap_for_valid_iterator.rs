//! Porte de `runtime/JSWrapForValidIterator.{h,cpp}` e `JSWrapForValidIteratorInlines.h`: a célula do
//! `%WrapForValidIteratorPrototype%` de `Iterator.from` (`JSInternalFieldObjectImpl<2>`: o iterador e o
//! método `next` dele) e `wrapForValidIteratorPrivateFuncCreate` (o `LinkTimeConstant`
//! `wrapForValidIteratorCreate`). O protótipo está em `wrap_for_valid_iterator_prototype.rs`.

use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction, JSFunctionRef};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_internal_field_object_impl::define_internal_field_cell;
use crate::runtime::js_value::{js_null, JSValue};
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// `JSWrapForValidIteratorNumberOfInternalFields`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    IteratedIterator = 0,
    IteratedNextMethod = 1,
}

define_internal_field_cell!(
    JSWrapForValidIterator,
    JSWrapForValidIteratorRef,
    WrapForValidIterator,
    JSWrapForValidIteratorType,
    JS_WRAP_FOR_VALID_ITERATOR_S_INFO,
    "Iterator",
    NUMBER_OF_INTERNAL_FIELDS as usize,
    [js_null(), js_null()]
);

impl JSWrapForValidIterator {
    /// `create(vm, structure, iterator, nextMethod)`.
    pub fn create_with_fields(vm: &VM, structure: &StructureRef, iterator: JSValue, next_method: JSValue) -> JSWrapForValidIteratorRef {
        let wrapper = JSWrapForValidIterator::create(vm, structure);
        wrapper.set_iterated_iterator(iterator);
        wrapper.set_iterated_next_method(next_method);
        wrapper
    }

    /// `iteratedIterator()`.
    pub fn iterated_iterator(&self) -> JSValue {
        self.internal_field(Field::IteratedIterator as u32)
    }

    /// `iteratedNextMethod()`.
    pub fn iterated_next_method(&self) -> JSValue {
        self.internal_field(Field::IteratedNextMethod as u32)
    }

    /// `setIteratedIterator(vm, iterator)`.
    pub fn set_iterated_iterator(&self, iterator: JSValue) {
        self.set_internal_field(Field::IteratedIterator as u32, iterator);
    }

    /// `setIteratedNextMethod(vm, nextMethod)`.
    pub fn set_iterated_next_method(&self, next_method: JSValue) {
        self.set_internal_field(Field::IteratedNextMethod as u32, next_method);
    }
}

/// `wrapForValidIteratorPrivateFuncCreate`: `JSWrapForValidIterator::create(vm,
/// globalObject->wrapForValidIteratorStructure(), callFrame->uncheckedArgument(0),
/// callFrame->uncheckedArgument(1))`.
fn wrap_for_valid_iterator_private_create(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let wrapper = JSWrapForValidIterator::create_with_fields(
        global_object.vm(),
        &global_object.wrap_for_valid_iterator_structure(),
        call.argument(0),
        call.argument(1),
    );
    Ok(wrapper.as_value())
}
crate::host_function!(wrap_for_valid_iterator_private_func_create, wrap_for_valid_iterator_private_create);

/// A `JSFunction` de `m_linkTimeConstants[LinkTimeConstant::wrapForValidIteratorCreate]`
/// (`JSFunction::create(vm, owner, 2, "wrapForValidIteratorCreate", wrapForValidIteratorPrivateFuncCreate,
/// Private, WrapForValidIteratorCreateIntrinsic)`).
pub fn create_wrap_for_valid_iterator_create_function(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        2,
        &WtfString::from_latin1(b"wrapForValidIteratorCreate"),
        wrap_for_valid_iterator_private_func_create,
        ImplementationVisibility::Private,
        Intrinsic::WrapForValidIteratorCreateIntrinsic,
        call_host_function_as_constructor,
    )
}
