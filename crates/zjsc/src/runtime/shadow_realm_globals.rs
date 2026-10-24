//! A parte de `JSGlobalObject::init(VM&)` que cria o `ShadowRealm` (JSGlobalObject.cpp:1359, 1360, 1510, 1511,
//! 1594 e 1873 a 1874) e os sete `LinkTimeConstant` de que os builtins de `ShadowRealmPrototype.js` dependem:
//! `importInRealm`, `evalInRealm`, `moveFunctionToRealm`, `createRemoteFunction` e `isRemoteFunction` (funções
//! nativas, JSGlobalObject.cpp:2141 a 2152 e 2211 a 2216) e
//! `wrapRemoteValue` e `crossRealmThrow` (as funções `@linkTimeConstant` do próprio `ShadowRealmPrototype.js`).
//!
//! Chamar `install_shadow_realm(global_object)` em `init`, depois de criados o `ObjectPrototype`, o
//! `FunctionPrototype` e o `Promise` (o `evaluate` e o `importValue` leem `@then` e `@toString` em execução,
//! não na instalação).
//!
//! DIVERGÊNCIAS:
//! - O `m_shadowRealmPrototype`, o `m_shadowRealmObjectStructure` e o `m_shadowRealmConstructor` não são
//!   guardados no global (ver `shadow_realm_constructor.rs`): quem precisa os deriva do `prototype` do construtor.
//! - Os dois `LinkTimeConstant` de função remota são criados na instalação, não com `initLater`.

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::runtime::builtins_source::BuiltinCodeIndex;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, create_builtin_function, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_remote_function::{create_remote_function, is_remote_function};
use crate::runtime::options::Options;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::shadow_realm_constructor::ShadowRealmConstructor;
use crate::runtime::shadow_realm_prototype::{eval_in_realm, import_in_realm, move_function_to_realm, ShadowRealmPrototype};
use crate::wtf::text::wtf_string::String as WtfString;

/// Cria `ShadowRealm.prototype`, o construtor e os `LinkTimeConstant`; a propriedade global `ShadowRealm`
/// só existe com `Options::useShadowRealm()` (desligada por padrão, `useShadowRealm = false`).
pub fn install_shadow_realm(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let object_prototype = global_object.object_prototype();
    let function_prototype = global_object.function_prototype();

    let prototype_structure = ShadowRealmPrototype::create_structure(vm, global_object, object_prototype.as_value());
    let prototype = ShadowRealmPrototype::create(vm, global_object, &prototype_structure);
    prototype.did_become_prototype(vm);

    let constructor_structure = ShadowRealmConstructor::create_structure(vm, global_object, function_prototype.as_value());
    let constructor = ShadowRealmConstructor::create(vm, global_object, constructor_structure, &prototype);
    prototype.put_direct_without_transition(vm, &PropertyName::from_identifier(&vm.property_names.constructor), constructor.as_value(), DONT_ENUM);

    if Options::use_shadow_realm() {
        global_object.put_direct_without_transition(vm, &PropertyName::from_identifier(&vm.property_names.shadow_realm), constructor.as_value(), DONT_ENUM);
    }

    let native = |name: &[u8], function| {
        JSFunction::create_native(
            vm,
            global_object,
            0,
            &WtfString::from_latin1(name),
            function,
            ImplementationVisibility::Private,
            Intrinsic::NoIntrinsic,
            call_host_function_as_constructor,
        )
    };
    global_object.set_link_time_constant(LinkTimeConstant::ImportInRealm, native(b"importInRealm", import_in_realm).as_value());
    global_object.set_link_time_constant(LinkTimeConstant::EvalInRealm, native(b"evalInRealm", eval_in_realm).as_value());
    global_object.set_link_time_constant(
        LinkTimeConstant::MoveFunctionToRealm,
        native(b"moveFunctionToRealm", move_function_to_realm).as_value(),
    );
    global_object.set_link_time_constant(
        LinkTimeConstant::CreateRemoteFunction,
        native(b"createRemoteFunction", create_remote_function).as_value(),
    );
    global_object.set_link_time_constant(LinkTimeConstant::IsRemoteFunction, native(b"isRemoteFunction", is_remote_function).as_value());

    let builtin_constants = [
        (LinkTimeConstant::WrapRemoteValue, BuiltinCodeIndex::ShadowRealmPrototypeWrapRemoteValueCode),
        (LinkTimeConstant::CrossRealmThrow, BuiltinCodeIndex::ShadowRealmPrototypeCrossRealmThrowCode),
    ];
    for (constant, index) in builtin_constants {
        global_object.set_link_time_constant(constant, create_builtin_function(vm, global_object, index).as_value());
    }
}
