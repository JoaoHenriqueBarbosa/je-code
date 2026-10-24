//! Porte de `runtime/RegExpMatchesArray.h` e `RegExpMatchesArray.cpp`: o array que `exec` devolve
//! (`[casamento, grupos...]` com `index`, `input`, `groups` e, com a flag `d`, `indices`).
//!
//! DIVERGÊNCIAS (heap ausente, camada 3):
//!
//! - As `Structure`s pré-montadas (`regExpMatchesArrayStructure`, `...WithIndicesStructure`,
//!   `...IndicesArrayStructure` e as `SlowPut`) e os `PropertyOffset` fixos
//!   (`RegExpMatchesArrayIndexPropertyOffset` e companhia) são otimização de layout para o JIT.
//!   Aqui o array nasce da `arrayStructure()` e as propriedades entram por `putDirect` na mesma
//!   ordem (`index`, `input`, `groups`, `indices`), com atributos zero, o que dá a mesma ordem de
//!   chaves e os mesmos descritores observáveis. `tryCreateUninitializedRegExpMatchesArray`,
//!   `createRegExpMatchesArrayForPlainRegExpHavingABadTime` e `isHavingABadTime` somem com isso (a
//!   forma do array é decidida pelo `JSArray`).
//! - `ensureGroupsStructure` (a `Structure` do objeto `groups` guardada no `RegExp`) ainda não
//!   existe: o objeto `groups` nasce de uma `Structure` de protótipo nulo e recebe as propriedades
//!   por `putDirect`, o caminho que o C++ usa quando `groupsStructure` é nulo.
//! - O casamento não lança: o `Option` de retorno é o `nullptr` sem exceção do C++ (sem casamento).

use crate::runtime::identifier::Identifier;
use crate::runtime::js_array::{construct_array, JSArray};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSFinalObject, JSObjectRef};
use crate::runtime::js_string::{js_empty_string, js_substring, JSStringRef};
use crate::runtime::js_value::{js_null, js_number_i32, js_undefined, JSValue};
use crate::runtime::match_result::MatchResult;
use crate::runtime::property_name::PropertyName;
use crate::runtime::reg_exp::RegExpRef;
use crate::wtf::text::string_view::StringView;

/// O `JSValue` de um trecho `[start, end)` do `input`, ou `undefined` se o grupo não participou
/// (`start >= 0 && end >= start`).
fn substring_or_undefined(global_object: &JSGlobalObject, input: &JSStringRef, start: i32, end: i32) -> JSValue {
    if start >= 0 && end >= start {
        return JSValue::from_js_string(js_substring(global_object.vm(), input, start as u32, (end - start) as u32));
    }
    js_undefined()
}

/// `createIndexArray` do `createRegExpMatchesArrayWithGroupsOrIndices`: o par `[start, end]`.
fn create_index_array(global_object: &JSGlobalObject, start: i32, end: i32) -> JSValue {
    let pair = construct_array(global_object.vm(), &global_object.array_structure(), &[js_number_i32(start), js_number_i32(end)]);
    pair.as_value()
}

/// Um objeto de protótipo nulo para o `groups` (e o `groups` do `indices`).
fn create_groups_object(global_object: &JSGlobalObject) -> JSObjectRef {
    let vm = global_object.vm();
    let structure =
        JSFinalObject::create_structure(vm, Some(global_object), js_null(), JSFinalObject::DEFAULT_INLINE_CAPACITY);
    JSFinalObject::create(vm, &structure)
}

/// `createEmptyRegExpMatchesArray(globalObject, input, regExp)`: o array do "último casamento" sem
/// casamento (`[""]` mais `undefined` por grupo, `index` -1).
pub fn create_empty_reg_exp_matches_array(global_object: &JSGlobalObject, input: &JSStringRef, reg_exp: &RegExpRef) -> JSArray {
    let vm = global_object.vm();
    let names = &vm.property_names;

    let mut values = vec![JSValue::from_js_string(js_empty_string(vm))];
    values.extend((0..reg_exp.num_subpatterns()).map(|_| js_undefined()));
    let array = construct_array(vm, &global_object.array_structure(), &values);

    array.put_direct(vm, &PropertyName::from_identifier(&names.index), js_number_i32(-1), 0);
    array.put_direct(vm, &PropertyName::from_identifier(&names.input), JSValue::from_js_string(input.clone()), 0);
    array.put_direct(vm, &PropertyName::from_identifier(&names.groups), js_undefined(), 0);
    if reg_exp.has_indices() {
        array.put_direct(vm, &PropertyName::from_identifier(&names.indices), js_undefined(), 0);
    }
    array
}

/// `createRegExpMatchesArray(vm, globalObject, input, inputValue, regExp, startOffset, result)`:
/// casa a partir de `start_offset` e monta o array; `None` é o `nullptr` (sem casamento, `result` vira
/// `MatchResult::failed()`). Não grava no `RegExpGlobalData`: quem chama (`execInline`,
/// `RegExpCachedResult::lastResult`) decide.
pub fn create_reg_exp_matches_array(
    global_object: &JSGlobalObject,
    input: &JSStringRef,
    reg_exp: &RegExpRef,
    start_offset: u32,
) -> Option<(JSArray, MatchResult)> {
    let vm = global_object.vm();
    let names = &vm.property_names;

    let input_value = input.value();
    let subpattern_results = reg_exp.match_ovector(StringView::from(&input_value), start_offset)?;
    let result = MatchResult::new(subpattern_results[0] as usize, subpattern_results[1] as usize);
    assert!(result.end >= result.start);

    let num_subpatterns = reg_exp.num_subpatterns();
    let has_named_captures = reg_exp.has_named_captures();
    let create_indices = reg_exp.has_indices();

    let mut values = Vec::with_capacity(num_subpatterns as usize + 1);
    values.push(substring_or_undefined(global_object, input, result.start as i32, result.end as i32));
    for i in 1..=num_subpatterns as usize {
        values.push(substring_or_undefined(global_object, input, subpattern_results[2 * i], subpattern_results[2 * i + 1]));
    }
    let array = construct_array(vm, &global_object.array_structure(), &values);

    let indices_array = create_indices.then(|| {
        let index_values: Vec<JSValue> = (0..=num_subpatterns as usize)
            .map(|i| {
                let (start, end) = (subpattern_results[2 * i], subpattern_results[2 * i + 1]);
                if start >= 0 && end >= start { create_index_array(global_object, start, end) } else { js_undefined() }
            })
            .collect();
        construct_array(vm, &global_object.array_structure(), &index_values)
    });

    let groups = has_named_captures.then(|| create_groups_object(global_object));
    let indices_groups = (has_named_captures && create_indices).then(|| create_groups_object(global_object));
    if let Some(groups) = &groups {
        let has_duplicate_named_capture_groups = reg_exp.has_duplicate_named_capture_groups();
        for i in 1..=num_subpatterns {
            let group_name = reg_exp.get_capture_group_name_for_subpattern_id(i);
            if group_name.is_empty() {
                continue;
            }
            let mut capture_index = i;
            if has_duplicate_named_capture_groups {
                capture_index = reg_exp.subpattern_id_for_group_name(group_name.string(), &subpattern_results);
            }
            let value = if capture_index > 0 { array.get_by_index(vm, capture_index) } else { js_undefined() };
            let ident = PropertyName::from_identifier(&Identifier::from_string(vm, group_name.string()));
            groups.put_direct(vm, &ident, value, 0);
            if let (Some(indices_groups), Some(indices_array)) = (&indices_groups, &indices_array) {
                let indices_value = if capture_index > 0 { indices_array.get_by_index(vm, capture_index) } else { js_undefined() };
                indices_groups.put_direct(vm, &ident, indices_value, 0);
            }
        }
    }

    array.put_direct(vm, &PropertyName::from_identifier(&names.index), JSValue::from_u32(result.start as u32), 0);
    array.put_direct(vm, &PropertyName::from_identifier(&names.input), JSValue::from_js_string(input.clone()), 0);
    array.put_direct(vm, &PropertyName::from_identifier(&names.groups), groups.map_or(js_undefined(), |g| g.as_value()), 0);
    if let Some(indices_array) = indices_array {
        indices_array.put_direct(
            vm,
            &PropertyName::from_identifier(&names.groups),
            indices_groups.map_or(js_undefined(), |g| g.as_value()),
            0,
        );
        array.put_direct(vm, &PropertyName::from_identifier(&names.indices), indices_array.as_value(), 0);
    }
    Some((array, result))
}
