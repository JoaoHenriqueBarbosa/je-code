//! As funções de `runtime/JSGlobalObjectInlines.h` que recebem um `ArrayAllocationProfile`:
//! `arrayStructureForProfileDuringAllocation`, `constructEmptyArray`, `constructArray` e
//! `constructArrayNegativeIndexed` com perfil, e o `arrayStructureForIndexingTypeDuringAllocation` com
//! `newTarget`.
//!
//! Divergências:
//!
//! - Os chamadores com perfil do LLInt e do JIT nunca passam `newTarget`: eles usam as funções sem o
//!   sufixo `_with_new_target`, que são as de `newTarget` vazio (o argumento padrão do C++). Quem tem
//!   `new.target` (o construtor `Array`) usa as `_with_new_target`.
//! - `newTarget == globalObject->arrayConstructor()` (o global não guarda o construtor) é "o valor é o
//!   `Array` de algum realm" (`is_array_constructor`): a estrutura sai direto do realm dele, que é o que
//!   o caminho de `getFunctionRealm` + `createSubclassStructure` devolveria para o `prototype` intocável
//!   (`ReadOnly`) do `Array`.
//! - `JSArray*` do C++ (o retorno de `updateLastAllocationFor`) é o `JSArray` do porte, e o que o perfil
//!   guarda é o `cell_id` dele (`HeapRef`).
//! - O C++ lança `OutOfMemoryError` pelo `ThrowScope`; aqui o erro sai como
//!   `ArrayError::Put(PutError::OutOfMemory)`, que o chamador converte na exceção. O erro do
//!   `getFunctionRealm` (`Thrown`) é lançado na hora e sai como `PutError::Pending`.

use crate::bytecode::array_allocation_profile::{HeapArrays, MIN_ARRAY_STORAGE_CONSTRUCTION_LENGTH};
use crate::bytecode::op_metadata::{ArrayAllocationProfile, HeapRef};
use crate::runtime::array_constructor::is_array_constructor;
use crate::runtime::host_call::throw_thrown;
use crate::runtime::indexing_type::{IndexingType, ARRAY_WITH_ARRAY_STORAGE};
use crate::runtime::internal_function::{get_function_realm, InternalFunction};
use crate::runtime::js_array::{ArrayError, JSArray};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::PutError;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `arrayStructureForIndexingTypeDuringAllocation(globalObject, indexingType, newTarget)`: vazio (ou
/// `undefined`, a chamada sem `new`) e o `Array` usam a estrutura de alocação direta; qualquer outro
/// `newTarget` lê a do `getFunctionRealm(newTarget)` e deriva dela com o `prototype` do `newTarget`.
pub fn array_structure_for_indexing_type_during_allocation_with_new_target(
    global_object: &JSGlobalObject,
    indexing_type: IndexingType,
    new_target: JSValue,
) -> Result<StructureRef, ArrayError> {
    if new_target.is_empty() || new_target.is_undefined() {
        return Ok(global_object.array_structure_for_indexing_type_during_allocation(indexing_type));
    }
    if is_array_constructor(&new_target) {
        let constructor = InternalFunction::from_cell_id(new_target.as_cell()).expect("is_array_constructor confere o InternalFunction");
        return Ok(constructor.global_object().array_structure_for_indexing_type_during_allocation(indexing_type));
    }
    let function_global_object = match get_function_realm(new_target) {
        Ok(realm) => realm,
        Err(thrown) => {
            throw_thrown(global_object, thrown);
            return Err(ArrayError::Put(PutError::Pending));
        }
    };
    let new_target_object = crate::runtime::host_function_support::ObjectRef::from_value(&new_target).expect("asObject(newTarget)");
    match InternalFunction::create_subclass_structure(
        global_object,
        &new_target_object,
        function_global_object.array_structure_for_indexing_type_during_allocation(indexing_type),
    ) {
        Ok(structure) => Ok(structure),
        Err(thrown) => {
            throw_thrown(global_object, thrown);
            Err(ArrayError::Put(PutError::Pending))
        }
    }
}

/// `arrayStructureForProfileDuringAllocation(globalObject, profile, newTarget)`:
/// `arrayStructureForIndexingTypeDuringAllocation(ArrayAllocationProfile::selectIndexingTypeFor(profile), newTarget)`.
pub fn array_structure_for_profile_during_allocation_with_new_target(
    global_object: &JSGlobalObject,
    profile: Option<&mut ArrayAllocationProfile>,
    new_target: JSValue,
) -> Result<StructureRef, ArrayError> {
    array_structure_for_indexing_type_during_allocation_with_new_target(
        global_object,
        ArrayAllocationProfile::select_indexing_type_for(profile, &HeapArrays),
        new_target,
    )
}

/// O `HeapRef` que o perfil guarda para o array (o `JSArray*` do C++).
pub fn heap_ref(array: &JSArray) -> HeapRef {
    HeapRef::try_from(array.cell_id()).expect("cell_id de JSArray cabe em u32")
}

/// `constructEmptyArray(globalObject, profile, initialLength, newTarget)`.
pub fn construct_empty_array_with_new_target(
    vm: &VM,
    global_object: &JSGlobalObject,
    mut profile: Option<&mut ArrayAllocationProfile>,
    initial_length: u32,
    new_target: JSValue,
) -> Result<JSArray, ArrayError> {
    let structure = if initial_length >= MIN_ARRAY_STORAGE_CONSTRUCTION_LENGTH {
        array_structure_for_indexing_type_during_allocation_with_new_target(global_object, ARRAY_WITH_ARRAY_STORAGE, new_target)?
    } else {
        array_structure_for_profile_during_allocation_with_new_target(
            global_object,
            profile.as_mut().map(|profile| &mut **profile),
            new_target,
        )?
    };
    let result = JSArray::try_create(vm, &structure, initial_length).ok_or(ArrayError::Put(PutError::OutOfMemory))?;
    ArrayAllocationProfile::update_last_allocation_for(profile, heap_ref(&result));
    Ok(result)
}

/// `constructEmptyArray(globalObject, profile, initialLength)`: `newTarget` vazio.
pub fn construct_empty_array(
    vm: &VM,
    global_object: &JSGlobalObject,
    profile: Option<&mut ArrayAllocationProfile>,
    initial_length: u32,
) -> Result<JSArray, ArrayError> {
    construct_empty_array_with_new_target(vm, global_object, profile, initial_length, JSValue::empty())
}

/// `constructArray(globalObject, profile, values, length, newTarget)` e `constructArrayNegativeIndexed(
/// globalObject, profile, values, length, newTarget)`: `values` já está na ordem dos índices (o chamador
/// lê `values[-i]` do frame). Falta de memória (`tryCreateUninitializedRestricted` nulo) é o
/// `OutOfMemoryError`.
pub fn construct_array_negative_indexed_with_new_target(
    vm: &VM,
    global_object: &JSGlobalObject,
    mut profile: Option<&mut ArrayAllocationProfile>,
    values: &[JSValue],
    new_target: JSValue,
) -> Result<JSArray, ArrayError> {
    let structure = array_structure_for_profile_during_allocation_with_new_target(
        global_object,
        profile.as_mut().map(|profile| &mut **profile),
        new_target,
    )?;
    let length = u32::try_from(values.len()).map_err(|_| ArrayError::Put(PutError::OutOfMemory))?;
    let array = JSArray::try_create_uninitialized_restricted(vm, &structure, length)
        .ok_or(ArrayError::Put(PutError::OutOfMemory))?;
    for (index, value) in values.iter().enumerate() {
        array.initialize_index(vm, index as u32, *value);
    }
    ArrayAllocationProfile::update_last_allocation_for(profile, heap_ref(&array));
    Ok(array)
}

/// `constructArrayNegativeIndexed(globalObject, profile, values, length)`: `newTarget` vazio.
pub fn construct_array_negative_indexed(
    vm: &VM,
    global_object: &JSGlobalObject,
    profile: Option<&mut ArrayAllocationProfile>,
    values: &[JSValue],
) -> Result<JSArray, ArrayError> {
    construct_array_negative_indexed_with_new_target(vm, global_object, profile, values, JSValue::empty())
}

