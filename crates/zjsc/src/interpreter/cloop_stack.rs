//! Porte de `interpreter/CLoopStack.h`, `CLoopStackInlines.h` e `CLoopStack.cpp`: a pilha de
//! registradores do interpretador.
//!
//! O build do Bun tem `ENABLE(C_LOOP)` = 0 porque usa o JIT, mas aqui o interpretador de bytecode
//! em Rust ocupa o lugar do CLoop (CONVENTIONS, item 4), então vale o ramo com a pilha de
//! `Register`. O `PageReservation` vira um `Vec` de bits de `Register` alocado zerado (`alloc_zeroed`, mmap
//! preguiçoso: a página só fica residente quando é escrita, como o commit sob demanda do C++; o
//! `Register::default()` tem bits 0) e um "ponteiro" é o índice no vetor (a pilha cresce para índices menores, como o C++ cresce para
//! endereços menores: `highAddress` é `registers.len()`, `reservationTop` é 0). O `commit` e o
//! `decommit` não tocam em memória, só mantêm `m_commitTop` e a contagem de bytes
//! comprometidos, que o C++ também expõe (`committedByteCount`).
//!
//! A pilha é uma só por `VM` e todo mundo a enxerga ao mesmo tempo: o laço de despacho, os slow
//! paths e as funções nativas que reentram no interpretador (getter, setter, callback) escrevem
//! nela pela mesma memória, como o `CLoopStack*` do C++. Por isso o estado tem mutabilidade
//! interior (`RefCell` no vetor, `Cell` nos índices) e cada acesso empresta o vetor só pelo tempo de
//! uma leitura ou escrita: nenhum empréstimo atravessa uma chamada que possa reentrar.
//!
//! Diferenças que não têm comportamento observável: `StackManager::setCLoopStackLimit` (o limite
//! que o gerenciador guarda) e `gatherConservativeRoots` (o heap tem raízes explícitas, CONVENTIONS
//! item 2) não existem; `isSafeToRecurse` recebe o topo do `topCallFrame` do VM como argumento
//! porque o `VM` não é dono desta pilha no porte.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicUsize, Ordering};

use crate::interpreter::register::Register;
use crate::runtime::js_value::EncodedJSValue;
use crate::runtime::options_list::Options;

/// `sizeof(Register)`.
const SIZEOF_REGISTER: isize = 8;

/// `pageSize()` em Linux x86_64.
const PAGE_SIZE: usize = 4096;

/// `committedBytesCount` (guardado por `stackStatisticsMutex` no C++).
static COMMITTED_BYTES_COUNT: AtomicUsize = AtomicUsize::new(0);

/// `commitSize()`: `std::max<size_t>(16 * 1024, pageSize())`.
fn commit_size() -> usize {
    std::cmp::max(16 * 1024, PAGE_SIZE)
}

fn round_up_to_multiple_of(divisor: usize, x: usize) -> usize {
    x.div_ceil(divisor) * divisor
}

/// `class CLoopStack`.
pub struct CLoopStack {
    /// A reserva (`m_reservation`): `registers.len()` é `highAddress()`.
    registers: RefCell<Vec<EncodedJSValue>>,
    /// `highAddress()`, copiado para que ler o limite não empreste o vetor.
    high_address: usize,
    /// `m_end`: o menor endereço da memória alocável pelo JS.
    end: Cell<usize>,
    /// `m_commitTop`: o menor endereço de memória comprometida.
    commit_top: Cell<usize>,
    /// `m_lastStackPointer`.
    last_stack_pointer: Cell<usize>,
    /// `m_currentStackPointer`.
    current_stack_pointer: Cell<usize>,
    /// `m_softReservedZoneSizeInRegisters`.
    soft_reserved_zone_size_in_registers: Cell<isize>,
}

impl CLoopStack {
    /// `Allow 8k of excess registers before we start trying to reap the stack`.
    pub const MAX_EXCESS_CAPACITY: isize = 8 * 1024;

    /// `CLoopStack::CLoopStack()`. O `vm().topCallFrame = 0` é responsabilidade do `VM` que cria
    /// a pilha, porque o `VM` não vive dentro dela.
    pub fn new() -> CLoopStack {
        // `vec![0; n]` vira `alloc_zeroed` (calloc/mmap): as páginas só ficam residentes quando
        // são escritas, como a região do `PageReservation`, que só vira memória de verdade no commit.
        let registers = vec![0 as EncodedJSValue; CLoopStack::reservation_registers()];

        let bottom_of_stack = registers.len();
        let stack = CLoopStack {
            registers: RefCell::new(registers),
            high_address: bottom_of_stack,
            end: Cell::new(bottom_of_stack),
            commit_top: Cell::new(bottom_of_stack),
            last_stack_pointer: Cell::new(bottom_of_stack),
            current_stack_pointer: Cell::new(bottom_of_stack),
            soft_reserved_zone_size_in_registers: Cell::new(0),
        };
        stack.set_cloop_stack_limit(bottom_of_stack);
        debug_assert!(stack.end.get() == bottom_of_stack);
        stack
    }

    /// Tamanho da reserva em registradores, que é também o `highAddress()` de toda pilha: a capacidade
    /// (`maxPerThreadStackUsage` em páginas) arredondada para o `commitSize()`.
    pub fn reservation_registers() -> usize {
        let capacity = round_up_to_multiple_of(PAGE_SIZE, Options::max_per_thread_stack_usage() as usize);
        debug_assert!(capacity != 0 && capacity % PAGE_SIZE == 0);
        round_up_to_multiple_of(commit_size(), capacity) / SIZEOF_REGISTER as usize
    }

    /// `highAddress()`.
    pub fn high_address(&self) -> usize {
        self.high_address
    }

    /// `lowAddress()`.
    pub fn low_address(&self) -> usize {
        self.end.get()
    }

    /// `reservationTop()`.
    pub fn reservation_top(&self) -> usize {
        0
    }

    /// `ensureCapacityFor`.
    pub fn ensure_capacity_for(&self, new_top_of_stack: isize) -> bool {
        if new_top_of_stack >= self.end.get() as isize {
            return true;
        }
        self.grow(new_top_of_stack)
    }

    /// `containsAddress`.
    pub fn contains_address(&self, address: usize) -> bool {
        self.low_address() <= address && address < self.high_address()
    }

    /// `committedByteCount()`.
    pub fn committed_byte_count() -> usize {
        COMMITTED_BYTES_COUNT.load(Ordering::SeqCst)
    }

    /// `sanitizeStack`: zera o que o último uso deixou acima do topo atual.
    pub fn sanitize_stack(&self) {
        let stack_top = self.current_stack_pointer();
        debug_assert!(stack_top <= self.high_address());
        let last_stack_pointer = self.last_stack_pointer.get();
        if last_stack_pointer < stack_top {
            self.registers.borrow_mut()[last_stack_pointer..stack_top].fill(0);
        }
        self.last_stack_pointer.set(stack_top);
    }

    /// `currentStackPointer`.
    pub fn current_stack_pointer(&self) -> usize {
        self.current_stack_pointer.get()
    }

    /// `setCurrentStackPointer`.
    pub fn set_current_stack_pointer(&self, sp: usize) {
        self.current_stack_pointer.set(sp);
    }

    /// `size()`.
    pub fn size(&self) -> usize {
        self.high_address() - self.low_address()
    }

    /// `setSoftReservedZoneSize`.
    pub fn set_soft_reserved_zone_size(&self, reserved_zone_size: usize) {
        self.soft_reserved_zone_size_in_registers.set((reserved_zone_size / SIZEOF_REGISTER as usize) as isize);
        if self.commit_top.get() as isize > self.end.get() as isize - self.soft_reserved_zone_size_in_registers.get() {
            self.grow(self.end.get() as isize);
        }
    }

    /// `isSafeToRecurse`: `top_of_top_call_frame` é `vm().topCallFrame->topOfFrame()` (índice na
    /// pilha) ou `None` quando `vm().topCallFrame` é nulo.
    pub fn is_safe_to_recurse(&self, top_of_top_call_frame: Option<isize>) -> bool {
        let reservation_limit = self.reservation_top() as isize + self.soft_reserved_zone_size_in_registers.get();
        match top_of_top_call_frame {
            None => true,
            Some(top) => top > reservation_limit,
        }
    }

    /// `Register&` na posição `index` da pilha (o `*ptr` do C++).
    ///
    /// Atenção ao fonte hostil (recursão profunda, função com muitos registradores): o índice só fica
    /// dentro da pilha se o chamador antes passou por `is_safe_to_recurse`/`grow` e lançou o
    /// `RangeError` de estouro de pilha como o C++ faz. Este acesso espelha o `*ptr` sem checagem
    /// própria; a defesa mora na checagem de capacidade do chamador.
    pub fn get(&self, index: usize) -> Register {
        Register::from_encoded(self.registers.borrow()[index])
    }

    /// A varredura conservadora da pilha do coletor: todo registrador de `from` até o topo (`highAddress`).
    pub(crate) fn visit_registers(&self, from: usize, marker: &mut crate::runtime::gc::Marker) {
        match self.registers.try_borrow() {
            Ok(registers) => {
                for &encoded in registers.get(from..).unwrap_or_default() {
                    marker.encoded(encoded);
                }
            }
            Err(_) => marker.abort(),
        }
    }

    /// Atribuição ao registrador na posição `index`.
    pub fn set(&self, index: usize, register: Register) {
        self.registers.borrow_mut()[index] = register.encoded_js_value();
    }

    /// `setCLoopStackLimit`: sem o `StackManager`, só o `m_end`.
    fn set_cloop_stack_limit(&self, new_top_of_stack: usize) {
        self.end.set(new_top_of_stack);
    }

    /// `grow`.
    fn grow(&self, new_top_of_stack: isize) -> bool {
        let soft_reserved_zone = self.soft_reserved_zone_size_in_registers.get();
        let new_top_of_stack_with_reserved_zone = new_top_of_stack - soft_reserved_zone;

        // If we have already committed enough memory to satisfy this request,
        // just update the end pointer and return.
        if new_top_of_stack_with_reserved_zone >= self.commit_top.get() as isize {
            self.set_cloop_stack_limit(new_top_of_stack as usize);
            return true;
        }

        // Compute the chunk size of additional memory to commit, and see if we
        // have it still within our budget. If not, we'll fail to grow and
        // return false.
        let delta = (self.commit_top.get() as isize - new_top_of_stack_with_reserved_zone) * SIZEOF_REGISTER;
        let delta = round_up_to_multiple_of(commit_size(), delta as usize) as isize;
        let new_commit_top = self.commit_top.get() as isize - delta / SIZEOF_REGISTER;
        if new_commit_top < self.reservation_top() as isize {
            return false;
        }

        // Otherwise, the growth is still within our budget. Commit it and return true.
        Self::add_to_committed_byte_count(delta);
        self.commit_top.set(new_commit_top as usize);
        let new_top_of_stack = self.commit_top.get() as isize + soft_reserved_zone;
        self.set_cloop_stack_limit(new_top_of_stack as usize);
        true
    }

    /// `releaseExcessCapacity`.
    pub fn release_excess_capacity(&self) {
        let high_address_with_reserved_zone =
            self.high_address() as isize - self.soft_reserved_zone_size_in_registers.get();
        let delta = (high_address_with_reserved_zone - self.commit_top.get() as isize) * SIZEOF_REGISTER;
        Self::add_to_committed_byte_count(-delta);
        self.commit_top.set(high_address_with_reserved_zone as usize);
    }

    /// `addToCommittedByteCount`.
    fn add_to_committed_byte_count(byte_count: isize) {
        let previous = COMMITTED_BYTES_COUNT.fetch_add(byte_count as usize, Ordering::SeqCst);
        debug_assert!(previous as isize + byte_count > -1);
    }
}

impl Default for CLoopStack {
    fn default() -> CLoopStack {
        CLoopStack::new()
    }
}

impl Drop for CLoopStack {
    /// `CLoopStack::~CLoopStack()`.
    fn drop(&mut self) {
        let size_to_decommit = (self.high_address() - self.commit_top.get()) as isize * SIZEOF_REGISTER;
        Self::add_to_committed_byte_count(-size_to_decommit);
    }
}
