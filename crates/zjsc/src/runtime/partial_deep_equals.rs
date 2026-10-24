//! `assert.partialDeepStrictEqual` (o modo `kPartial` do Node), como o `jsFunctionPartialDeepStrictEqual` do
//! `NodeUtilTypesModule.cpp` do bun 1.4.2: `expected` tem de estar contido, recursivamente, em `actual`. Ciclos só
//! valem quando os dois lados voltam juntos.
//!
//! O ramo do `KeyObject` do bun não existe aqui: o porte não instala o `node:crypto`, e nenhum valor é um.

use crate::host_function;
use crate::runtime::array_constructor::{is_array, IsArrayCaller};
use crate::runtime::cell_registry;
use crate::runtime::date_instance::DateInstance;
use crate::runtime::deep_equals::{deep_equals, float_bits, own_property_names, root_pairs, wrapper_kind, Mode};
use crate::runtime::gc::RootScope;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::host_function_support::ObjectRef;
use crate::runtime::identifier::{parse_index_identifier, Identifier};
use crate::runtime::js_array_buffer::JSArrayBuffer;
use crate::runtime::js_data_view::JSDataView;
use crate::runtime::js_generic_typed_array_view::JSGenericTypedArrayView;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_map::JSMap;
use crate::runtime::js_set::JSSet;
use crate::runtime::js_type::{is_typed_array_type_including_data_view, JSType};
use crate::runtime::js_value::JSValue;
use crate::runtime::js_wrapper_object::JSWrapperObject;
use crate::runtime::node_validators::get_index;
use crate::runtime::object_prototype::object_prototype_to_string;
use crate::runtime::operations::{same_value, same_value_zero, strict_equal};
use crate::runtime::property_attribute::DONT_ENUM;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::{InternalMethodType, PropertySlot};
use crate::runtime::proxy_object::{object_get, own_property_slot};
use crate::runtime::reg_exp_object::RegExpObject;
use crate::runtime::string_object::StringObject;

/// Os objetos já em comparação de cada lado (`CycleState`).
#[derive(Default)]
struct Cycles {
    actual_seen: Vec<JSValue>,
    expected_seen: Vec<JSValue>,
}

type Body = fn(&JSGlobalObject, &mut Cycles, JSValue, JSValue) -> Result<bool, Thrown>;

fn cell_type(value: JSValue) -> Option<JSType> {
    match value {
        JSValue::Cell(cell_id) => cell_registry::cell_type(cell_id),
        _ => None,
    }
}

fn object_of(value: JSValue) -> Option<ObjectRef> {
    ObjectRef::from_value(&value).filter(|_| value.is_object())
}

/// `withCycleGuard`: o ciclo só é aceito quando os dois lados já estão na pilha.
fn with_cycle_guard(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue, body: Body) -> Result<bool, Thrown> {
    let had_actual = cycles.actual_seen.iter().any(|seen| strict_equal(*seen, actual));
    let had_expected = cycles.expected_seen.iter().any(|seen| strict_equal(*seen, expected));
    if had_actual && had_expected {
        return Ok(true);
    }
    if had_actual || had_expected {
        return Ok(false);
    }
    cycles.actual_seen.push(actual);
    cycles.expected_seen.push(expected);
    let result = body(global_object, cycles, actual, expected);
    cycles.actual_seen.pop();
    cycles.expected_seen.pop();
    result
}

/// `actual[key]` só se a chave é própria e enumerável em `actual`; o valor é lido pelo `get` comum dos dois lados.
fn own_enumerable_then_compare(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: &ObjectRef, expected: &ObjectRef, name: &Identifier) -> Result<bool, Thrown> {
    let property_name = PropertyName::from_identifier(name);
    let mut slot = PropertySlot::new(actual.as_value(), InternalMethodType::GetOwnProperty);
    if !own_property_slot(global_object, actual.as_value(), &property_name, &mut slot)? || slot.attributes() & DONT_ENUM != 0 {
        return Ok(false);
    }
    let actual_value = object_get(global_object, actual, &property_name, actual.as_value())?;
    let _actual_value = RootScope::new(&[actual_value]);
    let expected_value = object_get(global_object, expected, &property_name, expected.as_value())?;
    let _expected_value = RootScope::new(&[expected_value]);
    compare_branch(global_object, cycles, actual_value, expected_value)
}

/// As chaves próprias enumeráveis de `expected` (`index_keys` falso tira as de índice, que a comparação de conteúdo já
/// cobriu).
fn subset_of_keys(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue, index_keys: bool) -> Result<bool, Thrown> {
    let (Some(actual), Some(expected)) = (object_of(actual), object_of(expected)) else { return Ok(false) };
    let names = own_property_names(global_object, &expected)?;
    for name in names.iter().filter(|name| index_keys || name.is_symbol() || parse_index_identifier(name).is_none()) {
        if !own_enumerable_then_compare(global_object, cycles, &actual, &expected, name)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `objectSubset`.
fn object_subset(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    subset_of_keys(global_object, cycles, actual, expected, true)
}

/// `nonIndexObjectSubset`.
fn non_index_object_subset(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    subset_of_keys(global_object, cycles, actual, expected, false)
}

/// `toLength(object.length)`.
fn length_of(global_object: &JSGlobalObject, object: JSValue) -> Result<u64, Thrown> {
    let object = object.as_object();
    let length = object_get(global_object, &object, &PropertyName::from_identifier(&global_object.vm().property_names.length), object.as_value())?;
    let length = length.to_length();
    pending_or(global_object, length)
}

/// `arraySubsequence`: os elementos de `expected` em ordem, com lacunas, dentro dos de `actual`; depois as chaves que
/// não são índice.
fn array_subsequence(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    let actual_length = length_of(global_object, actual)?;
    let expected_length = length_of(global_object, expected)?;
    if expected_length > actual_length {
        return Ok(false);
    }
    let mut actual_position = 0u64;
    for i in 0..expected_length {
        let last_candidate = actual_length - expected_length + i;
        let expected_element = get_index(global_object, expected, i as u32)?;
        let _expected_element = RootScope::new(&[expected_element]);
        let mut matched = false;
        while actual_position <= last_candidate {
            let actual_element = get_index(global_object, actual, actual_position as u32)?;
            let _actual_element = RootScope::new(&[actual_element]);
            let equal = compare_branch(global_object, cycles, actual_element, expected_element)?;
            actual_position += 1;
            if equal {
                matched = true;
                break;
            }
        }
        if !matched {
            return Ok(false);
        }
    }
    non_index_object_subset(global_object, cycles, actual, expected)
}

fn map_entries(map: &JSMap) -> Vec<(JSValue, JSValue)> {
    let cursor = map.table().borrow_mut().new_cursor();
    std::iter::from_fn(|| map.table().borrow().next_entry(&cursor)).collect()
}

fn set_items(set: &JSSet) -> Vec<JSValue> {
    let cursor = set.table().borrow_mut().new_cursor();
    std::iter::from_fn(|| set.table().borrow().next_entry(&cursor)).map(|(key, _)| key).collect()
}

/// `mapSubset`: a chave idêntica primeiro (reservando a entrada para não ser usada duas vezes); chave objeto procura
/// uma entrada de `actual` com chave e valor parcialmente iguais.
fn map_subset(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    let (Some(actual_map), Some(expected_map)) = ((match actual { JSValue::Cell(id) => JSMap::from_cell_id(id), _ => None }), (match expected { JSValue::Cell(id) => JSMap::from_cell_id(id), _ => None })) else { return Ok(false) };
    // As entradas de `actual`, materializadas só quando preciso.
    let mut entries: Option<(Vec<(JSValue, JSValue)>, RootScope)> = None;
    let mut used_indices: Vec<bool> = Vec::new();
    let mut used_identity_keys: Vec<JSValue> = Vec::new();
    let expected_entries = map_entries(&expected_map);
    let _expected_entries = root_pairs(&expected_entries);
    for (expected_key, expected_value) in expected_entries {
        let mut consumed = false;
        let identity_present = actual_map.table().borrow().has(expected_key);
        let key_already_used = used_identity_keys.iter().any(|key| same_value(*key, expected_key));
        if identity_present && !key_already_used {
            let identity_index = entries.as_ref().and_then(|(list, _)| list.iter().position(|(key, _)| same_value_zero(*key, expected_key)));
            if identity_index.is_none_or(|index| !used_indices[index]) {
                let actual_value = actual_map.table().borrow().get(expected_key);
                if compare_branch(global_object, cycles, actual_value, expected_value)? {
                    used_identity_keys.push(expected_key);
                    if let Some(index) = identity_index {
                        used_indices[index] = true;
                    }
                    consumed = true;
                }
            }
        }
        if consumed {
            continue;
        }
        if !expected_key.is_object() {
            return Ok(false);
        }
        let (list, _) = entries.get_or_insert_with(|| {
            let list = map_entries(&actual_map);
            used_indices = vec![false; list.len()];
            let roots = root_pairs(&list);
            (list, roots)
        });
        let mut matched = false;
        for (index, (candidate_key, candidate_value)) in list.iter().enumerate() {
            if used_indices[index] || used_identity_keys.iter().any(|key| same_value(*key, *candidate_key)) {
                continue;
            }
            if !compare_branch(global_object, cycles, *candidate_key, expected_key)? {
                continue;
            }
            if compare_branch(global_object, cycles, *candidate_value, expected_value)? {
                used_indices[index] = true;
                matched = true;
                break;
            }
        }
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `setSubset`: cada item de `expected` casa com um item ainda livre de `actual` pela própria comparação parcial.
fn set_subset(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    let (Some(actual_set), Some(expected_set)) = ((match actual { JSValue::Cell(id) => JSSet::from_cell_id(id), _ => None }), (match expected { JSValue::Cell(id) => JSSet::from_cell_id(id), _ => None })) else { return Ok(false) };
    let items = set_items(&actual_set);
    let _items = RootScope::new(&items);
    let mut used_indices = vec![false; items.len()];
    let expected_items = set_items(&expected_set);
    let _expected_items = RootScope::new(&expected_items);
    for expected_item in expected_items {
        let mut matched = false;
        for (index, item) in items.iter().enumerate() {
            if used_indices[index] {
                continue;
            }
            if compare_branch(global_object, cycles, *item, expected_item)? {
                used_indices[index] = true;
                matched = true;
                break;
            }
        }
        if !matched {
            return Ok(false);
        }
    }
    Ok(true)
}

fn map_subset_and_props(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    Ok(map_subset(global_object, cycles, actual, expected)? && object_subset(global_object, cycles, actual, expected)?)
}

fn set_subset_and_props(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    Ok(set_subset(global_object, cycles, actual, expected)? && object_subset(global_object, cycles, actual, expected)?)
}

/// `errorSubset`: `message`, `name` e `errors` só se `expected` os tem (mensagem vazia não conta); `cause` própria de
/// `expected` tem de existir em `actual`; depois as chaves de `expected`.
fn error_subset(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    let (Some(actual_object), Some(expected_object)) = (object_of(actual), object_of(expected)) else { return Ok(false) };
    let vm = global_object.vm();
    let keys = [vm.property_names.message.clone(), vm.property_names.name.clone(), Identifier::from_span(vm, b"errors")];
    for (index, key) in keys.iter().enumerate() {
        let name = PropertyName::from_identifier(key);
        let expected_value = object_get(global_object, &expected_object, &name, expected)?;
        if expected_value.is_undefined() {
            continue;
        }
        if index == 0 && expected_value.is_string() && expected_value.as_js_string().length() == 0 {
            continue;
        }
        let _expected_value = RootScope::new(&[expected_value]);
        let actual_value = object_get(global_object, &actual_object, &name, actual)?;
        let _actual_value = RootScope::new(&[actual_value]);
        if !compare_branch(global_object, cycles, actual_value, expected_value)? {
            return Ok(false);
        }
    }
    let cause = PropertyName::from_identifier(&Identifier::from_span(vm, b"cause"));
    let has_own = |value: JSValue| -> Result<bool, Thrown> {
        let mut slot = PropertySlot::new(value, InternalMethodType::GetOwnProperty);
        own_property_slot(global_object, value, &cause, &mut slot)
    };
    if has_own(expected)? {
        if !has_own(actual)? {
            return Ok(false);
        }
        let actual_cause = object_get(global_object, &actual_object, &cause, actual)?;
        let _actual_cause = RootScope::new(&[actual_cause]);
        let expected_cause = object_get(global_object, &expected_object, &cause, expected)?;
        let _expected_cause = RootScope::new(&[expected_cause]);
        if !compare_branch(global_object, cycles, actual_cause, expected_cause)? {
            return Ok(false);
        }
    }
    object_subset(global_object, cycles, actual, expected)
}

/// A largura do elemento de cada visão e se ele é de ponto flutuante (a `DataView` e o `ArrayBuffer` são bytes).
fn element_of(kind: Option<JSType>) -> (usize, bool) {
    match kind {
        Some(JSType::Int16ArrayType | JSType::Uint16ArrayType) => (2, false),
        Some(JSType::Int32ArrayType | JSType::Uint32ArrayType) => (4, false),
        Some(JSType::BigInt64ArrayType | JSType::BigUint64ArrayType) => (8, false),
        Some(JSType::Float16ArrayType) => (2, true),
        Some(JSType::Float32ArrayType) => (4, true),
        Some(JSType::Float64ArrayType) => (8, true),
        _ => (1, false),
    }
}

/// Os bytes de uma visão ou de um `ArrayBuffer` (`byteSpanOf`): `Err` se o buffer foi desligado.
fn bytes_of(value: JSValue) -> Result<Vec<u8>, Thrown> {
    let detached = || Thrown::type_error("Cannot perform Construct on a detached ArrayBuffer");
    let JSValue::Cell(cell_id) = value else { return Ok(Vec::new()) };
    if let Some(view) = JSGenericTypedArrayView::from_cell_id(cell_id) {
        if view.is_detached() {
            return Err(detached());
        }
        return Ok(view.with_view_bytes(<[u8]>::to_vec));
    }
    if let Some(view) = JSDataView::from_cell_id(cell_id) {
        if view.is_detached() {
            return Err(detached());
        }
        let (offset, length) = (view.byte_offset_raw(), view.view_byte_length().unwrap_or(0));
        return Ok(view.possibly_shared_buffer().with_bytes(|bytes| bytes.get(offset..offset + length).unwrap_or(&[]).to_vec()));
    }
    if let Some(buffer) = JSArrayBuffer::from_cell_id(cell_id) {
        let buffer = buffer.impl_();
        if buffer.is_detached() {
            return Err(detached());
        }
        return Ok(buffer.with_bytes(<[u8]>::to_vec));
    }
    Ok(Vec::new())
}

/// `partialBufferContentsEquiv`: os elementos de `expected` como subsequência, com lacunas, dos de `actual`, com a
/// igualdade do `Object.is` na largura do elemento.
fn partial_buffer_contents_equiv(actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    let (actual_bytes, expected_bytes) = (bytes_of(actual)?, bytes_of(expected)?);
    let (width, is_float) = element_of(cell_type(actual));
    let actual_elements: Vec<&[u8]> = actual_bytes.chunks_exact(width).collect();
    let expected_elements: Vec<&[u8]> = expected_bytes.chunks_exact(width).collect();
    let equal = |a: &[u8], b: &[u8]| {
        let (a_nan, b_nan) = (is_float && float_bits(a).0, is_float && float_bits(b).0);
        if a_nan || b_nan { a_nan && b_nan } else { a == b }
    };
    if expected_elements.len() > actual_elements.len() {
        return Ok(false);
    }
    let mut position = 0;
    for (i, expected_element) in expected_elements.iter().enumerate() {
        let last_candidate = actual_elements.len() - expected_elements.len() + i;
        while position <= last_candidate && !equal(actual_elements[position], expected_element) {
            position += 1;
        }
        if position > last_candidate {
            return Ok(false);
        }
        position += 1;
    }
    Ok(true)
}

fn object_tag(global_object: &JSGlobalObject, value: JSValue) -> Result<crate::wtf::text::wtf_string::String, Thrown> {
    Ok(object_prototype_to_string(global_object, value).ok_or(Thrown::Pending)?.value())
}

/// O valor interno dos invólucros de primitivo (`Number`, `Boolean`, `String`, `Symbol`, `BigInt`).
fn boxed_internal(value: JSValue) -> Option<JSValue> {
    match cell_type(value)? {
        kind @ (JSType::NumberObjectType | JSType::BooleanObjectType) => JSWrapperObject::from_value_of_type(&value, kind).map(|wrapper| wrapper.internal_value()),
        JSType::StringObjectType | JSType::DerivedStringObjectType => (match value { JSValue::Cell(id) => StringObject::from_cell_id(id), _ => None }).map(|object| JSValue::from_js_string(object.internal_value())),
        _ => wrapper_kind(&object_of(value)?).map(|(_, internal)| internal),
    }
}

/// Os dois lados do mesmo tipo, ou nenhum: `Some(false)` quando só um é.
fn both(actual: bool, expected: bool) -> Option<bool> {
    (actual || expected).then_some(actual && expected)
}

fn compare_branch(global_object: &JSGlobalObject, cycles: &mut Cycles, actual: JSValue, expected: JSValue) -> Result<bool, Thrown> {
    if !global_object.vm().is_safe_to_recurse() {
        return Err(Thrown::StackOverflow);
    }
    if same_value_zero(actual, expected) {
        // O `===` do JS menos `+0` contra `-0`, que o `Object.is` recusa.
        if actual.is_number() && actual.as_number() == 0.0 {
            return Ok(actual.as_number().is_sign_negative() == expected.as_number().is_sign_negative());
        }
        return Ok(true);
    }
    let (actual_type, expected_type) = (cell_type(actual), cell_type(expected));
    let never = |kind: Option<JSType>| matches!(kind, Some(JSType::JSWeakSetType | JSType::JSWeakMapType | JSType::JSPromiseType));
    if never(actual_type) || never(expected_type) {
        return Ok(false);
    }

    if actual_type == Some(JSType::JSMapType) && expected_type == Some(JSType::JSMapType) {
        let size = |value: JSValue| (match value { JSValue::Cell(id) => JSMap::from_cell_id(id), _ => None }).map_or(0, |map| map.table().borrow().size());
        if size(expected) > size(actual) {
            return Ok(false);
        }
        return with_cycle_guard(global_object, cycles, actual, expected, map_subset_and_props);
    }

    let is_view = |kind: Option<JSType>| kind.is_some_and(is_typed_array_type_including_data_view);
    if let Some(same) = both(is_view(actual_type), is_view(expected_type)) {
        if !same || actual_type != expected_type || !partial_buffer_contents_equiv(actual, expected)? {
            return Ok(false);
        }
        let body: Body = if actual_type == Some(JSType::DataViewType) { object_subset } else { non_index_object_subset };
        return with_cycle_guard(global_object, cycles, actual, expected, body);
    }

    let is_buffer = |kind: Option<JSType>| kind == Some(JSType::ArrayBufferType);
    if let Some(same) = both(is_buffer(actual_type), is_buffer(expected_type)) {
        if !same || object_tag(global_object, actual)? != object_tag(global_object, expected)? || !partial_buffer_contents_equiv(actual, expected)? {
            return Ok(false);
        }
        return with_cycle_guard(global_object, cycles, actual, expected, object_subset);
    }

    let (actual_url, expected_url) = (crate::runtime::url::url_of(actual), crate::runtime::url::url_of(expected));
    if let Some(same) = both(actual_url.is_some(), expected_url.is_some()) {
        if !same || actual_url.map(|url| url.string().to_owned()) != expected_url.map(|url| url.string().to_owned()) {
            return Ok(false);
        }
        return with_cycle_guard(global_object, cycles, actual, expected, object_subset);
    }

    let is_error = |kind: Option<JSType>| kind == Some(JSType::ErrorInstanceType);
    if let Some(same) = both(is_error(actual_type), is_error(expected_type)) {
        if !same {
            return Ok(false);
        }
        return with_cycle_guard(global_object, cycles, actual, expected, error_subset);
    }

    if actual_type == Some(JSType::JSSetType) && expected_type == Some(JSType::JSSetType) {
        let size = |value: JSValue| (match value { JSValue::Cell(id) => JSSet::from_cell_id(id), _ => None }).map_or(0, |set| set.table().borrow().size());
        if size(expected) > size(actual) {
            return Ok(false);
        }
        return with_cycle_guard(global_object, cycles, actual, expected, set_subset_and_props);
    }

    let (actual_date, expected_date) = (DateInstance::from_value(&actual), DateInstance::from_value(&expected));
    if let Some(same) = both(actual_date.is_some(), expected_date.is_some()) {
        let (Some(actual_date), Some(expected_date)) = (actual_date.filter(|_| same), expected_date) else { return Ok(false) };
        let (actual_time, expected_time) = (actual_date.internal_number(), expected_date.internal_number());
        if actual_time != expected_time && !(actual_time.is_nan() && expected_time.is_nan()) {
            return Ok(false);
        }
        return with_cycle_guard(global_object, cycles, actual, expected, object_subset);
    }

    let regexp_of = |kind: Option<JSType>, value: JSValue| match value {
        JSValue::Cell(cell_id) if kind == Some(JSType::RegExpObjectType) => RegExpObject::from_cell_id(cell_id),
        _ => None,
    };
    let (actual_regexp, expected_regexp) = (regexp_of(actual_type, actual), regexp_of(expected_type, expected));
    if let Some(same) = both(actual_regexp.is_some(), expected_regexp.is_some()) {
        let (Some(actual_regexp), Some(expected_regexp)) = (actual_regexp.filter(|_| same), expected_regexp) else { return Ok(false) };
        let (left, right) = (actual_regexp.reg_exp(), expected_regexp.reg_exp());
        if left.pattern() != right.pattern() || left.flags() != right.flags() || !same_value(actual_regexp.get_last_index(), expected_regexp.get_last_index()) {
            return Ok(false);
        }
        return with_cycle_guard(global_object, cycles, actual, expected, object_subset);
    }

    let (actual_boxed, expected_boxed) = (boxed_internal(actual), boxed_internal(expected));
    if let Some(same) = both(actual_boxed.is_some(), expected_boxed.is_some()) {
        if !same || !same_value(actual_boxed.unwrap_or(JSValue::undefined()), expected_boxed.unwrap_or(JSValue::undefined())) {
            return Ok(false);
        }
        return with_cycle_guard(global_object, cycles, actual, expected, object_subset);
    }

    let actual_is_array = is_array(&actual, IsArrayCaller::ArrayIsArray).map_err(Thrown::from)?;
    let expected_is_array = is_array(&expected, IsArrayCaller::ArrayIsArray).map_err(Thrown::from)?;
    if actual_is_array != expected_is_array {
        return Ok(false);
    }
    if actual_is_array {
        return with_cycle_guard(global_object, cycles, actual, expected, array_subsequence);
    }

    // Primitivo, `null` ou função de algum lado: a igualdade profunda estrita inteira decide.
    let is_special = |value: JSValue| !value.is_object() || value.is_callable();
    if is_special(actual) || is_special(expected) {
        return deep_equals(global_object, Mode { strict: true, skip_prototype_identity: false, check_prototypes: false }, actual, expected);
    }
    if object_tag(global_object, actual)? != object_tag(global_object, expected)? {
        return Ok(false);
    }
    with_cycle_guard(global_object, cycles, actual, expected, object_subset)
}

/// `jsFunctionPartialDeepStrictEqual(actual, expected)`.
fn partial_deep_strict_equal_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let result = compare_branch(global_object, &mut Cycles::default(), call.argument(0), call.argument(1))?;
    Ok(JSValue::Bool(result))
}
host_function!(pub partial_deep_strict_equal, partial_deep_strict_equal_body);
