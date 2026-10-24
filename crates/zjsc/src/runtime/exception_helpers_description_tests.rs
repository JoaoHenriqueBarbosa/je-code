//! Testes de `errorDescriptionForValue` (`exception_helpers.rs`): o que a mensagem de
//! "x is not a function" e parentes escreve no lugar do valor.

use crate::runtime::current_realm::CurrentRealmScope;
use crate::runtime::exception_helpers::error_description_for_value;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_big_int_ops::make_big_int_from_i64;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_boolean, js_null, js_undefined, JSValue};
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

fn global() -> JSGlobalObjectRef {
    let vm = std::rc::Rc::new(VM::new());
    let structure = JSGlobalObject::create_structure(&vm, js_null());
    JSGlobalObject::create(&vm, structure, js_null())
}

crate::host_function!(noop_host_function, noop_body);
fn noop_body(_global_object: &JSGlobalObject, _call: &crate::runtime::host_call::HostCall) -> crate::runtime::host_call::HostResult {
    Ok(JSValue::undefined())
}

fn text(description: WtfString) -> Vec<u8> {
    description.latin1()
}

#[test]
fn primitives_are_described_by_their_string_form() {
    let global = global();
    let _scope = CurrentRealmScope::enter(&global);
    let vm = global.vm();
    assert_eq!(text(error_description_for_value(js_undefined())), b"undefined");
    assert_eq!(text(error_description_for_value(js_null())), b"null");
    assert_eq!(text(error_description_for_value(js_boolean(true))), b"true");
    assert_eq!(text(error_description_for_value(JSValue::Int32(-4))), b"-4");
    assert_eq!(text(error_description_for_value(JSValue::Double(0.5))), b"0.5");
    // A string vem entre aspas.
    let string = JSValue::from_js_string(js_string(vm, &WtfString::from_latin1(b"abc")));
    assert_eq!(text(error_description_for_value(string)), b"\"abc\"");
}

#[test]
fn big_int_is_described_by_its_decimal_string() {
    let global = global();
    let _scope = CurrentRealmScope::enter(&global);
    assert_eq!(text(error_description_for_value(make_big_int_from_i64(12))), b"12");
    assert_eq!(text(error_description_for_value(make_big_int_from_i64(-30))), b"-30");
}

#[test]
fn callable_is_described_as_function() {
    let global = global();
    let _scope = CurrentRealmScope::enter(&global);
    let function = JSFunction::create_native(
        global.vm(),
        &global,
        0,
        &WtfString::from_latin1(b"foo"),
        noop_host_function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    );
    // `vm.smallStrings.functionString()` é o `typeof` ("function"), não o nome da classe.
    assert_eq!(text(error_description_for_value(function.as_value())), b"function");
}
