//! Tradução de `runtime/JSTemplateObjectDescriptor.h` e `.cpp`.
//!
//! DIVERGÊNCIA (depende do heap, camada 3): no C++ é um `JSCell` com `StructureIsImmortal` alocado
//! pelo `templateObjectDescriptorSpace`. Enquanto `crate::heap` não existe, é um valor imutável
//! compartilhado (`JSTemplateObjectDescriptorRef = Rc<..>`) e o `cell_id` vem do registro central
//! (`cell_registry`), que mantém a célula viva; a conferência de tipo é exata, então não colide com
//! `Empty` (0) nem com `Deleted` (4).
//!
//! `createTemplateObject(JSGlobalObject*)` constrói dois `JSArray` e congela o `rawObject` e o próprio
//! array com `objectConstructorFreeze` (`object_constructor.rs`). `create` ainda não usa `vm`: o
//! `Base(vm, vm.templateObjectDescriptorStructure)` só tem o que guardar quando a `Structure` existir.

use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{TypeInfo, STRUCTURE_IS_IMMORTAL};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::js_array::JSArray;
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_undefined, JSValue};
use crate::runtime::object_constructor::object_constructor_freeze;
use crate::runtime::property_attribute::{DONT_DELETE, DONT_ENUM, READ_ONLY};
use crate::runtime::property_name::PropertyName;
use crate::runtime::template_object_descriptor::TemplateObjectDescriptor;
use crate::runtime::vm::VM;

/// `const ClassInfo JSTemplateObjectDescriptor::s_info`.
pub static TEMPLATE_OBJECT_DESCRIPTOR_S_INFO: ClassInfo =
    ClassInfo { class_name: "TemplateObjectDescriptor", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSTemplateObjectDescriptor`.
#[derive(Debug)]
pub struct JSTemplateObjectDescriptor {
    /// `const Ref<TemplateObjectDescriptor> m_descriptor`.
    descriptor: Rc<TemplateObjectDescriptor>,
    /// `int m_endOffset`.
    end_offset: i32,
    cell_id: usize,
    /// `JSCell::m_structureID`: a `vm.templateObjectDescriptorStructure`.
    structure: StructureRef,
}

/// A célula como o resto do porte a enxerga.
pub type JSTemplateObjectDescriptorRef = Rc<JSTemplateObjectDescriptor>;

impl JSTemplateObjectDescriptor {
    /// `JSTemplateObjectDescriptor::createStructure(vm, globalObject, prototype)`: `TypeInfo(CellType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(vm, global_object, prototype, TypeInfo::new(JSType::CellType, STRUCTURE_IS_IMMORTAL), &TEMPLATE_OBJECT_DESCRIPTOR_S_INFO)
    }

    /// `JSCell::structure()`.
    pub fn structure(&self) -> &StructureRef {
        &self.structure
    }

    /// `create(VM&, Ref<TemplateObjectDescriptor>&&, int)`.
    pub fn create(vm: &VM, descriptor: Rc<TemplateObjectDescriptor>, end_offset: i32) -> JSTemplateObjectDescriptorRef {
        let cell_id = cell_registry::reserve();
        let cell = Rc::new(JSTemplateObjectDescriptor {
            descriptor,
            end_offset,
            cell_id,
            structure: vm.template_object_descriptor_structure(),
        });
        cell_registry::set(cell_id, CellEntry::TemplateObjectDescriptor(Rc::clone(&cell)));
        cell
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSTemplateObjectDescriptorRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::TemplateObjectDescriptor(cell)) => Some(cell),
            _ => None,
        }
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// `descriptor()`.
    pub fn descriptor(&self) -> &TemplateObjectDescriptor {
        &self.descriptor
    }

    /// `endOffset()`.
    pub fn end_offset(&self) -> i32 {
        self.end_offset
    }

    /// `createTemplateObject(JSGlobalObject*)`.
    ///
    /// DIVERGÊNCIA: os elementos entram por `initialize_index` (os arrays são criados já com o
    /// comprimento final) em vez de `putDirectIndex(..., ReadOnly | DontDelete, PutDirectIndexLikePutDirect)`;
    /// o `objectConstructorFreeze` que se segue marca todos eles `ReadOnly | DontDelete` (e `length`
    /// somente leitura) pela conversão para `ArrayStorage` em modo esparso, o mesmo estado final.
    /// `None` é o `nullptr` de um `objectConstructorFreeze` que lançou (a exceção fica pendente).
    pub fn create_template_object(&self, global_object: &JSGlobalObject) -> Option<JSArray> {
        let vm = global_object.vm();
        let count = self.descriptor().cooked_strings().len() as u32;
        let structure = global_object.array_structure();
        let template_object = JSArray::create(vm, &structure, count);
        let raw_object = JSArray::create(vm, &structure, count);

        for index in 0..count {
            let cooked = match &self.descriptor().cooked_strings()[index as usize] {
                Some(cooked) => JSValue::from_js_string(js_string(vm, cooked)),
                None => js_undefined(),
            };
            template_object.initialize_index(vm, index, cooked);
            let raw = JSValue::from_js_string(js_string(vm, &self.descriptor().raw_strings()[index as usize]));
            raw_object.initialize_index(vm, index, raw);
        }

        object_constructor_freeze(global_object, &raw_object).ok()?;

        let raw_name = PropertyName::from_identifier(&vm.property_names.raw);
        template_object.put_direct(vm, &raw_name, raw_object.as_value(), READ_ONLY | DONT_ENUM | DONT_DELETE);

        object_constructor_freeze(global_object, &template_object).ok()?;
        Some(template_object)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_registers_cell_and_keeps_end_offset() {
        let vm = VM::new();
        let descriptor = TemplateObjectDescriptor::create(Vec::new(), Vec::new());
        let cell = JSTemplateObjectDescriptor::create(&vm, descriptor, 42);
        assert_eq!(cell.end_offset(), 42);
        assert_eq!(cell.cell_id() & 7, 0);
        let found = JSTemplateObjectDescriptor::from_cell_id(cell.cell_id()).unwrap();
        assert!(Rc::ptr_eq(&cell, &found));
        assert!(JSTemplateObjectDescriptor::from_cell_id(8).is_none());
    }
}
