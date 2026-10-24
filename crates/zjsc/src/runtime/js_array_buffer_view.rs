//! Porte de `runtime/JSArrayBufferView.h`, `JSArrayBufferView.cpp`, `JSArrayBufferViewInlines.h`,
//! `JSArrayBufferViewInlinesLight.h` e `ArrayBufferView.h` (as constantes de verificação de faixa): a base
//! comum de `JSGenericTypedArrayView` e `JSDataView`, com o modo (`TypedArrayMode`), o vetor, o
//! comprimento, o deslocamento e a ligação com o `ArrayBuffer`.
//!
//! DIVERGÊNCIAS (sem `unsafe`, sem Gigacage, sem GC, sem `Butterfly` para o buffer):
//!
//! - O vetor de uma visão `FastTypedArray` ou `OversizeTypedArray` (que ainda não tem `ArrayBuffer`) é um
//!   `Vec<u8>` próprio (`m_vector` apontando para a memória que a visão possui); o limite entre as duas
//!   (`fastSizeLimit`) fica, só para o `mode()` responder o que o C++ responde. O vetor de uma visão com
//!   buffer (`Wasteful*` e `DataView*`) é o próprio conteúdo do `ArrayBuffer` a partir do `byteOffset`: o
//!   ponteiro `m_vector` não existe, `with_vector`/`with_vector_mut` emprestam o trecho. O buffer guardado
//!   no `IndexingHeader` do `Butterfly` (`existingBufferInButterfly`) e o do `JSDataView::m_buffer` são o
//!   mesmo campo, `buffer`.
//! - `slowDownAndWasteMemory` move o `Vec` para um `ArrayBuffer` novo (`ArrayBuffer::create_from_bytes`) e
//!   passa o modo para `WastefulTypedArray`, sem copiar (o `createAdopted` do `Oversize` e o `tryCreate`
//!   do `Fast` do C++ são o mesmo movimento aqui).
//! - `detachFromArrayBuffer` e `refreshVector` (o `ArrayBuffer::detach` avisa as visões ligadas) somem: o
//!   comprimento e o deslocamento "brutos" respondem 0 quando o buffer foi destacado, e o vetor é sempre
//!   o conteúdo atual do buffer, então não há ponteiro velho a atualizar. `hasVector()` é "o buffer
//!   existe e não foi destacado".
//! - `IdempotentArrayBufferByteLengthGetter` (leitura única do `byteLength` de um buffer compartilhado
//!   que cresce em outra thread) não existe: um só thread, a leitura é a mesma.
//! - `isIteratorProtocolFastAndNonObservable`, `possiblySharedImpl`/`unsharedImpl` e `toWrapped*` (o
//!   `ArrayBufferView` nativo do WebCore) não são portados: só o DOM os usa.

use std::cell::{Cell, RefCell};

use crate::runtime::array_buffer::{ArrayBufferRef, MAX_ARRAY_BUFFER_SIZE};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::js_object::{JSNonFinalObject, JS_NON_FINAL_OBJECT_S_INFO};
use crate::runtime::structure::StructureRef;
use crate::runtime::typed_array_adaptors::{read_element, write_element, NativeElement};
use crate::runtime::typed_array_type::{typed_array_type, TypedArrayType};
use crate::runtime::vm::VM;

/// `const ClassInfo JSArrayBufferView::s_info`.
pub static JS_ARRAY_BUFFER_VIEW_S_INFO: ClassInfo =
    ClassInfo { class_name: "ArrayBufferView", parent_class: Some(&JS_NON_FINAL_OBJECT_S_INFO), static_prop_hash_table: None, inherits_js_type_range: None };

/// `typedArrayBufferHasBeenDetachedErrorMessage`.
pub const TYPED_ARRAY_BUFFER_HAS_BEEN_DETACHED_ERROR_MESSAGE: &str = "Underlying ArrayBuffer has been detached from the view or out-of-bounds";

/// `arrayBufferViewErrorMessageOutOfRangeOfBuffer` (`JSGenericTypedArrayViewConstructor.h`).
pub const ARRAY_BUFFER_VIEW_ERROR_MESSAGE_OUT_OF_RANGE_OF_BUFFER: &str = "Length out of range of buffer";

/// `JSArrayBufferView::fastSizeLimit`.
pub const FAST_SIZE_LIMIT: usize = 1000;

/// `enum TypedArrayMode : uint8_t`.
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedArrayMode {
    FastTypedArray = 0b0001_0000,
    OversizeTypedArray = 0b0011_0000,
    WastefulTypedArray = 0b0101_1000,
    GrowableSharedWastefulTypedArray = 0b0101_1010,
    GrowableSharedAutoLengthWastefulTypedArray = 0b0101_1011,
    ResizableNonSharedWastefulTypedArray = 0b0101_1100,
    ResizableNonSharedAutoLengthWastefulTypedArray = 0b0101_1101,
    DataViewMode = 0b1000_1000,
    GrowableSharedDataViewMode = 0b1000_1010,
    GrowableSharedAutoLengthDataViewMode = 0b1000_1011,
    ResizableNonSharedDataViewMode = 0b1000_1100,
    ResizableNonSharedAutoLengthDataViewMode = 0b1000_1101,
}

pub const IS_AUTO_LENGTH_MODE: u8 = 0b0000_0001;
pub const IS_GROWABLE_SHARED_MODE: u8 = 0b0000_0010;
pub const IS_RESIZABLE_NON_SHARED_MODE: u8 = 0b0000_0100;
pub const IS_HAVING_ARRAY_BUFFER_MODE: u8 = 0b0000_1000;
pub const IS_TYPED_ARRAY_MODE: u8 = 0b0001_0000;
pub const IS_WASTEFUL_TYPED_ARRAY_MODE: u8 = 0b0100_0000;
pub const IS_DATA_VIEW_MODE: u8 = 0b1000_0000;

pub const IS_RESIZABLE_OR_GROWABLE_SHARED_MODE: u8 = IS_RESIZABLE_NON_SHARED_MODE | IS_GROWABLE_SHARED_MODE;
pub const RESIZABILITY_AND_AUTO_LENGTH_MASK: u8 = IS_AUTO_LENGTH_MODE | IS_GROWABLE_SHARED_MODE | IS_RESIZABLE_NON_SHARED_MODE;

impl TypedArrayMode {
    /// `hasArrayBuffer(mode)`.
    pub fn has_array_buffer(self) -> bool {
        self as u8 & IS_HAVING_ARRAY_BUFFER_MODE != 0
    }

    /// `isResizableOrGrowableShared(mode)`.
    pub fn is_resizable_or_growable_shared(self) -> bool {
        self as u8 & IS_RESIZABLE_OR_GROWABLE_SHARED_MODE != 0
    }

    /// `isGrowableShared(mode)`.
    pub fn is_growable_shared(self) -> bool {
        self as u8 & IS_GROWABLE_SHARED_MODE != 0
    }

    /// `isResizableNonShared(mode)`.
    pub fn is_resizable_non_shared(self) -> bool {
        self as u8 & IS_RESIZABLE_NON_SHARED_MODE != 0
    }

    /// `isAutoLength(mode)`.
    pub fn is_auto_length(self) -> bool {
        self as u8 & IS_AUTO_LENGTH_MODE != 0
    }

    /// `isWastefulTypedArray(mode)`.
    pub fn is_wasteful_typed_array(self) -> bool {
        self as u8 & IS_WASTEFUL_TYPED_ARRAY_MODE != 0
    }

    /// `canUseArrayBufferViewRawFieldsDirectly(mode)`: não redimensionável, ou crescível compartilhado sem
    /// comprimento automático (que só cresce, então `m_length` e `m_byteOffset` valem sempre).
    pub fn can_use_raw_fields_directly(self) -> bool {
        self as u8 & RESIZABILITY_AND_AUTO_LENGTH_MASK <= IS_GROWABLE_SHARED_MODE
    }

    /// O modo de uma visão tipada sobre `buffer` (`ConstructionContext(vm, structure, buffer, byteOffset,
    /// length)`): `has_length` é `length.has_value()`.
    pub fn for_typed_array(buffer: &ArrayBufferRef, has_length: bool) -> TypedArrayMode {
        if !buffer.is_resizable_or_growable_shared() {
            TypedArrayMode::WastefulTypedArray
        } else if buffer.is_growable_shared() {
            if has_length {
                TypedArrayMode::GrowableSharedWastefulTypedArray
            } else {
                TypedArrayMode::GrowableSharedAutoLengthWastefulTypedArray
            }
        } else if has_length {
            TypedArrayMode::ResizableNonSharedWastefulTypedArray
        } else {
            TypedArrayMode::ResizableNonSharedAutoLengthWastefulTypedArray
        }
    }

    /// O modo de um `DataView` sobre `buffer` (o `ConstructionContext` com `DataViewTag`).
    pub fn for_data_view(buffer: &ArrayBufferRef, has_length: bool) -> TypedArrayMode {
        if !buffer.is_resizable_or_growable_shared() {
            TypedArrayMode::DataViewMode
        } else if buffer.is_growable_shared() {
            if has_length {
                TypedArrayMode::GrowableSharedDataViewMode
            } else {
                TypedArrayMode::GrowableSharedAutoLengthDataViewMode
            }
        } else if has_length {
            TypedArrayMode::ResizableNonSharedDataViewMode
        } else {
            TypedArrayMode::ResizableNonSharedAutoLengthDataViewMode
        }
    }
}

/// `ArrayBufferView::verifyByteOffsetAlignment(byteOffset, elementSize)`.
pub fn verify_byte_offset_alignment(byte_offset: usize, element_size: usize) -> bool {
    byte_offset & (element_size - 1) == 0
}

/// `ArrayBufferView::verifySubRangeLength(byteLength, byteOffset, numElements, elementSize)`.
pub fn verify_sub_range_length(byte_length: usize, byte_offset: usize, num_elements: usize, element_size: usize) -> bool {
    if byte_offset > byte_length {
        return false;
    }
    let remaining_elements = (byte_length - byte_offset) / element_size;
    num_elements <= remaining_elements
}

/// `enum class CopyType { LeftToRight, Unobservable }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyType {
    LeftToRight,
    Unobservable,
}

/// Como uma visão nasce (o `ConstructionContext` do C++): o modo, o vetor próprio (só nos modos sem buffer)
/// e o buffer (só nos modos com buffer).
pub struct ConstructionContext {
    mode: TypedArrayMode,
    vector: Vec<u8>,
    buffer: Option<ArrayBufferRef>,
    length: usize,
    byte_offset: usize,
}

impl ConstructionContext {
    /// `ConstructionContext(vm, structure, length, elementSize, mode)`: `None` é o `!context` (a alocação
    /// falhou ou o tamanho passa de `MAX_ARRAY_BUFFER_SIZE`). `Vec<u8>` já nasce zerado: o
    /// `DontInitialize` do C++ só evita o custo de zerar.
    pub fn with_length(length: usize, element_size: usize) -> Option<ConstructionContext> {
        let size = length.checked_mul(element_size)?;
        let mode = if length <= FAST_SIZE_LIMIT {
            TypedArrayMode::FastTypedArray
        } else {
            if size as u64 > MAX_ARRAY_BUFFER_SIZE {
                return None;
            }
            TypedArrayMode::OversizeTypedArray
        };
        let vector = crate::runtime::fallible_alloc::try_zeroed_bytes(size)?;
        Some(ConstructionContext { mode, vector, buffer: None, length, byte_offset: 0 })
    }

    /// `ConstructionContext(vm, structure, buffer, byteOffset, length)`: `length` ausente é o
    /// comprimento automático (só com buffer redimensionável).
    pub fn with_buffer(buffer: ArrayBufferRef, byte_offset: usize, length: Option<usize>) -> ConstructionContext {
        debug_assert!(length.is_some() || buffer.is_resizable_or_growable_shared());
        let mode = TypedArrayMode::for_typed_array(&buffer, length.is_some());
        ConstructionContext { mode, vector: Vec::new(), buffer: Some(buffer), length: length.unwrap_or(0), byte_offset }
    }

    /// `ConstructionContext(structure, buffer, byteOffset, length, DataView)`.
    pub fn with_data_view_buffer(buffer: ArrayBufferRef, byte_offset: usize, length: Option<usize>) -> ConstructionContext {
        debug_assert!(length.is_some() || buffer.is_resizable_or_growable_shared());
        let mode = TypedArrayMode::for_data_view(&buffer, length.is_some());
        ConstructionContext { mode, vector: Vec::new(), buffer: Some(buffer), length: length.unwrap_or(0), byte_offset }
    }
}

/// `class JSArrayBufferView : public JSNonFinalObject`.
pub struct JSArrayBufferView {
    base: JSNonFinalObject,
    /// `m_mode`.
    mode: Cell<TypedArrayMode>,
    /// `m_length`.
    length: Cell<usize>,
    /// `m_byteOffset`.
    byte_offset: Cell<usize>,
    /// O vetor que a visão possui (`Fast` e `Oversize`); vazio nos modos com buffer.
    vector: RefCell<Vec<u8>>,
    /// O `ArrayBuffer` (`existingBufferInButterfly` / `JSDataView::m_buffer`), nos modos que o têm.
    buffer: RefCell<Option<ArrayBufferRef>>,
}

impl std::fmt::Debug for JSArrayBufferView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JSArrayBufferView")
            .field("cell_id", &self.base.cell_id())
            .field("mode", &self.mode.get())
            .field("length", &self.length.get())
            .field("byte_offset", &self.byte_offset.get())
            .finish()
    }
}

impl std::ops::Deref for JSArrayBufferView {
    type Target = JSNonFinalObject;

    fn deref(&self) -> &JSNonFinalObject {
        &self.base
    }
}

impl JSArrayBufferView {
    /// `StructureFlags = Base::StructureFlags`.
    pub const STRUCTURE_FLAGS: u32 = JSNonFinalObject::STRUCTURE_FLAGS;

    /// `JSArrayBufferView(VM&, ConstructionContext&)`. A subclasse embute o valor e se registra.
    pub fn new(vm: &VM, structure: StructureRef, context: ConstructionContext) -> JSArrayBufferView {
        JSArrayBufferView {
            base: JSNonFinalObject::new(vm, structure),
            mode: Cell::new(context.mode),
            length: Cell::new(context.length),
            byte_offset: Cell::new(context.byte_offset),
            vector: RefCell::new(context.vector),
            buffer: RefCell::new(context.buffer),
        }
    }

    /// O `TypedArrayType` do `JSType` do cabeçalho da célula (`typedArrayType(type())`).
    pub fn typed_array_type(&self) -> TypedArrayType {
        typed_array_type(self.base.type_())
    }

    /// `mode()`.
    pub fn mode(&self) -> TypedArrayMode {
        self.mode.get()
    }

    /// `hasArrayBuffer()`.
    pub fn has_array_buffer(&self) -> bool {
        self.mode().has_array_buffer()
    }

    /// `existingBufferInButterfly()` e o `m_buffer` do `JSDataView`: o buffer que já existe.
    pub fn existing_buffer(&self) -> Option<ArrayBufferRef> {
        self.buffer.borrow().clone()
    }

    /// `isShared()`.
    pub fn is_shared(&self) -> bool {
        match self.existing_buffer() {
            Some(buffer) => buffer.is_shared(),
            None => false,
        }
    }

    /// `slowDownAndWasteMemory()`: dá um `ArrayBuffer` à visão que só tinha o vetor próprio, passando o
    /// modo a `WastefulTypedArray`.
    fn slow_down_and_waste_memory(&self) -> ArrayBufferRef {
        debug_assert!(!self.has_array_buffer());
        let vector = std::mem::take(&mut *self.vector.borrow_mut());
        let buffer = crate::runtime::array_buffer::ArrayBuffer::create_from_bytes(vector);
        *self.buffer.borrow_mut() = Some(ArrayBufferRef::clone(&buffer));
        // Os modos redimensionáveis nunca começam `Fast` nem `Oversize`.
        self.mode.set(TypedArrayMode::WastefulTypedArray);
        buffer
    }

    /// `possiblySharedBuffer()`: o buffer, criando-o (`slowDownAndWasteMemory`) se a visão ainda não tem.
    pub fn possibly_shared_buffer(&self) -> ArrayBufferRef {
        match self.existing_buffer() {
            Some(buffer) => buffer,
            None => self.slow_down_and_waste_memory(),
        }
    }

    /// `unsharedBuffer()`.
    pub fn unshared_buffer(&self) -> ArrayBufferRef {
        let buffer = self.possibly_shared_buffer();
        assert!(!buffer.is_shared(), "unsharedBuffer de visão sobre SharedArrayBuffer");
        buffer
    }

    /// `isDetached()`: `hasArrayBuffer() && !hasVector()`.
    pub fn is_detached(&self) -> bool {
        self.existing_buffer().is_some_and(|buffer| buffer.is_detached())
    }

    /// `hasVector()`.
    pub fn has_vector(&self) -> bool {
        !self.is_detached()
    }

    /// `isResizableOrGrowableShared()`.
    pub fn is_resizable_or_growable_shared(&self) -> bool {
        self.mode().is_resizable_or_growable_shared()
    }

    /// `isGrowableShared()`.
    pub fn is_growable_shared(&self) -> bool {
        self.mode().is_growable_shared()
    }

    /// `isResizableNonShared()`.
    pub fn is_resizable_non_shared(&self) -> bool {
        self.mode().is_resizable_non_shared()
    }

    /// `isAutoLength()`.
    pub fn is_auto_length(&self) -> bool {
        self.mode().is_auto_length()
    }

    /// `canUseRawFieldsDirectly()`.
    pub fn can_use_raw_fields_directly(&self) -> bool {
        self.mode().can_use_raw_fields_directly()
    }

    /// `byteOffsetRaw()`: 0 depois do `detachFromArrayBuffer`.
    pub fn byte_offset_raw(&self) -> usize {
        if self.is_detached() { 0 } else { self.byte_offset.get() }
    }

    /// `lengthRaw()`: 0 depois do `detachFromArrayBuffer`.
    pub fn length_raw(&self) -> usize {
        if self.is_detached() { 0 } else { self.length.get() }
    }

    /// `byteLengthRaw()`.
    pub fn byte_length_raw(&self) -> usize {
        self.length_raw() << self.typed_array_type().log_element_size()
    }

    /// `isArrayBufferViewOutOfBounds(view, getter)` (https://tc39.es/proposal-resizablearraybuffer/
    /// #sec-isarraybufferviewoutofbounds): vale também para `DataView`.
    pub fn is_array_buffer_view_out_of_bounds(&self) -> bool {
        if self.is_detached() {
            return true;
        }
        if !self.is_resizable_or_growable_shared() {
            return false;
        }
        let Some(buffer) = self.existing_buffer() else { return true };
        let buffer_byte_length = buffer.byte_length();
        let byte_offset_start = self.byte_offset_raw();
        let byte_offset_end =
            if self.is_auto_length() { buffer_byte_length } else { byte_offset_start + self.byte_length_raw() };
        byte_offset_start > buffer_byte_length || byte_offset_end > buffer_byte_length
    }

    /// `isOutOfBounds()`: só os redimensionáveis não compartilhados saem da faixa sem destacar.
    pub fn is_out_of_bounds(&self) -> bool {
        if self.is_detached() {
            return true;
        }
        if !self.is_resizable_non_shared() {
            return false;
        }
        self.is_array_buffer_view_out_of_bounds()
    }

    /// `integerIndexedObjectLength(typedArray, getter)`: `None` é fora da faixa ou destacado.
    pub fn integer_indexed_object_length(&self) -> Option<usize> {
        if self.is_array_buffer_view_out_of_bounds() {
            return None;
        }
        if !self.is_auto_length() {
            return Some(self.length_raw());
        }
        let buffer = self.existing_buffer()?;
        let buffer_byte_length = buffer.byte_length();
        let byte_offset = self.byte_offset_raw();
        Some((buffer_byte_length - byte_offset) >> self.typed_array_type().log_element_size())
    }

    /// `integerIndexedObjectByteLength(typedArray, getter)`.
    pub fn integer_indexed_object_byte_length(&self) -> usize {
        match self.integer_indexed_object_length() {
            None | Some(0) => 0,
            Some(length) => {
                if !self.is_auto_length() {
                    self.byte_length_raw()
                } else {
                    length << self.typed_array_type().log_element_size()
                }
            }
        }
    }

    /// `byteOffset()`.
    pub fn byte_offset(&self) -> usize {
        if self.can_use_raw_fields_directly() {
            return self.byte_offset_raw();
        }
        if self.is_array_buffer_view_out_of_bounds() {
            return 0;
        }
        self.byte_offset_raw()
    }

    /// `length()`.
    pub fn length(&self) -> usize {
        if self.can_use_raw_fields_directly() {
            return self.length_raw();
        }
        self.integer_indexed_object_length().unwrap_or(0)
    }

    /// `byteLength()`.
    pub fn byte_length(&self) -> usize {
        if self.can_use_raw_fields_directly() {
            return self.byte_length_raw();
        }
        self.integer_indexed_object_byte_length()
    }

    /// `inBounds(i)` (`JSGenericTypedArrayView::inBounds`, que só usa o tipo do elemento).
    pub fn in_bounds(&self, index: u64) -> bool {
        if self.can_use_raw_fields_directly() {
            return index < self.length_raw() as u64;
        }
        let Some(buffer) = self.existing_buffer() else { return false };
        let buffer_byte_length = buffer.byte_length();
        let byte_offset = self.byte_offset_raw();
        // `byteLengthRaw` devolve 0 para a visão de comprimento automático.
        let byte_length = self.byte_length_raw() + byte_offset;
        if byte_length > buffer_byte_length {
            return false;
        }
        if self.is_auto_length() {
            let remaining_length = buffer_byte_length - byte_offset;
            return index < (remaining_length >> self.typed_array_type().log_element_size()) as u64;
        }
        index < self.length_raw() as u64
    }

    /// `vector()` como um empréstimo de leitura: o trecho do buffer a partir do `byteOffset` (ou o vetor
    /// próprio). Vazio se a visão foi destacada. Pode ser mais longo que a visão: quem lê confere os
    /// limites com `in_bounds`.
    pub fn with_vector<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        match self.existing_buffer() {
            None => f(&self.vector.borrow()),
            Some(buffer) => {
                let byte_offset = self.byte_offset_raw();
                buffer.with_bytes(|bytes| f(bytes.get(byte_offset..).unwrap_or(&[])))
            }
        }
    }

    /// `with_vector` limitado ao `byteLength` da visão: só o que a visão enxerga, sem o resto do buffer que vem depois
    /// dela (num `subarray` o trecho de `with_vector` continua até o fim do `ArrayBuffer`).
    pub fn with_view_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        let length = self.byte_length();
        self.with_vector(|bytes| f(&bytes[..length.min(bytes.len())]))
    }

    /// `vector()` como um empréstimo de escrita; o mesmo trecho de `with_vector`.
    pub fn with_vector_mut<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        match self.existing_buffer() {
            None => f(&mut self.vector.borrow_mut()),
            Some(buffer) => {
                let byte_offset = self.byte_offset_raw();
                buffer.with_bytes_mut(|bytes| match bytes.get_mut(byte_offset..) {
                    Some(tail) => f(tail),
                    None => f(&mut []),
                })
            }
        }
    }

    /// `getIndexQuicklyAsNativeValue(i)`: o elemento `index`, que tem de estar dentro dos limites.
    pub fn get_element(&self, index: usize) -> NativeElement {
        debug_assert!(self.in_bounds(index as u64));
        let type_ = self.typed_array_type();
        let offset = index << type_.log_element_size();
        self.with_vector(|bytes| read_element(type_, &bytes[offset..]))
    }

    /// `setIndexQuicklyToNativeValue(i, value)`.
    pub fn set_element(&self, index: usize, element: NativeElement) {
        debug_assert!(self.in_bounds(index as u64));
        let type_ = self.typed_array_type();
        let offset = index << type_.log_element_size();
        self.with_vector_mut(|bytes| write_element(type_, &mut bytes[offset..], element));
    }

    /// `canAccessRangeQuickly(offset, length)`: `offset + length <= this->length()` sem estouro.
    pub fn can_access_range_quickly(&self, offset: usize, length: usize) -> bool {
        offset.checked_add(length).is_some_and(|end| end <= self.length())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mode_predicates_match_the_bit_layout() {
        assert!(TypedArrayMode::WastefulTypedArray.can_use_raw_fields_directly());
        assert!(TypedArrayMode::GrowableSharedWastefulTypedArray.can_use_raw_fields_directly());
        assert!(!TypedArrayMode::GrowableSharedAutoLengthWastefulTypedArray.can_use_raw_fields_directly());
        assert!(!TypedArrayMode::ResizableNonSharedWastefulTypedArray.can_use_raw_fields_directly());
        assert!(TypedArrayMode::ResizableNonSharedAutoLengthDataViewMode.is_auto_length());
        assert!(!TypedArrayMode::FastTypedArray.has_array_buffer());
        assert!(TypedArrayMode::DataViewMode.has_array_buffer());
        assert!(TypedArrayMode::GrowableSharedDataViewMode.is_growable_shared());
    }

    #[test]
    fn range_verification() {
        assert!(verify_sub_range_length(16, 8, 2, 4));
        assert!(!verify_sub_range_length(16, 8, 3, 4));
        assert!(!verify_sub_range_length(16, 17, 0, 4));
        assert!(verify_byte_offset_alignment(8, 4));
        assert!(!verify_byte_offset_alignment(6, 4));
    }

    #[test]
    fn construction_context_picks_the_mode_by_length() {
        let fast = ConstructionContext::with_length(FAST_SIZE_LIMIT, 1).unwrap();
        assert_eq!(fast.mode, TypedArrayMode::FastTypedArray);
        let oversize = ConstructionContext::with_length(FAST_SIZE_LIMIT + 1, 1).unwrap();
        assert_eq!(oversize.mode, TypedArrayMode::OversizeTypedArray);
        assert!(ConstructionContext::with_length(usize::MAX, 8).is_none());
    }
}
