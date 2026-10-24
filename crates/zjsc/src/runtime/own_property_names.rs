//! `JSObject::getOwnPropertyNames` (`JSObject.cpp:2753`): `getOwnIndexedPropertyNames` seguido de
//! `getOwnNonIndexPropertyNames`, com os `getOwnSpecialPropertyNames` das classes que o `JSObject` do
//! porte ainda não despacha (`JSArray`, `StringObject`).
//!
//! O `JSObject` do porte não tem a tabela de métodos virtual (`methodTable()->getOwnPropertyNames`):
//! esta função faz o papel dela para as classes que existem, e é o ponto que o despacho vai assumir.
//!
//! LACUNAS: `getNonReifiedStaticPropertyNames` existe, mas nenhum `ClassInfo` do porte tem tabela ainda
//! (plano em `wip-notes/static-prop-reify.md`), e os demais `getOwnSpecialPropertyNames` (`JSLexicalEnvironment`, `RegExpObject`,
//! `ErrorInstance`...), que entram junto com as classes.

use crate::runtime::enumeration_mode::DontEnumPropertiesMode;
use crate::runtime::js_object::PutError;
use crate::runtime::generic_arguments;
use crate::runtime::host_call::Thrown;
use crate::runtime::indexing_type::{
    ARRAY_STORAGE_SHAPE, CONTIGUOUS_SHAPE, DOUBLE_SHAPE, INDEXING_SHAPE_MASK, INT32_SHAPE, NO_INDEXING_SHAPE,
    SLOW_PUT_ARRAY_STORAGE_SHAPE, UNDECIDED_SHAPE,
};
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_type::JSType;
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::string_object::StringObject;
use crate::runtime::typed_array_type::is_typed_view;
use crate::runtime::vm::VM;

/// `getOwnPropertyNames(object, globalObject, propertyNames, mode)`.
pub fn get_own_property_names(
    vm: &VM,
    object: &JSObject,
    property_names: &mut PropertyNameArrayBuilder<'_>,
    mode: DontEnumPropertiesMode,
) -> Result<(), Thrown> {
    get_own_property_names_with_special(vm, object, property_names, mode, |_| {})
}

/// `getOwnPropertyNames` com o `getOwnSpecialPropertyNames` de uma classe que precisa do `JSGlobalObject`
/// (`JSFunction`: `length`, `name`, `prototype`), chamado onde o C++ o chama: depois dos índices e antes
/// das propriedades da estrutura.
pub fn get_own_property_names_with_special(
    vm: &VM,
    object: &JSObject,
    property_names: &mut PropertyNameArrayBuilder<'_>,
    mode: DontEnumPropertiesMode,
    special: impl FnOnce(&mut PropertyNameArrayBuilder<'_>),
) -> Result<(), Thrown> {
    // `JSModuleNamespaceObject::getOwnPropertyNames`.
    if let Some(namespace) = crate::runtime::js_module_namespace_object::exotic_of(object) {
        return namespace.get_own_property_names(vm, property_names, mode);
    }
    get_own_indexed_property_names(object, property_names, mode)?;
    special(property_names);
    // `JSSymbolTableObject::getOwnSpecialPropertyNames`: as variáveis `var` e funções do script global vivem na
    // `SymbolTable` do global, não na `Structure`.
    if object.type_() == JSType::GlobalObjectType {
        let symbol_table = crate::runtime::js_scope::JSScope::from_cell_id(object.cell_id()).and_then(|scope| scope.symbol_table());
        if let Some(symbol_table) = symbol_table {
            let keys: Vec<_> = symbol_table
                .borrow()
                .iter()
                .filter(|(_, entry)| mode == DontEnumPropertiesMode::Include || !entry.is_dont_enum())
                .map(|(key, _)| key.clone())
                .collect();
            // `PropertyNameArray` descarta o nome privado (os globais do host, `@lazy` e companhia, moram aqui): o bun
            // dá `Object.getOwnPropertySymbols(globalThis).length === 0`.
            for key in &keys {
                property_names.add_uid(key, key.0.is_private_symbol());
            }
        }
    }
    // `ClonedArguments::getOwnSpecialPropertyNames` e o ramo `Include && !overrodeThings()` de
    // `GenericArgumentsImpl::getOwnPropertyNames`.
    if mode == DontEnumPropertiesMode::Include {
        crate::runtime::js_arguments_objects::special_property_names(object, property_names).map_err(|error| match error {
            PutError::OutOfMemory => Thrown::OutOfMemory,
            _ => Thrown::Pending,
        })?;
    }
    get_own_non_index_property_names(vm, object, property_names, mode);
    crate::runtime::process_env::add_reserved_names(vm, object, property_names, mode);
    Ok(())
}

/// `getOwnIndexedPropertyNames`, incluindo os índices do `StringObject` (`StringObject::getOwnPropertyNames`).
fn get_own_indexed_property_names(
    object: &JSObject,
    property_names: &mut PropertyNameArrayBuilder<'_>,
    mode: DontEnumPropertiesMode,
) -> Result<(), Thrown> {
    if !property_names.include_string_properties() {
        return Ok(());
    }

    if matches!(object.type_(), JSType::StringObjectType | JSType::DerivedStringObjectType) {
        if let Some(string_object) = StringObject::from_cell_id(object.cell_id()) {
            for index in string_object.own_index_names() {
                property_names.add_index(index);
            }
        }
    }

    // `JSGenericTypedArrayView::getOwnPropertyNames`: os índices `0..length()`.
    if is_typed_view(object.type_()) {
        if let Some(view) = JSGenericTypedArrayView::from_cell_id(object.cell_id()) {
            for index in view.own_index_names() {
                property_names.add_index(index as u32);
            }
        }
    }

    // `GenericArgumentsImpl::getOwnPropertyNames`: os índices ainda mapeados vêm antes dos elementos.
    if let Some(arguments) = generic_arguments::exotic_of(object) {
        for index in generic_arguments::mapped_indices(&*arguments) {
            property_names.add_index(index);
        }
    }

    // Add numeric properties first per step 2 of https://tc39.es/ecma262/#sec-ordinaryownpropertykeys
    match object.cell().indexing_type() & INDEXING_SHAPE_MASK {
        NO_INDEXING_SHAPE | UNDECIDED_SHAPE => {}
        INT32_SHAPE | CONTIGUOUS_SHAPE | DOUBLE_SHAPE => {
            for index in 0..object.public_length() {
                if object.can_get_index_quickly(index) {
                    property_names.add_index(index);
                }
            }
        }
        ARRAY_STORAGE_SHAPE | SLOW_PUT_ARRAY_STORAGE_SHAPE => {
            object.with_array_storage(|storage| {
                let used_vector_length = storage.length().min(storage.vector_length());
                for index in 0..used_vector_length {
                    if !storage.vector()[index as usize].is_empty() {
                        property_names.add_index(index);
                    }
                }

                if let Some(map) = storage.sparse_map() {
                    // O mapa itera em ordem crescente de índice (o C++ ordena as chaves).
                    for entry in map.iter() {
                        if mode == DontEnumPropertiesMode::Include || entry.attributes() & DONT_ENUM == 0 {
                            property_names.add_index(entry.index());
                        }
                    }
                }
            });
        }
        shape => unreachable!("forma de indexação {shape:#x} inválida"),
    }
    Ok(())
}

/// `JSObject::getNonReifiedStaticPropertyNames` (`JSObjectInlines.h:962`): os nomes das tabelas estáticas de
/// `classInfo` e dos pais (filho primeiro, ordem do `@begin`) que a `Structure` ainda não reificou, sem
/// reificar. Entram antes dos da `Structure`; o `PropertyNameArray` deduplica. Com `Exclude`, pula a entrada
/// `DontEnum` e a que a `Structure` sombreia como `DontEnum`. Sem tabela em nenhum `ClassInfo`, não faz nada.
fn get_non_reified_static_property_names(
    vm: &VM,
    object: &JSObject,
    property_names: &mut PropertyNameArrayBuilder<'_>,
    mode: DontEnumPropertiesMode,
) {
    let structure = object.structure();
    if !object.has_non_reified_static_properties(&structure) {
        return;
    }
    let mut class_info = Some(object.class_info());
    while let Some(current) = class_info {
        if let Some(table) = current.static_prop_hash_table {
            for entry in table.iter() {
                if mode == DontEnumPropertiesMode::Exclude && entry.attributes & DONT_ENUM != 0 {
                    continue;
                }
                let identifier = crate::runtime::identifier::Identifier::from_span(vm, entry.key.as_bytes());
                if mode == DontEnumPropertiesMode::Exclude {
                    let name = crate::runtime::property_name::PropertyName::from_identifier(&identifier);
                    let (offset, attributes) = structure.get_with_attributes(vm, &name);
                    if crate::runtime::property_offset::is_valid_offset(offset) && attributes & DONT_ENUM != 0 {
                        continue;
                    }
                }
                property_names.add(&identifier);
            }
        }
        class_info = current.parent_class;
    }
}

/// `getOwnNonIndexPropertyNames`: o especial da classe, depois `Structure::getPropertyNamesFromStructure`.
pub(crate) fn get_own_non_index_property_names(
    vm: &VM,
    object: &JSObject,
    property_names: &mut PropertyNameArrayBuilder<'_>,
    mode: DontEnumPropertiesMode,
) {
    // `ErrorInstance::getOwnSpecialPropertyNames`: com `DontEnumPropertiesMode::Include` materializa a pilha.
    if mode == DontEnumPropertiesMode::Include {
        crate::runtime::error_instance::materialize_error_info(object);
    }

    // `JSArray::getOwnSpecialPropertyNames` e `StringObject::getOwnPropertyNames` acrescentam `length`.
    let has_own_length = matches!(
        object.type_(),
        JSType::ArrayType | JSType::DerivedArrayType | JSType::StringObjectType | JSType::DerivedStringObjectType
    );
    if has_own_length && mode == DontEnumPropertiesMode::Include {
        property_names.add(&vm.property_names.length);
    }

    // `lastIndex` é a primeira propriedade da `Structure` do `RegExpObject` (writable, non-enum, non-config).
    if object.type_() == JSType::RegExpObjectType && mode == DontEnumPropertiesMode::Include {
        property_names.add(&vm.property_names.last_index);
    }

    get_non_reified_static_property_names(vm, object, property_names, mode);

    // `getPropertyNamesFromStructure`: as chaves de texto na ordem de inserção, depois os símbolos
    // ("To ensure the order defined in the spec, we append symbols at the last elements of keys").
    let properties = object.structure().properties_with_privacy();
    let enumerable = |attributes: u32| mode == DontEnumPropertiesMode::Include || attributes & DONT_ENUM == 0;
    for want_symbols in [false, true] {
        for (key, _, attributes, is_private) in properties.iter().filter(|entry| entry.0 .0.is_symbol() == want_symbols) {
            if enumerable(*attributes) {
                property_names.add_uid(key, *is_private);
            }
        }
    }
}
