//! Tradução de `wasm/WasmInstance.cpp` e da parte de ligação de `js/JSWebAssemblyInstance.cpp` e
//! `js/WebAssemblyModuleRecord.cpp` (`finalizeCreation`, `evaluate`): importações, memórias,
//! tabelas, globais, segmentos de elementos e de dados, função `start`, exports, e o
//! `ConstExprHost` do `wasm_const_expr_interpreter`.
//!
//! O que fica de fora, e por quê:
//! - Uma referência de função é um `u64` com `FUNC_REF_TAG` e a posição no registro `FuncRefRegistry`, que
//!   guarda `(instância, índice no espaço de funções)` e o cache do wrapper JS (identidade estável). O
//!   `call_indirect` de função de OUTRA instância ainda cai em `BadSignature` (plano em `wip/notes/wasm-js.md`).
//! - Uma função importada é um `HostFunction` (o que `WasmToJS` chama): recebe os argumentos como
//!   `u64` e devolve os resultados. Uma exceção de JS vira `WasmError::Runtime` com o texto dela.
//! - Objetos GC de expressão constante (`struct.new`, `array.new*`) entram em `Instance::gc_cells`
//!   (o vetor é criado antes das globais e movido para a instância). Um campo ou elemento `v128`
//!   ocupa dois slots `u64` consecutivos (baixo, alto), como os locais `v128` do laço.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::js_value::js_null;
use crate::runtime::js_web_assembly_tag::TagData;
use crate::wasm::page_count::PageCount;
use crate::wasm::wasm_const_expr_interpreter::{evaluate_extended_const_expr, ConstExprHost, ConstExprValue};
use crate::wasm::wasm_format::{
    ElementInitializationType, ExternalKind, GlobalInitialBits, GlobalInitializationType, I32InitExpr,
    PackedType, SegmentKind, StorageType, TableInitializationType, TypeKind,
};
use crate::wasm::wasm_global::Global;
use crate::wasm::wasm_memory::Memory;
use crate::wasm::wasm_module_information::{ModuleInformation, StructuralType};
use crate::wasm::wasm_table::Table;

/// O bit que separa uma referência de função de `null` e de um `externref`.
pub const FUNC_REF_TAG: u64 = 1 << 40;

/// `jsNull()` codificado: a referência nula, a mesma que o `ConstExprInterpreter` produz.
pub fn null_ref() -> u64 {
    js_null().encode() as u64
}

/// O identificador de uma instância, atribuído em `Instance::instantiate`; o `instance` de `(instância, índice)`.
pub type InstanceId = u32;

/// O registro de referências de função do thread (um por VM): `FuncRefTable`/`Instance::functionWrapper` do C++.
/// Uma referência é `FUNC_REF_TAG | posição` no registro e cada par `(instância, índice no espaço de funções)`
/// tem uma só posição, então a mesma função tem sempre o mesmo `u64` (a identidade que `t.get(0) === exports.add`
/// precisa). Vale entre instâncias, Table e Global. Cresce sem coleta.
#[derive(Default)]
struct FuncRefRegistry {
    targets: Vec<(InstanceId, u32)>,
    positions: std::collections::HashMap<(InstanceId, u32), u32>,
    /// `Instance::getFunctionWrapper`/`setFunctionWrapper`: o `JSValue` codificado do wrapper JS por posição.
    wrappers: std::collections::HashMap<u32, u64>,
    next_instance: InstanceId,
    /// A instância dona de cada `InstanceId`, fraca: o registro não mantém a instância viva (sem ciclo de
    /// vazamento entre a tabela, que guarda referências, e a instância, que guarda a tabela).
    owners: std::collections::HashMap<InstanceId, std::rc::Weak<Instance>>,
}

thread_local! {
    static FUNC_REFS: RefCell<FuncRefRegistry> = RefCell::new(FuncRefRegistry::default());
}

/// Fim do programa (`cell_registry::reset_program_state`): os wrappers JS (`JSValue` codificado) e as
/// instâncias fracas são do programa; os `InstanceId` recomeçam junto com o registro de funções e o heap GC.
pub(crate) fn reset_for_program() {
    let taken = FUNC_REFS.try_with(|registry| std::mem::take(&mut *registry.borrow_mut()));
    drop(taken);
    let _ = SHARED_GC_HEAP.try_with(|heap| {
        let cells = std::mem::take(&mut *heap.borrow_mut());
        drop(cells);
    });
}

/// Um identificador novo de instância.
pub fn allocate_instance_id() -> InstanceId {
    FUNC_REFS.with(|registry| {
        let mut registry = registry.borrow_mut();
        registry.next_instance += 1;
        registry.next_instance
    })
}

/// Registra a instância já em `Rc` como dona das referências de função com o `id` dela.
pub fn register_instance(instance: &Rc<Instance>) {
    FUNC_REFS.with(|registry| registry.borrow_mut().owners.insert(instance.id, Rc::downgrade(instance)));
}

/// A instância dona das referências com este `id`, `None` se ainda não foi registrada ou já foi liberada.
pub fn instance_of(id: InstanceId) -> Option<Rc<Instance>> {
    FUNC_REFS.with(|registry| registry.borrow().owners.get(&id).and_then(std::rc::Weak::upgrade))
}

/// A referência da função `function_index_space` da instância `instance`, a mesma a cada chamada.
pub fn func_ref(instance: InstanceId, function_index_space: u32) -> u64 {
    FUNC_REFS.with(|registry| {
        let mut registry = registry.borrow_mut();
        let key = (instance, function_index_space);
        if let Some(position) = registry.positions.get(&key) {
            return FUNC_REF_TAG | u64::from(*position);
        }
        let position = registry.targets.len() as u32;
        registry.targets.push(key);
        registry.positions.insert(key, position);
        FUNC_REF_TAG | u64::from(position)
    })
}

/// A instância dona e o índice no espaço de funções dela, `None` para qualquer outra coisa.
pub fn func_ref_target(reference: u64) -> Option<(InstanceId, u32)> {
    if reference & FUNC_REF_TAG != 0 && reference >> 41 == 0 {
        FUNC_REFS.with(|registry| registry.borrow().targets.get(reference as u32 as usize).copied())
    } else {
        None
    }
}

/// O índice de uma função da instância `instance`, `None` se a referência é de outra instância ou não é função.
pub fn func_ref_local(instance: InstanceId, reference: u64) -> Option<u32> {
    func_ref_target(reference).filter(|(owner, _)| *owner == instance).map(|(_, index)| index)
}

/// A referência é uma função (de qualquer instância).
pub fn is_func_ref(reference: u64) -> bool {
    func_ref_target(reference).is_some()
}

/// `Instance::getFunctionWrapper`: o wrapper JS (`JSValue` codificado) já criado para a função, se houver.
pub fn function_wrapper(reference: u64) -> Option<u64> {
    FUNC_REFS.with(|registry| registry.borrow().wrappers.get(&(reference as u32)).copied())
}

/// `Instance::setFunctionWrapper`: guarda o wrapper JS da função, para o próximo `ToJSValue` devolver o mesmo.
pub fn set_function_wrapper(reference: u64, wrapper: u64) {
    FUNC_REFS.with(|registry| registry.borrow_mut().wrappers.insert(reference as u32, wrapper));
}

/// Os wrappers JS do registro (o `JSValue` codificado de cada um), que o coletor mantém: a identidade da função
/// entre instâncias, Table e Global depende de o mesmo wrapper voltar.
pub fn function_wrappers() -> Vec<u64> {
    FUNC_REFS.with(|registry| registry.borrow().wrappers.values().copied().collect())
}

/// `ref.i31`: o bit 43 marca o inteiro de 31 bits (os 31 bits baixos); não colide com a função (bit 40),
/// o `exnref` (bit 42) nem com o `null` e as células do JS.
pub const I31_REF_TAG: u64 = 1 << 43;

/// A referência `i31ref` de um inteiro (só os 31 bits baixos ficam).
pub fn i31_ref(value: u32) -> u64 {
    I31_REF_TAG | u64::from(value & 0x7fff_ffff)
}

/// Os 31 bits de uma referência `i31ref`, `None` para qualquer outra coisa.
pub fn i31_ref_value(reference: u64) -> Option<u32> {
    if reference >> 31 == I31_REF_TAG >> 31 { Some(reference as u32 & 0x7fff_ffff) } else { None }
}

/// Uma referência a objeto GC (struct ou array): o bit 44 marca, os bits baixos são o índice em `Instance::gc_cells`.
/// Não colide com a função (bit 40), o `exnref` (bit 42), o i31 (bit 43) nem com o `null`.
pub const GC_REF_TAG: u64 = 1 << 44;

/// A referência do objeto GC na posição dada do heap da instância.
pub fn gc_ref(index: usize) -> u64 {
    GC_REF_TAG | index as u64
}

/// O índice de uma referência a objeto GC, `None` para qualquer outra coisa.
pub fn gc_ref_index(reference: u64) -> Option<usize> {
    if reference >> 32 == GC_REF_TAG >> 32 { Some(reference as u32 as usize) } else { None }
}

/// Uma célula do heap de objetos GC (`JSWebAssemblyStruct`/`JSWebAssemblyArray`): o tipo canônico (RTT) e os
/// valores crus dos campos ou elementos (campos packed já truncados).
#[derive(Clone, Debug)]
pub enum GcCell {
    Struct { canonical_type: u32, fields: Vec<u64> },
    Array { canonical_type: u32, elements: Vec<u64> },
}

/// O registro de células GC compartilhado: uma referência GC (`GC_REF_TAG | índice`) é um índice global neste
/// registro e vale entre instâncias, Table e Global. Cresce sem coleta.
pub type GcHeap = Rc<RefCell<Vec<GcCell>>>;

thread_local! {
    /// O registro do thread (um por VM, como `EXTERN_VALUES` da ponte JS).
    static SHARED_GC_HEAP: GcHeap = Rc::new(RefCell::new(Vec::new()));
}

/// O registro de células GC compartilhado por todas as instâncias do thread.
pub fn shared_gc_heap() -> GcHeap {
    SHARED_GC_HEAP.with(Rc::clone)
}

/// Uma referência a objeto GC atende ao tipo heap abstrato `kind` (`any`, `eq`, `struct`, `array`), sem módulo:
/// o que a Table/Global/importação (sem instância dona) consegue conferir. `None` se não é referência a objeto GC.
pub fn gc_abstract_matches(reference: u64, kind: TypeKind) -> Option<bool> {
    let index = gc_ref_index(reference)?;
    let heap = shared_gc_heap();
    let cells = heap.borrow();
    let cell = cells.get(index)?;
    Some(match kind {
        TypeKind::Anyref | TypeKind::Eqref => true,
        TypeKind::Structref => matches!(cell, GcCell::Struct { .. }),
        TypeKind::Arrayref => matches!(cell, GcCell::Array { .. }),
        _ => false,
    })
}

impl GcCell {
    pub fn canonical_type(&self) -> u32 {
        match self {
            GcCell::Struct { canonical_type, .. } | GcCell::Array { canonical_type, .. } => *canonical_type,
        }
    }
}

/// Uma função do hospedeiro (o que `WasmToJS` chama).
/// O erro é um `WasmError`: uma exceção de JS que atravessa a importação vira `WasmError::JsException`.
pub type HostFunction = Rc<dyn Fn(&[u64]) -> Result<Vec<u64>, WasmError>>;

/// O que o JS entrega para cada `Import`, na ordem de `ModuleInformation::imports`.
#[derive(Clone)]
pub enum ImportValue {
    Function(HostFunction),
    Memory(Rc<RefCell<Memory>>),
    Table(Rc<RefCell<Table>>),
    Global(Rc<RefCell<Global>>),
    Tag(Rc<RefCell<TagData>>),
}

/// O que cada export aponta.
#[derive(Clone)]
pub enum ExportValue {
    /// Índice no espaço de funções.
    Function(u32),
    Memory(Rc<RefCell<Memory>>),
    Table(Rc<RefCell<Table>>),
    Global(Rc<RefCell<Global>>),
    Tag(Rc<RefCell<TagData>>),
}

/// Uma tag com identidade: duas `TagRef` são iguais só se apontam para a mesma tag (`Rc::ptr_eq`).
#[derive(Clone)]
pub struct TagRef(pub Rc<RefCell<TagData>>);

impl PartialEq for TagRef {
    fn eq(&self, other: &TagRef) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for TagRef {}

impl std::fmt::Debug for TagRef {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "TagRef({:p})", Rc::as_ptr(&self.0))
    }
}

/// O valor JS lançado por uma importação (o `JSValue` fica opaco para a camada wasm). Duas são iguais só se
/// são a mesma captura (`Rc::ptr_eq`).
#[derive(Clone)]
pub struct JsThrown(pub Rc<dyn std::any::Any>);

impl PartialEq for JsThrown {
    fn eq(&self, other: &JsThrown) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for JsThrown {}

impl std::fmt::Debug for JsThrown {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "JsThrown({:p})", Rc::as_ptr(&self.0))
    }
}

/// `WebAssembly.LinkError`, `WebAssembly.RuntimeError` ou `RangeError` (falta de memória), e a exceção
/// wasm em voo (`throw`): a tag e o payload em bits, que o `try`/`catch` do `wasm_ipint` captura.
/// `JsException` é uma exceção de JS que atravessa o wasm (o `JSTag` do C++): só `catch_all` e
/// `catch_all_ref` a capturam, e ao sair do wasm ela é relançada como o valor original.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WasmError {
    Link(String),
    Runtime(String),
    /// Um `TypeError` do JS (a fronteira com o host recusou um valor, como `v128` numa importação).
    Type(String),
    OutOfMemory,
    Exception { tag: TagRef, payload: Vec<u64> },
    JsException(JsThrown),
    /// Pedido de suspensão (JSPI): uma importação `WebAssembly.Suspending` devolveu uma promessa. Nunca é lançado
    /// ao wasm; `invoke_resumable` o transforma em `Completion::Suspended`. O conteúdo é opaco para esta camada.
    Suspend(JsThrown),
}

/// O bit que marca um `exnref` (índice em `Instance::exnrefs`), distinto de `FUNC_REF_TAG`.
pub const EXNREF_TAG: u64 = 1 << 42;

/// Os parâmetros da assinatura da tag `exception_index` do espaço de tags.
fn tag_parameters(info: &ModuleInformation, exception_index: usize) -> Vec<crate::wasm::wasm_format::Type> {
    match &info.rtt_from_exception_index_space(exception_index).structural {
        StructuralType::Function { arguments, .. } => arguments.clone(),
        _ => unreachable!("tag com tipo que não é de função"),
    }
}

/// `importFailMessage`.
fn import_fail_message(import: &crate::wasm::wasm_format::Import, before: &str, after: &str) -> String {
    format!("{} {}:{} {}", before, import.module, import.field, after)
}

/// Uma instância de módulo.
pub struct Instance {
    /// O `instance` das referências de função desta instância (`allocate_instance_id`).
    pub(super) id: InstanceId,
    pub(super) info: Rc<ModuleInformation>,
    pub(super) host_functions: Vec<HostFunction>,
    pub(super) memories: Vec<Rc<RefCell<Memory>>>,
    pub(super) tables: Vec<Rc<RefCell<Table>>>,
    pub(super) globals: Vec<Rc<RefCell<Global>>>,
    /// O espaço de tags: as importadas primeiro, depois as da seção Tag.
    pub(super) tags: Vec<Rc<RefCell<TagData>>>,
    /// As exceções capturadas por `catch_ref`/`catch_all_ref`: um `exnref` é `EXNREF_TAG | índice`.
    /// Cresce sem coleta (o GC de `exnref` depende do modelo de células, fatia 37).
    pub(super) exnrefs: RefCell<Vec<WasmError>>,
    /// O heap de objetos GC (struct e array): uma referência é `GC_REF_TAG | índice`. Cresce sem coleta.
    pub(super) gc_cells: GcHeap,
    pub(super) dropped_data: RefCell<Vec<bool>>,
    pub(super) dropped_elements: RefCell<Vec<bool>>,
    pub(super) compiled: RefCell<Vec<Option<Rc<super::wasm_ipint::CompiledFunction>>>>,
    pub(super) call_depth: Cell<u32>,
}

/// Limite de um array GC em bytes (o do bun, medido: 2^30), o mesmo do `array.new` do laço.
const CONST_MAX_GC_ARRAY_BYTES: u64 = 1 << 30;

/// O `ConstExprHost` das expressões constantes: lê as globais já criadas e aloca no heap GC.
struct InstanceConstHost<'a> {
    instance: InstanceId,
    info: &'a ModuleInformation,
    globals: &'a [Rc<RefCell<Global>>],
    cells: &'a RefCell<Vec<GcCell>>,
}

/// Os slots de um campo ou elemento: o valor truncado se for packed, dois slots (baixo, alto) se for `v128`.
fn push_const_slots(storage: StorageType, value: &ConstExprValue, slots: &mut Vec<u64>) {
    match value {
        ConstExprValue::Vector(vector) => {
            let low = u64::from_le_bytes(vector[..8].try_into().expect("8 bytes"));
            let high = u64::from_le_bytes(vector[8..].try_into().expect("8 bytes"));
            slots.push(low);
            slots.push(high);
        }
        ConstExprValue::Numeric(bits) | ConstExprValue::Ref(bits) => slots.push(match storage {
            StorageType::Packed(PackedType::I8) => bits & 0xff,
            StorageType::Packed(PackedType::I16) => bits & 0xffff,
            StorageType::Type(_) => *bits,
        }),
    }
}

/// O tamanho em bytes de um elemento de array (para o limite de 2^30 bytes).
fn const_element_bytes(storage: StorageType) -> u64 {
    match storage {
        StorageType::Packed(PackedType::I8) => 1,
        StorageType::Packed(PackedType::I16) => 2,
        StorageType::Type(ty) => match ty.kind {
            TypeKind::I32 | TypeKind::F32 => 4,
            TypeKind::V128 => 16,
            _ => 8,
        },
    }
}

impl ConstExprHost for InstanceConstHost<'_> {
    fn load_global(&mut self, index: u32, is_v128: bool) -> ConstExprValue {
        let global = self.globals[index as usize].borrow();
        if is_v128 {
            ConstExprValue::Vector(global.get_vector())
        } else {
            ConstExprValue::Numeric(global.get())
        }
    }

    fn ref_func(&mut self, function_index_space: u32) -> u64 {
        func_ref(self.instance, function_index_space)
    }

    fn extern_internalize(&mut self, reference: u64) -> u64 {
        reference
    }

    fn struct_new(&mut self, type_index: u32, fields: Option<&[ConstExprValue]>) -> Option<u64> {
        let StructuralType::Struct { fields: declared } = &self.info.rtt(type_index as usize).structural else {
            unreachable!("struct.new de um tipo que não é struct");
        };
        let mut slots = Vec::with_capacity(declared.len());
        for (position, field) in declared.iter().enumerate() {
            match fields {
                Some(values) => push_const_slots(field.ty, &values[position], &mut slots),
                None => push_const_slots(field.ty, &self.default_value(field.ty), &mut slots),
            }
        }
        let canonical_type = self.info.canonical_type_id(type_index as usize);
        let mut cells = self.cells.borrow_mut();
        cells.push(GcCell::Struct { canonical_type, fields: slots });
        Some(gc_ref(cells.len() - 1))
    }

    fn array_new(&mut self, type_index: u32, size: u32, value: &ConstExprValue) -> Option<u64> {
        let StructuralType::Array { element } = &self.info.rtt(type_index as usize).structural else {
            unreachable!("array.new de um tipo que não é array");
        };
        if u64::from(size).checked_mul(const_element_bytes(element.ty)).is_none_or(|bytes| bytes > CONST_MAX_GC_ARRAY_BYTES) {
            return None;
        }
        let mut one = Vec::with_capacity(2);
        push_const_slots(element.ty, value, &mut one);
        let elements = one.iter().copied().cycle().take(one.len() * size as usize).collect();
        let canonical_type = self.info.canonical_type_id(type_index as usize);
        let mut cells = self.cells.borrow_mut();
        cells.push(GcCell::Array { canonical_type, elements });
        Some(gc_ref(cells.len() - 1))
    }

    fn array_new_fixed(&mut self, type_index: u32, elements: &[ConstExprValue]) -> Option<u64> {
        let StructuralType::Array { element } = &self.info.rtt(type_index as usize).structural else {
            unreachable!("array.new_fixed de um tipo que não é array");
        };
        let mut slots = Vec::with_capacity(elements.len());
        for value in elements {
            push_const_slots(element.ty, value, &mut slots);
        }
        let canonical_type = self.info.canonical_type_id(type_index as usize);
        let mut cells = self.cells.borrow_mut();
        cells.push(GcCell::Array { canonical_type, elements: slots });
        Some(gc_ref(cells.len() - 1))
    }
}

impl InstanceConstHost<'_> {
    /// O valor padrão de um campo: zero, `null` nas referências, vetor zero no `v128`.
    fn default_value(&self, storage: StorageType) -> ConstExprValue {
        match storage {
            StorageType::Packed(_) => ConstExprValue::Numeric(0),
            StorageType::Type(ty) if ty.kind == TypeKind::V128 => ConstExprValue::Vector([0; 16]),
            StorageType::Type(ty) if crate::wasm::wasm_format::is_ref_type(ty) => ConstExprValue::Ref(null_ref()),
            StorageType::Type(_) => ConstExprValue::Numeric(0),
        }
    }
}

/// `evaluateConstantExpression`.
fn evaluate_constant(
    instance: InstanceId,
    info: &ModuleInformation,
    globals: &[Rc<RefCell<Global>>],
    cells: &RefCell<Vec<GcCell>>,
    index: u64,
) -> Result<u64, WasmError> {
    let mut host = InstanceConstHost { instance, info, globals, cells };
    evaluate_extended_const_expr(&info.constant_expressions[index as usize], &mut host, info)
        .map_err(|error| WasmError::Runtime(format!("couldn't evaluate constant expression: {}", error)))
}

/// O valor inicial de uma global interna.
fn initial_global_value(
    instance: InstanceId,
    info: &ModuleInformation,
    globals: &[Rc<RefCell<Global>>],
    cells: &RefCell<Vec<GcCell>>,
    global: &crate::wasm::wasm_format::GlobalInformation,
) -> Result<Global, WasmError> {
    let bits = global.initial_bits.bits_or_import_number();
    match global.initialization_type {
        GlobalInitializationType::FromExpression => {
            if let GlobalInitialBits::Vector(vector) = global.initial_bits {
                return Ok(Global::new_vector(global.ty, global.mutability, vector));
            }
            Ok(Global::new(global.ty, global.mutability, bits))
        }
        GlobalInitializationType::FromGlobalImport => {
            let source = globals[bits as usize].borrow();
            if global.ty.is_v128() {
                return Ok(Global::new_vector(global.ty, global.mutability, source.get_vector()));
            }
            Ok(Global::new(global.ty, global.mutability, source.get()))
        }
        GlobalInitializationType::FromRefFunc => Ok(Global::new(global.ty, global.mutability, func_ref(instance, bits as u32))),
        GlobalInitializationType::FromExtendedExpression => {
            Ok(Global::new(global.ty, global.mutability, evaluate_constant(instance, info, globals, cells, bits)?))
        }
        GlobalInitializationType::IsImport | GlobalInitializationType::FromVector => {
            unreachable!("global interna com inicialização de importação")
        }
    }
}

impl Instance {
    /// `JSWebAssemblyInstance::finalizeCreation` seguido de `WebAssemblyModuleRecord::evaluate`:
    /// liga as importações, cria o que o módulo declara, roda os segmentos e a função `start`.
    pub fn instantiate(info: Rc<ModuleInformation>, imports: Vec<ImportValue>) -> Result<Instance, WasmError> {
        if imports.len() != info.imports.len() {
            return Err(WasmError::Link(format!(
                "module has {} imports but {} were provided",
                info.imports.len(),
                imports.len()
            )));
        }

        let id = allocate_instance_id();
        let mut host_functions: Vec<HostFunction> = Vec::new();
        let mut memories: Vec<Rc<RefCell<Memory>>> = Vec::new();
        let mut tables: Vec<Rc<RefCell<Table>>> = Vec::new();
        let mut globals: Vec<Rc<RefCell<Global>>> = Vec::new();
        let mut tags: Vec<Rc<RefCell<TagData>>> = Vec::new();

        for (import, value) in info.imports.iter().zip(imports) {
            match (import.kind, value) {
                (ExternalKind::Function, ImportValue::Function(function)) => host_functions.push(function),
                (ExternalKind::Function, _) => {
                    return Err(WasmError::Link(import_fail_message(import, "import function", "must be callable")));
                }
                (ExternalKind::Memory, ImportValue::Memory(memory)) => {
                    let declared = &info.memories[import.kind_index as usize];
                    {
                        let actual = memory.borrow();
                        if actual.size() < declared.initial.bytes() as usize {
                            return Err(WasmError::Link(import_fail_message(
                                import,
                                "Memory import",
                                "provided a 'size' that is smaller than the module's declared 'initial' import memory size",
                            )));
                        }
                        if declared.maximum.has_value() {
                            if !actual.maximum().has_value() {
                                return Err(WasmError::Link(import_fail_message(
                                    import,
                                    "Memory import",
                                    "did not have a 'maximum' but the module requires that it does",
                                )));
                            }
                            if actual.maximum() > declared.maximum {
                                return Err(WasmError::Link(import_fail_message(
                                    import,
                                    "Memory import",
                                    "provided a 'maximum' that is larger than the module's declared 'maximum' import memory size",
                                )));
                            }
                        }
                        if actual.is_shared() != declared.is_shared {
                            return Err(WasmError::Link(import_fail_message(
                                import,
                                "Memory import",
                                "provided a 'shared' that is different from the module's declared 'shared' import memory attribute",
                            )));
                        }
                        if actual.address_type() != declared.address_type {
                            return Err(WasmError::Link(import_fail_message(
                                import,
                                "Memory import",
                                "provided an 'address' that is different from the module's declared 'address' import memory attribute",
                            )));
                        }
                    }
                    memories.push(memory);
                }
                (ExternalKind::Memory, _) => {
                    return Err(WasmError::Link(import_fail_message(
                        import,
                        "Memory import",
                        "is not an instance of WebAssembly.Memory",
                    )));
                }
                (ExternalKind::Table, ImportValue::Table(table)) => {
                    let declared = &info.tables[import.kind_index as usize];
                    {
                        let actual = table.borrow();
                        if u64::from(actual.length()) < declared.initial {
                            return Err(WasmError::Link(import_fail_message(
                                import,
                                "Table import",
                                "provided an 'initial' that is too small",
                            )));
                        }
                        if let Some(maximum) = declared.maximum {
                            let Some(actual_maximum) = actual.maximum() else {
                                return Err(WasmError::Link(import_fail_message(
                                    import,
                                    "Table import",
                                    "does not have a 'maximum' but the module requires that it does",
                                )));
                            };
                            if actual_maximum > maximum {
                                return Err(WasmError::Link(import_fail_message(
                                    import,
                                    "Imported Table",
                                    "'maximum' is larger than the module's expected 'maximum'",
                                )));
                            }
                        }
                        if actual.wasm_type() != declared.wasm_type {
                            return Err(WasmError::Link(import_fail_message(
                                import,
                                "Table import",
                                "provided a 'type' that is wrong",
                            )));
                        }
                        if actual.address_type() != declared.address_type {
                            return Err(WasmError::Link(import_fail_message(
                                import,
                                "Table import",
                                "provided an 'address' that is different from the module's declared 'address' import table attribute",
                            )));
                        }
                    }
                    tables.push(table);
                }
                (ExternalKind::Table, _) => {
                    return Err(WasmError::Link(import_fail_message(
                        import,
                        "Table import",
                        "is not an instance of WebAssembly.Table",
                    )));
                }
                (ExternalKind::Global, ImportValue::Global(global)) => {
                    let declared = &info.globals[import.kind_index as usize];
                    {
                        let actual = global.borrow();
                        // Imutável: subtipo (ids canônicos globais, valem entre módulos); mutável: tipo exato.
                        let same_type = if declared.mutability == crate::wasm::wasm_format::Mutability::Immutable {
                            info.is_subtype(actual.ty(), declared.ty)
                        } else {
                            actual.ty() == declared.ty
                        };
                        if !same_type {
                            return Err(WasmError::Link(import_fail_message(import, "imported global", "must be a same type")));
                        }
                        if actual.mutability() != declared.mutability {
                            return Err(WasmError::Link(import_fail_message(
                                import,
                                "imported global",
                                "must be a same mutability",
                            )));
                        }
                    }
                    globals.push(global);
                }
                (ExternalKind::Global, _) => {
                    return Err(WasmError::Link(import_fail_message(
                        import,
                        "imported global",
                        "must be a WebAssembly.Global object since it is mutable",
                    )));
                }
                (ExternalKind::Exception, ImportValue::Tag(tag)) => {
                    let declared = tag_parameters(&info, import.kind_index as usize);
                    if tag.borrow().parameters != declared {
                        return Err(WasmError::Link(import_fail_message(
                            import,
                            "imported Tag",
                            "signature doesn't match the imported WebAssembly Tag's signature",
                        )));
                    }
                    tags.push(tag);
                }
                (ExternalKind::Exception, _) => {
                    return Err(WasmError::Link(import_fail_message(import, "Tag import", "is not an instance of WebAssembly.Tag")));
                }
            }
        }

        // Tags da seção Tag: cada uma é uma identidade nova.
        for index in tags.len()..info.exception_index_space_size() {
            tags.push(Rc::new(RefCell::new(TagData { parameters: tag_parameters(&info, index) })));
        }

        // Memórias definidas pelo módulo.
        for index in memories.len()..info.memories.len() {
            let declared = &info.memories[index];
            debug_assert!(!declared.is_import);
            let memory = Memory::try_create(declared.initial, declared.maximum, declared.is_shared, declared.address_type)
                .ok_or(WasmError::OutOfMemory)?;
            memories.push(Rc::new(RefCell::new(memory)));
        }

        // Globais internas, em ordem: uma pode ler as anteriores.
        let gc_cells = shared_gc_heap();
        for index in info.first_internal_global..info.globals.len() {
            let value = initial_global_value(id, &info, &globals, &gc_cells, &info.globals[index])?;
            globals.push(Rc::new(RefCell::new(value)));
        }

        // Tabelas definidas pelo módulo.
        for index in tables.len()..info.tables.len() {
            let declared = &info.tables[index];
            debug_assert!(!declared.is_import);
            let mut table = Table::try_create(
                declared.initial,
                declared.maximum,
                declared.element_type,
                declared.wasm_type,
                declared.address_type,
                null_ref(),
            )
            .ok_or(WasmError::OutOfMemory)?;
            let bits = declared.initial_bits_or_import_number;
            let initial = match declared.init_type {
                TableInitializationType::Default | TableInitializationType::FromRefNull => null_ref(),
                TableInitializationType::FromGlobalImport => globals[bits as usize].borrow().get(),
                TableInitializationType::FromRefFunc => func_ref(id, bits as u32),
                TableInitializationType::FromExtendedExpression => evaluate_constant(id, &info, &globals, &gc_cells, bits)?,
            };
            if initial != null_ref() {
                let length = table.length();
                table.fill_range(0, initial, length);
            }
            tables.push(Rc::new(RefCell::new(table)));
        }

        let function_count = info.function_index_space_size();
        let instance = Instance {
            id,
            host_functions,
            memories,
            tables,
            globals,
            tags,
            exnrefs: RefCell::new(Vec::new()),
            gc_cells,
            dropped_data: RefCell::new(vec![false; info.data.len()]),
            dropped_elements: RefCell::new(vec![false; info.elements.len()]),
            compiled: RefCell::new(vec![None; function_count]),
            call_depth: Cell::new(0),
            info,
        };
        instance.evaluate()?;
        Ok(instance)
    }

    /// `evaluateConstantExpression` sobre as globais da instância.
    pub(super) fn evaluate_constant_expression(&self, index: u64) -> Result<u64, WasmError> {
        evaluate_constant(self.id, &self.info, &self.globals, &self.gc_cells, index)
    }

    /// O deslocamento de um segmento, no tamanho de endereço da memória ou da tabela.
    fn evaluate_offset(&self, expression: I32InitExpr, is_64_bit: bool) -> Result<u64, WasmError> {
        let raw = match expression {
            I32InitExpr::Global(index) => self.globals[index as usize].borrow().get(),
            I32InitExpr::Const(value) => value,
            I32InitExpr::ExtendedExpression(index) => self.evaluate_constant_expression(index)?,
        };
        Ok(if is_64_bit { raw } else { u64::from(raw as u32) })
    }

    /// O valor da entrada `entry` de um segmento de elementos.
    pub(super) fn element_entry_value(&self, element_index: usize, entry: usize) -> Result<u64, WasmError> {
        let element = &self.info.elements[element_index];
        let bits = element.initial_bits_or_indices[entry];
        match element.init_types[entry] {
            ElementInitializationType::FromRefNull => Ok(null_ref()),
            ElementInitializationType::FromRefFunc => Ok(func_ref(self.id, bits as u32)),
            ElementInitializationType::FromGlobal => Ok(self.globals[bits as usize].borrow().get()),
            ElementInitializationType::FromExtendedExpression => self.evaluate_constant_expression(bits),
        }
    }

    /// `initElementSegment`: o chamador já conferiu os limites.
    pub(super) fn init_element_segment(
        &self,
        table_index: u32,
        element_index: usize,
        destination: u32,
        source: u32,
        length: u32,
    ) -> Result<(), WasmError> {
        for offset in 0..length {
            let value = self.element_entry_value(element_index, (source + offset) as usize)?;
            self.tables[table_index as usize].borrow_mut().set(destination + offset, value);
        }
        Ok(())
    }

    /// `WebAssemblyModuleRecord::evaluate`: segmentos ativos e `start`.
    fn evaluate(&self) -> Result<(), WasmError> {
        let info = &self.info;

        // Validation of all element ranges comes before all Table and Memory initialization.
        for (element_index, element) in info.elements.iter().enumerate() {
            if !element.is_active() {
                continue;
            }
            let table_index = element.table_index_if_active.expect("elemento ativo sem tabela");
            let is_table64 = info.tables[table_index as usize].address_type.is_64_bit();
            let offset = self.evaluate_offset(element.offset_if_active.expect("elemento ativo sem deslocamento"), is_table64)?;
            let table_length = u64::from(self.tables[table_index as usize].borrow().length());
            let segment_length = element.init_types.len() as u64;
            if offset > table_length || segment_length > table_length - offset {
                return Err(WasmError::Runtime("Element is trying to set an out of bounds table index".to_string()));
            }
            self.init_element_segment(table_index, element_index, offset as u32, 0, element.length())?;
            self.dropped_elements.borrow_mut()[element_index] = true;
        }

        for (segment_index, segment) in info.data.iter().enumerate() {
            if segment.kind != SegmentKind::Active {
                continue;
            }
            let memory = self.memories[segment.memory_index as usize].clone();
            let is_memory64 = info.memories[segment.memory_index as usize].is_memory64();
            let offset = self.evaluate_offset(segment.offset_if_active.expect("segmento ativo sem deslocamento"), is_memory64)?;
            let mut memory = memory.borrow_mut();
            let size_in_bytes = memory.size() as u64;
            let segment_size = u64::from(segment.size_in_bytes());
            let fail = |suffix: &str| {
                WasmError::Runtime(format!(
                    "Invalid data segment initialization: segment of {} bytes memory of {} bytes, at offset {}{}",
                    segment_size, size_in_bytes, offset, suffix
                ))
            };
            if size_in_bytes < segment_size {
                return Err(fail(", segment is too big"));
            }
            if offset > size_in_bytes - segment_size {
                return Err(fail(", segment writes outside of memory"));
            }
            let written = memory.init(offset, &segment.bytes);
            debug_assert!(written);
            self.dropped_data.borrow_mut()[segment_index] = true;
        }

        if let Some(start) = info.start_function_index_space {
            self.invoke(start, &[])?;
        }
        Ok(())
    }

    /// Procura um export pelo nome.
    pub fn export(&self, name: &str) -> Option<ExportValue> {
        let export = self.info.exports.iter().find(|export| export.field == name)?;
        let index = export.kind_index as usize;
        Some(match export.kind {
            ExternalKind::Function => ExportValue::Function(export.kind_index),
            ExternalKind::Memory => ExportValue::Memory(self.memories[index].clone()),
            ExternalKind::Table => ExportValue::Table(self.tables[index].clone()),
            ExternalKind::Global => ExportValue::Global(self.globals[index].clone()),
            ExternalKind::Exception => ExportValue::Tag(self.tags[index].clone()),
        })
    }

    /// Chama uma função exportada pelo nome, com os argumentos em bits.
    pub fn call_export(&self, name: &str, arguments: &[u64]) -> Result<Vec<u64>, WasmError> {
        match self.export(name) {
            Some(ExportValue::Function(index)) => self.invoke(index, arguments),
            _ => Err(WasmError::Link(format!("no exported function named '{}'", name))),
        }
    }

    /// O identificador desta instância nas referências de função (`func_ref`).
    pub fn id(&self) -> InstanceId {
        self.id
    }

    /// A informação do módulo desta instância.
    pub fn info(&self) -> &Rc<ModuleInformation> {
        &self.info
    }

    pub fn memory(&self, index: usize) -> Rc<RefCell<Memory>> {
        self.memories[index].clone()
    }

    pub fn table(&self, index: usize) -> Rc<RefCell<Table>> {
        self.tables[index].clone()
    }

    pub fn tag(&self, index: usize) -> Rc<RefCell<TagData>> {
        self.tags[index].clone()
    }

    pub fn global(&self, index: usize) -> Rc<RefCell<Global>> {
        self.globals[index].clone()
    }

    /// O tamanho atual da memória 0 em páginas (`None` sem memória).
    pub fn memory_pages(&self) -> Option<PageCount> {
        self.memories.first().map(|memory| memory.borrow().page_count())
    }
}
