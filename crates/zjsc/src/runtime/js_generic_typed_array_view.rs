//! Porte de `runtime/JSGenericTypedArrayView.h` e `JSGenericTypedArrayViewInlines.h`: a visão tipada
//! (`Int8Array`... `BigUint64Array`) com criação, acesso por índice, `set` entre visões e de array-like,
//! `sort`, `validateTypedArray` e os ganchos exóticos de propriedade (`getOwnPropertySlot`, `put`,
//! `defineOwnProperty`, `deleteProperty`, `getOwnPropertyNames`).
//!
//! DIVERGÊNCIAS:
//!
//! - `JSGenericTypedArrayView<Adaptor>` é um tipo só, com o `TypedArrayType` lido do `JSType` da `Structure`
//!   (ver `typed_array_adaptors.rs`); `JSGenericResizableOrGrowableSharedTypedArrayView` é a mesma célula com
//!   outra `Structure` e outro `ClassInfo` (`typed_array_class_info`). `preventExtensions` dele não é gancho
//!   aqui (o `JSObject` do porte não despacha por tipo).
//! - Os ganchos exóticos são métodos desta célula com o contrato do C++; o `JSObject` os alcança por
//!   `typed_array_dispatch.rs` (`get_own_property_slot`, `put`, `define_own_property`, `delete_property` e as
//!   versões por índice), que faz o papel da tabela de métodos virtual. O `ordinarySetSlow` (receptor
//!   alterado) é o de `proxy_object.rs`.
//! - Os atalhos `copyFromInt32ShapeArray`/`copyFromDoubleShapeArray`, `radixSort`/`countingSort` e
//!   `sortFloat` são otimizações de velocidade; o resultado observável é o do caminho genérico, que é o
//!   único aqui. A cópia entre visões lê tudo antes de gravar (o `memmove` e o buffer de transferência do
//!   C++ têm esse resultado), exceto em `CopyType::LeftToRight`, que lê e grava elemento a elemento.
//! - `create(vm, structure, impl)`, `tryCreate`, `toWrapped*` e `possiblySharedTypedImpl` usam o
//!   `ArrayBufferView` nativo do WebCore e não são portados.

use std::rc::Rc;

use crate::runtime::array_buffer::ArrayBufferRef;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::host_call::Thrown;
use crate::runtime::identifier::MAX_ARRAY_INDEX;
use crate::runtime::js_array_buffer_view::{
    verify_byte_offset_alignment, verify_sub_range_length, ConstructionContext, CopyType, JSArrayBufferView,
    ARRAY_BUFFER_VIEW_ERROR_MESSAGE_OUT_OF_RANGE_OF_BUFFER, TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE,
};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_type_info::{
    TypeInfo, INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO, OVERRIDES_GET_OWN_PROPERTY_NAMES,
    OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_PUT,
};
use crate::runtime::js_typed_arrays::typed_array_class_info;
use crate::runtime::js_value::JSValue;
use crate::runtime::js_value_conversions::{js_to_number, number_to_string_radix10};
use crate::runtime::property_attribute::NONE;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::property_slot::PropertySlot;
use crate::runtime::proxy_object::ordinary_set_slow;
use crate::runtime::string_prototype::code_units;
use crate::runtime::string_regexp_support::{get_object_index_u64, get_object_property};
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::typed_array_adaptors::{
    convert_to, float_value, integer_value, to_js_value, to_native_from_value, NativeElement,
};
use crate::runtime::typed_array_type::TypedArrayType;
use crate::runtime::vm::VM;
use crate::wtf::text::string_view::StringView;
use crate::wtf::text::wtf_string::String as WtfString;

/// `class JSGenericTypedArrayView<Adaptor> : public JSArrayBufferView`.
pub struct JSGenericTypedArrayView {
    base: JSArrayBufferView,
}

/// O `JSGenericTypedArrayView*`.
pub type JSGenericTypedArrayViewRef = Rc<JSGenericTypedArrayView>;

impl std::fmt::Debug for JSGenericTypedArrayView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("JSGenericTypedArrayView").field(&self.base).finish()
    }
}

impl std::ops::Deref for JSGenericTypedArrayView {
    type Target = JSArrayBufferView;

    fn deref(&self) -> &JSArrayBufferView {
        &self.base
    }
}

/// `enum class SortResult { Success, OutOfMemory, Failed }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortResult {
    Success,
    OutOfMemory,
    Failed,
}

/// Mensagem de `validateRange` e do `set` fora da faixa.
pub const RANGE_OUT_OF_BOUNDS_MESSAGE: &str = "Range consisting of offset and length are out of bounds";

impl JSGenericTypedArrayView {
    /// `StructureFlags = Base::StructureFlags | OverridesGetOwnPropertySlot | OverridesGetOwnPropertyNames |
    /// OverridesPut | InterceptsGetOwnPropertySlotByIndexEvenWhenLengthIsNotZero`.
    pub const STRUCTURE_FLAGS: u32 = JSArrayBufferView::STRUCTURE_FLAGS
        | OVERRIDES_GET_OWN_PROPERTY_SLOT
        | OVERRIDES_GET_OWN_PROPERTY_NAMES
        | OVERRIDES_PUT
        | INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO;

    /// `createStructure(vm, globalObject, prototype)` de `JSGenericTypedArrayView` (`resizable` falso) e de
    /// `JSGenericResizableOrGrowableSharedTypedArrayView` (verdadeiro).
    pub fn create_structure(
        vm: &VM,
        global_object: Option<&JSGlobalObject>,
        type_: TypedArrayType,
        prototype: JSValue,
        resizable_or_growable_shared: bool,
    ) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(type_.js_type(), JSGenericTypedArrayView::STRUCTURE_FLAGS),
            typed_array_class_info(type_, resizable_or_growable_shared),
        )
    }

    /// O construtor com `finishCreation(vm)` e o registro da célula.
    fn allocate(vm: &VM, structure: &StructureRef, context: ConstructionContext) -> JSGenericTypedArrayViewRef {
        debug_assert!(crate::runtime::typed_array_type::is_typed_view(structure.type_info().type_()));
        let cell_id = cell_registry::reserve();
        let view = Rc::new(JSGenericTypedArrayView { base: JSArrayBufferView::new(vm, Rc::clone(structure), context) });
        view.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::TypedArray(Rc::clone(&view)));
        view.finish_creation(vm);
        view
    }

    /// `create(globalObject, structure, length)`: zerada. `Err(OutOfMemory)` é o `throwOutOfMemoryError`.
    pub fn create(global_object: &JSGlobalObject, structure: &StructureRef, length: usize) -> Result<JSGenericTypedArrayViewRef, Thrown> {
        let type_ = crate::runtime::typed_array_type::typed_array_type(structure.type_info().type_());
        let Some(context) = ConstructionContext::with_length(length, type_.element_size()) else {
            return Err(Thrown::OutOfMemory);
        };
        Ok(JSGenericTypedArrayView::allocate(global_object.vm(), structure, context))
    }

    /// `createUninitialized(globalObject, structure, length)`: o `Vec` já nasce zerado, então é o `create`.
    pub fn create_uninitialized(
        global_object: &JSGlobalObject,
        structure: &StructureRef,
        length: usize,
    ) -> Result<JSGenericTypedArrayViewRef, Thrown> {
        JSGenericTypedArrayView::create(global_object, structure, length)
    }

    /// `create(globalObject, structure, buffer, byteOffset, length)`.
    pub fn create_with_buffer(
        global_object: &JSGlobalObject,
        structure: &StructureRef,
        buffer: ArrayBufferRef,
        byte_offset: usize,
        length: Option<usize>,
    ) -> Result<JSGenericTypedArrayViewRef, Thrown> {
        let type_ = crate::runtime::typed_array_type::typed_array_type(structure.type_info().type_());
        let element_size = type_.element_size();
        if buffer.is_detached() {
            return Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE));
        }
        debug_assert!(length.is_some() || buffer.is_resizable_or_growable_shared());
        if !verify_sub_range_length(buffer.byte_length(), byte_offset, length.unwrap_or(0), element_size) {
            return Err(Thrown::range_error(ARRAY_BUFFER_VIEW_ERROR_MESSAGE_OUT_OF_RANGE_OF_BUFFER));
        }
        if !verify_byte_offset_alignment(byte_offset, element_size) {
            return Err(Thrown::range_error("Byte offset is not aligned"));
        }
        let context = ConstructionContext::with_buffer(buffer, byte_offset, length);
        Ok(JSGenericTypedArrayView::allocate(global_object.vm(), structure, context))
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSGenericTypedArrayViewRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::TypedArray(view)) => Some(view),
            _ => None,
        }
    }

    /// `dynamicDowncast<JSGenericTypedArrayView>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSGenericTypedArrayViewRef> {
        match value {
            JSValue::Cell(cell_id) => JSGenericTypedArrayView::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.cell_id())
    }

    /// `getIndexQuickly(i)` (e `Adaptor::toJSValue` do `getIndexQuicklyAsNativeValue`).
    pub fn get_index_quickly(&self, index: usize) -> Result<JSValue, Thrown> {
        to_js_value(self.typed_array_type(), self.get_element(index))
    }

    /// `setIndex(globalObject, i, value)`: coage o valor antes de olhar os limites. `Ok(false)` é fora dos
    /// limites; destacado é `Ok(true)` sem gravar.
    pub fn set_index(&self, global_object: &JSGlobalObject, index: u64, value: JSValue) -> Result<bool, Thrown> {
        let native = to_native_from_value(global_object, self.typed_array_type(), value)?;
        if self.is_detached() {
            return Ok(true);
        }
        if !self.in_bounds(index) {
            return Ok(false);
        }
        self.set_element(index as usize, native);
        Ok(true)
    }

    /// `validateRange(globalObject, offset, length)`.
    pub fn validate_range(&self, offset: usize, length: usize) -> Result<(), Thrown> {
        if self.can_access_range_quickly(offset, length) {
            return Ok(());
        }
        Err(Thrown::range_error(RANGE_OUT_OF_BOUNDS_MESSAGE))
    }

    /// `setFromTypedArray(globalObject, offset, object, objectOffset, length, type)`.
    pub fn set_from_typed_array(
        &self,
        offset: usize,
        object: &JSArrayBufferView,
        object_offset: usize,
        length: usize,
        copy_type: CopyType,
    ) -> Result<(), Thrown> {
        let own_type = self.typed_array_type();
        let other_type = object.typed_array_type();
        debug_assert!(other_type.is_typed_view());
        let length = length.min(object.length());
        self.validate_range(offset, length)?;

        // Os casos em que o C++ copia bytes (`memmoveFastPath`): mesmo tipo, os dois `Uint8`, ou inteiros do
        // mesmo tamanho sem o `Uint8Clamped` como destino.
        let same_bits = other_type == own_type
            || (other_type.is_some_uint8() && own_type.is_some_uint8())
            || (own_type.is_int() && other_type.is_int() && !own_type.is_clamped() && own_type.element_size() == other_type.element_size());
        if !same_bits && own_type.content_type() != other_type.content_type() {
            return Err(Thrown::type_error("Content types of source and destination typed arrays are different"));
        }
        debug_assert!(object.can_access_range_quickly(object_offset, length));

        let convert = |element: NativeElement| if same_bits { element } else { convert_to(other_type, own_type, element) };
        if copy_type == CopyType::LeftToRight {
            for i in 0..length {
                let element = convert(object.get_element(object_offset + i));
                self.set_element(offset + i, element);
            }
            return Ok(());
        }
        let snapshot: Vec<NativeElement> = (0..length).map(|i| convert(object.get_element(object_offset + i))).collect();
        for (i, element) in snapshot.into_iter().enumerate() {
            self.set_element(offset + i, element);
        }
        Ok(())
    }

    /// `setFromArrayLike(globalObject, offset, object, objectOffset, length)`.
    pub fn set_from_array_like_object(
        &self,
        global_object: &JSGlobalObject,
        offset: usize,
        object: JSValue,
        object_offset: usize,
        length: usize,
    ) -> Result<(), Thrown> {
        self.validate_range(offset, length)?;
        for i in 0..length {
            let index = i + object_offset;
            let value = get_object_index_u64(global_object, object, index as u64)?;
            self.set_index(global_object, (offset + i) as u64, value)?;
        }
        Ok(())
    }

    /// `setFromArrayLike(globalObject, offset, source)` (https://tc39.es/ecma262/#sec-settypedarrayfromarraylike).
    pub fn set_from_array_like(&self, global_object: &JSGlobalObject, offset: usize, source: JSValue) -> Result<(), Thrown> {
        if self.is_detached() {
            return Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE));
        }
        let vm = global_object.vm();
        let target_length = self.length();
        let Some(object) = source.to_object(global_object) else { return Err(Thrown::Pending) };
        let object = object.as_value();
        let length_value = get_object_property(global_object, object, &vm.property_names.length)?;
        let source_length = length_value.to_length_checked()? as usize;
        if offset as u64 > crate::runtime::array_buffer::MAX_ARRAY_BUFFER_SIZE
            || source_length.checked_add(offset).is_none_or(|end| end > target_length)
        {
            return Err(Thrown::range_error(RANGE_OUT_OF_BOUNDS_MESSAGE));
        }
        for i in 0..source_length {
            let value = get_object_index_u64(global_object, object, i as u64)?;
            self.set_index(global_object, (offset + i) as u64, value)?;
        }
        Ok(())
    }

    /// `sort()`: a ordem de `%TypedArray%.prototype.sort` sem comparador. Os de ponto flutuante seguem
    /// `-Infinity < finitos negativos < -0 < +0 < finitos positivos < Infinity < NaN`.
    pub fn sort(&self) -> SortResult {
        assert!(!self.is_detached());
        let Some(length) = self.integer_indexed_object_length() else { return SortResult::Failed };
        let type_ = self.typed_array_type();
        let mut elements: Vec<NativeElement> = (0..length).map(|i| self.get_element(i)).collect();
        if type_.is_float() {
            elements.sort_by(|&a, &b| {
                let (a, b) = (float_value(type_, a), float_value(type_, b));
                match (a.is_nan(), b.is_nan()) {
                    (true, true) => std::cmp::Ordering::Equal,
                    (true, false) => std::cmp::Ordering::Greater,
                    (false, true) => std::cmp::Ordering::Less,
                    (false, false) => a.total_cmp(&b),
                }
            });
        } else {
            elements.sort_by_key(|&element| integer_value(type_, element));
        }
        for (i, element) in elements.into_iter().enumerate() {
            self.set_element(i, element);
        }
        SortResult::Success
    }

    /// `getOwnPropertySlotByIndex(thisObject, globalObject, i, slot)`.
    pub fn get_own_property_slot_by_index(&self, index: u32, slot: &mut PropertySlot) -> Result<bool, Thrown> {
        if self.is_detached() || !self.in_bounds(u64::from(index)) {
            return Ok(false);
        }
        let value = self.get_index_quickly(index as usize)?;
        slot.set_value(self, NONE, value);
        Ok(true)
    }

    /// `getOwnPropertySlot(thisObject, globalObject, propertyName, slot)`: `None` quando o nome não é um
    /// índice numérico canônico e o `JSObject` base deve responder.
    pub fn get_own_property_slot(&self, vm: &VM, property_name: &PropertyName, slot: &mut PropertySlot) -> Result<Option<bool>, Thrown> {
        if let Some(index) = property_name.parse_index() {
            return self.get_own_property_slot_by_index(index, slot).map(Some);
        }
        match is_canonical_numeric_index_string(vm, property_name) {
            None => Ok(None),
            Some(None) => Ok(Some(false)),
            Some(Some(index)) => {
                if self.is_detached() || !self.in_bounds(index) {
                    return Ok(Some(false));
                }
                let value = self.get_index_quickly(index as usize)?;
                slot.set_value(self, NONE, value);
                Ok(Some(true))
            }
        }
    }

    /// `putByIndex(cell, globalObject, i, value, shouldThrow)`: sempre `true`.
    pub fn put_by_index(&self, global_object: &JSGlobalObject, index: u32, value: JSValue) -> Result<bool, Thrown> {
        self.set_index(global_object, u64::from(index), value)?;
        Ok(true)
    }

    /// `put(cell, globalObject, propertyName, value, slot)`: `None` quando o `JSObject` base deve responder.
    /// `receiver` é o `slot.thisValue()` e `should_throw` o `slot.isStrictMode()`; o receptor diferente da
    /// própria visão é o `isThisValueAltered(slot, thisObject)`.
    pub fn put(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        value: JSValue,
        receiver: JSValue,
        should_throw: bool,
    ) -> Result<Option<bool>, Thrown> {
        let vm = global_object.vm();
        let this_value_altered = receiver != self.as_value();
        if let Some(index) = property_name.parse_index() {
            if this_value_altered {
                if self.is_detached() || !self.in_bounds(u64::from(index)) {
                    return Ok(Some(true));
                }
                return ordinary_set_slow(global_object, self, property_name, value, receiver, should_throw).map(Some);
            }
            return self.put_by_index(global_object, index, value).map(Some);
        }
        let Some(integer_index) = is_canonical_numeric_index_string(vm, property_name) else { return Ok(None) };
        if this_value_altered {
            let Some(index) = integer_index else { return Ok(Some(true)) };
            if self.is_detached() || !self.in_bounds(index) {
                return Ok(Some(true));
            }
            return ordinary_set_slow(global_object, self, property_name, value, receiver, should_throw).map(Some);
        }
        // `TypedArraySetElement` coage o valor antes de decidir se o índice é válido.
        match integer_index {
            Some(index) => {
                self.set_index(global_object, index, value)?;
            }
            None => {
                to_native_from_value(global_object, self.typed_array_type(), value)?;
            }
        }
        Ok(Some(true))
    }

    /// `defineOwnProperty(object, globalObject, propertyName, descriptor, shouldThrow)`: `None` quando o
    /// `JSObject` base deve responder.
    pub fn define_own_property(
        &self,
        global_object: &JSGlobalObject,
        property_name: &PropertyName,
        descriptor: &PropertyDescriptor,
        should_throw: bool,
    ) -> Result<Option<bool>, Thrown> {
        let vm = global_object.vm();
        let mut index = property_name.parse_index().map(u64::from);
        let mut is_canonical_numeric = false;
        if index.is_none() {
            if let Some(integer_index) = is_canonical_numeric_index_string(vm, property_name) {
                is_canonical_numeric = true;
                index = integer_index;
            }
        }
        if let Some(index) = index {
            let fail = |message: &str| -> Result<Option<bool>, Thrown> {
                if should_throw { Err(Thrown::type_error(&format!("{message}{index}"))) } else { Ok(Some(false)) }
            };
            if self.is_detached() {
                return if should_throw {
                    Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE))
                } else {
                    Ok(Some(false))
                };
            }
            if !self.in_bounds(index) {
                return fail("Attempting to store out-of-bounds property on a typed array at index: ");
            }
            if descriptor.is_accessor_descriptor() {
                return fail("Attempting to store accessor property on a typed array at index: ");
            }
            if descriptor.configurable_present() && !descriptor.configurable() {
                return fail("Attempting to store non-configurable property on a typed array at index: ");
            }
            if descriptor.enumerable_present() && !descriptor.enumerable() {
                return fail("Attempting to store non-enumerable property on a typed array at index: ");
            }
            if descriptor.writable_present() && !descriptor.writable() {
                return fail("Attempting to store non-writable property on a typed array at index: ");
            }
            if !descriptor.value().is_empty() {
                self.set_index(global_object, index, descriptor.value())?;
            }
            return Ok(Some(true));
        }
        if is_canonical_numeric {
            return if should_throw {
                Err(Thrown::type_error("Attempting to store canonical numeric string property on a typed array"))
            } else {
                Ok(Some(false))
            };
        }
        Ok(None)
    }

    /// `deletePropertyByIndex(cell, globalObject, i)`: os elementos não se apagam.
    pub fn delete_property_by_index(&self, index: u32) -> bool {
        self.is_detached() || !self.in_bounds(u64::from(index))
    }

    /// `deleteProperty(cell, globalObject, propertyName, slot)`: `None` quando o `JSObject` base responde.
    pub fn delete_property(&self, vm: &VM, property_name: &PropertyName) -> Option<bool> {
        if let Some(index) = property_name.parse_index() {
            return Some(self.delete_property_by_index(index));
        }
        match is_canonical_numeric_index_string(vm, property_name)? {
            Some(index) => Some(self.is_detached() || !self.in_bounds(index)),
            None => Some(true),
        }
    }

    /// Os índices que `getOwnPropertyNames` acrescenta antes das propriedades da estrutura: `0..length()`.
    pub fn own_index_names(&self) -> std::ops::Range<u64> {
        0..self.length() as u64
    }
}

/// `validateTypedArray(globalObject, typedArrayValue)` (https://tc39.es/ecma262/#sec-validatetypedarray).
pub fn validate_typed_array(value: JSValue) -> Result<JSGenericTypedArrayViewRef, Thrown> {
    let Some(view) = JSGenericTypedArrayView::from_value(&value) else {
        return Err(Thrown::type_error("Argument needs to be a typed array."));
    };
    check_typed_array_in_bounds(&view)?;
    Ok(view)
}

/// O fim de `validateTypedArray(globalObject, JSArrayBufferView*)`: a visão destacada ou fora da faixa lança.
pub fn check_typed_array_in_bounds(view: &JSArrayBufferView) -> Result<(), Thrown> {
    if view.is_array_buffer_view_out_of_bounds() {
        return Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE));
    }
    Ok(())
}

/// `fastIsCanonicalNumericIndexString(characters)`: `Some` decide sem converter.
fn fast_is_canonical_numeric_index_string(units: &[u16]) -> Option<bool> {
    let digit = |unit: u16| (u16::from(b'0')..=u16::from(b'9')).contains(&unit);
    let first = units[0];
    if units.len() == 1 {
        return Some(digit(first));
    }
    let second = units[1];
    if first == u16::from(b'-') {
        if !digit(second) && (units.len() != "-Infinity".len() || second != u16::from(b'I')) {
            return Some(false);
        }
        if units.len() == 2 {
            return Some(true);
        }
    } else if !digit(first)
        && !(units.len() == "Infinity".len() && first == u16::from(b'I'))
        && !(units.len() == "NaN".len() && first == u16::from(b'N'))
    {
        return Some(false);
    }
    None
}

/// `isCanonicalNumericIndexString(propertyName.uid(), &integerIndex)`: `None` não é canônico; `Some(None)` é
/// canônico sem índice a reportar; `Some(Some(i))` é canônico com o índice inteiro acima de `MAX_ARRAY_INDEX`.
pub fn is_canonical_numeric_index_string(vm: &VM, property_name: &PropertyName) -> Option<Option<u64>> {
    let uid = property_name.uid()?;
    if uid.0.is_symbol() || uid.0.length() == 0 {
        return None;
    }
    let units: Vec<u16> =
        if uid.0.is_8bit() { uid.0.span8().iter().map(|&unit| u16::from(unit)).collect() } else { uid.0.span16().to_vec() };
    if let Some(fast) = fast_is_canonical_numeric_index_string(&units) {
        return fast.then_some(None);
    }
    let text = WtfString::from_utf16(&units);
    let index = js_to_number(StringView::from(&text));
    let round_trip = number_to_string_radix10(vm, index).value();
    if code_units(&round_trip).as_ref() != units.as_slice() {
        return None;
    }
    let smallest_reported_index = f64::from(MAX_ARRAY_INDEX) + 1.0;
    if index >= smallest_reported_index && index < u64::MAX as f64 {
        let candidate = index as u64;
        if candidate as f64 == index {
            return Some(Some(candidate));
        }
    }
    Some(None)
}
