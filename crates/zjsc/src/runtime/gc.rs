//! Porte de `heap/Heap.cpp` (`collectNow`, `collectIfNecessaryOrDefer`) na forma que o registro de células
//! permite: marcação e varredura sobre o `cell_registry`, parando o mundo, sem geração e sem `unsafe`.
//!
//! As raízes:
//! - a célula cujo `Rc` alguém fora do registro segura (contagem forte maior que 1): um local de função
//!   nativa, um campo `Rc` de outra estrutura ou um `thread_local` a está usando, então ela não cai. É o papel
//!   da varredura conservadora da pilha nativa do C++, só que exata;
//! - a pilha de registradores do interpretador, conservadora (todo padrão de bits de célula conta);
//! - as constantes de todos os `CodeBlock`s ligados e dos `UnlinkedCodeBlock`s vivos;
//! - as raízes explícitas (`Root`), de quem guarda um `JSValue` solto num local Rust entre execuções;
//! - o estado do VM e dos `thread_local`s que guardam `JSValue` (`visit_global_roots`).
//!
//! A marcação segue os `JSValue` de cada célula marcada (`visit_children`): a base `JSObject` (armazenamento
//! inline, butterfly e protótipo da estrutura), os campos internos e o que cada tipo guarda a mais.
//!
//! Só se coleta num ponto seguro: o `op_loop_hint` com uma única execução do `llint` na pilha nativa (nenhuma
//! função nativa no meio, que poderia segurar um `JSValue` solto num local) e fora de `DeferGC`, ou a chamada
//! explícita (`VM::collect_garbage`) fora de qualquer execução.
//!
//! Em build de depuração a célula varrida deixa uma lápide: ler o id dela depois é pânico com o tipo, o que
//! transforma uma aresta esquecida em erro barulhento no lugar de comportamento estranho.
//!
//! Depois da marcação forte vêm os efêmeros de `WeakMap` (ponto fixo) e o `finalizeUnconditionally` das células
//! fracas e dos `CodeBlock`s: a entrada de `WeakMap`/`WeakSet` de chave morta sai, o `WeakRef` de alvo morto fica
//! vazio (o alvo lido ou criado na versão corrente é forte, `m_lastAccessVersion`), o `FinalizationRegistry` move
//! os registros mortos e agenda a limpeza numa tarefa posterior.
//!
//! DIVERGÊNCIAS:
//! - Sem gerações nem coleta concorrente: toda coleta é completa e para o mundo.
//! - Os caches por id fora dos metadados de bytecode (o da `Structure` por protótipo, as tabelas laterais por
//!   célula) não são enfraquecidos; o id carrega a geração do slot, e o id velho não acha a célula nova.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Weak;

use crate::bytecode::unlinked_code_block::UnlinkedCodeBlock;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::js_value::{JSValue, NOT_CELL_MASK};

/// `Heap::m_minBytesPerCycle`, agora em bytes: ver `heap_size`. O estresse (`ZJSC_GC_STRESS`) continua contando
/// alocações, para o teste das raízes.

/// O conjunto marcado e a pilha de trabalho de uma coleta (`SlotVisitor`).
pub struct Marker {
    marked: Vec<bool>,
    worklist: Vec<usize>,
    /// Algum `RefCell` estava emprestado para escrita: a célula não pôde ser lida e a coleta desiste (não
    /// varre nada), porque não dá para provar o que ela segura.
    aborted: bool,
    /// `vm.currentWeakRefVersion()` na hora da coleta.
    weak_ref_version: u64,
    /// As células de semântica fraca alcançadas (`WeakMap`, `WeakSet`, `WeakRef`, `FinalizationRegistry`), para
    /// os efêmeros e a finalização depois da marcação forte.
    deferred_weak: Vec<CellEntry>,
}

impl Marker {
    fn new(slot_count: usize, weak_ref_version: u64) -> Marker {
        Marker { marked: vec![false; slot_count], worklist: Vec::new(), aborted: false, weak_ref_version, deferred_weak: Vec::new() }
    }

    /// `vm.currentWeakRefVersion()` da coleta.
    pub fn weak_ref_version(&self) -> u64 {
        self.weak_ref_version
    }

    /// Guarda uma célula de semântica fraca alcançada.
    pub(crate) fn defer_weak(&mut self, entry: CellEntry) {
        self.deferred_weak.push(entry);
    }

    /// `SlotVisitor::append(JSValue)`; o `Option` vazio é o `WriteBarrier` nulo.
    pub fn value(&mut self, value: impl Into<Option<JSValue>>) {
        if let Some(JSValue::Cell(cell_id)) = value.into() {
            self.cell(cell_id);
        }
    }

    /// `appendValues`.
    pub fn values(&mut self, values: &[JSValue]) {
        for value in values {
            self.value(*value);
        }
    }

    /// Marca a célula do id (ids fora do registro, de lápide ou de padrão conservador, não contam).
    pub fn cell(&mut self, cell_id: usize) {
        let Some(index) = cell_registry::index_of(cell_id) else { return };
        if let Some(marked) = self.marked.get_mut(index) {
            if !*marked {
                *marked = true;
                self.worklist.push(cell_id);
            }
        }
    }

    /// Um registrador da pilha ou outro valor codificado lido sem saber se é `JSValue`: conta se tem o padrão
    /// de bits de uma célula (a varredura conservadora do C++).
    pub fn encoded(&mut self, encoded: i64) {
        if encoded != 0 && encoded & NOT_CELL_MASK == 0 {
            self.cell(encoded as usize);
        }
    }

    /// A célula do id existe e não foi marcada: a varredura vai soltá-la (o `!isMarked` do `finalizeUnconditionally`).
    pub fn is_doomed(&self, cell_id: usize) -> bool {
        cell_registry::index_of(cell_id).is_some_and(|index| {
            !self.marked.get(index).copied().unwrap_or(true) && cell_registry::get_for_marking(cell_id).is_some()
        })
    }

    /// O `RefCell` estava emprestado para escrita: desiste da coleta.
    pub fn abort(&mut self) {
        self.aborted = true;
    }

    /// Um módulo do host segura estado que o coletor não sabe percorrer (closures que capturam valores, árvores de
    /// estado de stream): enquanto `held`, a coleta não acontece, como um `DeferGC` com motivo.
    pub fn defer_while(&mut self, held: bool) {
        if held {
            self.aborted = true;
        }
    }
}

thread_local! {
    static HOST_ROOTS: std::cell::RefCell<Vec<fn(&mut Marker)>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// As tabelas laterais por célula dos módulos do host (ver `side_table`).
fn visit_side_tables(marker: &mut Marker, phase: SidePhase) -> bool {
    use crate::runtime as rt;
    let mut progressed = false;
    for visit in [
        rt::url::visit_side_table,
        rt::url_pattern::visit_side_table,
        rt::request::visit_side_table,
        rt::mime::visit_side_table,
        rt::response::visit_side_table,
        rt::abort_signal::visit_side_table,
        rt::crypto::visit_side_table,
        rt::performance::visit_side_table,
        rt::performance_observer::visit_side_table,
        rt::web_iterable::visit_side_table,
        rt::js_web_assembly::visit_side_table,
        rt::js_web_assembly_jspi::visit_side_table,
        rt::js_module_loader::visit_side_table,
        rt::streams::visit_side_table,
    ] {
        progressed |= visit(marker, phase);
    }
    progressed
}

/// A fase de uma tabela lateral por célula na coleta.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidePhase {
    /// O ponto fixo dos efêmeros: o estado de objeto marcado marca o que segura.
    Mark,
    /// Antes da varredura: o estado do objeto que vai ser solto sai com ele.
    Sweep,
}

/// A chave de uma tabela lateral: o valor codificado da célula ou o `cell_id` dela.
pub(crate) trait SideKey: Copy + Eq + std::hash::Hash {
    fn cell_id(self) -> Option<usize>;
}

impl SideKey for crate::runtime::js_value::EncodedJSValue {
    fn cell_id(self) -> Option<usize> {
        if let JSValue::Cell(cell_id) = JSValue::decode(self) { Some(cell_id) } else { None }
    }
}

impl SideKey for usize {
    fn cell_id(self) -> Option<usize> {
        Some(self)
    }
}

/// Uma tabela lateral por célula (o estado que no C++ é campo do próprio objeto: o `URL` do `DOMURL`, o
/// `AbortSignal`, o `Request`): a entrada vive enquanto o objeto da chave vive, e o que ela segura é filho dele.
/// `children` põe em `out` os valores que o estado segura. Na fase `Mark` devolve se marcou algo novo.
pub(crate) fn side_table<K: SideKey, S>(
    table: &'static std::thread::LocalKey<std::cell::RefCell<HashMap<K, S>>>,
    marker: &mut Marker,
    phase: SidePhase,
    children: impl Fn(&S, &mut Vec<JSValue>),
) -> bool {
    table.with(|table| match table.try_borrow_mut() {
        Ok(mut table) => side_map(&mut table, marker, phase, children),
        Err(_) => {
            marker.abort();
            false
        }
    })
}

/// O `side_table` de um mapa que mora dentro de outro estado do módulo.
pub(crate) fn side_map<K: SideKey, S>(
    map: &mut HashMap<K, S>,
    marker: &mut Marker,
    phase: SidePhase,
    children: impl Fn(&S, &mut Vec<JSValue>),
) -> bool {
    let key_doomed = |marker: &Marker, key: K| key.cell_id().is_some_and(|cell_id| marker.is_doomed(cell_id));
    if phase == SidePhase::Sweep {
        map.retain(|key, _| !key_doomed(marker, *key));
        return false;
    }
    let mut values = Vec::new();
    map.iter().filter(|(key, _)| !key_doomed(marker, **key)).for_each(|(_, state)| children(state, &mut values));
    let mut progressed = false;
    for value in values {
        if let JSValue::Cell(cell_id) = value {
            if marker.is_doomed(cell_id) {
                marker.cell(cell_id);
                progressed = true;
            }
        }
    }
    progressed
}

/// O gancho do embedder (o ul-chromium registra o estado do DOM aqui): a função marca com `Marker::value`/`cell` o
/// que o host segura e adia a coleta com `Marker::defer_while` para o que ainda não sabe percorrer.
pub fn register_host_roots(visit: fn(&mut Marker)) {
    HOST_ROOTS.with(|roots| roots.borrow_mut().push(visit));
}

/// As raízes dos módulos do host (os `thread_local`s que guardam valores do programa), consultadas antes da
/// marcação: um módulo com estado opaco adia a coleta sem gastar a marcação inteira.
fn visit_host_roots(marker: &mut Marker) {
    use crate::runtime as rt;
    rt::timers::visit_gc_roots(marker);
    rt::net_loop::visit_gc_roots(marker);
    rt::process_object::visit_gc_roots(marker);
    rt::process_stdio::visit_gc_roots(marker);
    rt::js_web_assembly::visit_gc_roots(marker);
    rt::crypto::visit_gc_roots(marker);
    rt::js_web_assembly_jspi::visit_gc_roots(marker);
    rt::event_target::visit_gc_roots(marker);
    rt::js_module_loader::visit_gc_roots(marker);
    rt::url::visit_gc_roots(marker);
    rt::performance_observer::visit_gc_roots(marker);
    rt::performance::visit_gc_roots(marker);
    rt::abort_signal::visit_gc_roots(marker);
    rt::response::visit_gc_roots(marker);
    rt::request::visit_gc_roots(marker);
    rt::mime::visit_gc_roots(marker);
    rt::queuing_strategy::visit_gc_roots(marker);
    rt::message_channel::visit_gc_roots(marker);
    rt::js_arguments_objects::visit_gc_roots(marker);
    rt::global_named_lookup::visit_gc_roots(marker);
    rt::broadcast_channel::visit_gc_roots(marker);
    rt::blob::visit_gc_roots(marker);
    rt::array_prototype::visit_gc_roots(marker);
    rt::url_search_params::visit_gc_roots(marker);
    rt::text_encoder::visit_gc_roots(marker);
    rt::text_decoder::visit_gc_roots(marker);
    rt::process_shape::visit_gc_roots(marker);
    rt::process_env::visit_gc_roots(marker);
    rt::ordered_hash_table_storage::visit_gc_roots(marker);
    rt::node_buffer::visit_gc_roots(marker);
    rt::node_fs::visit_gc_roots(marker);
    rt::js_web_assembly_gc_object::visit_gc_roots(marker);
    rt::headers::visit_gc_roots(marker);
    rt::form_data::visit_gc_roots(marker);
    rt::file::visit_gc_roots(marker);
    rt::current_realm::visit_gc_roots(marker);
    rt::console_format::visit_gc_roots(marker);
    rt::streams::visit_gc_roots(marker);
    crate::api::builtin_modules::visit_gc_roots(marker);
    crate::api::eval::visit_gc_roots(marker);
    HOST_ROOTS.with(|roots| roots.borrow().iter().for_each(|visit| visit(marker)));
}

#[derive(Default)]
struct State {
    /// As raízes explícitas (`Root`), por id, com a contagem de quem as segura.
    roots: HashMap<usize, u32>,
    /// A pilha dos `RootScope`s abertos.
    scoped_roots: Vec<usize>,
    /// A contagem de alocações logo depois da última coleta (ou do início do programa).
    allocations_after_collection: usize,
    /// Quantas alocações disparam a próxima coleta automática (só no modo de estresse, que conta células).
    threshold: usize,
    /// O relógio de bytes (`heap_size`) logo depois da última coleta (ou do início do programa).
    bytes_after_collection: usize,
    /// Quantos bytes o ciclo atual pode alocar antes da próxima coleta (`m_maxEdenSize`).
    bytes_allowed: usize,
    /// Os `UnlinkedCodeBlock`s criados: as constantes dos vivos são raízes (o código pode ser ligado de novo).
    unlinked_code_blocks: Vec<Weak<RefCell<UnlinkedCodeBlock>>>,
    /// Os protótipos de toda `Structure` criada: a estrutura guarda o protótipo por id e vive em caches (o perfil
    /// de alocação de um construtor, o cache por protótipo, as transições) que podem criar objeto novo com ela.
    /// Como as estruturas não morrem antes do programa (a árvore de transições é cíclica), o protótipo também
    /// não.
    prototypes: std::collections::HashSet<usize>,
    collecting: bool,
    /// As lápides do build de depuração: id varrido e o tipo que ele tinha.
    #[cfg(debug_assertions)]
    swept: HashMap<usize, crate::runtime::js_type::JSType>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State { threshold: initial_threshold(), bytes_allowed: crate::runtime::heap_size::MIN_BYTES_PER_CYCLE, ..State::default() });
}

thread_local! {
    /// `set_stress_for_thread`: o estresse só desta thread, para os testes.
    static STRESS_FOR_THREAD: std::cell::Cell<Option<usize>> = const { std::cell::Cell::new(None) };
}

/// Coleta a cada `allocations` alocações nesta thread (`None` volta ao normal), como `ZJSC_GC_STRESS`.
pub fn set_stress_for_thread(allocations: Option<usize>) {
    STRESS_FOR_THREAD.with(|stress| stress.set(allocations.filter(|&value| value > 0)));
    let _ = STATE.try_with(|state| state.borrow_mut().threshold = initial_threshold());
}

/// `ZJSC_GC_STRESS=N` coleta a cada N alocações (teste de estresse das raízes e das arestas).
fn stress_threshold() -> Option<usize> {
    STRESS_FOR_THREAD
        .try_with(std::cell::Cell::get)
        .ok()
        .flatten()
        .or_else(|| crate::runtime::host::host().getenv("ZJSC_GC_STRESS").and_then(|value| value.parse().ok()).filter(|&value| value > 0))
}

fn initial_threshold() -> usize {
    stress_threshold().unwrap_or(usize::MAX)
}

/// Fim do programa (`cell_registry::reset_program_state`): o estado do coletor é do VM que cai.
pub(crate) fn reset_for_program() {
    let taken = STATE.try_with(|state| std::mem::take(&mut *state.borrow_mut()));
    drop(taken);
    let _ = STATE.try_with(|state| {
        let mut state = state.borrow_mut();
        state.threshold = initial_threshold();
        state.allocations_after_collection = cell_registry::allocation_count();
        state.bytes_after_collection = crate::runtime::heap_size::bytes_allocated();
        state.bytes_allowed = crate::runtime::heap_size::MIN_BYTES_PER_CYCLE;
    });
}

/// Guarda um `UnlinkedCodeBlock` recém-criado entre as raízes (fraco: some com ele).
pub(crate) fn register_unlinked_code_block(code_block: &std::rc::Rc<RefCell<UnlinkedCodeBlock>>) {
    STATE.with(|state| state.borrow_mut().unlinked_code_blocks.push(std::rc::Rc::downgrade(code_block)));
}

/// Guarda o protótipo de uma `Structure` nova entre as raízes (ver `State::prototypes`).
pub(crate) fn retain_prototype(prototype: JSValue) {
    if let JSValue::Cell(cell_id) = prototype {
        let _ = STATE.try_with(|state| {
            if let Ok(mut state) = state.try_borrow_mut() {
                state.prototypes.insert(cell_id);
            }
        });
    }
}

/// `MarkedArgumentBuffer` de código nativo: os valores que ele segura em locais Rust enquanto chama JS (a pilha C++
/// que o coletor do JSC varre de modo conservador). É uma pilha: o escopo mais novo cai primeiro.
pub struct RootScope(usize);

impl RootScope {
    pub fn new(values: &[JSValue]) -> RootScope {
        let scope = RootScope(STATE.with(|state| state.borrow().scoped_roots.len()));
        values.iter().for_each(|value| scope.append(*value));
        scope
    }

    /// `MarkedArgumentBuffer::append`: só no topo da pilha (os escopos abertos pelo JS chamado já caíram).
    pub fn append(&self, value: JSValue) {
        if let JSValue::Cell(cell_id) = value {
            STATE.with(|state| state.borrow_mut().scoped_roots.push(cell_id));
        }
    }
}

impl Drop for RootScope {
    fn drop(&mut self) {
        let _ = STATE.try_with(|state| state.borrow_mut().scoped_roots.truncate(self.0));
    }
}

/// `Strong<Unknown>`: mantém o valor vivo enquanto o `Root` existir, para quem o guarda num local Rust
/// enquanto JS roda (o resultado de um programa que o laço de eventos ainda não leu, por exemplo).
pub struct Root(usize);

impl Root {
    pub fn new(value: JSValue) -> Root {
        let cell_id = if let JSValue::Cell(cell_id) = value { cell_id } else { 0 };
        if cell_id != 0 {
            STATE.with(|state| *state.borrow_mut().roots.entry(cell_id).or_insert(0) += 1);
        }
        Root(cell_id)
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        if self.0 == 0 {
            return;
        }
        let _ = STATE.try_with(|state| {
            let mut state = state.borrow_mut();
            if let Some(count) = state.roots.get_mut(&self.0) {
                *count -= 1;
                if *count == 0 {
                    state.roots.remove(&self.0);
                }
            }
        });
    }
}

/// O id foi varrido? Em depuração, ler uma lápide é pânico (aresta ou raiz esquecida).
#[cfg(debug_assertions)]
pub(crate) fn check_not_swept(cell_id: usize) {
    let swept = STATE.try_with(|state| state.borrow().swept.get(&cell_id).copied()).ok().flatten();
    if let Some(js_type) = swept {
        panic!("célula {cell_id:#x} ({js_type:?}) varrida pelo coletor ainda referenciada: falta uma raiz ou uma aresta");
    }
}

/// O slot do id voltou a ser usado (o registro cortou o rabo e realocou): a lápide sai.
#[cfg(debug_assertions)]
pub(crate) fn forget_tombstone(cell_id: usize) {
    let _ = STATE.try_with(|state| {
        if let Ok(mut state) = state.try_borrow_mut() {
            state.swept.remove(&cell_id);
        }
    });
}

/// `Heap::collectIfNecessaryOrDefer` no ponto seguro do interpretador: coleta se o limite de alocações foi
/// passado e não há `DeferGC`, em qualquer profundidade (o corpo do programa, módulos, callbacks chamados por
/// nativos). Os frames JS de baixo estão na pilha de registradores que a varredura conservadora percorre; o valor
/// que um nativo segura num local Rust enquanto chama JS precisa estar enraizado (`RootScope`), e a lápide do build
/// de depuração acusa o que faltar.
pub(crate) fn safe_point(interpreter: &crate::interpreter::interpreter::Interpreter, vm: &crate::runtime::vm::VM) {
    if vm.heap().is_deferred() {
        return;
    }
    let due = STATE.with(|state| {
        let state = state.borrow();
        if state.collecting {
            return false;
        }
        if state.threshold != usize::MAX {
            // Estresse: coleta a cada N alocações, em células.
            return cell_registry::allocation_count().saturating_sub(state.allocations_after_collection) >= state.threshold;
        }
        // `Heap::shouldRequestGC`: os bytes alocados no ciclo passaram do que o heap permite.
        crate::runtime::heap_size::bytes_allocated().saturating_sub(state.bytes_after_collection) > state.bytes_allowed
    });
    if due {
        collect(Some(interpreter), vm);
    }
}

/// `Heap::collectNow(Sync, CollectionScope::Full)`: marca a partir das raízes e varre o resto. Devolve
/// quantas células foram soltas.
pub fn collect(interpreter: Option<&crate::interpreter::interpreter::Interpreter>, vm: &crate::runtime::vm::VM) -> usize {
    let already = STATE.with(|state| std::mem::replace(&mut state.borrow_mut().collecting, true));
    if already {
        return 0;
    }
    let (freed, registries_to_clean) = mark_and_sweep(interpreter, vm);
    let live = cell_registry::live_cell_count();
    STATISTICS.with(|statistics| {
        let mut current = statistics.get();
        current.collections += 1;
        current.freed_cells += freed;
        current.peak_live_cells = current.peak_live_cells.max(live);
        statistics.set(current);
    });
    let live_bytes = cell_registry::live_heap_bytes();
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        state.collecting = false;
        state.allocations_after_collection = cell_registry::allocation_count();
        // `Heap::updateAllocationLimits`: o próximo ciclo aloca até `maxHeapSize - currentHeapSize`.
        state.threshold = initial_threshold();
        state.bytes_after_collection = crate::runtime::heap_size::bytes_allocated();
        state.bytes_allowed = crate::runtime::heap_size::bytes_allowed_after_full_collection(live_bytes);
    });
    // `DeferredWorkTimer::scheduleWorkSoon` do `runFinalizationCleanup`: uma tarefa posterior que não segura o laço.
    if let Some(global_object) = cell_registry::first_global_object() {
        for registry in registries_to_clean {
            crate::runtime::timers::schedule_native(
                &global_object,
                0.0,
                Vec::new(),
                std::rc::Rc::new(move |global: &crate::runtime::js_global_object::JSGlobalObject| registry.run_finalization_cleanup(global)),
            );
        }
    }
    freed
}

/// O que as coletas da thread fizeram (`Heap::numberOfCollections` e afins), para medir o coletor de fora.
#[derive(Clone, Copy, Debug, Default)]
pub struct Statistics {
    pub collections: usize,
    pub freed_cells: usize,
    /// O maior número de células vivas logo depois de uma coleta.
    pub peak_live_cells: usize,
}

thread_local! {
    /// Fora do `State` porque sobrevive ao fim do programa, que é quando o teste lê.
    static STATISTICS: std::cell::Cell<Statistics> = const { std::cell::Cell::new(Statistics { collections: 0, freed_cells: 0, peak_live_cells: 0 }) };
}

/// As estatísticas acumuladas da thread.
pub fn statistics() -> Statistics {
    STATISTICS.with(std::cell::Cell::get)
}

fn mark_and_sweep(
    interpreter: Option<&crate::interpreter::interpreter::Interpreter>,
    vm: &crate::runtime::vm::VM,
) -> (usize, Vec<crate::runtime::js_finalization_registry::JSFinalizationRegistryRef>) {
    let (slot_count, pinned) = cell_registry::pinned_cells();
    let mut marker = Marker::new(slot_count, vm.current_weak_ref_version());
    for cell_id in pinned {
        marker.cell(cell_id);
    }
    visit_host_roots(&mut marker);
    if marker.aborted {
        return (0, Vec::new());
    }
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        for &cell_id in state.roots.keys().chain(state.prototypes.iter()).chain(state.scoped_roots.iter()) {
            marker.cell(cell_id);
        }
        state.unlinked_code_blocks.retain(|code_block| code_block.strong_count() > 0);
        for code_block in &state.unlinked_code_blocks {
            if let Some(code_block) = code_block.upgrade() {
                match code_block.try_borrow() {
                    Ok(code_block) => marker.values(&code_block.constant_registers),
                    Err(_) => marker.abort(),
                }
            }
        }
    });
    let owned;
    let interpreter = match interpreter {
        Some(interpreter) => interpreter,
        None => {
            owned = vm.interpreter();
            &owned
        }
    };
    interpreter.visit_gc_roots(&mut marker);
    vm.visit_gc_roots(&mut marker);
    // A marcação forte e os efêmeros (`Heap::runFixpointPhase`): o valor de `WeakMap` cuja chave ficou marcada é
    // marcado, e o que ele alcança pode marcar outra chave; repete até nada mudar.
    loop {
        while let Some(cell_id) = marker.worklist.pop() {
            if let Some(entry) = cell_registry::get_for_marking(cell_id) {
                visit_children(&entry, &mut marker);
            }
        }
        let deferred = std::mem::take(&mut marker.deferred_weak);
        let mut progressed = false;
        for entry in &deferred {
            if let CellEntry::WeakMap(map) = entry {
                match map.table().try_borrow() {
                    Ok(table) => progressed |= table.visit_ephemerons(&mut marker),
                    Err(_) => marker.abort(),
                }
            }
        }
        marker.deferred_weak.splice(0..0, deferred);
        // As tabelas laterais por célula (o estado da pilha V8) são efêmeros do mesmo jeito.
        progressed |= crate::runtime::v8_stack::visit_ephemerons(&mut marker);
        progressed |= visit_side_tables(&mut marker, SidePhase::Mark);
        if !progressed {
            break;
        }
    }
    if marker.aborted || !interpreter.finalize_code_blocks_before_sweep(&|cell_id| marker.is_doomed(cell_id)) {
        return (0, Vec::new());
    }
    // `finalizeUnconditionally` das células fracas, antes da varredura (as lápides ainda são células).
    let mut registries_to_clean = Vec::new();
    crate::runtime::v8_stack::remove_dead(&marker);
    visit_side_tables(&mut marker, SidePhase::Sweep);
    for entry in &marker.deferred_weak {
        match entry {
            CellEntry::WeakMap(map) => map.table().borrow_mut().remove_dead_keys(&marker),
            CellEntry::WeakSet(set) => set.table().borrow_mut().remove_dead_keys(&marker),
            CellEntry::WeakObjectRef(weak_ref) => weak_ref.finalize(&marker),
            CellEntry::ErrorInstance(error) => error.finalize_frames(&marker),
            CellEntry::FinalizationRegistry(registry) => {
                if registry.finalize(&marker) {
                    registries_to_clean.push(std::rc::Rc::clone(registry));
                }
            }
            _ => {}
        }
    }
    let swept = cell_registry::sweep(&marker.marked);
    let count = swept.len();
    #[cfg(debug_assertions)]
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        for (cell_id, js_type) in &swept {
            state.swept.insert(*cell_id, *js_type);
        }
    });
    (count, registries_to_clean)
}

/// `JSCell::visitChildren` por tipo: os `JSValue` que a célula segura.
fn visit_children(entry: &CellEntry, marker: &mut Marker) {
    if let Some(object) = entry.as_js_object() {
        object.visit_children(marker);
    }
    if let Some(fields) = entry.internal_fields() {
        for index in 0..fields.field_count() {
            marker.value(fields.field(index));
        }
    }
    entry.visit_extra_children(marker);
}
