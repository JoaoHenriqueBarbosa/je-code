//! Tradução de `runtime/PutPropertySlot.h`: `class PutPropertySlot`.
//!
//! O `m_putFunction` (`CustomAccessorValueFunc`) é o [`PutValueFunc`] de `property_slot.rs`; o
//! ponteiro nulo do C++ é o `None`.

use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::property_offset::{PropertyOffset, INVALID_OFFSET};
use crate::runtime::property_slot::{CacheabilityType, PutValueFunc};

/// `PutPropertySlot::Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutType {
    Uncachable,
    ExistingProperty,
    NewProperty,
    SetterProperty,
    CustomValue,
    CustomAccessor,
}

/// `PutPropertySlot::Context`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutContext {
    UnknownContext,
    PutById,
    PutByIdEval,
}

/// `class PutPropertySlot`.
#[derive(Debug)]
pub struct PutPropertySlot {
    /// `m_base`, como o `cell_id` do objeto (0 é o `nullptr`).
    base: usize,
    this_value: JSValue,
    offset: PropertyOffset,
    is_strict_mode: bool,
    is_initialization: bool,
    is_tainted_by_opaque_object: bool,
    type_: PutType,
    context: PutContext,
    cacheability: CacheabilityType,
    put_function: Option<PutValueFunc>,
}

impl PutPropertySlot {
    /// `PutPropertySlot(JSValue thisValue, bool isStrictMode = false, Context = UnknownContext,
    /// bool isInitialization = false)`.
    pub fn new(this_value: JSValue, is_strict_mode: bool, context: PutContext, is_initialization: bool) -> PutPropertySlot {
        PutPropertySlot {
            base: 0,
            this_value,
            offset: INVALID_OFFSET,
            is_strict_mode,
            is_initialization,
            is_tainted_by_opaque_object: false,
            type_: PutType::Uncachable,
            context,
            cacheability: CacheabilityType::CachingAllowed,
            put_function: None,
        }
    }

    /// `setCustomValue(JSObject* base, PutValueFunc function)`.
    pub fn set_custom_value(&mut self, base: &JSObject, function: PutValueFunc) {
        self.type_ = PutType::CustomValue;
        self.base = base.cell_id();
        self.put_function = Some(function);
    }

    /// `setCustomAccessor(JSObject* base, PutValueFunc function)`.
    pub fn set_custom_accessor(&mut self, base: &JSObject, function: PutValueFunc) {
        self.type_ = PutType::CustomAccessor;
        self.base = base.cell_id();
        self.put_function = Some(function);
    }

    /// `customSetter()`: `None` é o `PutValueFunc` nulo.
    pub fn custom_setter(&self) -> Option<PutValueFunc> {
        debug_assert!(self.is_cacheable_custom());
        self.put_function
    }

    /// `isCacheableCustom()`.
    pub fn is_cacheable_custom(&self) -> bool {
        self.is_cacheable()
            && (self.type_ == PutType::CustomValue || self.type_ == PutType::CustomAccessor)
            && self.put_function.is_some()
    }

    /// `isCustomAccessor()`.
    pub fn is_custom_accessor(&self) -> bool {
        self.is_cacheable() && self.type_ == PutType::CustomAccessor
    }

    /// `setExistingProperty(JSObject* base, PropertyOffset)`.
    pub fn set_existing_property(&mut self, base: &JSObject, offset: PropertyOffset) {
        self.type_ = PutType::ExistingProperty;
        self.base = base.cell_id();
        self.offset = offset;
    }

    /// `setNewProperty(JSObject* base, PropertyOffset)`.
    pub fn set_new_property(&mut self, base: &JSObject, offset: PropertyOffset) {
        self.type_ = PutType::NewProperty;
        self.base = base.cell_id();
        self.offset = offset;
    }

    /// `setCacheableSetter(JSObject* base, PropertyOffset)`.
    pub fn set_cacheable_setter(&mut self, base: &JSObject, offset: PropertyOffset) {
        self.type_ = PutType::SetterProperty;
        self.base = base.cell_id();
        self.offset = offset;
    }

    /// `setThisValue(JSValue)`.
    pub fn set_this_value(&mut self, this_value: JSValue) {
        self.this_value = this_value;
    }

    /// `setStrictMode(bool)`.
    pub fn set_strict_mode(&mut self, value: bool) {
        self.is_strict_mode = value;
    }

    pub fn type_(&self) -> PutType {
        self.type_
    }

    pub fn context(&self) -> PutContext {
        self.context
    }

    /// `base()`: o `cell_id` do objeto (`JSObject::from_cell_id` o resolve), `None` é o `nullptr`.
    pub fn base(&self) -> Option<usize> {
        (self.base != 0).then_some(self.base)
    }

    pub fn this_value(&self) -> JSValue {
        self.this_value
    }

    pub fn is_strict_mode(&self) -> bool {
        self.is_strict_mode
    }

    fn is_cacheable(&self) -> bool {
        self.cacheability == CacheabilityType::CachingAllowed
    }

    /// `isCacheablePut()`.
    pub fn is_cacheable_put(&self) -> bool {
        self.is_cacheable() && (self.type_ == PutType::NewProperty || self.type_ == PutType::ExistingProperty)
    }

    /// `isCacheableSetter()`.
    pub fn is_cacheable_setter(&self) -> bool {
        self.is_cacheable() && self.type_ == PutType::SetterProperty
    }

    /// `isInitialization()`.
    pub fn is_initialization(&self) -> bool {
        self.is_initialization
    }

    /// `isTaintedByOpaqueObject()`.
    pub fn is_tainted_by_opaque_object(&self) -> bool {
        self.is_tainted_by_opaque_object
    }

    /// `setIsTaintedByOpaqueObject()`.
    pub fn set_is_tainted_by_opaque_object(&mut self) {
        self.is_tainted_by_opaque_object = true;
    }

    /// `cachedOffset()`.
    pub fn cached_offset(&self) -> PropertyOffset {
        self.offset
    }

    /// `disableCaching()`.
    pub fn disable_caching(&mut self) {
        self.cacheability = CacheabilityType::CachingDisallowed;
    }
}
