//! Tradução de `runtime/JSInternalFieldObjectImpl.h`: o objeto com `N` campos internos (`JSGenerator`,
//! `JSAsyncGenerator`, `JSAsyncFunctionGenerator`).
//!
//! DIVERGÊNCIAS (heap ausente):
//! - `WriteBarrier<Unknown>` vira `Cell<JSValue>`: sem GC não há barreira, então `set` e
//!   `setWithoutWriteBarrier` são a mesma escrita e `visitChildren` some.
//! - A classe-base é um tipo genérico e cada filha concreta (`JSGenerator` e irmãs) sai de
//!   [`define_internal_field_cell`], que registra a célula no `cell_registry` (uma variante por tipo),
//!   como `define_collection_cell` faz para `JSMap`.
//! - `JSInternalFieldObjectImpl` não declara `s_info`: o `ClassInfo` das filhas tem o `JSNonFinalObject`
//!   como pai.

use std::cell::Cell;

use crate::runtime::js_object::JSNonFinalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::StructureRef;
use crate::runtime::vm::VM;

/// `class JSInternalFieldObjectImpl<N> : public JSNonFinalObject`.
pub struct JSInternalFieldObjectImpl<const N: usize> {
    base: JSNonFinalObject,
    /// `m_internalFields`.
    internal_fields: [Cell<JSValue>; N],
}

impl<const N: usize> std::ops::Deref for JSInternalFieldObjectImpl<N> {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl<const N: usize> JSInternalFieldObjectImpl<N> {
    /// `numberOfInternalFields`.
    pub const NUMBER_OF_INTERNAL_FIELDS: u32 = N as u32;

    /// O construtor mais o laço de `finishCreation` que grava `initialValues()`.
    pub fn new(vm: &VM, structure: &StructureRef, initial_values: [JSValue; N]) -> JSInternalFieldObjectImpl<N> {
        JSInternalFieldObjectImpl {
            base: JSNonFinalObject::new(vm, StructureRef::clone(structure)),
            internal_fields: initial_values.map(Cell::new),
        }
    }

    /// `internalField(index).get()`.
    pub fn internal_field(&self, index: u32) -> JSValue {
        self.internal_fields[index as usize].get()
    }

    /// `internalField(index).set(vm, this, value)`.
    pub fn set_internal_field(&self, index: u32, value: JSValue) {
        self.internal_fields[index as usize].set(value);
    }

    /// `internalField(Field::State).get().asInt32AsAnyInt()`: o campo é sempre um `jsNumber(int32)`.
    pub fn internal_field_as_int32(&self, index: u32) -> i32 {
        // `asInt32AsAnyInt()`: um campo gravado por bytecode pode chegar como `double` inteiro.
        let value = self.internal_field(index);
        if value.is_int32() {
            value.as_int32()
        } else {
            value.as_double() as i32
        }
    }
}

/// O acesso por índice aos campos internos, o que `op_get_internal_field` e `op_put_internal_field` fazem
/// via `jsCast<JSInternalFieldObjectImpl<>*>` e que `CellEntry::internal_fields` devolve para qualquer
/// célula com campos internos.
pub trait InternalFields {
    /// `numberOfInternalFields()`.
    fn field_count(&self) -> u32;

    /// `internalField(index).get()`.
    fn field(&self, index: u32) -> JSValue;

    /// `internalField(index).set(vm, this, value)`.
    fn set_field(&self, index: u32, value: JSValue);
}

impl<const N: usize> InternalFields for JSInternalFieldObjectImpl<N> {
    fn field_count(&self) -> u32 {
        N as u32
    }

    fn field(&self, index: u32) -> JSValue {
        self.internal_field(index)
    }

    fn set_field(&self, index: u32, value: JSValue) {
        self.set_internal_field(index, value);
    }
}

/// `InternalFields` para as células que guardam os campos num `fields: RefCell<[JSValue; N]>` próprio
/// (`JSArrayIterator`, `JSStringIterator`, `JSRegExpStringIterator`), a mesma forma do
/// `JSInternalFieldObjectImpl<N>` sem a classe-base genérica.
macro_rules! impl_internal_fields_for_ref_cell {
    ($name:ty) => {
        impl $crate::runtime::js_internal_field_object_impl::InternalFields for $name {
            fn field_count(&self) -> u32 {
                self.fields.borrow().len() as u32
            }

            fn field(&self, index: u32) -> $crate::runtime::js_value::JSValue {
                self.fields.borrow()[index as usize]
            }

            fn set_field(&self, index: u32, value: $crate::runtime::js_value::JSValue) {
                self.fields.borrow_mut()[index as usize] = value;
            }
        }
    };
}

pub(crate) use impl_internal_fields_for_ref_cell;

/// Define uma filha concreta de `JSInternalFieldObjectImpl<N>`: o struct com `Deref` para a base, o
/// `ClassInfo`, `create`/`create_with_initial_values`, `create_structure`, `from_cell_id`/`from_value` e o
/// registro no `cell_registry` pela variante `$variant`. `$initial` é a `initialValues()` da classe.
macro_rules! define_internal_field_cell {
    ($name:ident, $ref_name:ident, $variant:ident, $js_type:ident, $info:ident, $class_name:literal, $count:expr, $initial:expr) => {
        /// `const ClassInfo ::s_info`.
        pub static $info: $crate::runtime::class_info::ClassInfo = $crate::runtime::class_info::ClassInfo {
            class_name: $class_name,
            parent_class: Some(&$crate::runtime::js_object::JS_NON_FINAL_OBJECT_S_INFO),
            static_prop_hash_table: None, inherits_js_type_range: None,
        };

        pub struct $name {
            base: $crate::runtime::js_internal_field_object_impl::JSInternalFieldObjectImpl<{ $count }>,
        }

        /// A referência à célula, o `*` do C++.
        pub type $ref_name = ::std::rc::Rc<$name>;

        impl ::std::ops::Deref for $name {
            type Target = $crate::runtime::js_internal_field_object_impl::JSInternalFieldObjectImpl<{ $count }>;

            fn deref(&self) -> &Self::Target {
                &self.base
            }
        }

        impl $name {
            /// `create(vm, structure)`: os campos nascem com `initialValues()`.
            pub fn create(vm: &$crate::runtime::vm::VM, structure: &$crate::runtime::structure::StructureRef) -> $ref_name {
                let cell_id = $crate::runtime::cell_registry::reserve();
                let cell = ::std::rc::Rc::new($name {
                    base: $crate::runtime::js_internal_field_object_impl::JSInternalFieldObjectImpl::new(
                        vm,
                        structure,
                        $initial,
                    ),
                });
                cell.set_cell_id(cell_id);
                $crate::runtime::cell_registry::set(cell_id, $crate::runtime::cell_registry::CellEntry::$variant(::std::rc::Rc::clone(&cell)));
                cell
            }

            /// `createStructure(vm, globalObject, prototype)`.
            pub fn create_structure(
                vm: &$crate::runtime::vm::VM,
                global_object: Option<&$crate::runtime::js_global_object::JSGlobalObject>,
                prototype: $crate::runtime::js_value::JSValue,
            ) -> $crate::runtime::structure::StructureRef {
                $crate::runtime::structure::Structure::create(
                    vm,
                    global_object,
                    prototype,
                    $crate::runtime::js_type_info::TypeInfo::new(
                        $crate::runtime::js_type::JSType::$js_type,
                        $crate::runtime::js_object::JSNonFinalObject::STRUCTURE_FLAGS,
                    ),
                    &$info,
                )
            }

            /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
            pub fn from_cell_id(cell_id: usize) -> Option<$ref_name> {
                match $crate::runtime::cell_registry::get(cell_id) {
                    Some($crate::runtime::cell_registry::CellEntry::$variant(cell)) => Some(cell),
                    _ => None,
                }
            }

            /// `dynamicDowncast<...>(value)`.
            pub fn from_value(value: &$crate::runtime::js_value::JSValue) -> Option<$ref_name> {
                match value {
                    $crate::runtime::js_value::JSValue::Cell(cell_id) => $name::from_cell_id(*cell_id),
                    _ => None,
                }
            }
        }
    };
}

pub(crate) use define_internal_field_cell;
