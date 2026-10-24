//! Porte de `runtime/JSDataView.h` e `JSDataView.cpp` (com a parte de `JSArrayBufferView.h` que o
//! `DataView` usa): o `JSDataView`, um `JSNonFinalObject` que olha um trecho (`byteOffset`, `byteLength`) de
//! um `ArrayBuffer`, de comprimento fixo ou acompanhando um buffer redimensionável (`AutoLength`).
//!
//! DIVERGÊNCIAS:
//!
//! - O `JSArrayBufferView` (o `m_vector`, o `m_mode` de dezenas de combinações, o `ConstructionContext`) não
//!   existe no porte, e o `JSGenericTypedArrayView` fica para outra fatia: o `JSDataView` guarda direto o
//!   buffer, o `byteOffset`, o comprimento e as três características do `TypedArrayMode` que a visão
//!   consulta (`DataViewMode`). O `m_vector` (o `data() + byteOffset` que o `detachFromArrayBuffer` anulava)
//!   some: a visão destacada é a do buffer destacado (`isDetached()`), e os bytes saem de
//!   `read_bytes`/`write_bytes`.
//! - O `ClassInfo` do `JSDataView` tem por pai o `JSNonFinalObject` (a classe `JSArrayBufferView` não existe).
//! - `toWrapped*`/`possiblySharedTypedImpl`/`unsharedTypedImpl` devolvem o `DataView` nativo (`DataView.h`,
//!   o `ArrayBufferView` do WebCore), que não faz parte do motor: ficam de fora.
//! - `createUninitialized`, `create(length)`, `setFromTypedArray`, `setFromArrayLike` e `setIndex` são
//!   `UNREACHABLE_FOR_PLATFORM` no C++ (só existem para um `template` especializar): ficam de fora.
//! - `IdempotentArrayBufferByteLengthGetter` (o `Getter` de `viewByteLength`) só guarda a primeira leitura
//!   do comprimento do buffer: `view_byte_length` lê uma vez.

use std::rc::Rc;

use crate::runtime::array_buffer::ArrayBufferRef;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::host_call::Thrown;
use crate::runtime::js_array_buffer_view::{verify_byte_offset_alignment, verify_sub_range_length};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::TypeInfo;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;

/// `typedArrayBufferHasBeenDetachedErrorMessage`.
pub const TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE: &str =
    "Underlying ArrayBuffer has been detached from the view or out-of-bounds";

/// `arrayBufferViewErrorMessageOutOfRangeOfBuffer`.
pub const ARRAY_BUFFER_VIEW_ERROR_MESSAGE_OUT_OF_RANGE_OF_BUFFER: &str = "Length out of range of buffer";

/// `typedArrayErrorMessageBufferIsAlreadyDetached`.
pub const TYPED_ARRAY_ERROR_MESSAGE_BUFFER_IS_ALREADY_DETACHED: &str = "Buffer is already detached";

/// `typedArrayErrorMessageByteOffsetExceedSourceBufferByteLength`.
pub const TYPED_ARRAY_ERROR_MESSAGE_BYTE_OFFSET_EXCEED_SOURCE_BUFFER_BYTE_LENGTH: &str =
    "byteOffset exceeds source ArrayBuffer byteLength";

/// `const ClassInfo JSDataView::s_info`.
pub static JS_DATA_VIEW_S_INFO: ClassInfo =
    ClassInfo { class_name: "DataView", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `const ClassInfo JSResizableOrGrowableSharedDataView::s_info`.
pub static JS_RESIZABLE_OR_GROWABLE_SHARED_DATA_VIEW_S_INFO: ClassInfo =
    ClassInfo { class_name: "DataView", parent_class: Some(&JS_DATA_VIEW_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// As características do `TypedArrayMode` de um `DataView` (`DataViewMode` e as variantes de buffer
/// redimensionável ou compartilhado crescível).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DataViewMode {
    /// `isAutoLengthMode`: sem comprimento, acompanha o buffer.
    auto_length: bool,
    /// `isGrowableSharedMode`.
    growable_shared: bool,
    /// `isResizableNonSharedMode`.
    resizable_non_shared: bool,
}

/// `class JSDataView : public JSArrayBufferView`.
pub struct JSDataView {
    base: JSNonFinalObject,
    /// `m_buffer`.
    buffer: ArrayBufferRef,
    /// `m_byteOffset`.
    byte_offset: usize,
    /// `m_length` (o comprimento em bytes: o elemento do `DataView` tem 1 byte; 0 no `AutoLength`).
    length: usize,
    /// `m_mode`.
    mode: DataViewMode,
}

/// O `JSDataView*`.
pub type JSDataViewRef = Rc<JSDataView>;

impl std::fmt::Debug for JSDataView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JSDataView")
            .field("cell_id", &self.base.cell_id())
            .field("byte_offset", &self.byte_offset)
            .field("length", &self.length)
            .field("mode", &self.mode)
            .finish()
    }
}

impl std::ops::Deref for JSDataView {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSDataView {
    /// `elementSize`.
    pub const ELEMENT_SIZE: usize = 1;

    /// `StructureFlags = Base::StructureFlags` (nenhuma flag).
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `createStructure(vm, globalObject, prototype)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::DataViewType, JSDataView::STRUCTURE_FLAGS),
            &JS_DATA_VIEW_S_INFO,
        )
    }

    /// `JSResizableOrGrowableSharedDataView::createStructure(vm, globalObject, prototype)`.
    pub fn create_resizable_or_growable_shared_structure(
        vm: &VM,
        global_object: Option<&JSGlobalObject>,
        prototype: JSValue,
    ) -> StructureRef {
        Structure::create(
            vm,
            global_object,
            prototype,
            TypeInfo::new(JSType::DataViewType, JSDataView::STRUCTURE_FLAGS),
            &JS_RESIZABLE_OR_GROWABLE_SHARED_DATA_VIEW_S_INFO,
        )
    }

    /// `create(globalObject, structure, buffer, byteOffset, byteLength)`: `byte_length` ausente é o
    /// `AutoLength`, só de um buffer redimensionável ou compartilhado crescível.
    pub fn create(
        global_object: &JSGlobalObject,
        structure: &StructureRef,
        buffer: ArrayBufferRef,
        byte_offset: usize,
        byte_length: Option<usize>,
    ) -> Result<JSDataViewRef, Thrown> {
        let vm = global_object.vm();

        if buffer.is_detached() {
            return Err(Thrown::type_error(TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE));
        }

        debug_assert!(byte_length.is_some() || buffer.is_resizable_or_growable_shared());

        if !verify_sub_range_length(buffer.byte_length(), byte_offset, byte_length.unwrap_or(0), JSDataView::ELEMENT_SIZE) {
            return Err(Thrown::range_error(ARRAY_BUFFER_VIEW_ERROR_MESSAGE_OUT_OF_RANGE_OF_BUFFER));
        }

        if !verify_byte_offset_alignment(byte_offset, JSDataView::ELEMENT_SIZE) {
            return Err(Thrown::range_error("Byte offset is not aligned"));
        }

        // `ConstructionContext(structure, buffer, byteOffset, length, DataView)`.
        let mode = DataViewMode {
            auto_length: buffer.is_resizable_or_growable_shared() && byte_length.is_none(),
            growable_shared: buffer.is_growable_shared(),
            resizable_non_shared: buffer.is_resizable_non_shared(),
        };

        let cell_id = cell_registry::reserve();
        let view = Rc::new(JSDataView {
            base: JSNonFinalObject::new(vm, Rc::clone(structure)),
            buffer,
            byte_offset,
            length: byte_length.unwrap_or(0),
            mode,
        });
        view.set_cell_id(cell_id);
        cell_registry::set(cell_id, CellEntry::DataView(Rc::clone(&view)));
        debug_assert_eq!(view.type_(), JSType::DataViewType);
        Ok(view)
    }

    /// Procura a célula pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSDataViewRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::DataView(view)) => Some(view),
            _ => None,
        }
    }

    /// `dynamicDowncast<JSDataView>(value)`.
    pub fn from_value(value: &JSValue) -> Option<JSDataViewRef> {
        match value {
            JSValue::Cell(cell_id) => JSDataView::from_cell_id(*cell_id),
            _ => None,
        }
    }

    /// `JSValue(JSCell*)`.
    pub fn as_value(&self) -> JSValue {
        JSValue::from_cell(self.base.cell_id())
    }

    /// `isDetached()`: o buffer foi destacado.
    pub fn is_detached(&self) -> bool {
        self.buffer.is_detached()
    }

    /// `isAutoLength()`.
    pub fn is_auto_length(&self) -> bool {
        self.mode.auto_length
    }

    /// `canUseRawFieldsDirectly()`: de comprimento fixo, ou compartilhado crescível sem `AutoLength` (só
    /// cresce, então `m_length` e `m_byteOffset` seguem válidos).
    pub fn can_use_raw_fields_directly(&self) -> bool {
        !self.mode.resizable_non_shared && !(self.mode.growable_shared && self.mode.auto_length)
    }

    /// `byteOffsetRaw()`.
    pub fn byte_offset_raw(&self) -> usize {
        self.byte_offset
    }

    /// `byteLengthRaw()` (`lengthRaw() << logElementSize`, e o elemento tem 1 byte).
    pub fn byte_length_raw(&self) -> usize {
        self.length
    }

    /// `possiblySharedBuffer()`.
    pub fn possibly_shared_buffer(&self) -> &ArrayBufferRef {
        &self.buffer
    }

    /// `unsharedBuffer()`.
    pub fn unshared_buffer(&self) -> &ArrayBufferRef {
        assert!(!self.buffer.is_shared());
        &self.buffer
    }

    /// `viewByteLength(getter)`: `None` se a visão está destacada ou fora dos limites do buffer.
    pub fn view_byte_length(&self) -> Option<usize> {
        // https://tc39.es/proposal-resizablearraybuffer/#sec-isviewoutofbounds
        // https://tc39.es/proposal-resizablearraybuffer/#sec-getviewbytelength
        if self.is_detached() {
            return None;
        }

        if self.can_use_raw_fields_directly() {
            return Some(self.byte_length_raw());
        }

        let buffer_byte_length = self.buffer.byte_length();
        let byte_offset = self.byte_offset_raw();
        let byte_length = self.byte_length_raw() + byte_offset; // Keep in mind that byteLengthRaw returns 0 for AutoLength TypedArray.
        if byte_length > buffer_byte_length {
            return None;
        }
        if self.is_auto_length() {
            return Some(buffer_byte_length - byte_offset);
        }
        Some(self.byte_length_raw())
    }

    /// `isShared()`.
    pub fn is_shared(&self) -> bool {
        self.buffer.is_shared()
    }

    /// Lê `out.len()` bytes a partir de `index` bytes depois do início da visão (`vector() + index`). O
    /// chamador já conferiu o limite com `view_byte_length`.
    pub fn read_bytes(&self, index: usize, out: &mut [u8]) {
        let start = self.byte_offset + index;
        self.buffer.with_bytes(|bytes| out.copy_from_slice(&bytes[start..start + out.len()]));
    }

    /// Grava `source` a partir de `index` bytes depois do início da visão. O chamador já conferiu o limite.
    pub fn write_bytes(&self, index: usize, source: &[u8]) {
        let start = self.byte_offset + index;
        self.buffer.with_bytes_mut(|bytes| bytes[start..start + source.len()].copy_from_slice(source));
    }
}
