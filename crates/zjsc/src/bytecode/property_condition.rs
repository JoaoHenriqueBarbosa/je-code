//! Porte de `bytecode/PropertyCondition.{h,cpp}` e `bytecode/ObjectPropertyCondition.{h,cpp}`.
//!
//! Os oito tipos do upstream (`Presence`, `Replacement`, `Absence`, `AbsenceOfSetEffect`,
//! `AbsenceOfIndexedProperties`, `Equivalence`, `HasStaticProperty`, `HasPrototype`), cada um com o
//! `isStillValidAssumingImpurePropertyWatchpoint`, o `isStillValid`, o `isWatchableWhenValid` e o
//! `isWatchable` na ordem do `PropertyCondition.cpp`. Este upstream não tem `CustomFunctionEquivalence`.
//! Sem `Concurrency`: não há compilador concorrente. `AdaptiveInferredPropertyValueWatchpointBase` continua
//! exigindo `Equivalence` (o `RELEASE_ASSERT` dela).

use crate::bytecode::watchpoint::WatchpointState;
use crate::runtime::indexing_type::has_indexed_properties;
use crate::runtime::js_object::JSObjectHandle;
use crate::runtime::js_type::JSType;
use crate::runtime::js_value::JSValue;
use crate::runtime::lookup::HashTableValue;
use crate::runtime::property_attribute::{
    ACCESSOR, ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE, CUSTOM_ACCESSOR_OR_VALUE, READ_ONLY,
    READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE,
};
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_offset::{is_valid_offset, PropertyOffset, INVALID_OFFSET};
use crate::runtime::structure::Structure;
use crate::runtime::vm::VM;
use crate::wtf::text::string_impl::UniquedKey;

use std::rc::Rc;

/// `PropertyCondition::Kind`, na ordem do enum do C++.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyConditionKind {
    Presence,
    /// Implica `Presence`, mas a propriedade não pode ser vigiada por substituição.
    Replacement,
    Absence,
    AbsenceOfSetEffect,
    AbsenceOfIndexedProperties,
    /// O watchpoint adaptativo é um par; na transição, o set de substituição é armado na estrutura nova.
    Equivalence,
    /// Valor ou acessor customizado.
    HasStaticProperty,
    HasPrototype,
}

/// `PropertyCondition::WatchabilityEffort`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WatchabilityEffort {
    /// Só confere; não cria set de substituição.
    MakeNoChanges,
    /// Cria o set de substituição se faltar (o `Equivalence` precisa dele) ou dispara o do `Replacement`.
    EnsureWatchability,
}

/// `JSObject::mightBeSpecialProperty(vm, type, uid)`.
fn might_be_special_property(vm: &VM, ty: JSType, uid: &UniquedKey) -> bool {
    let name = PropertyName::from_uid(Some(uid.clone()), false);
    match ty {
        JSType::ArrayType | JSType::DerivedArrayType => name == vm.property_names.length,
        JSType::JSFunctionType => {
            name == vm.property_names.length || name == vm.property_names.name || name == vm.property_names.prototype
        }
        _ => true,
    }
}

/// `Structure::findPropertyHashEntry(uid)`: percorre a cadeia de `ClassInfo` até a primeira tabela estática
/// que tem o nome.
fn find_property_hash_entry(vm: &VM, structure: &Rc<Structure>, uid: &UniquedKey) -> Option<&'static HashTableValue> {
    let key = crate::runtime::identifier::Identifier::from_uid(vm, Some(uid)).utf8();
    let key = std::str::from_utf8(&key).ok()?;
    let mut class_info = Some(structure.class_info());
    while let Some(current) = class_info {
        if let Some(entry) = current.static_prop_hash_table.and_then(|table| table.entry(key)) {
            return Some(entry);
        }
        class_info = current.parent_class;
    }
    None
}

/// `structure->hasNonReifiedStaticProperties()`.
fn has_non_reified_static_properties(structure: &Rc<Structure>) -> bool {
    structure.type_info().has_static_property_table() && !structure.static_properties_reified()
}

/// `structure->storedPrototypeObject() != prototype`: compara a identidade da célula (o `cell_id`).
fn stored_prototype_is(structure: &Rc<Structure>, prototype: Option<&JSObjectHandle>) -> bool {
    let stored = structure.stored_prototype();
    match (crate::runtime::js_object::JSObject::from_value(&stored), prototype) {
        (None, None) => true,
        (Some(stored), Some(prototype)) => stored.cell_id() == prototype.cell_id(),
        _ => false,
    }
}

/// `class PropertyCondition`. Os campos que o tipo não usa ficam no valor neutro (o `memset` do C++).
#[derive(Clone, Debug)]
pub struct PropertyCondition {
    kind: PropertyConditionKind,
    /// `HasPrototype` não tem nome (o `uid` do cabeçalho é `nullptr`).
    uid: Option<UniquedKey>,
    offset: PropertyOffset,
    attributes: u32,
    prototype: Option<JSObjectHandle>,
    required_value: JSValue,
}

impl PropertyCondition {
    fn new(kind: PropertyConditionKind, uid: Option<UniquedKey>) -> PropertyCondition {
        PropertyCondition {
            kind,
            uid,
            offset: INVALID_OFFSET,
            attributes: 0,
            prototype: None,
            required_value: JSValue::empty(),
        }
    }

    /// `presenceWithoutBarrier(uid, offset, attributes)`.
    pub fn presence_without_barrier(uid: UniquedKey, offset: PropertyOffset, attributes: u32) -> PropertyCondition {
        let mut result = PropertyCondition::new(PropertyConditionKind::Presence, Some(uid));
        result.offset = offset;
        result.attributes = attributes;
        result
    }

    /// `replacementWithoutBarrier(uid, offset, attributes)`.
    pub fn replacement_without_barrier(uid: UniquedKey, offset: PropertyOffset, attributes: u32) -> PropertyCondition {
        debug_assert!(attributes & READ_ONLY == 0);
        let mut result = PropertyCondition::new(PropertyConditionKind::Replacement, Some(uid));
        result.offset = offset;
        result.attributes = attributes;
        result
    }

    /// `absenceWithoutBarrier(uid, prototype)`: o `storedPrototype`, não o `prototypeForLookup`.
    pub fn absence_without_barrier(uid: UniquedKey, prototype: Option<JSObjectHandle>) -> PropertyCondition {
        let mut result = PropertyCondition::new(PropertyConditionKind::Absence, Some(uid));
        result.prototype = prototype;
        result
    }

    /// `absenceOfSetEffectWithoutBarrier(uid, prototype)`.
    pub fn absence_of_set_effect_without_barrier(uid: UniquedKey, prototype: Option<JSObjectHandle>) -> PropertyCondition {
        let mut result = PropertyCondition::new(PropertyConditionKind::AbsenceOfSetEffect, Some(uid));
        result.prototype = prototype;
        result
    }

    /// `absenceOfIndexedPropertiesWithoutBarrier(prototype)`: sem nome.
    pub fn absence_of_indexed_properties_without_barrier(prototype: Option<JSObjectHandle>) -> PropertyCondition {
        let mut result = PropertyCondition::new(PropertyConditionKind::AbsenceOfIndexedProperties, None);
        result.prototype = prototype;
        result
    }

    /// `equivalenceWithoutBarrier(uid, value)`.
    pub fn equivalence_without_barrier(uid: UniquedKey, value: JSValue) -> PropertyCondition {
        let mut result = PropertyCondition::new(PropertyConditionKind::Equivalence, Some(uid));
        result.required_value = value;
        result
    }

    /// `hasStaticProperty(uid)`.
    pub fn has_static_property(uid: UniquedKey) -> PropertyCondition {
        PropertyCondition::new(PropertyConditionKind::HasStaticProperty, Some(uid))
    }

    /// `hasPrototypeWithoutBarrier(prototype)`.
    pub fn has_prototype_without_barrier(prototype: Option<JSObjectHandle>) -> PropertyCondition {
        let mut result = PropertyCondition::new(PropertyConditionKind::HasPrototype, None);
        result.prototype = prototype;
        result
    }

    pub fn kind(&self) -> PropertyConditionKind {
        self.kind
    }

    /// `uid()`: só `HasPrototype` (e `AbsenceOfIndexedProperties`) não tem.
    pub fn uid(&self) -> &UniquedKey {
        self.uid.as_ref().expect("a condição não tem uid")
    }

    pub fn has_uid(&self) -> bool {
        self.uid.is_some()
    }

    /// `hasOffset()`: `Presence` e `Replacement`.
    pub fn has_offset(&self) -> bool {
        matches!(self.kind, PropertyConditionKind::Presence | PropertyConditionKind::Replacement)
    }

    pub fn offset(&self) -> PropertyOffset {
        debug_assert!(self.has_offset());
        self.offset
    }

    /// `hasAttributes()`: o mesmo que `hasOffset()`.
    pub fn has_attributes(&self) -> bool {
        self.has_offset()
    }

    pub fn attributes(&self) -> u32 {
        debug_assert!(self.has_attributes());
        self.attributes
    }

    /// `hasPrototype()`.
    pub fn has_prototype(&self) -> bool {
        matches!(
            self.kind,
            PropertyConditionKind::Absence
                | PropertyConditionKind::AbsenceOfSetEffect
                | PropertyConditionKind::AbsenceOfIndexedProperties
                | PropertyConditionKind::HasPrototype
        )
    }

    pub fn prototype(&self) -> Option<&JSObjectHandle> {
        debug_assert!(self.has_prototype());
        self.prototype.as_ref()
    }

    /// `hasRequiredValue()`.
    pub fn has_required_value(&self) -> bool {
        self.kind == PropertyConditionKind::Equivalence
    }

    pub fn required_value(&self) -> JSValue {
        debug_assert!(self.has_required_value());
        self.required_value
    }

    /// `watchingRequiresStructureTransitionWatchpoint()`: tudo menos `Replacement`.
    pub fn watching_requires_structure_transition_watchpoint(&self) -> bool {
        self.kind != PropertyConditionKind::Replacement
    }

    /// `watchingRequiresReplacementWatchpoint()`: `Equivalence` e `Replacement`.
    pub fn watching_requires_replacement_watchpoint(&self) -> bool {
        matches!(self.kind, PropertyConditionKind::Equivalence | PropertyConditionKind::Replacement)
    }

    fn property_name(&self) -> PropertyName {
        PropertyName::from_uid(self.uid.clone(), false)
    }

    /// `isValidValueForAttributes(value, attributes)`. O `inherits<GetterSetter>`/`<CustomGetterSetter>` do C++
    /// é injetado por quem chama, porque o valor não carrega o tipo de célula sozinho.
    pub fn is_valid_value_for_attributes(value: JSValue, attributes: u32, is_getter_setter: bool, is_custom_getter_setter: bool) -> bool {
        if value.is_empty() {
            return false;
        }
        if is_getter_setter {
            return attributes & ACCESSOR != 0;
        }
        if is_custom_getter_setter {
            return attributes & CUSTOM_ACCESSOR_OR_VALUE != 0;
        }
        attributes & ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE == 0
    }

    /// `validityRequiresImpurePropertyWatchpoint(structure)`.
    pub fn validity_requires_impure_property_watchpoint(&self, structure: &Rc<Structure>) -> bool {
        match self.kind {
            PropertyConditionKind::Presence
            | PropertyConditionKind::Replacement
            | PropertyConditionKind::Absence
            | PropertyConditionKind::Equivalence
            | PropertyConditionKind::HasStaticProperty => structure.need_impure_property_watchpoint(),
            PropertyConditionKind::AbsenceOfSetEffect
            | PropertyConditionKind::AbsenceOfIndexedProperties
            | PropertyConditionKind::HasPrototype => false,
        }
    }

    /// `isStillValidAssumingImpurePropertyWatchpoint(concurrency, structure, base)`.
    pub fn is_still_valid_assuming_impure_property_watchpoint(
        &self,
        vm: &VM,
        structure: &Rc<Structure>,
        base: Option<&JSObjectHandle>,
    ) -> bool {
        match self.kind {
            PropertyConditionKind::HasPrototype => {
                if !structure.prototype_queries_are_cacheable() {
                    return false;
                }
            }
            _ => {
                if !structure.property_accesses_are_cacheable() {
                    return false;
                }
            }
        }

        match self.kind {
            PropertyConditionKind::Presence | PropertyConditionKind::Replacement => {
                let (current_offset, current_attributes) = structure.get_with_attributes(vm, &self.property_name());
                if current_offset != self.offset || current_attributes != self.attributes {
                    return false;
                }
                if self.kind == PropertyConditionKind::Replacement {
                    match structure.property_replacement_watchpoint_set(current_offset) {
                        // `isStillValid()` do set: a substituição precisa já ter disparado.
                        Some(set) if set.borrow().state() == WatchpointState::IsInvalidated => {}
                        _ => return false,
                    }
                }
                true
            }

            PropertyConditionKind::Absence => {
                if structure.is_dictionary() {
                    return false;
                }
                if structure.has_poly_proto() {
                    // FIXME do upstream: conservador demais (https://bugs.webkit.org/show_bug.cgi?id=177339).
                    return false;
                }
                if structure.get(vm, &self.property_name()) != INVALID_OFFSET {
                    return false;
                }
                if has_non_reified_static_properties(structure) && find_property_hash_entry(vm, structure, self.uid()).is_some() {
                    return false;
                }
                stored_prototype_is(structure, self.prototype.as_ref())
            }

            PropertyConditionKind::AbsenceOfSetEffect => {
                if structure.is_dictionary() {
                    return false;
                }
                if structure.type_info().overrides_put()
                    && might_be_special_property(vm, structure.type_info().type_(), self.uid())
                {
                    return false;
                }
                let (current_offset, current_attributes) = structure.get_with_attributes(vm, &self.property_name());
                if current_offset != INVALID_OFFSET {
                    if current_attributes & (READ_ONLY | ACCESSOR | CUSTOM_ACCESSOR_OR_VALUE) != 0 {
                        return false;
                    }
                } else if has_non_reified_static_properties(structure) {
                    if let Some(entry) = find_property_hash_entry(vm, structure, self.uid()) {
                        if entry.attributes & READ_ONLY_OR_ACCESSOR_OR_CUSTOM_ACCESSOR_OR_VALUE != 0 {
                            return false;
                        }
                    }
                }
                if structure.has_poly_proto() {
                    return false;
                }
                stored_prototype_is(structure, self.prototype.as_ref())
            }

            PropertyConditionKind::AbsenceOfIndexedProperties => {
                if structure.has_poly_proto() {
                    return false;
                }
                if has_indexed_properties(structure.indexing_type()) {
                    return false;
                }
                if structure.may_intercept_indexed_accesses()
                    || structure.type_info().intercepts_get_own_property_slot_by_index_even_when_length_is_not_zero()
                {
                    return false;
                }
                stored_prototype_is(structure, self.prototype.as_ref())
            }

            PropertyConditionKind::HasPrototype => {
                if structure.has_poly_proto() {
                    return false;
                }
                stored_prototype_is(structure, self.prototype.as_ref())
            }

            PropertyConditionKind::Equivalence => {
                let Some(base) = base else {
                    // Sem o objeto não dá para verificar, então retorna falso.
                    return false;
                };
                if !Rc::ptr_eq(&base.structure(), structure) {
                    return false;
                }
                let current_offset = structure.get(vm, &self.property_name());
                if current_offset == INVALID_OFFSET {
                    return false;
                }
                let current_value = base.get_direct(current_offset);
                !(current_value != self.required_value || current_value.is_empty())
            }

            PropertyConditionKind::HasStaticProperty => {
                if is_valid_offset(structure.get(vm, &self.property_name())) {
                    return false;
                }
                if structure.static_properties_reified() {
                    return false;
                }
                find_property_hash_entry(vm, structure, self.uid()).is_some()
            }
        }
    }

    /// `isStillValid(concurrency, structure, base)`: o impuro também precisa estar fora do caminho.
    pub fn is_still_valid(&self, vm: &VM, structure: &Rc<Structure>, base: Option<&JSObjectHandle>) -> bool {
        if !self.is_still_valid_assuming_impure_property_watchpoint(vm, structure, base) {
            return false;
        }
        // Uma propriedade impura pode aparecer e "sombrear" uma JS existente no mesmo objeto, então afeta
        // presença, substituição e ausência; não afeta `AbsenceOfSetEffect` (impuras nunca são setters).
        let info = structure.type_info();
        match self.kind {
            PropertyConditionKind::Absence => {
                if info.get_own_property_slot_is_impure() || info.get_own_property_slot_is_impure_for_property_absence() {
                    return false;
                }
            }
            PropertyConditionKind::Presence
            | PropertyConditionKind::Replacement
            | PropertyConditionKind::Equivalence
            | PropertyConditionKind::HasStaticProperty => {
                if info.get_own_property_slot_is_impure() {
                    return false;
                }
            }
            _ => {}
        }
        true
    }

    /// `isWatchableWhenValid(structure, effort, concurrency)`.
    pub fn is_watchable_when_valid(&self, vm: &VM, structure: &Rc<Structure>, effort: WatchabilityEffort) -> bool {
        if structure.transition_watchpoint_set_has_been_invalidated() {
            return false;
        }
        match self.kind {
            PropertyConditionKind::Replacement => {
                let offset = structure.get(vm, &self.property_name());
                // Só se chama depois de um `isValid`, que já confirmou que a estrutura conhece a propriedade.
                assert!(offset != INVALID_OFFSET);
                let set = match effort {
                    WatchabilityEffort::MakeNoChanges => structure.property_replacement_watchpoint_set(offset),
                    WatchabilityEffort::EnsureWatchability => {
                        structure.fire_property_replacement_watchpoint_set(vm, offset, "Firing replacement to ensure validity")
                    }
                };
                // O set de `Replacement` precisa ter disparado: `set->isStillValid()` é falha.
                match set {
                    Some(set) => set.borrow().state() == WatchpointState::IsInvalidated,
                    None => false,
                }
            }
            PropertyConditionKind::Equivalence => {
                let offset = structure.get(vm, &self.property_name());
                assert!(offset != INVALID_OFFSET);
                let set = match effort {
                    WatchabilityEffort::MakeNoChanges => structure.property_replacement_watchpoint_set(offset),
                    WatchabilityEffort::EnsureWatchability => structure.ensure_property_replacement_watchpoint_set(vm, offset),
                };
                match set {
                    Some(set) => set.borrow().state() != WatchpointState::IsInvalidated,
                    None => false,
                }
            }
            // `HasStaticProperty` usa só o watchpoint de transição, conferido acima; os demais tipos também.
            _ => true,
        }
    }

    /// `isWatchableAssumingImpurePropertyWatchpoint(structure, base, effort)`.
    pub fn is_watchable_assuming_impure_property_watchpoint(
        &self,
        vm: &VM,
        structure: &Rc<Structure>,
        base: Option<&JSObjectHandle>,
        effort: WatchabilityEffort,
    ) -> bool {
        self.is_still_valid_assuming_impure_property_watchpoint(vm, structure, base)
            && self.is_watchable_when_valid(vm, structure, effort)
    }

    /// `isWatchable(structure, base, effort)`.
    pub fn is_watchable(&self, vm: &VM, structure: &Rc<Structure>, base: Option<&JSObjectHandle>, effort: WatchabilityEffort) -> bool {
        self.is_still_valid(vm, structure, base) && self.is_watchable_when_valid(vm, structure, effort)
    }
}

/// `class ObjectPropertyCondition`.
#[derive(Clone, Debug)]
pub struct ObjectPropertyCondition {
    object: JSObjectHandle,
    condition: PropertyCondition,
}

impl ObjectPropertyCondition {
    /// `presence(vm, owner, object, uid, offset, attributes)`: o `writeBarrier(owner)` não existe sem GC.
    pub fn presence(object: JSObjectHandle, uid: UniquedKey, offset: PropertyOffset, attributes: u32) -> ObjectPropertyCondition {
        ObjectPropertyCondition { object, condition: PropertyCondition::presence_without_barrier(uid, offset, attributes) }
    }

    /// `replacement(vm, owner, object, uid, offset, attributes)`.
    pub fn replacement(object: JSObjectHandle, uid: UniquedKey, offset: PropertyOffset, attributes: u32) -> ObjectPropertyCondition {
        ObjectPropertyCondition { object, condition: PropertyCondition::replacement_without_barrier(uid, offset, attributes) }
    }

    /// `absence(vm, owner, object, uid, prototype)`.
    pub fn absence(object: JSObjectHandle, uid: UniquedKey, prototype: Option<JSObjectHandle>) -> ObjectPropertyCondition {
        ObjectPropertyCondition { object, condition: PropertyCondition::absence_without_barrier(uid, prototype) }
    }

    /// `absenceOfSetEffect(vm, owner, object, uid, prototype)`.
    pub fn absence_of_set_effect(object: JSObjectHandle, uid: UniquedKey, prototype: Option<JSObjectHandle>) -> ObjectPropertyCondition {
        ObjectPropertyCondition { object, condition: PropertyCondition::absence_of_set_effect_without_barrier(uid, prototype) }
    }

    /// `absenceOfIndexedProperties(vm, owner, object, prototype)`.
    pub fn absence_of_indexed_properties(object: JSObjectHandle, prototype: Option<JSObjectHandle>) -> ObjectPropertyCondition {
        ObjectPropertyCondition { object, condition: PropertyCondition::absence_of_indexed_properties_without_barrier(prototype) }
    }

    /// `ObjectPropertyCondition::equivalence(vm, owner, object, uid, value)`.
    pub fn equivalence(object: JSObjectHandle, uid: UniquedKey, value: JSValue) -> ObjectPropertyCondition {
        ObjectPropertyCondition { object, condition: PropertyCondition::equivalence_without_barrier(uid, value) }
    }

    /// `hasStaticProperty(vm, owner, object, uid)`.
    pub fn has_static_property(object: JSObjectHandle, uid: UniquedKey) -> ObjectPropertyCondition {
        ObjectPropertyCondition { object, condition: PropertyCondition::has_static_property(uid) }
    }

    /// `hasPrototype(vm, owner, object, prototype)`.
    pub fn has_prototype(object: JSObjectHandle, prototype: Option<JSObjectHandle>) -> ObjectPropertyCondition {
        ObjectPropertyCondition { object, condition: PropertyCondition::has_prototype_without_barrier(prototype) }
    }

    pub fn object(&self) -> &JSObjectHandle {
        &self.object
    }

    pub fn condition(&self) -> &PropertyCondition {
        &self.condition
    }

    pub fn kind(&self) -> PropertyConditionKind {
        self.condition.kind()
    }

    pub fn uid(&self) -> &UniquedKey {
        self.condition.uid()
    }

    /// `structureEnsuresValidityAssumingImpurePropertyWatchpoint()`: sem o objeto, só a estrutura.
    pub fn structure_ensures_validity_assuming_impure_property_watchpoint(&self, vm: &VM) -> bool {
        self.condition.is_still_valid_assuming_impure_property_watchpoint(vm, &self.object.structure(), None)
    }

    /// `validityRequiresImpurePropertyWatchpoint()`.
    pub fn validity_requires_impure_property_watchpoint(&self) -> bool {
        self.condition.validity_requires_impure_property_watchpoint(&self.object.structure())
    }

    /// `isStillValidAssumingImpurePropertyWatchpoint(concurrency)`.
    pub fn is_still_valid_assuming_impure_property_watchpoint(&self, vm: &VM) -> bool {
        self.condition.is_still_valid_assuming_impure_property_watchpoint(vm, &self.object.structure(), Some(&self.object))
    }

    /// `isStillValid(concurrency)`.
    pub fn is_still_valid(&self, vm: &VM) -> bool {
        self.condition.is_still_valid(vm, &self.object.structure(), Some(&self.object))
    }

    /// `structureEnsuresValidity(concurrency)`.
    pub fn structure_ensures_validity(&self, vm: &VM) -> bool {
        self.condition.is_still_valid(vm, &self.object.structure(), None)
    }

    /// `isWatchableAssumingImpurePropertyWatchpoint(effort)`.
    pub fn is_watchable_assuming_impure_property_watchpoint(&self, vm: &VM, effort: WatchabilityEffort) -> bool {
        self.condition.is_watchable_assuming_impure_property_watchpoint(vm, &self.object.structure(), Some(&self.object), effort)
    }

    /// `isWatchable(effort)`.
    pub fn is_watchable(&self, vm: &VM, effort: WatchabilityEffort) -> bool {
        self.condition.is_watchable(vm, &self.object.structure(), Some(&self.object), effort)
    }
}
