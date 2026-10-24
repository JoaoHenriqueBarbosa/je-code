//! A parte de `JSGlobalObject::init(VM&)` que cria os `LazyClassStructure` `m_disposableStackStructure` e
//! `m_asyncDisposableStackStructure` (JSGlobalObject.cpp) e as publica: as propriedades globais
//! `DisposableStack` e `AsyncDisposableStack` (`DontEnum`, só com `Options::useExplicitResourceManagement()`)
//! e os `LinkTimeConstant::DisposableStack` e `LinkTimeConstant::AsyncDisposableStack` (o construtor, que o
//! builtin `move` lê como `@DisposableStack`).
//!
//! DIVERGÊNCIA: o `LazyClassStructure` cria protótipo, estrutura e construtor na primeira leitura; aqui é
//! eager, como em `weak_ref_globals.rs`. O global não guarda as estruturas: os construtores derivam a
//! estrutura do `prototype` próprio (ver `collection_support::derived_structure`). A condição da opção fica
//! com o chamador.

use crate::bytecode::bytecode_intrinsics_table::LinkTimeConstant;
use crate::runtime::async_disposable_stack_constructor::AsyncDisposableStackConstructor;
use crate::runtime::async_disposable_stack_prototype::AsyncDisposableStackPrototype;
use crate::runtime::disposable_stack_constructor::DisposableStackConstructor;
use crate::runtime::disposable_stack_prototype::DisposableStackPrototype;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;

/// Cria `DisposableStack` e `AsyncDisposableStack` (protótipo com `didBecomePrototype`, construtor, o
/// `constructor` do protótipo como `DontEnum`, a propriedade global `DontEnum` e o `LinkTimeConstant`).
pub fn install_disposable_stacks(global_object: &JSGlobalObject) {
    let vm = global_object.vm();
    let object_prototype_value = global_object.object_prototype().as_value();
    let function_prototype_value = global_object.function_prototype().as_value();
    let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);

    // `DisposableStack`.
    let prototype_structure = DisposableStackPrototype::create_structure(vm, global_object, object_prototype_value);
    let prototype = DisposableStackPrototype::create(vm, global_object, &prototype_structure);
    prototype.did_become_prototype(vm);
    let constructor_structure = DisposableStackConstructor::create_structure(vm, global_object, function_prototype_value);
    let constructor = DisposableStackConstructor::create(vm, global_object, constructor_structure, &prototype);
    prototype.put_direct(vm, &constructor_name, constructor.as_value(), DONT_ENUM);
    global_object.put_direct(vm, &PropertyName::from_identifier(&vm.property_names.disposable_stack), constructor.as_value(), DONT_ENUM);
    global_object.set_link_time_constant(LinkTimeConstant::DisposableStack, constructor.as_value());

    // `AsyncDisposableStack`.
    let async_prototype_structure = AsyncDisposableStackPrototype::create_structure(vm, global_object, object_prototype_value);
    let async_prototype = AsyncDisposableStackPrototype::create(vm, global_object, &async_prototype_structure);
    async_prototype.did_become_prototype(vm);
    let async_constructor_structure = AsyncDisposableStackConstructor::create_structure(vm, global_object, function_prototype_value);
    let async_constructor = AsyncDisposableStackConstructor::create(vm, global_object, async_constructor_structure, &async_prototype);
    async_prototype.put_direct(vm, &constructor_name, async_constructor.as_value(), DONT_ENUM);
    global_object.put_direct(
        vm,
        &PropertyName::from_identifier(&vm.property_names.async_disposable_stack),
        async_constructor.as_value(),
        DONT_ENUM,
    );
    global_object.set_link_time_constant(LinkTimeConstant::AsyncDisposableStack, async_constructor.as_value());
}
