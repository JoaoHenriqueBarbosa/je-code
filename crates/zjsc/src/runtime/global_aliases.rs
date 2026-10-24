//! `global` e `self` do global. O JavaScriptCore não os define: quem os instala é o bun. Medidos no bun 1.4.2:
//!
//! - `global` é uma propriedade de dados comum (`writable`, `enumerable`, `configurable`) cujo valor é o próprio
//!   `globalThis`. Atribuir troca o valor, `delete` remove (depois disso `global` é `ReferenceError`). Na ordem
//!   de chaves do bun ele vem logo depois de `structuredClone`, o que a tabela `ORDER` de
//!   `js_global_object_init.rs` já resolve.
//! - `self` é um acessor `enumerable` e `configurable` (sem `DontDelete`) cujos `get` e `set` são funções
//!   nativas de `name` "get" e "set" e `length` 0 (`String(fn)` é `function get() { [native code] }`). O `get`
//!   devolve o `globalThis` seja qual for o `this`. O `set` também ignora o `this`: redefine `self` no global
//!   como propriedade de dados comum com o valor do primeiro argumento (`undefined` se não houver) e devolve
//!   esse valor. Por isso `self = 7`, em modo estrito ou não, nunca lança, e atribuir por um objeto que herda
//!   do global (`Object.create(globalThis).self = 4`) também troca o `self` do global, sem criar nada no
//!   herdeiro. A posição da chave fica a mesma depois da troca.
//!
//! Na ordem de chaves o `self` vem depois de `ShadowRealm` e antes de `onmessage`. A reordenação do global
//! (`reorder_standard_globals`) não move acessores, então `install_self` roda depois dela.

use crate::host_function;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction, JSFunctionRef};
use crate::runtime::js_getter_setter::GetterSetter;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::native_function::NativeFunction;
use crate::runtime::property_attribute::ACCESSOR;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::wtf::text::wtf_string::String as WtfString;

fn property_name(global_object: &JSGlobalObject, name: &str) -> PropertyName {
    PropertyName::from_identifier(&Identifier::from_span(global_object.vm(), name.as_bytes()))
}

/// Instala `global`: dados comuns, valor `globalThis`.
pub fn install_global(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    if let Some(global_this) = global_object.global_this() {
        global_object.put_direct(vm, &property_name(global_object, "global"), global_this.as_value(), 0);
    }
}

fn self_get_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(global_object.global_this().map_or_else(JSValue::undefined, |global_this| global_this.as_value()))
}

fn self_set_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let value = call.argument(0);
    let descriptor = PropertyDescriptor::new(value, 0);
    let _ = global_object.define_own_property(global_object.vm(), &property_name(global_object, "self"), &descriptor, false);
    Ok(value)
}

host_function!(global_func_self_get, self_get_body);
host_function!(global_func_self_set, self_set_body);

fn native_accessor_function(global_object: &JSGlobalObject, name: &str, function: NativeFunction) -> JSFunctionRef {
    JSFunction::create_native(
        global_object.vm(),
        global_object,
        0,
        &WtfString::from_latin1(name.as_bytes()),
        function,
        ImplementationVisibility::Public,
        Intrinsic::NoIntrinsic,
        call_host_function_as_constructor,
    )
}

/// Instala `self`: o acessor `get`/`set`, sem `DontEnum` nem `DontDelete`.
pub fn install_self(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let getter = native_accessor_function(global_object, "get", global_func_self_get as NativeFunction);
    let setter = native_accessor_function(global_object, "set", global_func_self_set as NativeFunction);
    let accessor = GetterSetter::create_from_values(vm, getter.as_value(), setter.as_value());
    let _ = global_object.put_direct_accessor(vm, &property_name(global_object, "self"), accessor, ACCESSOR);
}
