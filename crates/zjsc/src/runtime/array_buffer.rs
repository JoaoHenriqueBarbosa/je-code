//! Porte de `runtime/ArrayBuffer.h`, `ArrayBuffer.cpp` e `ArrayBufferSharingMode.h`: o `ArrayBuffer` (e o
//! `SharedArrayBuffer`, que é o mesmo `ArrayBuffer` com o conteúdo compartilhado), o `ArrayBufferContents`,
//! o `SharedArrayBufferContents`, o redimensionamento (`resize`/`grow`), `transferTo` e o `detach`.
//!
//! DIVERGÊNCIAS (sem `unsafe`, sem Gigacage, sem GC e sem WebAssembly):
//!
//! - O armazenamento é `Rc<RefCell<Vec<u8>>>`, e o comprimento do `Vec` é o `m_sizeInBytes`: o `Vec` do
//!   buffer compartilhado (`SharedArrayBufferContents`) é o mesmo `Rc` do `ArrayBufferContents` que o
//!   enxerga, então o `grow` de um é visto por todos. `m_data` nulo (buffer destacado) é `data: None`; o
//!   `ArrayBuffer` de zero byte é `Some(vec![])` (o C++ aloca 1 byte só para o ponteiro não ser nulo).
//! - O ponteiro `data()` não existe: `with_bytes`/`with_bytes_mut` emprestam o conteúdo. A alocação que
//!   falha (`try_reserve_exact`) dá `None`/`Err(OutOfMemory)`, como o `tryAllocate` do C++.
//! - `m_destructor` some (o `Vec` se libera sozinho), junto de `primitiveGigacageDestructor`,
//!   `createAdopted`/`createFromBytes` (adotam um ponteiro de fora; `create_from_bytes` adota um `Vec`),
//!   `createUninitialized`/`tryCreateUninitialized` (a diferença para `create` é só não zerar a memória) e
//!   `offsetOf*`.
//! - O `BufferMemoryHandle` guarda só a contabilidade de páginas (`size` e `mappedCapacity`): a reserva de
//!   endereços, o `BufferMemoryManager` (limite de memória física, `collectAsync`/`collectSync`) e o
//!   `OSAllocator::protect` não existem. O limite de reserva de um buffer redimensionável
//!   (`maxGrowableBufferReservationBytes`, um quarto do orçamento de 64 GB do Gigacage) fica.
//! - `notifyDetaching` (os `JSArrayBufferView` ligados e o `m_detachingWatchpointSet`) some: a visão
//!   pergunta ao buffer se ele foi destacado, em vez de ser avisada. Sem `vm`, então, nos métodos que só o
//!   passavam a ele.
//! - `refreshAfterWasmMemoryGrow`, `Mode::WebAssembly` e o `requirePageMultiple` do `grow` são do
//!   WebAssembly, que não existe. `makeWasmMemory`/`isWasmMemory` ficam para o dia em que ele existir.
//! - `gcSizeEstimateInBytes` e o `GCIncomingRefCounted` servem ao GC. `IdempotentArrayBufferByteLengthGetter`
//!   só guarda a primeira leitura do comprimento para um mesmo `Getter`; as visões leem uma vez só.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

/// `MAX_ARRAY_BUFFER_SIZE` (`PageCount.h`, `USE(LARGE_TYPED_ARRAYS)` com `USE(BUN_JSC_ADDITIONS)`).
pub const MAX_ARRAY_BUFFER_SIZE: u64 = 1 << 32;

/// `PageCount::pageSize`.
pub const PAGE_SIZE: usize = 64 * 1024;

/// `maxGrowableBufferReservationBytes`: `Gigacage::primitiveAddressSpaceBudget / 4`, com o orçamento de
/// 64 GB de `hasCapacityToUseLargeGigacage`.
const MAX_GROWABLE_BUFFER_RESERVATION_BYTES: u64 = 16 * 1024 * 1024 * 1024;

/// `s_lockedFlag` (`INT32_MIN` guardado num `unsigned`).
const LOCKED_FLAG: u32 = 1 << 31;

/// `enum class ArrayBufferSharingMode : bool { Default, Shared }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrayBufferSharingMode {
    Default,
    Shared,
}

impl ArrayBufferSharingMode {
    /// `arrayBufferSharingModeName(sharingMode)`.
    pub fn name(self) -> &'static str {
        match self {
            ArrayBufferSharingMode::Default => "ArrayBuffer",
            ArrayBufferSharingMode::Shared => "SharedArrayBuffer",
        }
    }

    /// O índice dos arrays por modo (`static_cast<unsigned>(sharingMode)`).
    pub fn index(self) -> usize {
        self as usize
    }
}

/// `enum class GrowFailReason` (`BufferMemoryHandle.h`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GrowFailReason {
    InvalidDelta,
    InvalidGrowSize,
    WouldExceedMaximum,
    OutOfMemory,
    GrowSharedUnavailable,
}

/// `roundUpToMultipleOf<PageCount::pageSize>(bytes)`.
fn round_up_to_page(bytes: usize) -> u64 {
    (bytes as u64).div_ceil(PAGE_SIZE as u64) * PAGE_SIZE as u64
}

/// `Gigacage::tryZeroedMalloc`: `None` quando a alocação falha.
fn try_zeroed_vec(len: usize) -> Option<Vec<u8>> {
    crate::runtime::fallible_alloc::try_zeroed_bytes(len)
}

/// `Gigacage::tryMalloc` seguido de `memcpy`.
fn try_copied_vec(source: &[u8]) -> Option<Vec<u8>> {
    let mut data = Vec::new();
    data.try_reserve_exact(source.len()).ok()?;
    data.extend_from_slice(source);
    Some(data)
}

/// Muda o comprimento do `Vec` (`m_sizeInBytes`); o que cresce nasce zerado (`zeroFill`). `false` se a
/// alocação falhou, e então nada mudou.
fn try_resize_zeroed(data: &RefCell<Vec<u8>>, new_byte_length: usize) -> bool {
    let mut data = data.borrow_mut();
    let additional = new_byte_length.saturating_sub(data.len());
    if additional > 0 {
        if data.try_reserve_exact(additional).is_err() {
            return false;
        }
        data.resize(new_byte_length, 0);
    } else {
        data.truncate(new_byte_length);
    }
    true
}

/// `BufferMemoryHandle` reduzido à contabilidade de páginas (ver as DIVERGÊNCIAS).
#[derive(Debug)]
pub struct BufferMemoryHandle {
    /// `m_size`: sempre múltiplo de página.
    size: Cell<usize>,
    /// `m_mappedCapacity`.
    mapped_capacity: usize,
}

impl BufferMemoryHandle {
    /// `size()`.
    pub fn size(&self) -> usize {
        self.size.get()
    }

    /// `mappedCapacity()`.
    pub fn mapped_capacity(&self) -> usize {
        self.mapped_capacity
    }

    /// O trecho comum a `ArrayBuffer::resize` e `SharedArrayBufferContents::tryGrow` depois de conferido o
    /// tamanho: o limite do mapeamento, o limite de `MAX_ARRAY_BUFFER_SIZE`, a mudança de comprimento (que
    /// zera o que cresce) e o `updateSize` do `m_size` em páginas.
    fn resize(&self, data: &RefCell<Vec<u8>>, new_byte_length: usize) -> Result<(), GrowFailReason> {
        // A maxByteLength may exceed the region actually mapped for it, so growth is bounded by the mapping.
        if new_byte_length > self.mapped_capacity {
            return Err(GrowFailReason::WouldExceedMaximum);
        }

        let desired_size = round_up_to_page(new_byte_length);
        if desired_size > MAX_ARRAY_BUFFER_SIZE {
            return Err(GrowFailReason::WouldExceedMaximum);
        }

        if !try_resize_zeroed(data, new_byte_length) {
            return Err(GrowFailReason::OutOfMemory);
        }
        if desired_size as usize != self.size.get() {
            self.size.set(desired_size as usize);
        }
        Ok(())
    }
}

/// `tryAllocateResizableMemory(vm, sizeInBytes, maxByteLength)`.
fn try_allocate_resizable_memory(size_in_bytes: usize, max_byte_length: usize) -> Option<Rc<BufferMemoryHandle>> {
    // Make sure malloc actually allocates something, but not too much. We use null to mean that the buffer is detached.
    let mut initial_bytes = round_up_to_page(size_in_bytes);
    if initial_bytes == 0 {
        initial_bytes = PAGE_SIZE as u64;
    }
    let mut maximum_bytes = round_up_to_page(max_byte_length);
    if maximum_bytes == 0 {
        maximum_bytes = PAGE_SIZE as u64;
    }

    // The whole maximum is reserved up front while only the initial size is charged against the
    // physical budget, so without this a single buffer could claim all the address space there is.
    if maximum_bytes > MAX_GROWABLE_BUFFER_RESERVATION_BYTES {
        return None;
    }

    Some(Rc::new(BufferMemoryHandle { size: Cell::new(initial_bytes as usize), mapped_capacity: maximum_bytes as usize }))
}

/// `class SharedArrayBufferContents`: o conteúdo que vários `ArrayBuffer` enxergam, com o comprimento que
/// só cresce (`grow`).
#[derive(Debug)]
pub struct SharedArrayBufferContents {
    /// `m_data` e `m_sizeInBytes` (o comprimento do `Vec`).
    data: Rc<RefCell<Vec<u8>>>,
    /// `m_maxByteLength` com `m_hasMaxByteLength`.
    max_byte_length: Option<usize>,
    /// `m_memoryHandle`.
    memory_handle: Option<Rc<BufferMemoryHandle>>,
}

impl SharedArrayBufferContents {
    /// `create(data, maxByteLength, memoryHandle, destructor, mode)`.
    pub fn create(
        data: Rc<RefCell<Vec<u8>>>,
        max_byte_length: Option<usize>,
        memory_handle: Option<Rc<BufferMemoryHandle>>,
    ) -> Rc<SharedArrayBufferContents> {
        assert!(max_byte_length.unwrap_or(data.borrow().len()) as u64 <= MAX_ARRAY_BUFFER_SIZE);
        debug_assert!(max_byte_length.is_none() || memory_handle.is_some());
        Rc::new(SharedArrayBufferContents { data, max_byte_length, memory_handle })
    }

    /// `sizeInBytes(order)`.
    pub fn size_in_bytes(&self) -> usize {
        self.data.borrow().len()
    }

    /// `maxByteLength()`.
    pub fn max_byte_length(&self) -> Option<usize> {
        self.max_byte_length
    }

    /// `memoryHandle()`.
    pub fn memory_handle(&self) -> Option<&Rc<BufferMemoryHandle>> {
        self.memory_handle.as_ref()
    }

    /// `grow(vm, newByteLength, requirePageMultiple)` e `tryGrow` (a coleta de lixo que o `tryGrow` pede
    /// depois de soltar o lock não existe): devolve o quanto cresceu.
    pub fn grow(&self, new_byte_length: usize) -> Result<i64, GrowFailReason> {
        let Some(max_byte_length) = self.max_byte_length else {
            return Err(GrowFailReason::GrowSharedUnavailable);
        };
        let memory_handle = self.memory_handle.as_ref().expect("buffer compartilhado crescível sem memoryHandle");

        // Keep in mind that newByteLength may not be page-size-aligned.
        let size_in_bytes = self.size_in_bytes();
        if size_in_bytes > new_byte_length || max_byte_length < new_byte_length {
            return Err(GrowFailReason::InvalidGrowSize);
        }

        let delta_byte_length = (new_byte_length - size_in_bytes) as i64;
        if delta_byte_length == 0 {
            return Ok(0);
        }

        memory_handle.resize(&self.data, new_byte_length)?;
        Ok(delta_byte_length)
    }
}

/// `class ArrayBufferContents`.
#[derive(Debug, Default)]
pub struct ArrayBufferContents {
    /// `m_data` (`None` é nulo: o buffer destacado) e `m_sizeInBytes` (o comprimento do `Vec`).
    data: Option<Rc<RefCell<Vec<u8>>>>,
    /// `m_shared`.
    shared: Option<Rc<SharedArrayBufferContents>>,
    /// `m_memoryHandle`.
    memory_handle: Option<Rc<BufferMemoryHandle>>,
    /// `m_maxByteLength`.
    max_byte_length: usize,
    /// `m_hasMaxByteLength`.
    has_max_byte_length: bool,
    /// Comprimento próprio, independente do `Vec`: o `SharedArrayBuffer` de uma `WebAssembly.Memory` compartilhada
    /// tem comprimento fixo e o `grow` da memória não o altera (o buffer antigo mantém o comprimento antigo).
    fixed_length: Option<usize>,
}

impl ArrayBufferContents {
    /// `ArrayBufferContents(data, sizeInBytes, maxByteLength, destructor)`: adota o `Vec`.
    pub fn from_data(data: Vec<u8>, max_byte_length: Option<usize>) -> ArrayBufferContents {
        assert!(data.len() as u64 <= MAX_ARRAY_BUFFER_SIZE);
        ArrayBufferContents {
            max_byte_length: max_byte_length.unwrap_or(data.len()),
            has_max_byte_length: max_byte_length.is_some(),
            data: Some(Rc::new(RefCell::new(data))),
            shared: None,
            memory_handle: None,
            fixed_length: None,
        }
    }

    /// `ArrayBufferContents(Ref<SharedArrayBufferContents>&&, forceFixedLengthIfWasm)`.
    pub fn from_shared(shared: Rc<SharedArrayBufferContents>) -> ArrayBufferContents {
        let size_in_bytes = shared.size_in_bytes();
        assert!(size_in_bytes as u64 <= MAX_ARRAY_BUFFER_SIZE);
        ArrayBufferContents {
            data: Some(Rc::clone(&shared.data)),
            memory_handle: shared.memory_handle().cloned(),
            has_max_byte_length: shared.max_byte_length().is_some(),
            max_byte_length: shared.max_byte_length().unwrap_or(size_in_bytes),
            shared: Some(shared),
            fixed_length: None,
        }
    }

    /// `ArrayBufferContents(data, sizeInBytes, maxByteLength, Ref<BufferMemoryHandle>&&)`.
    pub fn from_resizable(data: Vec<u8>, max_byte_length: usize, memory_handle: Rc<BufferMemoryHandle>) -> ArrayBufferContents {
        assert!(data.len() as u64 <= MAX_ARRAY_BUFFER_SIZE);
        ArrayBufferContents {
            data: Some(Rc::new(RefCell::new(data))),
            shared: None,
            memory_handle: Some(memory_handle),
            max_byte_length,
            has_max_byte_length: true,
            fixed_length: None,
        }
    }

    /// `ArrayBufferContents::fromSpan(data)`: copia os bytes.
    pub fn from_span(data: &[u8]) -> Option<ArrayBufferContents> {
        Some(ArrayBufferContents::from_data(try_copied_vec(data)?, None))
    }

    /// `explicit operator bool`: há dados (não é destacado nem falhou a alocação).
    pub fn has_data(&self) -> bool {
        self.data.is_some()
    }

    /// `sizeInBytes(order)`.
    pub fn size_in_bytes(&self) -> usize {
        match (&self.data, self.fixed_length) {
            (Some(_), Some(fixed)) => fixed,
            (Some(data), None) => data.borrow().len(),
            (None, _) => 0,
        }
    }

    /// `maxByteLength()`.
    pub fn max_byte_length(&self) -> Option<usize> {
        if self.has_max_byte_length { Some(self.max_byte_length) } else { None }
    }

    /// `isShared()`.
    pub fn is_shared(&self) -> bool {
        self.shared.is_some()
    }

    /// `isResizableOrGrowableShared()`.
    pub fn is_resizable_or_growable_shared(&self) -> bool {
        self.has_max_byte_length
    }

    /// `isGrowableShared()`.
    pub fn is_growable_shared(&self) -> bool {
        self.is_resizable_or_growable_shared() && self.is_shared()
    }

    /// `isResizableNonShared()`.
    pub fn is_resizable_non_shared(&self) -> bool {
        self.is_resizable_or_growable_shared() && !self.is_shared()
    }

    /// `span()`: empresta os bytes (vazio se destacado).
    pub fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        match &self.data {
            Some(data) => f(&data.borrow()),
            None => f(&[]),
        }
    }

    /// `mutableSpan()`.
    pub fn with_bytes_mut<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        match &self.data {
            Some(data) => f(&mut data.borrow_mut()),
            None => f(&mut []),
        }
    }

    /// `detach()`: tira o conteúdo e deixa o `m_hasMaxByteLength` (o `m_maxByteLength` zera, a informação de
    /// que o buffer era redimensionável fica).
    pub fn detach(&mut self) -> ArrayBufferContents {
        let contents = std::mem::take(self);
        self.has_max_byte_length = contents.has_max_byte_length;
        contents
    }

    /// `shareWith(other)`: `other` enxerga o mesmo conteúdo compartilhado.
    pub fn share_with(&self, other: &mut ArrayBufferContents) {
        debug_assert!(other.data.is_none());
        debug_assert!(self.shared.is_some());
        other.data = self.data.clone();
        other.shared = self.shared.clone();
        other.memory_handle = self.memory_handle.clone();
        other.max_byte_length = self.max_byte_length;
        other.has_max_byte_length = self.has_max_byte_length;
        assert!(other.size_in_bytes() as u64 <= MAX_ARRAY_BUFFER_SIZE);
        debug_assert!(other.max_byte_length as u64 <= MAX_ARRAY_BUFFER_SIZE);
    }

    /// `reset()`.
    fn reset(&mut self) {
        *self = ArrayBufferContents::default();
    }

    /// `tryAllocate(numElements, elementByteSize, policy)`: o conteúdo nasce zerado (a política
    /// `DontInitialize` só deixaria de zerar). Na falha o conteúdo fica vazio (`reset`).
    fn try_allocate(&mut self, num_elements: usize, element_byte_size: usize) {
        let Some(size_in_bytes) = num_elements.checked_mul(element_byte_size).filter(|size| *size as u64 <= MAX_ARRAY_BUFFER_SIZE)
        else {
            self.reset();
            return;
        };
        let Some(data) = try_zeroed_vec(size_in_bytes) else {
            self.reset();
            return;
        };

        self.data = Some(Rc::new(RefCell::new(data)));
        self.max_byte_length = size_in_bytes;
        self.has_max_byte_length = false;
    }

    /// `makeShared()`: o `Vec` passa a ser também do `SharedArrayBufferContents`.
    fn make_shared(&mut self) {
        let data = self.data.clone().expect("makeShared em buffer destacado");
        self.shared = Some(SharedArrayBufferContents::create(data, self.max_byte_length(), self.memory_handle.clone()));
    }

    /// `copyTo(other)`: uma cópia não compartilhada dos bytes.
    fn copy_to(&self, other: &mut ArrayBufferContents) {
        debug_assert!(other.data.is_none());
        let copy = self.with_bytes(try_copied_vec);
        match copy {
            Some(copy) => {
                other.data = Some(Rc::new(RefCell::new(copy)));
                other.max_byte_length = other.size_in_bytes();
                other.has_max_byte_length = false;
            }
            None => other.reset(),
        }
    }
}

/// `class ArrayBuffer`.
pub struct ArrayBuffer {
    /// `m_contents`.
    contents: RefCell<ArrayBufferContents>,
    /// `m_wrapper` (`Weak<JSArrayBuffer>`): o `cell_id` do `JSArrayBuffer` que embrulha este buffer.
    wrapper: Cell<Option<usize>>,
    /// `m_pinCount`.
    pin_count: Cell<u32>,
    /// `m_isWasmMemory`.
    is_wasm_memory: Cell<bool>,
    /// Bytes deste buffer somados em `LIVE_BUFFER_BYTES` (diagnóstico de vazamento; ver `live_buffer_bytes`).
    accounted_bytes: Cell<usize>,
}

thread_local! {
    /// Soma dos bytes dos `ArrayBuffer` vivos nesta thread: sobe quando o buffer nasce ou cresce, desce no
    /// `Drop`, no destacamento e no encolhimento. Dados compartilhados (SharedArrayBuffer, memória do
    /// WebAssembly) contam uma vez por `ArrayBuffer` que os enxerga.
    static LIVE_BUFFER_BYTES: Cell<usize> = const { Cell::new(0) };
}

/// Bytes de `ArrayBuffer` vivos nesta thread. Serve para separar vazamento real de fragmentação do alocador.
pub fn live_buffer_bytes() -> usize {
    LIVE_BUFFER_BYTES.with(Cell::get)
}

impl ArrayBuffer {
    /// Acerta `LIVE_BUFFER_BYTES` pelo comprimento atual do buffer.
    fn account_bytes(&self) {
        let current = self.contents.try_borrow().map(|contents| contents.size_in_bytes()).unwrap_or(self.accounted_bytes.get());
        let previous = self.accounted_bytes.replace(current);
        LIVE_BUFFER_BYTES.with(|total| total.set((total.get() + current).saturating_sub(previous)));
    }
}

impl Drop for ArrayBuffer {
    fn drop(&mut self) {
        let previous = self.accounted_bytes.replace(0);
        LIVE_BUFFER_BYTES.with(|total| total.set(total.get().saturating_sub(previous)));
    }
}

/// O `Ref<ArrayBuffer>`/`RefPtr<ArrayBuffer>`.
pub type ArrayBufferRef = Rc<ArrayBuffer>;

impl fmt::Debug for ArrayBuffer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ArrayBuffer")
            .field("byte_length", &self.byte_length())
            .field("max_byte_length", &self.max_byte_length())
            .field("shared", &self.is_shared())
            .field("detached", &self.is_detached())
            .finish()
    }
}

impl ArrayBuffer {
    /// `ArrayBuffer(ArrayBufferContents&&)` e `create(ArrayBufferContents&&)`.
    pub fn new(contents: ArrayBufferContents) -> ArrayBufferRef {
        let buffer = Rc::new(ArrayBuffer {
            contents: RefCell::new(contents),
            wrapper: Cell::new(None),
            pin_count: Cell::new(0),
            is_wasm_memory: Cell::new(false),
            accounted_bytes: Cell::new(0),
        });
        buffer.account_bytes();
        buffer
    }

    /// `create(numElements, elementByteSize)`: falha de alocação é `CRASH()`.
    pub fn create(num_elements: usize, element_byte_size: usize) -> ArrayBufferRef {
        ArrayBuffer::try_create(num_elements, element_byte_size, None).expect("ArrayBuffer::create: falha de alocação")
    }

    /// `create(span)`: copia os bytes; falha de alocação é `CRASH()`.
    pub fn create_from_span(span: &[u8]) -> ArrayBufferRef {
        ArrayBuffer::try_create_from_span(span).expect("ArrayBuffer::create: falha de alocação")
    }

    /// `createFromBytes` sobre os bytes de uma `wasm::Memory` (`Memory::createBufferContents`): o buffer
    /// enxerga o mesmo `Vec` que a memória, é `makeWasmMemory` e só `detach` (o `grow`) o solta.
    pub fn create_from_wasm_memory(data: Rc<RefCell<Vec<u8>>>) -> ArrayBufferRef {
        let size = data.borrow().len();
        let contents = ArrayBufferContents {
            data: Some(data),
            shared: None,
            memory_handle: None,
            max_byte_length: size,
            has_max_byte_length: false,
            fixed_length: None,
        };
        let buffer = ArrayBuffer::new(contents);
        buffer.make_wasm_memory();
        buffer
    }

    /// O `SharedArrayBuffer` de uma `WebAssembly.Memory` compartilhada: comprimento fixo (`fixed_length`, o tamanho
    /// da memória quando o buffer foi criado), `growable` falso e `maxByteLength` igual ao comprimento. Os bytes
    /// são o `Vec` da memória; depois do `grow` o buffer antigo mantém o comprimento antigo.
    pub fn create_from_wasm_memory_shared(data: Rc<RefCell<Vec<u8>>>) -> ArrayBufferRef {
        let size = data.borrow().len();
        let shared = SharedArrayBufferContents::create(Rc::clone(&data), None, None);
        let contents = ArrayBufferContents {
            data: Some(data),
            shared: Some(shared),
            memory_handle: None,
            max_byte_length: size,
            has_max_byte_length: false,
            fixed_length: Some(size),
        };
        ArrayBuffer::new(contents)
    }

    /// O `SharedArrayBuffer` crescível de `toResizableBuffer` numa memória compartilhada (medido no bun 1.4.2:
    /// `growable` verdadeiro, `maxByteLength` do descritor, acompanha o `Vec` quando a memória cresce).
    pub fn create_from_wasm_memory_growable_shared(data: Rc<RefCell<Vec<u8>>>, max_byte_length: usize) -> ArrayBufferRef {
        let shared = SharedArrayBufferContents::create(Rc::clone(&data), Some(max_byte_length), None);
        let contents = ArrayBufferContents {
            data: Some(data),
            shared: Some(shared),
            memory_handle: None,
            max_byte_length,
            has_max_byte_length: true,
            fixed_length: None,
        };
        ArrayBuffer::new(contents)
    }

    /// Como `create_from_wasm_memory`, mas redimensionável (`toResizableBuffer`): `max_byte_length` vem do
    /// descritor da memória e o buffer acompanha o `Vec` compartilhado quando a memória cresce. Também é
    /// `makeWasmMemory` (não destacável por `transfer`).
    pub fn create_from_wasm_memory_resizable(data: Rc<RefCell<Vec<u8>>>, max_byte_length: usize) -> ArrayBufferRef {
        let contents = ArrayBufferContents {
            data: Some(data),
            shared: None,
            memory_handle: None,
            max_byte_length,
            has_max_byte_length: true,
            fixed_length: None,
        };
        let buffer = ArrayBuffer::new(contents);
        buffer.make_wasm_memory();
        buffer
    }

    /// `resize` de um buffer redimensionável de `Memory.toResizableBuffer`: cresce os bytes da própria memória
    /// (o `Vec` é o mesmo), zerando o que entra. Quem chama já conferiu encolhimento e múltiplo de página.
    pub fn resize_wasm_memory(&self, new_byte_length: usize) -> Result<(), GrowFailReason> {
        debug_assert!(self.is_wasm_memory());
        let contents = self.contents.borrow();
        let Some(data) = &contents.data else {
            return Err(GrowFailReason::GrowSharedUnavailable);
        };
        if new_byte_length > contents.max_byte_length {
            return Err(GrowFailReason::InvalidGrowSize);
        }
        if !try_resize_zeroed(data, new_byte_length) {
            return Err(GrowFailReason::OutOfMemory);
        }
        drop(contents);
        self.account_bytes();
        Ok(())
    }

    /// `createFromBytes(data, destructor)`: adota o `Vec`.
    pub fn create_from_bytes(data: Vec<u8>) -> ArrayBufferRef {
        ArrayBuffer::new(ArrayBufferContents::from_data(data, None))
    }

    /// `createShared(shared, forceFixedLengthIfWasm)`.
    pub fn create_shared(shared: Rc<SharedArrayBufferContents>) -> ArrayBufferRef {
        ArrayBuffer::new(ArrayBufferContents::from_shared(shared))
    }

    /// `tryCreate(numElements, elementByteSize, maxByteLength)`.
    pub fn try_create(num_elements: usize, element_byte_size: usize, max_byte_length: Option<usize>) -> Option<ArrayBufferRef> {
        let Some(max_byte_length) = max_byte_length else {
            let mut contents = ArrayBufferContents::default();
            contents.try_allocate(num_elements, element_byte_size);
            if !contents.has_data() {
                return None;
            }
            return Some(ArrayBuffer::new(contents));
        };

        let size_in_bytes = num_elements.checked_mul(element_byte_size).filter(|size| *size as u64 <= MAX_ARRAY_BUFFER_SIZE)?;
        if size_in_bytes > max_byte_length || max_byte_length as u64 > MAX_ARRAY_BUFFER_SIZE {
            return None;
        }

        let handle = try_allocate_resizable_memory(size_in_bytes, max_byte_length)?;
        let data = try_zeroed_vec(size_in_bytes)?;
        Some(ArrayBuffer::new(ArrayBufferContents::from_resizable(data, max_byte_length, handle)))
    }

    /// `tryCreate(span)`: copia os bytes.
    pub fn try_create_from_span(span: &[u8]) -> Option<ArrayBufferRef> {
        let mut contents = ArrayBufferContents::default();
        contents.try_allocate(span.len(), 1);
        if !contents.has_data() {
            return None;
        }
        contents.with_bytes_mut(|bytes| bytes.copy_from_slice(span));
        Some(ArrayBuffer::new(contents))
    }

    /// `tryCreateShared(vm, numElements, elementByteSize, maxByteLength)`.
    pub fn try_create_shared(num_elements: usize, element_byte_size: usize, max_byte_length: usize) -> Option<ArrayBufferRef> {
        let size_in_bytes = num_elements.checked_mul(element_byte_size)?;
        if size_in_bytes > max_byte_length || max_byte_length as u64 > MAX_ARRAY_BUFFER_SIZE {
            return None;
        }

        let handle = try_allocate_resizable_memory(size_in_bytes, max_byte_length)?;
        let data = Rc::new(RefCell::new(try_zeroed_vec(size_in_bytes)?));
        Some(ArrayBuffer::create_shared(SharedArrayBufferContents::create(data, Some(max_byte_length), Some(handle))))
    }

    /// `byteLength(order)`.
    pub fn byte_length(&self) -> usize {
        self.contents.borrow().size_in_bytes()
    }

    /// `maxByteLength()`.
    pub fn max_byte_length(&self) -> Option<usize> {
        self.contents.borrow().max_byte_length()
    }

    /// `span()`: empresta os bytes (vazio se destacado).
    pub fn with_bytes<R>(&self, f: impl FnOnce(&[u8]) -> R) -> R {
        self.contents.borrow().with_bytes(f)
    }

    /// `mutableSpan()`.
    pub fn with_bytes_mut<R>(&self, f: impl FnOnce(&mut [u8]) -> R) -> R {
        self.contents.borrow().with_bytes_mut(f)
    }

    /// `isShared()`.
    pub fn is_shared(&self) -> bool {
        self.contents.borrow().is_shared()
    }

    /// `sharingMode()`.
    pub fn sharing_mode(&self) -> ArrayBufferSharingMode {
        if self.is_shared() { ArrayBufferSharingMode::Shared } else { ArrayBufferSharingMode::Default }
    }

    /// `isResizableOrGrowableShared()`.
    pub fn is_resizable_or_growable_shared(&self) -> bool {
        self.contents.borrow().is_resizable_or_growable_shared()
    }

    /// `isFixedLength()`.
    pub fn is_fixed_length(&self) -> bool {
        !self.is_resizable_or_growable_shared()
    }

    /// `isGrowableShared()`.
    pub fn is_growable_shared(&self) -> bool {
        self.contents.borrow().is_growable_shared()
    }

    /// `isResizableNonShared()`.
    pub fn is_resizable_non_shared(&self) -> bool {
        self.contents.borrow().is_resizable_non_shared()
    }

    /// `isDetached()`.
    pub fn is_detached(&self) -> bool {
        !self.contents.borrow().has_data()
    }

    /// `m_wrapper.get()`: o `cell_id` do `JSArrayBuffer`. O `Weak` do C++ vira `None` quando o coletor solta o
    /// wrapper (a conferência de geração do registro: o id varrido não acha mais a célula).
    pub fn wrapper(&self) -> Option<usize> {
        self.wrapper.get().filter(|cell_id| crate::runtime::cell_registry::get_for_marking(*cell_id).is_some())
    }

    /// `m_wrapper = Weak<JSArrayBuffer>(&wrapper, ...)` (o `registerWrapper` do `TypedArrayController`).
    pub fn set_wrapper(&self, cell_id: usize) {
        debug_assert!(self.wrapper().is_none());
        self.wrapper.set(Some(cell_id));
    }

    /// `clampValue(x, left, right)`.
    fn clamp_value(x: f64, left: usize, right: usize) -> usize {
        debug_assert!(left <= right);
        let mut x = x;
        if x < left as f64 {
            x = left as f64;
        }
        if (right as f64) < x {
            x = right as f64;
        }
        x as usize
    }

    /// `clampIndex(index)`.
    fn clamp_index(&self, index: f64) -> usize {
        let current_length = self.byte_length();
        let mut index = index;
        if index < 0.0 {
            index += current_length as f64;
        }
        ArrayBuffer::clamp_value(index, 0, current_length)
    }

    /// `slice(begin, end)`.
    pub fn slice(&self, begin: f64, end: f64) -> Option<ArrayBufferRef> {
        self.slice_with_clamped_index(self.clamp_index(begin), self.clamp_index(end))
    }

    /// `slice(begin)`.
    pub fn slice_to_end(&self, begin: f64) -> Option<ArrayBufferRef> {
        self.slice_with_clamped_index(self.clamp_index(begin), self.byte_length())
    }

    /// `sliceWithClampedIndex(begin, end)`.
    pub fn slice_with_clamped_index(&self, begin: usize, end: usize) -> Option<ArrayBufferRef> {
        let size = if begin <= end { end - begin } else { 0 };
        let result = self.with_bytes(|bytes| ArrayBuffer::try_create_from_span(&bytes[begin..begin + size]));
        if let Some(result) = &result {
            result.set_sharing_mode(self.sharing_mode());
        }
        result
    }

    /// `makeShared()`.
    fn make_shared(&self) {
        self.contents.borrow_mut().make_shared();
        self.pin_and_lock();
        debug_assert!(!self.is_detached());
    }

    /// `setSharingMode(newSharingMode)`.
    pub fn set_sharing_mode(&self, new_sharing_mode: ArrayBufferSharingMode) {
        if new_sharing_mode == self.sharing_mode() {
            return;
        }
        assert!(!self.is_shared(), "não se desfaz o compartilhamento");
        assert_eq!(new_sharing_mode, ArrayBufferSharingMode::Shared);
        self.make_shared();
    }

    /// `pin()`.
    pub fn pin(&self) {
        self.pin_count.set(self.pin_count.get().checked_add(1).expect("estouro de m_pinCount"));
    }

    /// `unpin()`: preserva o bit de trava.
    pub fn unpin(&self) {
        let old = self.pin_count.get();
        self.pin_count.set(old.wrapping_sub(1) | (old & LOCKED_FLAG));
    }

    /// `isDetachable()`.
    pub fn is_detachable(&self) -> bool {
        self.pin_count.get() == 0 && !self.is_shared()
    }

    /// `pinAndLock()`: o buffer nunca mais é destacável.
    pub fn pin_and_lock(&self) {
        self.pin_count.set(self.pin_count.get() | LOCKED_FLAG);
    }

    /// `makeWasmMemory()`.
    pub fn make_wasm_memory(&self) {
        self.is_wasm_memory.set(true);
        self.pin_and_lock();
        debug_assert!(!self.is_detachable());
    }

    /// `isWasmMemory()`.
    pub fn is_wasm_memory(&self) -> bool {
        self.is_wasm_memory.get()
    }

    /// `shareWith(result)`: `false` (e `result` vazio) se o buffer foi destacado ou não é compartilhado.
    pub fn share_with(&self, result: &mut ArrayBufferContents) -> bool {
        let contents = self.contents.borrow();
        if !contents.has_data() || !contents.is_shared() {
            result.data = None;
            return false;
        }

        contents.share_with(result);
        true
    }

    /// `transferTo(vm, result)`: passa o conteúdo para `result` (e destaca o buffer), compartilha-o, ou, se
    /// o buffer não é destacável, copia-o. `false` (e `result` vazio) se não havia o que passar.
    pub fn transfer_to(&self, result: &mut ArrayBufferContents) -> bool {
        if !self.contents.borrow().has_data() {
            result.data = None;
            return false;
        }

        if self.is_shared() {
            self.contents.borrow().share_with(result);
            return true;
        }

        if !self.is_detachable() {
            self.contents.borrow().copy_to(result);
            return result.has_data();
        }

        *result = self.contents.borrow_mut().detach();
        self.account_bytes();
        true
    }

    /// `detach(vm)`: destaca mesmo o buffer travado (a memória do WebAssembly).
    pub fn detach(&self) {
        let _unused = self.contents.borrow_mut().detach();
        self.account_bytes();
    }

    /// `grow(vm, newByteLength)`: o `grow` do `SharedArrayBuffer` crescível.
    pub fn grow(&self, new_byte_length: usize) -> Result<i64, GrowFailReason> {
        let shared = self.contents.borrow().shared.clone();
        let Some(shared) = shared else {
            return Err(GrowFailReason::GrowSharedUnavailable);
        };
        let delta = shared.grow(new_byte_length)?;
        self.account_bytes();
        Ok(delta)
    }

    /// `resize(vm, newByteLength)`: o `resize` do `ArrayBuffer` redimensionável.
    pub fn resize(&self, new_byte_length: usize) -> Result<i64, GrowFailReason> {
        assert!(!self.is_wasm_memory());

        let contents = self.contents.borrow();
        let (Some(memory_handle), None, Some(data)) = (&contents.memory_handle, &contents.shared, &contents.data) else {
            return Err(GrowFailReason::GrowSharedUnavailable);
        };

        // Keep in mind that newByteLength may not be page-size-aligned.
        if contents.max_byte_length < new_byte_length {
            return Err(GrowFailReason::InvalidGrowSize);
        }

        let delta_byte_length = new_byte_length as i64 - data.borrow().len() as i64;
        if delta_byte_length == 0 {
            return Ok(0);
        }

        memory_handle.resize(data, new_byte_length)?;
        drop(contents);
        self.account_bytes();
        Ok(delta_byte_length)
    }
}

/// `errorMessageForTransfer(buffer)`.
pub fn error_message_for_transfer(buffer: &ArrayBuffer) -> &'static str {
    debug_assert!(!buffer.is_detachable());
    if buffer.is_shared() {
        return "Cannot transfer a SharedArrayBuffer";
    }
    if buffer.is_wasm_memory() {
        return "Cannot transfer a WebAssembly.Memory";
    }
    "Cannot transfer an ArrayBuffer whose backing store has been accessed by the JavaScriptCore C API"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn filled(bytes: &[u8]) -> ArrayBufferRef {
        ArrayBuffer::create_from_span(bytes)
    }

    #[test]
    fn create_is_zeroed_and_slice_clamps() {
        let buffer = ArrayBuffer::create(4, 2);
        assert_eq!(buffer.byte_length(), 8);
        assert!(buffer.with_bytes(|bytes| bytes.iter().all(|byte| *byte == 0)));

        let buffer = filled(&[1, 2, 3, 4, 5]);
        let slice = buffer.slice(1.0, -1.0).unwrap();
        assert_eq!(slice.with_bytes(|bytes| bytes.to_vec()), vec![2, 3, 4]);
        let slice = buffer.slice_to_end(-2.0).unwrap();
        assert_eq!(slice.with_bytes(|bytes| bytes.to_vec()), vec![4, 5]);
        assert_eq!(buffer.slice(4.0, 2.0).unwrap().byte_length(), 0);
        assert_eq!(buffer.slice(f64::NAN, f64::INFINITY).unwrap().byte_length(), 5);
    }

    #[test]
    fn oversized_allocation_fails_without_aborting() {
        assert!(ArrayBuffer::try_create(usize::MAX, 1, None).is_none());
        assert!(ArrayBuffer::try_create(usize::MAX, 2, None).is_none());
        assert!(ArrayBuffer::try_create((MAX_ARRAY_BUFFER_SIZE + 1) as usize, 1, None).is_none());
        assert!(ArrayBuffer::try_create(8, 1, Some(4)).is_none());
    }

    #[test]
    fn transfer_detaches_and_keeps_resizability() {
        let buffer = ArrayBuffer::try_create(4, 1, Some(16)).unwrap();
        assert!(buffer.is_resizable_non_shared());
        buffer.with_bytes_mut(|bytes| bytes.copy_from_slice(&[9, 8, 7, 6]));

        let mut contents = ArrayBufferContents::default();
        assert!(buffer.transfer_to(&mut contents));
        assert!(buffer.is_detached());
        assert_eq!(buffer.byte_length(), 0);
        assert!(buffer.is_resizable_or_growable_shared());
        assert_eq!(buffer.max_byte_length(), Some(0));

        let moved = ArrayBuffer::new(contents);
        assert_eq!(moved.with_bytes(|bytes| bytes.to_vec()), vec![9, 8, 7, 6]);
        assert!(!buffer.transfer_to(&mut ArrayBufferContents::default()));
    }

    #[test]
    fn pinned_buffer_is_copied_not_detached() {
        let buffer = filled(&[1, 2, 3]);
        buffer.pin();
        assert!(!buffer.is_detachable());
        let mut contents = ArrayBufferContents::default();
        assert!(buffer.transfer_to(&mut contents));
        assert!(!buffer.is_detached());
        buffer.unpin();
        assert!(buffer.is_detachable());
        assert_eq!(ArrayBuffer::new(contents).with_bytes(|bytes| bytes.to_vec()), vec![1, 2, 3]);

        buffer.pin_and_lock();
        buffer.pin();
        buffer.unpin();
        assert!(!buffer.is_detachable());
    }

    #[test]
    fn resize_zero_fills_what_regrows() {
        let buffer = ArrayBuffer::try_create(4, 1, Some(8)).unwrap();
        buffer.with_bytes_mut(|bytes| bytes.copy_from_slice(&[1, 2, 3, 4]));
        assert_eq!(buffer.resize(2), Ok(-2));
        assert_eq!(buffer.resize(6), Ok(4));
        assert_eq!(buffer.with_bytes(|bytes| bytes.to_vec()), vec![1, 2, 0, 0, 0, 0]);
        assert_eq!(buffer.resize(6), Ok(0));
        assert_eq!(buffer.resize(9), Err(GrowFailReason::InvalidGrowSize));
        assert_eq!(filled(&[1]).resize(1), Err(GrowFailReason::GrowSharedUnavailable));
    }

    #[test]
    fn growable_shared_buffer_only_grows_and_is_seen_by_every_sharer() {
        let buffer = ArrayBuffer::try_create_shared(2, 1, 8).unwrap();
        assert!(buffer.is_shared() && buffer.is_growable_shared());
        let mut other = ArrayBufferContents::default();
        assert!(buffer.share_with(&mut other));
        let other = ArrayBuffer::new(other);

        assert_eq!(buffer.grow(5), Ok(3));
        assert_eq!(other.byte_length(), 5);
        assert_eq!(buffer.grow(4), Err(GrowFailReason::InvalidGrowSize));
        assert_eq!(buffer.grow(9), Err(GrowFailReason::InvalidGrowSize));
        assert_eq!(buffer.grow(5), Ok(0));
        assert_eq!(filled(&[1]).grow(2), Err(GrowFailReason::GrowSharedUnavailable));
    }

    #[test]
    fn making_a_buffer_shared_locks_it() {
        let buffer = filled(&[1, 2]);
        buffer.set_sharing_mode(ArrayBufferSharingMode::Shared);
        assert!(buffer.is_shared() && !buffer.is_detachable());
        assert!(!buffer.is_resizable_or_growable_shared());
        assert_eq!(error_message_for_transfer(&buffer), "Cannot transfer a SharedArrayBuffer");
        assert_eq!(buffer.slice(0.0, 1.0).unwrap().sharing_mode(), ArrayBufferSharingMode::Shared);
    }

    #[test]
    fn resizable_memory_is_bounded_by_the_reservation_limit() {
        assert!(ArrayBuffer::try_create(0, 1, Some(MAX_ARRAY_BUFFER_SIZE as usize)).is_some());
        assert!(ArrayBuffer::try_create(0, 1, Some(MAX_ARRAY_BUFFER_SIZE as usize + 1)).is_none());
        assert!(ArrayBuffer::try_create_shared(0, 1, MAX_ARRAY_BUFFER_SIZE as usize + 1).is_none());
    }
}
