//! Porte de `runtime/JSString.h`, `JSString.cpp` e `JSStringInlines.h` (string plana e rope).
//!
//! DIVERGÊNCIA (depende do heap, camada 3): no C++ `JSString` é um `JSCell` alocado no heap do GC e
//! o `JSValue` guarda o ponteiro cru. Enquanto `crate::heap` não existe, a string é um valor
//! compartilhado (`JSStringRef = Rc<JSString>`), e o `JSValue::Cell(usize)` guarda o "endereço"
//! atribuído pelo registro central (`runtime::cell_registry`, espaço único de `cell_id` para todos os
//! tipos de célula), que mantém a string viva (sem coleta). Quando o heap chegar, `JSString` vira
//! célula e `cell_id()` vira o `CellId`.
//!
//! ## Modelo (fatias 1 e 2)
//!
//! O C++ tem duas classes (`JSString` plana e `JSRopeString`) que trocam de forma no lugar:
//! `convertToNonRope` grava a `String` no campo `m_value` e a rope vira uma string plana. Aqui o
//! conteúdo é um `RefCell<StringState>`: `Flat(WtfString)` ou `Rope(RopeFibers)` (comprimento,
//! `is8Bit` e até `s_maxInternalRopeLength = 3` fibras). `length()` e `is_8bit()` leem direto do estado
//! sem resolver. `value()` resolve a rope (`resolveRope`) e a converte em `Flat`, soltando as fibras.
//!
//! * A resolução é ITERATIVA, com pilha explícita (o `resolveToBufferSlow` do C++): o `resolveToBuffer`
//!   recursivo com `stackLimit` não existe aqui, uma rope de milhões de níveis não estoura a pilha nativa.
//!   Preenche o buffer de trás para frente, um buffer só, alocado com `try_vec_with_capacity`.
//! * `Drop` de `JSString` também é iterativo (uma cadeia de ropes soltaria `Rc` recursivamente).
//! * Falta de memória: `try_resolve` devolve `None` (o C++ devolve `nullString()` e, havendo
//!   `globalObject`, lança `throwOutOfMemoryError`). Chamadores com global object convertem o `None`
//!   em `Thrown::OutOfMemory`; `value()` (sem global object) devolve a string nula, como o C++ com
//!   `nullOrGlobalObjectForOOM == nullptr`, e a rope continua rope.
//! * `StringImpl::adopt` copia o `Vec` (não há adoção de buffer no porte), então o pico é o dobro do
//!   comprimento; `build_resolved` sonda a segunda alocação com `try_reserve` antes para a falta de
//!   memória virar `None` e não um abort.
//!
//! ## Plano das fatias seguintes
//!
//! 3. Acessores de atom: `resolveRopeToAtomString` e `resolveRopeToExistingAtomString` em cima de
//!    `resolve_rope_with` (já `pub(crate)`: recebe o `WtfString` resolvido e devolve o que será gravado,
//!    o `Function` do `resolveRopeWithFunction`), mais `JSString::to_identifier`/`view` e o
//!    `maxLengthForOnStackResolve` (2048, só otimização no C++, aqui nenhum efeito além da API).
//! 4. Construtores de rope: `jsString(globalObject, JSString*, JSString*)`, as variantes com `String`,
//!    de 3 operandos, e `RopeBuilder` (`append` com `checkedSum<int32_t>` e `MaxLength`, `expand()` que
//!    funde as três fibras numa rope nova e a recoloca como primeira, `release()` com os casos 0 a 3),
//!    em `operations.rs` (junto de `js_string_concat`, que hoje copia tudo e é quadrático). Strings
//!    vazias são puladas e o limite `MaxLength` devolve `None`/`Thrown::OutOfMemory` como já faz.
//! 5. Migrar chamadores para ropes: `operations.rs` (os 3 pontos que chamam `js_string_concat` com
//!    `.value()` nos operandos, linhas ~117, ~128, ~134), `llint/handlers_misc.rs` (`op_strcat`,
//!    `jsStringFromRegisterArray` com `RopeBuilder`), `js_bound_function.rs:169` (nome `bound `),
//!    `js_function_reify.rs:101` (nome `get `/`set `), `handlers_accessor.rs:162` (nome da função
//!    de acessor). Todo uso de `get_value_impl` sobre valor que pode ser rope passa por `value()` ou
//!    `try_resolve` antes (`dispatch_ext.rs` switch de string, `nodes_codegen_cpp1.rs` só vê constantes
//!    planas).
//! 6. Substring rope: `JSRopeString::createSubstringOfResolved` (base plana, offset e length, sem
//!    copiar; `jsSubstring` passa a devolver substring rope acima do limiar do C++), com uma terceira
//!    forma de `StringState` (`Substring { base, offset, length, is_8bit }`), a travessia do
//!    `resolveToBufferSlow` que trata substring pela `view.substring(offset, length)`, e o
//!    `resolveRopeWithFunction` que, para substring, faz `substringSharingImpl` em vez de montar buffer.

use std::cell::RefCell;
use std::rc::Rc;

use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::class_info::ClassInfo;
use crate::runtime::fallible_alloc::{try_filled_vec, try_vec_with_capacity};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_type::JSType;
use crate::runtime::js_type_info::{
    TypeInfo, INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO, OVERRIDES_GET_OWN_PROPERTY_SLOT, OVERRIDES_PUT,
    STRUCTURE_IS_IMMORTAL,
};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_value::JSValue;
use crate::runtime::structure::{Structure, StructureRef};
use crate::runtime::vm::VM;
use crate::wtf::text::atom_string::{to_existing_atom_string, AtomString};
use crate::wtf::text::string_impl::{CharType, StringImpl};
use crate::wtf::text::wtf_string::String as WtfString;

/// `const ClassInfo JSString::s_info = { "string"_s, nullptr, ... }`.
pub static STRING_S_INFO: ClassInfo = ClassInfo { class_name: "string", parent_class: None, static_prop_hash_table: None, inherits_js_type_range: None };

/// `JSString::StructureFlags` (JSString.h:117).
const STRUCTURE_FLAGS: u32 = OVERRIDES_GET_OWN_PROPERTY_SLOT
    | INTERCEPTS_GET_OWN_PROPERTY_SLOT_BY_INDEX_EVEN_WHEN_LENGTH_IS_NOT_ZERO
    | STRUCTURE_IS_IMMORTAL
    | OVERRIDES_PUT;

/// `JSRopeString::s_maxInternalRopeLength`.
pub const MAX_INTERNAL_ROPE_LENGTH: usize = 3;

/// `JSString::MaxLength` (`std::numeric_limits<int32_t>::max()`).
pub const MAX_LENGTH: u32 = i32::MAX as u32;

/// `JSString::maxLengthForOnStackResolve`: acima disso o C++ resolve a rope sem buffer na pilha. Aqui só
/// separa os dois caminhos de `resolveRopeToExistingAtomString`.
pub const MAX_LENGTH_FOR_ON_STACK_RESOLVE: u32 = 2048;

/// As fibras de uma `JSRopeString`: `m_fiber` (fibra 0), `m_compactFibers` (comprimento e fibras 1 e 2) e
/// o bit `is8BitInPointer`. As fibras presentes são as primeiras (a fibra `i + 1` só existe se a `i`
/// existe), como no C++ onde `fiber1 == nullptr` encerra a rope de uma fibra só.
#[derive(Clone)]
pub struct RopeFibers {
    length: u32,
    is_8bit: bool,
    fibers: [Option<JSStringRef>; MAX_INTERNAL_ROPE_LENGTH],
}

impl RopeFibers {
    /// `JSRopeString::create(vm, s1, s2)`: o comprimento é a soma e `is8Bit` é o E das fibras.
    pub fn from_two(s1: JSStringRef, s2: JSStringRef) -> RopeFibers {
        RopeFibers::from_fibers([Some(s1), Some(s2), None])
    }

    /// `JSRopeString::create(vm, s1, s2, s3)`.
    pub fn from_three(s1: JSStringRef, s2: JSStringRef, s3: JSStringRef) -> RopeFibers {
        RopeFibers::from_fibers([Some(s1), Some(s2), Some(s3)])
    }

    fn from_fibers(fibers: [Option<JSStringRef>; MAX_INTERNAL_ROPE_LENGTH]) -> RopeFibers {
        let mut length: u64 = 0;
        let mut is_8bit = true;
        for fiber in fibers.iter().flatten() {
            length += u64::from(fiber.length());
            is_8bit &= fiber.is_8bit();
        }
        // ASSERT(!sumOverflows<int32_t>(...)): quem monta já limitou por `MaxLength` (RopeBuilder).
        debug_assert!(length != 0 && length <= u64::from(MAX_LENGTH), "rope vazia ou acima de MaxLength");
        RopeFibers { length: length as u32, is_8bit, fibers }
    }

    /// `JSRopeString::length()`.
    pub fn length(&self) -> u32 {
        self.length
    }

    /// `JSRopeString::is8Bit()`.
    pub fn is_8bit(&self) -> bool {
        self.is_8bit
    }

    /// As fibras presentes, em ordem.
    pub fn iter(&self) -> impl Iterator<Item = &JSStringRef> {
        self.fibers.iter().map_while(Option::as_ref)
    }
}

impl std::fmt::Debug for RopeFibers {
    // Sem recursão: uma rope funda não pode ser impressa pelo `Debug` derivado.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "RopeFibers {{ length: {}, is_8bit: {}, fibers: {} }}", self.length, self.is_8bit, self.iter().count())
    }
}

/// O que o `m_value` do C++ guarda: o texto resolvido, ou (rope) as fibras ainda por concatenar.
#[derive(Debug)]
enum StringState {
    Flat(WtfString),
    Rope(RopeFibers),
    /// `JSRopeString` na forma substring (`createSubstringOfResolved`): `base` é sempre plana, e o
    /// texto é `base[offset .. offset + length]`, sem cópia até a resolução.
    Substring { base: JSStringRef, offset: u32, length: u32, is_8bit: bool },
}

/// `class JSString` (com `JSRopeString` como um dos dois estados).
#[derive(Debug)]
pub struct JSString {
    state: RefCell<StringState>,
    cell_id: usize,
    /// `JSCell::m_structureID`: a `vm.stringStructure`.
    structure: StructureRef,
}

/// Referência compartilhada, o `JSString*` do C++.
pub type JSStringRef = Rc<JSString>;

thread_local! {
    /// `SmallStrings::emptyString()`.
    static EMPTY_STRING: RefCell<Option<JSStringRef>> = const { RefCell::new(None) };
}

/// Fim do programa (`cell_registry::reset_program_state`): a string vazia é uma célula do programa
/// (`SmallStrings` vive no `VM` no C++); a próxima chamada cria outra.
pub(crate) fn reset_for_program() {
    let taken = EMPTY_STRING.try_with(|empty| empty.borrow_mut().take());
    drop(taken);
}

/// Tira as fibras de uma rope do estado (que vira a string nula plana) e as empilha.
fn take_fibers(state: &mut StringState, stack: &mut Vec<JSStringRef>) {
    if let StringState::Rope(fibers) = std::mem::replace(state, StringState::Flat(WtfString::default())) {
        stack.extend(fibers.fibers.into_iter().flatten());
    }
}

impl Drop for JSString {
    /// Solta as fibras sem recursão: uma rope de `a + b + c + ...` é uma cadeia profunda de `Rc`, e o
    /// `Drop` padrão estouraria a pilha nativa ao soltar o último elo.
    fn drop(&mut self) {
        if !matches!(self.state.get_mut(), StringState::Rope(_)) {
            return;
        }
        let mut stack = Vec::new();
        take_fibers(self.state.get_mut(), &mut stack);
        while let Some(fiber) = stack.pop() {
            // Só o último dono desmonta o elo; os demais apenas perdem uma referência.
            if let Ok(mut inner) = Rc::try_unwrap(fiber) {
                take_fibers(inner.state.get_mut(), &mut stack);
            }
        }
    }
}

/// `StringView::getCharacters`: copia o texto de `value` para `destination`, alargando Latin1 para 16 bits
/// quando a rope é de 16 bits mas a fibra é de 8.
fn copy_characters<T: CharType>(destination: &mut [T], value: &WtfString) {
    if value.is_8bit() {
        for (slot, character) in destination.iter_mut().zip(value.span8()) {
            *slot = T::from_u16(u16::from(*character));
        }
    } else {
        for (slot, character) in destination.iter_mut().zip(value.span16()) {
            *slot = T::from_u16(*character);
        }
    }
}

/// `JSRopeString::resolveToBuffer` (na forma `resolveToBufferSlow`): preenche `buffer` de trás para
/// frente com uma pilha explícita de fibras. A pilha mantém as fibras vivas (o pai as segura no C++).
fn resolve_to_buffer<T: CharType>(fibers: &RopeFibers, buffer: &mut [T]) {
    let mut position = buffer.len();
    let mut work_queue: Vec<JSStringRef> = fibers.iter().cloned().collect();
    while let Some(current_fiber) = work_queue.pop() {
        let state = current_fiber.state.borrow();
        match &*state {
            StringState::Rope(inner) => work_queue.extend(inner.iter().cloned()),
            StringState::Substring { base, offset, length, .. } => {
                // `view.substring(offset, length)` do `resolveToBufferSlow`.
                let length = *length as usize;
                position -= length;
                let slice = base.flat_value().expect("base de substring plana").substring_sharing_impl(*offset, length as u32);
                copy_characters(&mut buffer[position..position + length], &slice);
            }
            StringState::Flat(value) => {
                let length = value.length() as usize;
                if length == 0 {
                    continue;
                }
                position -= length;
                copy_characters(&mut buffer[position..position + length], value);
            }
        }
    }
    debug_assert_eq!(position, 0, "o comprimento da rope não bate com a soma das fibras");
}

/// `StringImpl::tryCreateUninitialized` mais `resolveRopeInternalNoSubstring`: um buffer só, alocado de
/// uma vez, ou `None` na falta de memória (ou comprimento inválido).
fn build_resolved<T: CharType>(fibers: &RopeFibers) -> Option<WtfString> {
    let length = fibers.length() as usize;
    if !StringImpl::is_valid_length::<T>(length) {
        return None;
    }
    // O buffer é alocado uma vez, aqui.
    let mut buffer: Vec<T> = try_filled_vec(T::from_u16(0), length)?;
    // A cópia que o `StringImpl::adopt` faz precisa de outro bloco do mesmo tamanho: sonda antes de
    // preencher, para a falta de memória aparecer aqui e não como abort dentro do `adopt`.
    drop(try_vec_with_capacity::<T>(length)?);
    resolve_to_buffer(fibers, &mut buffer);
    Some(WtfString::adopt(buffer))
}

impl JSString {
    /// `JSString::createStructure(vm, globalObject, prototype)`: `TypeInfo(StringType, StructureFlags)`.
    pub fn create_structure(vm: &VM, global_object: Option<&JSGlobalObject>, prototype: JSValue) -> StructureRef {
        Structure::create(vm, global_object, prototype, TypeInfo::new(JSType::StringType, STRUCTURE_FLAGS), &STRING_S_INFO)
    }

    /// `JSCell::structure()`.
    pub fn structure(&self) -> &StructureRef {
        &self.structure
    }

    /// Registra a célula com o `state` dado. O `cell_id` vem do registro central (`cell_registry`) e a
    /// `Structure` é a `vm.stringStructure`.
    fn create_with_state(vm: &VM, state: StringState) -> JSStringRef {
        let cell_id = cell_registry::reserve();
        let string = Rc::new(JSString { state: RefCell::new(state), cell_id, structure: vm.string_structure() });
        cell_registry::set(cell_id, CellEntry::String(Rc::clone(&string)));
        string
    }

    /// `JSString::create(vm, String)`.
    fn create(vm: &VM, value: WtfString) -> JSStringRef {
        JSString::create_with_state(vm, StringState::Flat(value))
    }

    /// `JSRopeString::create(vm, s1, s2[, s3])`: uma rope sobre `fibers` (montadas por
    /// [`RopeFibers::from_two`] e [`RopeFibers::from_three`]). Nenhum chamador do porte usa ainda.
    pub fn create_rope(vm: &VM, fibers: RopeFibers) -> JSStringRef {
        let rope = JSString::create_with_state(vm, StringState::Rope(fibers));
        debug_assert!(rope.length() != 0 && rope.is_rope());
        rope
    }

    /// Procura a string pelo `cell_id` guardado em `JSValue::Cell`.
    pub fn from_cell_id(cell_id: usize) -> Option<JSStringRef> {
        match cell_registry::get(cell_id) {
            Some(CellEntry::String(string)) => Some(string),
            _ => None,
        }
    }

    /// O "endereço" da célula, o que o `JSValue` codifica.
    pub fn cell_id(&self) -> usize {
        self.cell_id
    }

    /// `length()`: sem resolver a rope.
    pub fn length(&self) -> u32 {
        match &*self.state.borrow() {
            StringState::Flat(value) => value.length(),
            StringState::Rope(fibers) => fibers.length(),
            StringState::Substring { length, .. } => *length,
        }
    }

    /// `is8Bit()`: sem resolver a rope.
    pub fn is_8bit(&self) -> bool {
        match &*self.state.borrow() {
            StringState::Flat(value) => value.is_8bit(),
            StringState::Rope(fibers) => fibers.is_8bit(),
            StringState::Substring { is_8bit, .. } => *is_8bit,
        }
    }

    /// `isRope()` (a substring também é `JSRopeString`).
    pub fn is_rope(&self) -> bool {
        !matches!(&*self.state.borrow(), StringState::Flat(_))
    }

    /// `isSubstring()`.
    pub fn is_substring(&self) -> bool {
        matches!(&*self.state.borrow(), StringState::Substring { .. })
    }

    /// `JSRopeString::createSubstringOfResolved(vm, nullptr, base, offset, length, is8Bit)`. `base` é plana.
    fn create_substring_of_resolved(vm: &VM, base: &JSStringRef, offset: u32, length: u32) -> JSStringRef {
        debug_assert!(!base.is_rope() && length != 0);
        let is_8bit = base.is_8bit();
        JSString::create_with_state(vm, StringState::Substring { base: Rc::clone(base), offset, length, is_8bit })
    }

    /// `convertToNonRope(String&&)`: a rope vira string plana e solta as fibras.
    fn convert_to_non_rope(&self, value: WtfString) {
        let previous = std::mem::replace(&mut *self.state.borrow_mut(), StringState::Flat(value));
        // As fibras soltam fora do empréstimo (o `Drop` delas é iterativo por conta própria).
        drop(previous);
    }

    /// `resolveRopeWithFunction(nullOrGlobalObjectForOOM, function)`: monta o texto, passa por `function`
    /// (o atom lookup da fatia 3, ou a identidade) e grava o resultado. `None` na falta de memória, e a
    /// rope continua rope. Em string plana devolve o texto como está, sem chamar `function`.
    pub(crate) fn resolve_rope_with(&self, function: impl FnOnce(WtfString) -> WtfString) -> Option<WtfString> {
        if !self.is_rope() {
            return self.flat_value();
        }
        let resolved = function(self.build_rope_text()?);
        self.convert_to_non_rope(resolved.clone());
        Some(resolved)
    }

    /// O texto da string plana, ou `None` numa rope.
    fn flat_value(&self) -> Option<WtfString> {
        match &*self.state.borrow() {
            StringState::Flat(value) => Some(value.clone()),
            StringState::Rope(_) | StringState::Substring { .. } => None,
        }
    }

    /// `resolveRopeInternalNoSubstring` sobre um buffer novo, sem converter a rope: o texto montado, ou
    /// `None` na falta de memória. Em string plana devolve o texto; na substring, `substringSharingImpl`
    /// da base (o `resolveRopeWithFunction` do C++).
    fn build_rope_text(&self) -> Option<WtfString> {
        let fibers = match &*self.state.borrow() {
            StringState::Flat(value) => return Some(value.clone()),
            StringState::Rope(fibers) => fibers.clone(),
            StringState::Substring { base, offset, length, .. } => {
                return Some(base.flat_value()?.substring_sharing_impl(*offset, *length));
            }
        };
        if fibers.is_8bit() { build_resolved::<u8>(&fibers) } else { build_resolved::<u16>(&fibers) }
    }

    /// `swapToAtomString`: a string plana passa a guardar o átomo. DIVERGÊNCIA: sem a lista de strings
    /// acessíveis por threads concorrentes (`appendPossiblyAccessedStringFromConcurrentThreads...`), o
    /// porte tem uma thread só e o texto antigo morre com a troca.
    fn swap_to_atom_string(&self, atom: &AtomString) {
        *self.state.borrow_mut() = StringState::Flat(atom.string().clone());
    }

    /// `JSRopeString::resolveRopeToAtomString`: monta o texto, atomiza e grava o átomo. `None` na falta de
    /// memória. DIVERGÊNCIA: não há o ramo do buffer na pilha (`maxLengthForOnStackResolve`), que só
    /// evita uma alocação; o resultado é o mesmo, e o `reportExtraMemoryAllocated` é do heap (camada 3).
    fn resolve_rope_to_atom_string(&self) -> Option<AtomString> {
        let resolved = self.resolve_rope_with(|text| AtomString::from_string_owned(text).string().clone())?;
        Some(AtomString::from_string(&resolved))
    }

    /// `JSRopeString::resolveRopeToExistingAtomString`: o átomo se o texto já existe na tabela (e então a
    /// rope vira esse átomo), senão o átomo nulo e a rope fica como está (acima de
    /// [`MAX_LENGTH_FOR_ON_STACK_RESOLVE`] o C++ resolve a rope de qualquer jeito, e aqui também).
    /// `None` é a falta de memória.
    fn resolve_rope_to_existing_atom_string(&self) -> Option<AtomString> {
        if self.length() > MAX_LENGTH_FOR_ON_STACK_RESOLVE {
            let mut existing = AtomString::new();
            self.resolve_rope_with(|text| {
                let atom = to_existing_atom_string(&text);
                if atom.is_null() {
                    return text;
                }
                let resolved = atom.string().clone();
                existing = atom;
                resolved
            })?;
            return Some(existing);
        }
        let text = self.build_rope_text()?;
        let atom = to_existing_atom_string(&text);
        if !atom.is_null() {
            self.convert_to_non_rope(atom.string().clone());
        }
        Some(atom)
    }

    /// `JSString::toAtomString(globalObject)`: o átomo do texto, resolvendo a rope. A string plana passa a
    /// guardar o átomo (`swapToAtomString`). `None` é a falta de memória (`Thrown::OutOfMemory`).
    pub fn to_atom_string(&self) -> Option<AtomString> {
        if self.is_rope() {
            return self.resolve_rope_to_atom_string();
        }
        let value = self.flat_value()?;
        let atom = AtomString::from_string(&value);
        if !value.impl_().is_some_and(|string_impl| string_impl.is_atom()) {
            self.swap_to_atom_string(&atom);
        }
        Some(atom)
    }

    /// `JSString::toExistingAtomString(globalObject)`: o átomo só se o texto já está na tabela, senão o
    /// átomo nulo (o `{ }` do C++). `None` é a falta de memória.
    pub fn to_existing_atom_string(&self) -> Option<AtomString> {
        if self.is_rope() {
            return self.resolve_rope_to_existing_atom_string();
        }
        let value = self.flat_value()?;
        let atom = to_existing_atom_string(&value);
        if !atom.is_null() && !value.impl_().is_some_and(|string_impl| string_impl.is_atom()) {
            self.swap_to_atom_string(&atom);
        }
        Some(atom)
    }

    /// `JSString::toIdentifier(globalObject)` (e `JSRopeString::toIdentifier`): `Identifier::fromString`
    /// sobre o átomo. DIVERGÊNCIA: sem o cache `vm.lastAtomizedIdentifierStringImpl`, só otimização.
    /// `None` é a falta de memória.
    pub fn to_identifier(&self, vm: &VM) -> Option<Identifier> {
        Some(Identifier::from_atom_string(vm, &self.to_atom_string()?))
    }

    /// `resolveRope(globalObject)`: o texto, resolvendo a rope se for o caso. `None` é a falta de memória:
    /// quem tem global object lança `Thrown::OutOfMemory` (o `throwOutOfMemoryError` do C++).
    pub fn try_resolve(&self) -> Option<WtfString> {
        self.resolve_rope_with(|resolved| resolved)
    }

    /// `value(globalObject)` sem global object: na falta de memória devolve a string nula, como o C++
    /// com `nullOrGlobalObjectForOOM == nullptr`. Quem pode lançar usa [`JSString::try_resolve`].
    pub fn value(&self) -> WtfString {
        self.try_resolve().unwrap_or_default()
    }

    /// `getValueImpl()` (JSString.h:831). O C++ tem `ASSERT(!isRope())`; aqui, por segurança em release,
    /// uma rope é resolvida. A string nula do WTF (sem `StringImpl`) não ocorre, pois o texto é sempre
    /// criado por `create`. DIVERGÊNCIA: devolve um clone do `Rc` (o estado agora fica atrás de um
    /// `RefCell`, não há referência emprestada que sobreviva).
    pub fn get_value_impl(&self) -> Rc<StringImpl> {
        debug_assert!(!self.is_rope(), "getValueImpl() numa rope");
        let value = self.try_resolve().expect("JSString: sem memória para resolver a rope");
        // Invariante do `create`: o valor guardado nunca é a string nula.
        value.impl_().cloned().expect("JSString sem StringImpl")
    }

    /// `tryGetValueImpl()` (JSString.h:837): `nullptr` quando é rope.
    pub fn try_get_value_impl(&self) -> Option<Rc<StringImpl>> {
        match &*self.state.borrow() {
            StringState::Flat(value) => value.impl_().cloned(),
            StringState::Rope(_) | StringState::Substring { .. } => None,
        }
    }

    /// `tryGetValue()`: resolve a rope (o `allocationAllowed` padrão), nulo na falta de memória.
    pub fn try_get_value(&self) -> WtfString {
        self.value()
    }
}

/// `jsString(vm, const String&)`.
pub fn js_string(vm: &VM, value: &WtfString) -> JSStringRef {
    JSString::create(vm, value.clone())
}

/// `jsOwnedString(vm, const String&)`: o texto não é internado, o mesmo que `js_string` aqui.
pub fn js_owned_string(vm: &VM, value: &WtfString) -> JSStringRef {
    JSString::create(vm, value.clone())
}

/// `jsSubstring(vm, s, offset, length)`: a própria `s` quando pega tudo, a string vazia compartilhada
/// quando `length` é zero.
pub fn js_substring(vm: &VM, string: &JSStringRef, offset: u32, length: u32) -> JSStringRef {
    debug_assert!(offset <= string.length() && length <= string.length() - offset);
    if offset == 0 && length == string.length() {
        return Rc::clone(string);
    }
    if length == 0 {
        return js_empty_string(vm);
    }
    // `tryJSSubstringImpl`/`jsSubstring(globalObject, ...)`: rope que não é substring é resolvida antes.
    // DIVERGÊNCIA: o C++ primeiro desce pelas fibras quando o trecho cabe numa só; aqui resolve direto.
    if string.is_rope() && !string.is_substring() {
        let _ = string.value();
    }
    js_substring_of_resolved(vm, string, offset, length)
}

/// `jsSubstringOfResolved(vm, nullptr, s, offset, length)` (JSStringInlines.h:831). Sem a tabela de
/// `SmallStrings` e o `keyAtomStringCache`, os comprimentos 1 e 2 ficam planos (sem substring rope).
fn js_substring_of_resolved(vm: &VM, string: &JSStringRef, offset: u32, length: u32) -> JSStringRef {
    if length == 0 {
        return js_empty_string(vm);
    }
    let (base, offset) = match &*string.state.borrow() {
        StringState::Substring { base, offset: base_offset, .. } => (Rc::clone(base), offset + base_offset),
        _ => (Rc::clone(string), offset),
    };
    if base.is_rope() {
        // Só se a resolução falhou por falta de memória (a string nula): o C++ nem chega aqui.
        return js_empty_string(vm);
    }
    if offset == 0 && length == base.length() {
        return base;
    }
    if length <= 2 {
        let text = base.flat_value().expect("base plana");
        return JSString::create(vm, text.substring(offset, length));
    }
    JSString::create_substring_of_resolved(vm, &base, offset, length)
}

/// `jsEmptyString(vm)`: uma só instância por thread, como `SmallStrings::emptyString()`.
pub fn js_empty_string(vm: &VM) -> JSStringRef {
    EMPTY_STRING.with(|slot| {
        let mut slot = slot.borrow_mut();
        Rc::clone(slot.get_or_insert_with(|| JSString::create(vm, WtfString::from_latin1(b""))))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(vm: &VM, text: &str) -> JSStringRef {
        js_string(vm, &WtfString::from_latin1(text.as_bytes()))
    }

    #[test]
    fn rope_reports_length_and_width_without_resolving() {
        let vm = VM::new();
        let wide = js_string(&vm, &WtfString::from_utf16(&[0x20ac, 0x61]));
        let rope = JSString::create_rope(&vm, RopeFibers::from_three(flat(&vm, "ab"), wide, flat(&vm, "c")));
        assert!(rope.is_rope());
        assert_eq!(rope.length(), 5);
        assert!(!rope.is_8bit());
        assert!(rope.try_get_value_impl().is_none());
        assert!(rope.is_rope());
    }

    #[test]
    fn resolve_turns_rope_into_flat_text() {
        let vm = VM::new();
        let inner = JSString::create_rope(&vm, RopeFibers::from_two(flat(&vm, "he"), flat(&vm, "llo")));
        let rope = JSString::create_rope(&vm, RopeFibers::from_three(inner, flat(&vm, ", "), flat(&vm, "world")));
        assert_eq!(rope.length(), 12);
        assert!(rope.is_8bit());
        let value = rope.try_resolve().unwrap();
        assert_eq!(value.span8(), b"hello, world");
        assert!(!rope.is_rope());
        assert_eq!(rope.length(), 12);
        assert_eq!(rope.get_value_impl().span8(), b"hello, world");
        assert!(rope.try_get_value_impl().is_some());
    }

    #[test]
    fn resolve_widens_latin1_fibers() {
        let vm = VM::new();
        let narrow = js_string(&vm, &WtfString::from_latin1(&[0xe9, 0x62]));
        let wide = js_string(&vm, &WtfString::from_utf16(&[0x20ac, 0x61]));
        let rope = JSString::create_rope(&vm, RopeFibers::from_two(narrow, wide));
        assert_eq!(rope.value().span16(), &[0xe9, 0x62, 0x20ac, 0x61]);
    }

    #[test]
    fn deep_left_rope_resolves_and_drops_without_recursion() {
        let vm = VM::new();
        let mut rope = flat(&vm, "x");
        for _ in 0..200_000 {
            rope = JSString::create_rope(&vm, RopeFibers::from_two(rope, flat(&vm, "y")));
        }
        assert_eq!(rope.length(), 200_001);
        let value = rope.value();
        assert_eq!(value.length(), 200_001);
        assert_eq!(value.span8()[0], b'x');
        assert!(value.span8()[1..].iter().all(|character| *character == b'y'));
    }
}
