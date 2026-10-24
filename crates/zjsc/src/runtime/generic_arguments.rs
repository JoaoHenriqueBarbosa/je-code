//! Porte de `runtime/GenericArgumentsImpl.h` e `GenericArgumentsImplInlines.h`: o que `DirectArguments` e
//! `ScopedArguments` têm em comum, os índices do `arguments` mapeados para uma variável (ou para o
//! armazenamento do próprio objeto) e o `m_modifiedArgumentsDescriptor`.
//!
//! DIVERGÊNCIAS:
//!
//! - O `JSObject` do porte não tem tabela de métodos virtual. O `GenericArgumentsImpl<Type>` vira o trait
//!   [`MappedArguments`], e o `getOwnPropertySlotByIndex`, `putByIndex`, `deletePropertyByIndex`,
//!   `defineOwnProperty` de índice e os nomes de índice do `getOwnPropertyNames` são chamados por
//!   [`exotic_of`], consultado no começo dos métodos de `JSObject` e de `own_property_names` (o mesmo lugar
//!   onde o `JSGlobalProxy` e o `StringObject` se encaixam).
//! - A chamada a `Base::...` (o `JSObject` comum, que usa os elementos do próprio objeto) roda dentro de
//!   [`with_base`], que desliga o despacho para este objeto enquanto o corpo executa. É o papel da chamada
//!   não virtual do C++.
//! - `overrodeThings`, `overrideThings` e o ramo das três chaves (`length`, `callee`, `@@iterator`) em
//!   `getOwnPropertySlot`/`put`/`deleteProperty`/`defineOwnProperty`/`getOwnPropertyNames` existem, mas o
//!   `JSObject` não tem tabela virtual: os ganchos ficam em `js_arguments_objects.rs`
//!   (`special_own_slot`, `materialize_specials_for_property`, `special_property_names`), chamados de
//!   `js_object.rs` e `own_property_names.rs`.
//! - `initModifiedArgumentsDescriptor` não falha por falta de memória: o `Vec<bool>` vazio é o ponteiro
//!   nulo. `copyToArguments` e `visitChildren` não são portados (quem copia argumentos lê pelo `get`).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSObject, PutError};
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_slot::PropertySlot;
use crate::runtime::sparse_array_value_map::PutDirectIndexMode;
use crate::runtime::vm::VM;

/// O estado que o `GenericArgumentsImpl` guarda além do `Type`: `m_modifiedArgumentsDescriptor` (vazio é
/// nulo) e a marca de chamada à base.
#[derive(Debug, Default)]
pub struct ArgumentsState {
    modified: RefCell<Vec<bool>>,
    in_base: Cell<bool>,
}

/// Desliga o despacho do objeto durante a chamada a `Base::...`.
struct BaseCall<'a> {
    flag: &'a Cell<bool>,
    previous: bool,
}

impl<'a> BaseCall<'a> {
    fn enter(flag: &'a Cell<bool>) -> BaseCall<'a> {
        let previous = flag.replace(true);
        BaseCall { flag, previous }
    }
}

impl Drop for BaseCall<'_> {
    fn drop(&mut self) {
        self.flag.set(self.previous);
    }
}

/// O corpo de uma chamada a `Base::...` (o `JSObject` comum) de um dos dois objetos `arguments`.
fn with_base<R>(arguments: &dyn MappedArguments, body: impl FnOnce(&JSObject) -> R) -> R {
    let _guard = BaseCall::enter(&arguments.state().in_base);
    body(arguments.object())
}

/// `GenericArgumentsImpl<Type>`: o que o `Type` (`DirectArguments` ou `ScopedArguments`) dá ao mixin.
pub trait MappedArguments {
    /// O `JSObject` base.
    fn object(&self) -> &JSObject;
    /// O estado do mixin.
    fn state(&self) -> &ArgumentsState;
    /// `internalLength()`.
    fn internal_length(&self) -> u32;
    /// O comprimento da tabela do `m_modifiedArgumentsDescriptor`: `m_length` no `DirectArguments`,
    /// `m_table->length()` no `ScopedArguments`.
    fn modified_length(&self) -> u32;
    /// `isMappedArgument(i)`.
    fn is_mapped_argument(&self, index: u32) -> bool;
    /// `getIndexQuickly(i)`: só para índice mapeado.
    fn get_index_quickly(&self, index: u32) -> JSValue;
    /// `setIndexQuickly(vm, i, value)`: só para índice mapeado.
    fn set_index_quickly(&self, index: u32, value: JSValue);
    /// `unmapArgument(globalObject, i)`.
    fn unmap_argument(&self, index: u32) -> Result<(), PutError>;
    /// `callee()` (`m_callee`).
    fn callee(&self) -> JSValue;
    /// `overrodeThings()`: `length`, `callee` e `@@iterator` já são propriedades do objeto.
    fn overrode_things(&self) -> bool;
    /// `overrideThings(globalObject)`.
    fn override_things(&self, global_object: &JSGlobalObject) -> Result<(), PutError>;

    /// `overrideThingsIfNecessary(globalObject)`, com o realm do objeto.
    fn override_things_if_necessary(&self) -> Result<(), PutError> {
        if self.overrode_things() {
            return Ok(());
        }
        match self.object().structure().realm() {
            Some(realm) => self.override_things(&realm),
            None => Ok(()),
        }
    }
    /// `initModifiedArgumentsDescriptorIfNecessary` e `setModifiedArgumentDescriptor(globalObject, index, length)`.
    /// `OutOfMemory` é a falta de memória para a tabela de `length` posições.
    fn set_modified_argument_descriptor(&self, index: u32) -> Result<(), PutError> {
        let length = self.modified_length();
        let mut modified = self.state().modified.borrow_mut();
        if modified.is_empty() && length != 0 {
            *modified = crate::runtime::fallible_alloc::try_filled_vec(false, length as usize).ok_or(PutError::OutOfMemory)?;
        }
        if index < length {
            modified[index as usize] = true;
        }
        Ok(())
    }

    /// `isModifiedArgumentDescriptor(index, length)`.
    fn is_modified_argument_descriptor(&self, index: u32) -> bool {
        let modified = self.state().modified.borrow();
        index < self.modified_length() && modified.get(index as usize).copied().unwrap_or(false)
    }
}

/// `getOwnPropertySlotByIndex`.
pub fn get_own_property_slot_by_index(arguments: &dyn MappedArguments, vm: &VM, index: u32, slot: &mut PropertySlot) -> bool {
    if !arguments.is_modified_argument_descriptor(index) && arguments.is_mapped_argument(index) {
        slot.set_value(arguments.object(), 0, arguments.get_index_quickly(index));
        return true;
    }

    let result = with_base(arguments, |object| object.get_own_property_slot_by_index(vm, index, slot));

    if arguments.is_mapped_argument(index) {
        debug_assert!(result);
        slot.set_value(arguments.object(), slot.attributes(), arguments.get_index_quickly(index));
        return true;
    }

    result
}

/// `putByIndex(cell, globalObject, index, value, shouldThrow)`.
pub fn put_by_index(
    arguments: &dyn MappedArguments,
    vm: &VM,
    index: u32,
    value: JSValue,
    should_throw: bool,
) -> Result<bool, PutError> {
    if arguments.is_mapped_argument(index) {
        arguments.set_index_quickly(index, value);
        return Ok(true);
    }

    with_base(arguments, |object| object.put_by_index(vm, index, value, should_throw))
}

/// `deletePropertyByIndex(cell, globalObject, index)`.
pub fn delete_property_by_index(arguments: &dyn MappedArguments, vm: &VM, index: u32) -> Result<bool, PutError> {
    let property_might_be_in_object_storage = arguments.is_modified_argument_descriptor(index) || !arguments.is_mapped_argument(index);
    let mut deleted_property = true;
    if property_might_be_in_object_storage {
        deleted_property = with_base(arguments, |object| object.delete_property_by_index(vm, index))?;
    }

    if deleted_property {
        // Deleting an indexed property unconditionally unmaps it.
        if arguments.is_mapped_argument(index) {
            // We need to check that the property was mapped so we don't write to random memory.
            arguments.unmap_argument(index)?;
        }
        arguments.set_modified_argument_descriptor(index)?;
    }

    Ok(deleted_property)
}

/// `defineOwnProperty` para um nome de índice
/// (https://tc39.es/ecma262/#sec-arguments-exotic-objects-defineownproperty-p-desc).
pub fn define_own_indexed_property(
    arguments: &dyn MappedArguments,
    vm: &VM,
    index: u32,
    descriptor: &PropertyDescriptor,
    should_throw: bool,
) -> Result<bool, PutError> {
    let is_mapped = arguments.is_mapped_argument(index);
    let mut new_descriptor = *descriptor;

    if is_mapped {
        if arguments.is_modified_argument_descriptor(index) {
            if descriptor.value().is_empty() && descriptor.writable_present() && !descriptor.writable() {
                new_descriptor.set_value(arguments.get_index_quickly(index));
            }
        } else {
            let current = arguments.get_index_quickly(index);
            with_base(arguments, |object| {
                object.put_direct_index(vm, index, current, 0, PutDirectIndexMode::PutDirectIndexLikePutDirect)
            })?;
        }
    }

    let status = with_base(arguments, |object| object.define_own_indexed_property(vm, index, &new_descriptor, should_throw))?;
    if !status {
        debug_assert!(!is_mapped || arguments.is_modified_argument_descriptor(index));
        return Ok(false);
    }

    arguments.set_modified_argument_descriptor(index)?;

    if is_mapped {
        if descriptor.is_accessor_descriptor() {
            arguments.unmap_argument(index)?;
        } else {
            if !descriptor.value().is_empty() {
                arguments.set_index_quickly(index, descriptor.value());
            }
            if descriptor.writable_present() && !descriptor.writable() {
                arguments.unmap_argument(index)?;
            }
        }
    }

    Ok(true)
}

/// Os índices mapeados que `getOwnPropertyNames` acrescenta antes dos elementos do objeto.
pub fn mapped_indices(arguments: &dyn MappedArguments) -> Vec<u32> {
    (0..arguments.internal_length()).filter(|index| arguments.is_mapped_argument(*index)).collect()
}

/// O `DirectArguments` ou `ScopedArguments` que o `object` é, a menos que o corpo de uma chamada a
/// `Base::...` esteja rodando (aí o `JSObject` comum responde).
pub fn exotic_of(object: &JSObject) -> Option<Rc<dyn MappedArguments>> {
    let type_ = object.type_();
    if !TypeInfo::is_arguments_type(type_) {
        return None;
    }
    let arguments: Rc<dyn MappedArguments> = match cell_registry::get(object.cell_id())? {
        CellEntry::DirectArguments(arguments) => arguments as Rc<dyn MappedArguments>,
        CellEntry::ScopedArguments(arguments) => arguments as Rc<dyn MappedArguments>,
        _ => return None,
    };
    if arguments.state().in_base.get() {
        return None;
    }
    Some(arguments)
}
