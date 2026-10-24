//! Os globais estáticos privados que o bun acrescenta à `SymbolTable` do global principal
//! (`Zig::GlobalObject::finishCreation`, ZigGlobalObject.cpp:2851-2883): `addStaticGlobals` com 23 entradas
//! depois de `initStaticGlobals` (`@lazy`, 18 funções privadas, `ArrayBuffer`, `internalModuleRegistry`,
//! `processBindingConstants`, `requireMap`).
//!
//! Por que importa: as 26 entradas fazem a tabela crescer até 64 buckets, e a ordem de
//! `Object.getOwnPropertyNames(globalThis)` (`Infinity, undefined, NaN`) vem da ordem de bucket do
//! `KeyHashMap` da `SymbolTable`. Os nomes são símbolos privados, então nenhum aparece ao JavaScript.
//!
//! DIVERGÊNCIA: as 18 funções privadas e o `@lazy` só são chamados pelos builtins JS do bun (os `.ts` de
//! `src/js/builtins`), que o porte não tem. Aqui são funções nativas com o `name` e o `length` do bun cujo
//! corpo lança `TypeError`, e `internalModuleRegistry`, `processBindingConstants` e `requireMap` são um
//! objeto vazio, um objeto vazio e um `Map` vazio. Nada disso é observável pelo programa.

use crate::interpreter::call_frame::NativeCallFrame;
use crate::runtime::array_buffer::ArrayBufferSharingMode;
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_function::{call_host_function_as_constructor, JSFunction};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_global_object_static_globals::GlobalPropertyInfo;
use crate::runtime::js_map::JSMap;
use crate::runtime::js_value::{EncodedJSValue, JSValue};
use crate::runtime::node_error::throw_native_type_error;
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::wtf::text::string_impl::StringImpl;
use crate::wtf::text::symbol_impl::PrivateSymbolImpl;
use crate::wtf::text::wtf_string::String as WtfString;

/// `privateFunctions[]` de ZigGlobalObject.cpp:2853: nome do `BuiltinName` e `length`.
const PRIVATE_FUNCTIONS: [(&str, u32); 18] = [
    ("makeGetterTypeError", 2),
    ("makeDOMException", 2),
    ("addAbortAlgorithmToSignal", 2),
    ("removeAbortAlgorithmFromSignal", 2),
    ("isAbortSignal", 1),
    ("peekPromiseStatus", 1),
    ("peekPromiseSettledValue", 1),
    ("pokePromiseAsHandled", 1),
    ("webStreamClosedPromise", 1),
    ("webStreamControllerError", 2),
    ("esmNamespaceForCjs", 1),
    ("esmRegistryDelete", 1),
    ("esmRegistryEvaluatedKeys", 0),
    ("esmLoadSync", 1),
    ("makeErrorWithCode", 2),
    ("toClass", 1),
    ("inherits", 1),
    ("makeAbortError", 1),
];

/// O corpo das funções privadas: os builtins JS do bun que as chamam não existem no porte.
fn host_private_function(global_object: &JSGlobalObject, _call_frame: &mut NativeCallFrame<'_>) -> EncodedJSValue {
    let _pending: Thrown = throw_native_type_error(global_object, "Builtin do host indisponível");
    JSValue::undefined().encode()
}

/// `builtinNames.xxxPrivateName()`: um `PrivateSymbolImpl` novo com a descrição do nome.
fn private_name(description: &str) -> Identifier {
    let symbol = PrivateSymbolImpl::create(&StringImpl::create(description.as_bytes()));
    Identifier::from_uid_symbol(&symbol)
}

impl JSGlobalObject {
    /// O `addStaticGlobals(staticGlobals)` de ZigGlobalObject.cpp:2883, na mesma ordem e com os mesmos atributos.
    pub fn init_host_static_globals(&self) {
        let vm = self.vm();
        let native = |name: &str, length: u32| {
            JSFunction::create_native(
                vm,
                self,
                length,
                &WtfString::from_latin1(name.as_bytes()),
                host_private_function,
                ImplementationVisibility::Public,
                Intrinsic::NoIntrinsic,
                call_host_function_as_constructor,
            )
            .as_value()
        };
        let lazy_attributes = READ_ONLY | DONT_ENUM | DONT_DELETE;
        let attributes = DONT_DELETE | READ_ONLY;

        let mut entries: Vec<(Identifier, JSValue, u32)> = Vec::with_capacity(23);
        entries.push((private_name("lazy"), native("@lazy", 0), lazy_attributes));
        for (name, length) in PRIVATE_FUNCTIONS {
            // `JSFunction::create(vm, this, length, String(), ...)`: o nome é a string vazia.
            entries.push((private_name(name), native("", length), attributes));
        }
        // `vm.propertyNames->builtinNames().ArrayBufferPrivateName()`, o do JavaScriptCore.
        entries.push((vm.property_names.builtin_names().array_buffer_private_name(),self.array_buffer_realm.array_buffer_constructor(ArrayBufferSharingMode::Default).as_value(), attributes));
        entries.push((private_name("internalModuleRegistry"), construct_empty_object(self).as_value(), attributes));
        entries.push((private_name("processBindingConstants"), construct_empty_object(self).as_value(), attributes));
        entries.push((private_name("requireMap"), JSMap::create(vm, &self.map_structure()).as_value(), attributes));

        let globals: Vec<GlobalPropertyInfo<'_>> = entries
            .iter()
            .map(|(identifier, value, attributes)| GlobalPropertyInfo { identifier, value: *value, attributes: *attributes })
            .collect();
        self.add_static_globals(&globals);
    }
}
