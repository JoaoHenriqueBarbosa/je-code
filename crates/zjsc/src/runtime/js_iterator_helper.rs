//! Porte de `runtime/JSIteratorHelper.{h,cpp}`: a célula do `Iterator Helper` (`JSInternalFieldObjectImpl<2>`:
//! o gerador e o iterador subjacente) e `iteratorHelperPrivateFuncCreate` (o `LinkTimeConstant`
//! `iteratorHelperCreate`). O protótipo está em `iterator_helper_prototype.rs`.

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

/// `JSInternalFieldObjectImpl<2>`.
pub const NUMBER_OF_INTERNAL_FIELDS: u32 = 2;

#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Generator = 0,
    UnderlyingIterator = 1,
}

define_internal_field_cell!(
    JSIteratorHelper,
    JSIteratorHelperRef,
    IteratorHelper,
    JSIteratorHelperType,
    JS_ITERATOR_HELPER_S_INFO,
    "Iterator Helper",
    NUMBER_OF_INTERNAL_FIELDS as usize,
    [js_null(), js_null()]
);

impl JSIteratorHelper {
    /// `create(vm, structure, generator, underlyingIterator)`: o gerador é objeto e o iterador subjacente é
    /// objeto ou `null`.
    pub fn create_with_fields(vm: &VM, structure: &StructureRef, generator: JSValue, underlying_iterator: JSValue) -> JSIteratorHelperRef {
        debug_assert!(generator.is_object() && (underlying_iterator.is_object() || underlying_iterator.is_null()));
        let helper = JSIteratorHelper::create(vm, structure);
        helper.set_internal_field(Field::Generator as u32, generator);
        helper.set_internal_field(Field::UnderlyingIterator as u32, underlying_iterator);
        helper
    }
}

/// `iteratorHelperPrivateFuncCreate`: `JSIteratorHelper::create(vm, globalObject->iteratorHelperStructure(),
/// callFrame->uncheckedArgument(0), callFrame->uncheckedArgument(1))`.
fn iterator_helper_private_create(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let helper = JSIteratorHelper::create_with_fields(
        global_object.vm(),
        &global_object.iterator_helper_structure(),
        call.argument(0),
        call.argument(1),
    );
    Ok(helper.as_value())
}
crate::host_function!(iterator_helper_private_func_create, iterator_helper_private_create);

/// A `JSFunction` de `m_linkTimeConstants[LinkTimeConstant::iteratorHelperCreate]` (`JSFunction::create(vm,
/// owner, 2, "iteratorHelperCreate", iteratorHelperPrivateFuncCreate, Private,
/// IteratorHelperCreateIntrinsic)`).
pub fn create_iterator_helper_create_function(vm: &VM, global_object: &JSGlobalObject) -> JSFunctionRef {
    JSFunction::create_native(
        vm,
        global_object,
        2,
        &WtfString::from_latin1(b"iteratorHelperCreate"),
        iterator_helper_private_func_create,
        ImplementationVisibility::Private,
        Intrinsic::IteratorHelperCreateIntrinsic,
        call_host_function_as_constructor,
    )
}
