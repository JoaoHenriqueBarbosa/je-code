//! `Bun__deepEquals` de `bindings.cpp` (bun 1.4.2), o algoritmo de `Bun.deepEquals(a, b, strict)`: `SameValue`
//! primeiro; primitivo contra o resto é diferente; a pilha de pares em comparação fecha os ciclos; depois os tipos
//! especiais (`specialObjectsDequal`, nos dois sentidos), arrays (buracos e as chaves símbolo) e, por fim, as
//! propriedades: pela `Structure` quando as duas permitem a enumeração rápida, senão pelo `getPropertyNames`, que
//! sobe a cadeia de protótipos.
//!
//! Os modos são os de `Bun.deepEquals`: o comum (`isStrict` falso), o estrito e o estrito que ignora a identidade do
//! protótipo (`Bun.deepEquals(a, b, true, true)`). O modo do `node:assert`/`node:util` (`checkPrototypes`) e os
//! `asymmetric matchers` do `bun:test` entram com eles. `Temporal` não existe no porte; o ramo dele também não.

use std::rc::Rc;

use crate::runtime::aggregate_error::get_if_property_exists;
use crate::runtime::array_constructor::{is_array, IsArrayCaller};
use crate::runtime::bigint_object::BigIntObject;
use crate::runtime::date_instance::DateInstance;
use crate::runtime::enumeration_mode::{DontEnumPropertiesMode, PrivateSymbolMode, PropertyNameMode};
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::exception_helpers::calculated_class_name;
use crate::runtime::gc::RootScope;
use crate::runtime::host_call::{pending_or, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::{parse_index_identifier, Identifier};
use crate::runtime::indexing_type::has_indexed_properties;
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_data_view::JSDataView;
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_map::JSMap;
use crate::runtime::js_object::JSObject;
use crate::runtime::js_property_name_enumerator::get_property_names_with;
use crate::runtime::js_set::JSSet;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_wrapper_object::JSWrapperObject;
use crate::runtime::operations::{same_value, strict_equal};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_name_array::PropertyNameArrayBuilder;
use crate::runtime::property_offset::INVALID_OFFSET;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::array_buffer_constructor::ARRAY_BUFFER_CONSTRUCTOR_S_INFO;
use crate::runtime::array_constructor::ARRAY_CONSTRUCTOR_S_INFO;
use crate::runtime::bigint_constructor::BIG_INT_CONSTRUCTOR_S_INFO;
use crate::runtime::boolean_constructor::BOOLEAN_CONSTRUCTOR_S_INFO;
use crate::runtime::class_info::ClassInfo;
use crate::runtime::data_view_constructor::DATA_VIEW_CONSTRUCTOR_S_INFO;
use crate::runtime::date_constructor::DATE_CONSTRUCTOR_S_INFO;
use crate::runtime::error_natives::ERROR_CONSTRUCTOR_S_INFO;
use crate::runtime::function_constructor::FUNCTION_CONSTRUCTOR_S_INFO;
use crate::runtime::map_constructor::MAP_CONSTRUCTOR_S_INFO;
use crate::runtime::node_buffer::BUFFER_CONSTRUCTOR_S_INFO;
use crate::runtime::number_constructor::NUMBER_CONSTRUCTOR_S_INFO;
use crate::runtime::object_constructor::OBJECT_CONSTRUCTOR_S_INFO;
use crate::runtime::object_prototype::object_prototype_to_string;
use crate::runtime::promise_constructor::JS_PROMISE_CONSTRUCTOR_S_INFO;
use crate::runtime::proxy_object::{get_property_slot, object_get, object_get_own_property_names, own_property_slot};
use crate::runtime::reg_exp_prototype_natives::REG_EXP_CONSTRUCTOR_S_INFO;
use crate::runtime::set_constructor::SET_CONSTRUCTOR_S_INFO;
use crate::runtime::string_constructor_natives::STRING_CONSTRUCTOR_S_INFO;
use crate::runtime::symbol_constructor::SYMBOL_CONSTRUCTOR_S_INFO;
use crate::runtime::typed_array_constructors::TYPED_ARRAY_CONSTRUCTOR_S_INFOS;
use crate::runtime::weak_map_constructor::WEAK_MAP_CONSTRUCTOR_S_INFO;
use crate::runtime::weak_set_constructor::WEAK_SET_CONSTRUCTOR_S_INFO;
use crate::runtime::reg_exp_object::RegExpObject;
use crate::runtime::string_object::StringObject;
use crate::runtime::structure::StructureRef;
use crate::runtime::symbol_object::SymbolObject;
use crate::wtf::text::wtf_string::String as WtfString;

/// Os parâmetros de modelo de `Bun__deepEquals` que o `Bun.deepEquals` usa.
#[derive(Clone, Copy)]
pub(crate) struct Mode {
    pub(crate) strict: bool,
    pub(crate) skip_prototype_identity: bool,
    /// `checkPrototypes`: o modo do `node:assert` e do `node:util` (`isDeepStrictEqual`), que segue o `objectComparisonStart`
    /// do Node e só olha as propriedades próprias.
    pub(crate) check_prototypes: bool,
}

/// Os pares em comparação (`Vector<std::pair<JSValue, JSValue>, 16>& stack`).
type Stack = Vec<(JSValue, JSValue)>;

/// Os dois lados de cada par como raiz enquanto o JS de getters e traps pode rodar o coletor.
pub(crate) fn root_pairs(pairs: &[(JSValue, JSValue)]) -> RootScope {
    RootScope::new(&pairs.iter().flat_map(|(left, right)| [*left, *right]).collect::<Vec<JSValue>>())
}

/// `Bun__deepEquals(globalObject, v1, v2, gcBuffer, stack, scope, true)` com a pilha vazia.
pub(crate) fn deep_equals(global_object: &JSGlobalObject, mode: Mode, v1: JSValue, v2: JSValue) -> Result<bool, Thrown> {
    deep_equals_in(global_object, mode, &mut Stack::new(), v1, v2, true)
}

fn deep_equals_in(global_object: &JSGlobalObject, mode: Mode, stack: &mut Stack, v1: JSValue, v2: JSValue, add_to_stack: bool) -> Result<bool, Thrown> {
    if !global_object.vm().is_safe_to_recurse() {
        return Err(Thrown::StackOverflow);
    }
    if !v1.is_empty() && !v2.is_empty() && same_value(v1, v2) {
        return Ok(true);
    }
    if v1.is_empty() || v2.is_empty() {
        return Ok(v1.is_empty() == v2.is_empty());
    }
    let (Some(o1), Some(o2)) = (ObjectRef::from_value(&v1).filter(|_| v1.is_object()), ObjectRef::from_value(&v2).filter(|_| v2.is_object())) else {
        return Ok(false);
    };

    for (first, second) in stack.iter() {
        let first_matches = strict_equal(*first, v1);
        let second_matches = strict_equal(*second, v2);
        if first_matches {
            return Ok(second_matches);
        }
        if second_matches {
            return Ok(false);
        }
    }

    let length = stack.len();
    // `gcBuffer.append(v1); gcBuffer.append(v2)`: um getter ou um trap no caminho pode tirar o par do grafo.
    let _roots = add_to_stack.then(|| RootScope::new(&[v1, v2]));
    if add_to_stack {
        stack.push((v1, v2));
    }
    let result = compare_objects(global_object, mode, stack, &o1, &o2);
    if add_to_stack {
        stack.remove(length);
    }
    result
}

/// O que `Bun__deepEquals` faz depois de empilhar o par: os tipos especiais, os arrays e as propriedades.
fn compare_objects(global_object: &JSGlobalObject, mode: Mode, stack: &mut Stack, o1: &ObjectRef, o2: &ObjectRef) -> Result<bool, Thrown> {
    if mode.check_prototypes && !object_comparison_start(global_object, mode, o1, o2)? {
        return Ok(false);
    }
    if let Some(equal) = special_objects_dequal(global_object, mode, stack, o1, o2)? {
        return Ok(equal);
    }
    if let Some(equal) = special_objects_dequal(global_object, mode, stack, o2, o1)? {
        return Ok(equal);
    }

    let array_of = |object: &ObjectRef| is_array(&object.as_value(), IsArrayCaller::ArrayIsArray).map_err(Thrown::from);
    let (v1_array, v2_array) = (array_of(o1)?, array_of(o2)?);
    if v1_array != v2_array {
        return Ok(false);
    }
    if v1_array && !o1.cell().is_proxy() && !o2.cell().is_proxy() {
        return arrays_equal(global_object, mode, stack, o1, o2);
    }

    if mode.strict && !mode.check_prototypes && !mode.skip_prototype_identity && calculated_class_name(o1) != calculated_class_name(o2) {
        return Ok(false);
    }

    let vm = global_object.vm();
    let (s1, s2) = (o1.structure(), o2.structure());
    // O caminho de estruturas diferentes acha com `getDirect` também o que não é enumerável, que o Node ignora.
    if fast_enumerable(o1, &s1) && fast_enumerable(o2, &s2) && (!mode.check_prototypes || s1.id() == s2.id()) {
        return structures_equal(global_object, mode, stack, o1, o2, &s1, &s2);
    }

    let names_of = |object: &ObjectRef| if mode.check_prototypes { own_property_names(global_object, object) } else { property_names(global_object, object) };
    let (a1, a2) = (names_of(o1)?, names_of(o2)?);
    properties_equal(global_object, mode, stack, o1, o2, &a1, &a2, None, mode.check_prototypes)
}

/// `isWellKnownConstructor` (o `wellKnownConstructors` do Node, de qualquer realm): a identidade do `ClassInfo` de
/// um `JSFunction` ou `InternalFunction`.
fn is_well_known_constructor(value: JSValue) -> bool {
    if !value.is_object() {
        return false;
    }
    let Some(object) = ObjectRef::from_value(&value) else { return false };
    if !matches!(object.cell().type_(), JSType::InternalFunctionType | JSType::JSFunctionType) {
        return false;
    }
    let info: &'static ClassInfo = object.cell().class_info();
    let known: [&'static ClassInfo; 19] = [
        &OBJECT_CONSTRUCTOR_S_INFO,
        &ARRAY_CONSTRUCTOR_S_INFO,
        &FUNCTION_CONSTRUCTOR_S_INFO,
        &REG_EXP_CONSTRUCTOR_S_INFO,
        &JS_PROMISE_CONSTRUCTOR_S_INFO,
        &STRING_CONSTRUCTOR_S_INFO,
        &SYMBOL_CONSTRUCTOR_S_INFO,
        &BIG_INT_CONSTRUCTOR_S_INFO,
        &BOOLEAN_CONSTRUCTOR_S_INFO,
        &NUMBER_CONSTRUCTOR_S_INFO,
        &DATE_CONSTRUCTOR_S_INFO,
        &ERROR_CONSTRUCTOR_S_INFO,
        &MAP_CONSTRUCTOR_S_INFO,
        &SET_CONSTRUCTOR_S_INFO,
        &WEAK_MAP_CONSTRUCTOR_S_INFO,
        &WEAK_SET_CONSTRUCTOR_S_INFO,
        &ARRAY_BUFFER_CONSTRUCTOR_S_INFO,
        &DATA_VIEW_CONSTRUCTOR_S_INFO,
        &BUFFER_CONSTRUCTOR_S_INFO,
    ];
    known.iter().any(|candidate| std::ptr::eq(*candidate, info)) || TYPED_ARRAY_CONSTRUCTOR_S_INFOS.iter().any(|candidate| std::ptr::eq(candidate, info))
}

/// O valor de um slot já achado, com a exceção do getter pendente (`slot.getValue` seguido de `RETURN_IF_EXCEPTION`).
pub(crate) fn slot_value(global_object: &JSGlobalObject, slot: &PropertySlot, name: &PropertyName) -> Result<JSValue, Thrown> {
    let value = slot.get_value_for(name);
    pending_or(global_object, value)
}

/// O começo de `objectComparisonStart` do Node (`lib/internal/util/comparisons.js`): em modo estrito a regra do
/// `constructor` e do `[[Prototype]]`, e em qualquer modo as etiquetas de `Object.prototype.toString` iguais.
fn object_comparison_start(global_object: &JSGlobalObject, mode: Mode, o1: &ObjectRef, o2: &ObjectRef) -> Result<bool, Thrown> {
    let vm = global_object.vm();
    if !mode.skip_prototype_identity {
        let constructor_name = PropertyName::from_identifier(&vm.property_names.constructor);
        let mut slot1 = PropertySlot::new(o1.as_value(), InternalMethodType::Get);
        let has_constructor = get_property_slot(global_object, o1.as_value(), &constructor_name, &mut slot1)?;
        let constructor1 = if has_constructor { slot_value(global_object, &slot1, &constructor_name)? } else { JSValue::undefined() };
        let mut compare_constructors = is_well_known_constructor(constructor1);
        if !compare_constructors && !constructor1.is_undefined() {
            if slot1.is_tainted_by_opaque_object() {
                let mut own_slot = PropertySlot::new(o1.as_value(), InternalMethodType::GetOwnProperty);
                compare_constructors = !own_property_slot(global_object, o1.as_value(), &constructor_name, &mut own_slot)?;
            } else {
                compare_constructors = slot1.slot_base() != Some(o1.cell_id());
            }
        }
        if compare_constructors {
            // A mesma estrutura mono-proto é a mesma cadeia: o dado achado nela vale também para `val2.constructor`.
            let inherited_from_shared_chain = slot1.is_cacheable_value()
                && slot1.slot_base() != Some(o1.cell_id())
                && o1.structure().id() == o2.structure().id()
                && !o1.structure().has_poly_proto();
            if !inherited_from_shared_chain {
                let constructor2 = object_get(global_object, o2, &constructor_name, o2.as_value())?;
                if !strict_equal(constructor1, constructor2) {
                    return Ok(false);
                }
            }
        } else {
            let proto1 = o1.as_value().get_prototype(global_object)?;
            let proto2 = o2.as_value().get_prototype(global_object)?;
            if !same_value(proto1, proto2) {
                return Ok(false);
            }
        }
    }
    let tag_of = |object: &ObjectRef| -> Result<WtfString, Thrown> {
        let tag = object_prototype_to_string(global_object, object.as_value()).ok_or(Thrown::Pending)?;
        Ok(tag.value())
    };
    Ok(tag_of(o1)? == tag_of(o2)?)
}

/// `nonIndexOwnPropertiesEqual`: as chaves próprias enumeráveis de texto e símbolo que não são índice, lidas pelo
/// `getOwnPropertySlot` nos dois lados (um getter pode ter tirado a chave no meio do caminho).
fn non_index_own_properties_equal(global_object: &JSGlobalObject, mode: Mode, stack: &mut Stack, o1: &ObjectRef, o2: &ObjectRef) -> Result<bool, Thrown> {
    let non_index = |object: &ObjectRef| -> Result<Vec<Identifier>, Thrown> {
        Ok(own_property_names(global_object, object)?.into_iter().filter(|name| name.is_symbol() || parse_index_identifier(name).is_none()).collect())
    };
    let (a1, a2) = (non_index(o1)?, non_index(o2)?);
    if a1.len() != a2.len() {
        return Ok(false);
    }
    for name in &a1 {
        let property_name = PropertyName::from_identifier(name);
        let mut slot2 = PropertySlot::new(o2.as_value(), InternalMethodType::GetOwnProperty);
        if !own_property_slot(global_object, o2.as_value(), &property_name, &mut slot2)? || slot2.attributes() & DONT_ENUM != 0 {
            return Ok(false);
        }
        let v2 = slot_value(global_object, &slot2, &property_name)?;
        let _v2 = RootScope::new(&[v2]);
        let mut slot1 = PropertySlot::new(o1.as_value(), InternalMethodType::GetOwnProperty);
        if !own_property_slot(global_object, o1.as_value(), &property_name, &mut slot1)? || slot1.attributes() & DONT_ENUM != 0 {
            return Ok(false);
        }
        let v1 = slot_value(global_object, &slot1, &property_name)?;
        let _v1 = RootScope::new(&[v1]);
        if !deep_equals_in(global_object, mode, stack, v1, v2, true)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `o->methodTable()->getOwnPropertyNames(o, globalObject, names, DontEnumPropertiesMode::Exclude)`: só as chaves próprias.
pub(crate) fn own_property_names(global_object: &JSGlobalObject, object: &ObjectRef) -> Result<Vec<Identifier>, Thrown> {
    let mut names = PropertyNameArrayBuilder::new(global_object.vm(), PropertyNameMode::StringsAndSymbols, PrivateSymbolMode::Exclude);
    object_get_own_property_names(global_object, object, &mut names, DontEnumPropertiesMode::Exclude)?;
    Ok(names.iter().cloned().collect())
}

/// `object->getPropertyNames(globalObject, names, DontEnumPropertiesMode::Exclude)`: as chaves enumeráveis de texto e
/// símbolo do objeto e da cadeia de protótipos.
fn property_names(global_object: &JSGlobalObject, object: &ObjectRef) -> Result<Vec<Identifier>, Thrown> {
    let mut names = PropertyNameArrayBuilder::new(global_object.vm(), PropertyNameMode::StringsAndSymbols, PrivateSymbolMode::Exclude);
    get_property_names_with(global_object, object, &mut names, |object, names| {
        object_get_own_property_names(global_object, object, names, DontEnumPropertiesMode::Exclude)
    })?;
    Ok(names.iter().cloned().collect())
}

/// `!hasNonReifiedStaticProperties() && canPerformFastPropertyEnumeration()`.
fn fast_enumerable(object: &JSObject, structure: &StructureRef) -> bool {
    !object.has_non_reified_static_properties(structure) && structure.can_perform_fast_property_enumeration()
}

/// A comparação pelos nomes `a1` e `a2` (o fim de `Bun__deepEquals` e o laço das propriedades do `ErrorInstance`):
/// cada nome de `a1` lido nos dois lados, e os nomes que sobram em `a2` têm de valer `undefined`. `skip` é o nome que
/// não conta (o `stack` do erro). `own_only` lê o lado direito só pelas próprias enumeráveis (o fim de `Bun__deepEquals`
/// em modo Node); o ramo do `ErrorInstance` segue com o `getIfPropertyExists` também nesse modo.
#[allow(clippy::too_many_arguments)]
fn properties_equal(
    global_object: &JSGlobalObject,
    mode: Mode,
    stack: &mut Stack,
    o1: &ObjectRef,
    o2: &ObjectRef,
    a1: &[Identifier],
    a2: &[Identifier],
    skip: Option<&Identifier>,
    own_only: bool,
) -> Result<bool, Thrown> {
    if mode.strict && a1.len() != a2.len() {
        return Ok(false);
    }
    if !each_property_equal(global_object, mode, stack, o1, o2, a1, skip, own_only)? {
        return Ok(false);
    }
    for name in a2.iter().skip(a1.len()) {
        if skip == Some(name) {
            continue;
        }
        let prop2 = get_if_property_exists(global_object, o2, &PropertyName::from_identifier(name))?;
        if !prop2.is_some_and(|value| value.is_undefined()) {
            return Ok(false);
        }
    }
    Ok(true)
}

/// "take a property name from one, try to get it from both": cada nome lido em `o1` (o `[[Get]]`, com getter) e em
/// `o2` (`getIfPropertyExists`); no modo comum `undefined` casa com a ausência.
#[allow(clippy::too_many_arguments)]
fn each_property_equal(
    global_object: &JSGlobalObject,
    mode: Mode,
    stack: &mut Stack,
    o1: &ObjectRef,
    o2: &ObjectRef,
    names: &[Identifier],
    skip: Option<&Identifier>,
    own_only: bool,
) -> Result<bool, Thrown> {
    for name in names {
        if skip == Some(name) {
            continue;
        }
        let property_name = PropertyName::from_identifier(name);
        let prop1 = object_get(global_object, o1, &property_name, o1.as_value())?;
        if prop1.is_empty() {
            return Ok(false);
        }
        // O C++ guarda `prop1` na pilha nativa, varrida pelo coletor conservador; aqui vira raiz enquanto o getter de
        // `o2` roda.
        let _prop1 = RootScope::new(&[prop1]);
        let prop2 = if own_only {
            // O Node só casa propriedade própria enumerável; `getIfPropertyExists` acharia a herdada e a oculta.
            let mut slot2 = PropertySlot::new(o2.as_value(), InternalMethodType::GetOwnProperty);
            if !own_property_slot(global_object, o2.as_value(), &property_name, &mut slot2)? || slot2.attributes() & DONT_ENUM != 0 {
                return Ok(false);
            }
            Some(slot_value(global_object, &slot2, &property_name)?)
        } else {
            get_if_property_exists(global_object, o2, &property_name)?
        };
        if !mode.strict && prop1.is_undefined() && prop2.is_none() {
            continue;
        }
        let Some(prop2) = prop2 else { return Ok(false) };
        if !deep_equals_in(global_object, mode, stack, prop1, prop2, true)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `getIndexWithoutAccessors`: o elemento `i`, ou `empty` para buraco e acessor.
fn index_without_accessors(global_object: &JSGlobalObject, object: &JSObject, i: u32) -> Result<JSValue, Thrown> {
    if object.can_get_index_quickly(i) {
        return Ok(object.get_index_quickly(i));
    }
    let vm = global_object.vm();
    let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::Get);
    let has_slot = object.get_own_property_slot_by_index(vm, i, &mut slot);
    if vm.exception().is_some() {
        return Err(Thrown::Pending);
    }
    if has_slot && !slot.is_accessor() {
        return Ok(slot.get_value_for_index(vm, i));
    }
    Ok(JSValue::empty())
}

/// O ramo dos arrays: os elementos (buracos contam como `undefined` só no modo comum), o excesso do segundo tem de ser
/// buraco ou `undefined`, e as chaves símbolo enumeráveis (as de texto ficam de fora).
fn arrays_equal(global_object: &JSGlobalObject, mode: Mode, stack: &mut Stack, o1: &ObjectRef, o2: &ObjectRef) -> Result<bool, Thrown> {
    let length_of = |object: &ObjectRef| crate::runtime::js_array::JSArray::from_value_by_class(&object.as_value()).map_or(0, |array| array.length());
    let (length1, length2) = (length_of(o1), length_of(o2));
    if mode.strict && length1 != length2 {
        return Ok(false);
    }
    for i in 0..length1 {
        let left = index_without_accessors(global_object, o1, i)?;
        let right = index_without_accessors(global_object, o2, i)?;
        if mode.strict {
            if left.is_empty() && right.is_empty() {
                continue;
            }
            if left.is_empty() || right.is_empty() {
                return Ok(false);
            }
        } else if (left.is_empty() || right.is_empty()) && (left.is_undefined() || right.is_undefined()) {
            continue;
        }
        if !deep_equals_in(global_object, mode, stack, left, right, true)? {
            return Ok(false);
        }
    }
    for i in length1..length2 {
        let right = index_without_accessors(global_object, o2, i)?;
        if !(right.is_empty() || right.is_undefined()) {
            return Ok(false);
        }
    }

    if mode.check_prototypes {
        // O Node compara as chaves próprias de texto e símbolo que não são índice; o bloco só de símbolos abaixo sobe a cadeia.
        return non_index_own_properties_equal(global_object, mode, stack, o1, o2);
    }

    let vm = global_object.vm();
    let symbols_of = |object: &ObjectRef| -> Result<Vec<Identifier>, Thrown> {
        let mut names = PropertyNameArrayBuilder::new(vm, PropertyNameMode::Symbols, PrivateSymbolMode::Exclude);
        crate::runtime::own_property_names::get_own_property_names(vm, object, &mut names, DontEnumPropertiesMode::Exclude)?;
        Ok(names.iter().cloned().collect())
    };
    let (a1, a2) = (symbols_of(o1)?, symbols_of(o2)?);
    if mode.strict && a1.len() != a2.len() {
        return Ok(false);
    }
    each_property_equal(global_object, mode, stack, o1, o2, &a1, None, false)
}

/// O caminho rápido pela `Structure`: com a mesma estrutura, os pares saem dos mesmos offsets; com estruturas
/// diferentes, cada propriedade enumerável de `o1` é procurada em `o2` e as de `o2` têm de existir em `o1`. Os pares se
/// comparam no fim, depois das travessias (um getter pode mexer na tabela).
fn structures_equal(
    global_object: &JSGlobalObject,
    mode: Mode,
    stack: &mut Stack,
    o1: &ObjectRef,
    o2: &ObjectRef,
    s1: &StructureRef,
    s2: &StructureRef,
) -> Result<bool, Thrown> {
    let vm = global_object.vm();
    let counted = |attributes: u32, is_private: bool| attributes & DONT_ENUM == 0 && !is_private;
    let mut pairs: Vec<(JSValue, JSValue)> = Vec::new();
    if s1.id() == s2.id() {
        for (_, offset, attributes, is_private) in s1.properties_with_privacy() {
            if !counted(attributes, is_private) {
                continue;
            }
            let (left, right) = (o1.get_direct(offset), o2.get_direct(offset));
            if !mode.strict && left.is_undefined() && right.is_empty() {
                continue;
            }
            if right.is_empty() {
                return Ok(false);
            }
            pairs.push((left, right));
        }
    } else {
        let mut count = 0usize;
        for (key, offset, attributes, is_private) in s1.properties_with_privacy() {
            if !counted(attributes, is_private) {
                continue;
            }
            count += 1;
            let left = o1.get_direct(offset);
            let property_name = PropertyName::from_uid(Some(key), is_private);
            let right = if mode.strict {
                // Só uma propriedade enumerável de `o2` casa com uma enumerável de `o1`.
                let (o2_offset, o2_attributes) = s2.get_with_attributes(vm, &property_name);
                if o2_offset != INVALID_OFFSET && o2_attributes & DONT_ENUM == 0 { o2.get_direct(o2_offset) } else { JSValue::empty() }
            } else {
                o2.get_direct_by_name(vm, &property_name)
            };
            if !mode.strict && left.is_undefined() && right.is_empty() {
                continue;
            }
            if right.is_empty() {
                return Ok(false);
            }
            pairs.push((left, right));
        }
        let mut remain = count;
        for (key, offset, attributes, is_private) in s2.properties_with_privacy() {
            if !counted(attributes, is_private) {
                continue;
            }
            if !mode.strict && o2.get_direct(offset).is_undefined() {
                continue;
            }
            if o1.get_direct_offset(vm, &PropertyName::from_uid(Some(key), is_private)) == INVALID_OFFSET || remain == 0 {
                return Ok(false);
            }
            remain -= 1;
        }
    }
    // `MarkedArgumentBuffer pairs`.
    let _pairs = root_pairs(&pairs);
    for (left, right) in pairs {
        if same_value(left, right) {
            continue;
        }
        if !deep_equals_in(global_object, mode, stack, left, right, true)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `hasExtraOwnProperties(structure)`: propriedade nomeada ou indexada além do conteúdo sintetizado.
fn has_extra_own_properties(structure: &StructureRef) -> bool {
    structure.out_of_line_size() != 0 || structure.inline_size() != 0 || has_indexed_properties(structure.indexing_type())
}

/// `ErrorInstance::sanitizedNameString`: o `name` próprio do erro ou do protótipo (dois níveis, sem chamar getter), ou
/// `Error`.
fn sanitized_name(global_object: &JSGlobalObject, error: &ObjectRef) -> Result<WtfString, Thrown> {
    let vm = global_object.vm();
    let name = PropertyName::from_identifier(&vm.property_names.name);
    let mut current = Some(error.clone());
    for _ in 0..2 {
        let Some(object) = current else { break };
        let mut slot = PropertySlot::new(object.as_value(), InternalMethodType::VMInquiry);
        if object.get_own_property_slot(global_object, &name, &mut slot) && slot.is_value() {
            let value = slot.get_value_for(&name);
            if value.is_object() {
                break;
            }
            return pending_or(global_object, value.to_wtf_string());
        }
        let prototype = object.get_prototype_direct();
        current = ObjectRef::from_value(&prototype).filter(|_| prototype.is_object());
    }
    Ok(WtfString::from_latin1(b"Error"))
}

/// `ErrorInstance::sanitizedMessageString`: o `message` próprio, se for primitivo; senão a string nula.
fn sanitized_message(global_object: &JSGlobalObject, error: &ObjectRef) -> Result<WtfString, Thrown> {
    let vm = global_object.vm();
    let message = PropertyName::from_identifier(&vm.property_names.message);
    let mut slot = PropertySlot::new(error.as_value(), InternalMethodType::VMInquiry);
    if error.get_own_property_slot(global_object, &message, &mut slot) && slot.is_value() {
        let value = slot.get_value_for(&message);
        if !value.is_object() && !value.is_empty() {
            return pending_or(global_object, value.to_wtf_string());
        }
    }
    Ok(WtfString::default())
}

/// O ramo `ErrorInstanceType`: tipo, `name`, `message`, a presença de `cause` (estrito) e o valor dela, e as
/// propriedades enumeráveis da cadeia, menos `stack`.
fn errors_equal(global_object: &JSGlobalObject, mode: Mode, stack: &mut Stack, left: &ObjectRef, right: &ObjectRef) -> Result<bool, Thrown> {
    let (Some(left_error), Some(right_error)) = (ErrorInstance::from_cell_id(left.cell_id()), ErrorInstance::from_cell_id(right.cell_id())) else {
        return Ok(false);
    };
    if left_error.error_type() != right_error.error_type() {
        return Ok(false);
    }
    if sanitized_name(global_object, left)? != sanitized_name(global_object, right)? {
        return Ok(false);
    }
    if sanitized_message(global_object, left)? != sanitized_message(global_object, right)? {
        return Ok(false);
    }
    let vm = global_object.vm();
    let cause = PropertyName::from_identifier(&vm.property_names.cause);
    if mode.strict {
        let has = |object: &ObjectRef| crate::runtime::proxy_object::object_has_property(global_object, object, &cause);
        if has(left)? != has(right)? {
            return Ok(false);
        }
    }
    let left_cause = object_get(global_object, left, &cause, left.as_value())?;
    let _left_cause = RootScope::new(&[left_cause]);
    let right_cause = object_get(global_object, right, &cause, right.as_value())?;
    if !deep_equals_in(global_object, mode, stack, left_cause, right_cause, true)? {
        return Ok(false);
    }
    let (a1, a2) = (property_names(global_object, left)?, property_names(global_object, right)?);
    properties_equal(global_object, mode, stack, left, right, &a1, &a2, Some(&vm.property_names.stack), false)
}

/// `NaN` e zero de um número de ponto flutuante de `width` bytes (little-endian), pelos bits.
pub(crate) fn float_bits(bytes: &[u8]) -> (bool, bool) {
    let bits = bytes.iter().rev().fold(0u64, |acc, byte| (acc << 8) | u64::from(*byte));
    let (exponent_bits, mantissa_bits) = match bytes.len() {
        2 => (5, 10),
        4 => (8, 23),
        _ => (11, 52),
    };
    let mantissa = bits & ((1u64 << mantissa_bits) - 1);
    let exponent = (bits >> mantissa_bits) & ((1u64 << exponent_bits) - 1);
    let magnitude = bits & ((1u64 << (mantissa_bits + exponent_bits)) - 1);
    (exponent == (1u64 << exponent_bits) - 1 && mantissa != 0, magnitude == 0)
}

/// `looseFloatContentsEqual`: `==` do IEEE elemento a elemento (`+0` igual a `-0`, `NaN` diferente de tudo).
fn loose_float_contents_equal(a: &[u8], b: &[u8], width: usize) -> bool {
    a.chunks(width).zip(b.chunks(width)).all(|(x, y)| {
        let ((x_nan, x_zero), (y_nan, y_zero)) = (float_bits(x), float_bits(y));
        !x_nan && !y_nan && (x == y || (x_zero && y_zero))
    })
}

/// O ramo das typed arrays: o mesmo tipo, o mesmo `byteLength`, e o conteúdo (por bytes; as de ponto flutuante no
/// modo comum por `==`). No estrito, propriedades a mais mandam para a comparação de propriedades.
fn typed_arrays_equal(global_object: &JSGlobalObject, mode: Mode, stack: &mut Stack, o1: &ObjectRef, o2: &ObjectRef) -> Result<Option<bool>, Thrown> {
    let (c1_type, c2_type) = (o1.type_(), o2.type_());
    if c1_type != c2_type {
        return Ok(Some(false));
    }
    let (Some(left), Some(right)) = (JSGenericTypedArrayView::from_cell_id(o1.cell_id()), JSGenericTypedArrayView::from_cell_id(o2.cell_id())) else {
        return Ok(Some(false));
    };
    let compare_own_properties = mode.strict && (has_extra_own_properties(&o1.structure()) || has_extra_own_properties(&o2.structure()));
    // Todas as saídas "bytes iguais" passam pelo mesmo fim: em modo Node as chaves próprias que não são índice, no
    // estrito as propriedades a mais mandam para a comparação de propriedades, e senão é igual.
    let equal_tail = |stack: &mut Stack| -> Result<Option<bool>, Thrown> {
        if mode.check_prototypes {
            return non_index_own_properties_equal(global_object, mode, stack, o1, o2).map(Some);
        }
        Ok(if compare_own_properties { None } else { Some(true) })
    };
    let byte_length = left.byte_length();
    if right.byte_length() != byte_length {
        return Ok(Some(false));
    }
    if byte_length == 0 {
        return equal_tail(stack);
    }
    if right.is_detached() || left.is_detached() {
        return Ok(Some(false));
    }
    // `vector == rightVector`: as duas visões leem a mesma memória.
    let same_vector = match (left.existing_buffer(), right.existing_buffer()) {
        (Some(a), Some(b)) => Rc::ptr_eq(&a, &b) && left.byte_offset_raw() == right.byte_offset_raw(),
        _ => false,
    };
    if same_vector {
        return equal_tail(stack);
    }
    let width = match c1_type {
        JSType::Float16ArrayType => Some(2),
        JSType::Float32ArrayType => Some(4),
        JSType::Float64ArrayType => Some(8),
        _ => None,
    };
    let contents_equal = left.with_view_bytes(|a| {
        right.with_view_bytes(|b| match width {
            Some(width) if !mode.strict => loose_float_contents_equal(a, b, width),
            _ => a == b,
        })
    });
    if !contents_equal {
        return Ok(Some(false));
    }
    equal_tail(stack)
}

/// O que o `Bun.deepEquals` compara dos objetos do WebCore (`JSDOMWrapperType`): o `href` do `URL`, os pares do
/// `URLSearchParams` e os cabeçalhos do `Headers`. `None` é seguir para a comparação de propriedades.
fn dom_objects_equal(mode: Mode, o1: &ObjectRef, o2: &ObjectRef) -> Option<bool> {
    let (v1, v2) = (o1.as_value(), o2.as_value());
    let (url1, url2) = (crate::runtime::url::url_of(v1), crate::runtime::url::url_of(v2));
    if url1.is_some() != url2.is_some() {
        return mode.strict.then_some(false);
    }
    if let (Some(url1), Some(url2)) = (url1, url2) {
        return (url1.string() != url2.string()).then_some(false);
    }
    let (params1, params2) = (crate::runtime::url_search_params::pairs_of_instance(v1), crate::runtime::url_search_params::pairs_of_instance(v2));
    match (params1, params2) {
        (Some(params1), Some(params2)) => {
            if params1.len() != params2.len() {
                return Some(false);
            }
            let mismatch = params1.iter().any(|(key, value)| params2.iter().find(|pair| &pair.0 == key).map(|pair| &pair.1) != Some(value));
            return mismatch.then_some(false);
        }
        (None, None) => {}
        _ => return mode.strict.then_some(false),
    }
    match (crate::runtime::headers::combined_headers(v1), crate::runtime::headers::combined_headers(v2)) {
        (Some(headers1), Some(headers2)) => {
            if headers1.len() != headers2.len() {
                return Some(false);
            }
            let mismatch = headers1.iter().any(|(name, value)| headers2.iter().find(|pair| &pair.0 == name).map(|pair| &pair.1) != Some(value));
            mismatch.then_some(false)
        }
        (None, None) => None,
        _ => mode.strict.then_some(false),
    }
}

/// Os objetos do WebCore que o `Bun.deepEquals` reconhece (no porte são objetos comuns com estado nativo).
fn is_dom_object(object: &ObjectRef) -> bool {
    let value = object.as_value();
    crate::runtime::url::url_of(value).is_some()
        || crate::runtime::url_search_params::pairs_of_instance(value).is_some()
        || crate::runtime::headers::combined_headers(value).is_some()
}

/// O valor interno dos invólucros `Symbol` e `BigInt` (no JSC são `ObjectType` comuns).
pub(crate) fn wrapper_kind(object: &ObjectRef) -> Option<(bool, JSValue)> {
    let value = object.as_value();
    if let Some(wrapper) = SymbolObject::from_value(&value) {
        return Some((true, wrapper.internal_value()));
    }
    BigIntObject::from_value(&value).map(|wrapper| (false, wrapper.internal_value()))
}

/// `specialObjectsDequal(globalObject, ..., c1, c2)`: `Some` decide, `None` segue para arrays e propriedades.
fn special_objects_dequal(global_object: &JSGlobalObject, mode: Mode, stack: &mut Stack, o1: &ObjectRef, o2: &ObjectRef) -> Result<Option<bool>, Thrown> {
    let (c1_type, c2_type) = (o1.type_(), o2.type_());
    // O Node nunca acha iguais dois `WeakMap`, `WeakSet` ou `Promise` distintos (o conteúdo não se inspeciona).
    if mode.check_prototypes
        && [c1_type, c2_type].iter().any(|kind| matches!(kind, JSType::JSWeakMapType | JSType::JSWeakSetType | JSType::JSPromiseType))
    {
        return Ok(Some(false));
    }
    match c1_type {
        JSType::JSSetType => {
            let (Some(set1), Some(set2)) = (JSSet::from_cell_id(o1.cell_id()), JSSet::from_cell_id(o2.cell_id())) else { return Ok(Some(false)) };
            if set1.table().borrow().size() != set2.table().borrow().size() {
                return Ok(Some(false));
            }
            let keys = |set: &JSSet| -> Vec<JSValue> {
                let cursor = set.table().borrow_mut().new_cursor();
                std::iter::from_fn(|| set.table().borrow().next_entry(&cursor)).map(|(key, _)| key).collect()
            };
            let keys1 = keys(&set1);
            let _keys1 = RootScope::new(&keys1);
            for key1 in keys1 {
                if set2.table().borrow().has(key1) {
                    continue;
                }
                // Não achou: pode ser falso negativo da representação dos valores, então a busca linear.
                let mut found = false;
                let keys2 = keys(&set2);
                let _keys2 = RootScope::new(&keys2);
                for key2 in keys2 {
                    if deep_equals_in(global_object, mode, stack, key1, key2, false)? {
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Ok(Some(false));
                }
            }
            Ok(if mode.check_prototypes { None } else { Some(true) })
        }
        JSType::JSMapType => {
            let (Some(map1), Some(map2)) = (JSMap::from_cell_id(o1.cell_id()), JSMap::from_cell_id(o2.cell_id())) else { return Ok(Some(false)) };
            if map1.table().borrow().size() != map2.table().borrow().size() {
                return Ok(Some(false));
            }
            let entries = |map: &JSMap| -> Vec<(JSValue, JSValue)> {
                let cursor = map.table().borrow_mut().new_cursor();
                std::iter::from_fn(|| map.table().borrow().next_entry(&cursor)).collect()
            };
            let entries1 = entries(&map1);
            let _entries1 = root_pairs(&entries1);
            for (key1, value1) in entries1 {
                let mut value2 = map2.table().borrow().get(key1);
                if value2.is_undefined() {
                    let mut found = false;
                    let entries2 = entries(&map2);
                    let _entries2 = root_pairs(&entries2);
                    for (key2, candidate) in entries2 {
                        value2 = candidate;
                        if deep_equals_in(global_object, mode, stack, key1, key2, false)? {
                            found = true;
                            break;
                        }
                    }
                    if !found {
                        return Ok(Some(false));
                    }
                }
                if !deep_equals_in(global_object, mode, stack, value1, value2, false)? {
                    return Ok(Some(false));
                }
            }
            Ok(if mode.check_prototypes { None } else { Some(true) })
        }
        JSType::ArrayBufferType => {
            if c2_type != JSType::ArrayBufferType {
                return Ok(Some(false));
            }
            let (Some(left), Some(right)) = (JSArrayBuffer::from_cell_id(o1.cell_id()), JSArrayBuffer::from_cell_id(o2.cell_id())) else { return Ok(Some(false)) };
            let (left, right) = (left.impl_(), right.impl_());
            if left.byte_length() != right.byte_length() || left.is_shared() != right.is_shared() {
                return Ok(Some(false));
            }
            if left.is_detached() || right.is_detached() {
                return Err(Thrown::type_error("Cannot perform Construct on a detached ArrayBuffer"));
            }
            if !left.with_bytes(|a| right.with_bytes(|b| a == b)) {
                return Ok(Some(false));
            }
            // O Node compara também as propriedades próprias enumeráveis do `ArrayBuffer`.
            Ok(if mode.check_prototypes { None } else { Some(true) })
        }
        JSType::DataViewType => {
            if !mode.check_prototypes {
                // O `Bun.deepEquals` compara `DataView` como objeto comum; os bytes ficam para as instâncias do Node.
                return Ok(None);
            }
            if c2_type != JSType::DataViewType {
                return Ok(Some(false));
            }
            let (Some(left), Some(right)) = (JSDataView::from_cell_id(o1.cell_id()), JSDataView::from_cell_id(o2.cell_id())) else { return Ok(Some(false)) };
            if left.is_detached() || right.is_detached() {
                return Err(Thrown::type_error("Cannot perform get DataView.prototype.byteLength on a detached or out-of-bounds ArrayBuffer"));
            }
            let (length1, length2) = (left.view_byte_length().unwrap_or(0), right.view_byte_length().unwrap_or(0));
            if length1 != length2 {
                return Ok(Some(false));
            }
            let (offset1, offset2) = (left.byte_offset_raw(), right.byte_offset_raw());
            let equal = left.possibly_shared_buffer().with_bytes(|a| {
                right.possibly_shared_buffer().with_bytes(|b| a.get(offset1..offset1 + length1) == b.get(offset2..offset2 + length2))
            });
            if !equal {
                return Ok(Some(false));
            }
            non_index_own_properties_equal(global_object, mode, stack, o1, o2).map(Some)
        }
        JSType::JSDateType => {
            if c2_type != JSType::JSDateType {
                return Ok(Some(false));
            }
            let (Some(left), Some(right)) = (DateInstance::from_cell_id(o1.cell_id()), DateInstance::from_cell_id(o2.cell_id())) else { return Ok(Some(false)) };
            let (time1, time2) = (left.internal_number(), right.internal_number());
            if mode.check_prototypes {
                // Duas datas inválidas são iguais para o Node, que compara também as propriedades próprias.
                if time1 != time2 && !(time1.is_nan() && time2.is_nan()) {
                    return Ok(Some(false));
                }
                return Ok(None);
            }
            Ok(Some(time1 == time2))
        }
        JSType::RegExpObjectType => {
            if c2_type != JSType::RegExpObjectType {
                return Ok(Some(false));
            }
            let (Some(left), Some(right)) = (RegExpObject::from_cell_id(o1.cell_id()), RegExpObject::from_cell_id(o2.cell_id())) else { return Ok(Some(false)) };
            let (left, right) = (left.reg_exp(), right.reg_exp());
            // `regExp()->key()`: o par (flags, padrão).
            if left.flags() != right.flags() || left.pattern() != right.pattern() {
                return Ok(Some(false));
            }
            if mode.check_prototypes {
                // O Node compara também o `lastIndex` e as propriedades próprias.
                let (index1, index2) = (RegExpObject::from_cell_id(o1.cell_id()).map(|r| r.get_last_index()), RegExpObject::from_cell_id(o2.cell_id()).map(|r| r.get_last_index()));
                if !same_value(index1.unwrap_or(JSValue::undefined()), index2.unwrap_or(JSValue::undefined())) {
                    return Ok(Some(false));
                }
                return Ok(None);
            }
            Ok(Some(true))
        }
        JSType::ErrorInstanceType => {
            if c2_type != JSType::ErrorInstanceType {
                return Ok(Some(false));
            }
            errors_equal(global_object, mode, stack, o1, o2).map(Some)
        }
        JSType::Int8ArrayType
        | JSType::Uint8ArrayType
        | JSType::Uint8ClampedArrayType
        | JSType::Int16ArrayType
        | JSType::Uint16ArrayType
        | JSType::Int32ArrayType
        | JSType::Uint32ArrayType
        | JSType::Float16ArrayType
        | JSType::Float32ArrayType
        | JSType::Float64ArrayType
        | JSType::BigInt64ArrayType
        | JSType::BigUint64ArrayType => typed_arrays_equal(global_object, mode, stack, o1, o2),
        JSType::StringObjectType => {
            // A instância de subclasse de `String` é `DerivedStringObjectType`; só o modo que ignora o protótipo a aceita.
            if c2_type != JSType::StringObjectType && !(mode.strict && mode.skip_prototype_identity && c2_type == JSType::DerivedStringObjectType) {
                return Ok(Some(false));
            }
            if !mode.check_prototypes && !mode.skip_prototype_identity && calculated_class_name(o1) != calculated_class_name(o2) {
                return Ok(Some(false));
            }
            let (Some(left), Some(right)) = (StringObject::from_cell_id(o1.cell_id()), StringObject::from_cell_id(o2.cell_id())) else { return Ok(Some(false)) };
            if left.internal_value().value() != right.internal_value().value() {
                return Ok(Some(false));
            }
            if mode.strict && (has_extra_own_properties(&o1.structure()) || has_extra_own_properties(&o2.structure())) {
                return Ok(None);
            }
            Ok(Some(true))
        }
        JSType::JSFunctionType => Ok(Some(false)),
        // `globalThis` só é igual a si mesmo, e o mesmo objeto já saiu no `SameValue`.
        JSType::GlobalObjectType | JSType::GlobalProxyType => Ok(Some(false)),
        JSType::NumberObjectType | JSType::BooleanObjectType => {
            if c1_type != c2_type {
                return Ok(Some(false));
            }
            let internal = |object: &ObjectRef| JSWrapperObject::from_value_of_type(&object.as_value(), c1_type).map(|wrapper| wrapper.internal_value());
            if !same_value(internal(o1).unwrap_or(JSValue::undefined()), internal(o2).unwrap_or(JSValue::undefined())) {
                return Ok(Some(false));
            }
            Ok(None)
        }
        _ if is_dom_object(o1) && is_dom_object(o2) => Ok(dom_objects_equal(mode, o1, o2)),
        _ => {
            if let Some((is_symbol1, value1)) = wrapper_kind(o1) {
                match wrapper_kind(o2) {
                    Some((is_symbol2, value2)) if is_symbol1 == is_symbol2 && same_value(value1, value2) => {}
                    _ => return Ok(Some(false)),
                }
            }
            Ok(None)
        }
    }
}
