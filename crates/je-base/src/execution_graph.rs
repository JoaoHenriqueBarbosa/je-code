//! Porte de `src/vs/base/test/common/executionGraph.ts`: descrição simples de um histórico de execução
//! produzido por um agendador rastreado, e os dois renderizadores de texto dele (raias e grafo de colunas).
//!
//! Notas de tradução:
//! - Objetos do original que se comparam por identidade (raiz, evento, `trace`) são `Rc`, e as tabelas
//!   indexadas por objeto (`Map<ExecutionRoot, ...>`) usam o endereço do `Rc` como chave.
//! - Os tipos estruturais `TraceLike`, `ScheduledTaskLike` e `LogEntryLike` viram structs. O `source` da
//!   tarefa (`{ toString(): string; stackTrace?: string }`) vira [`TaskSourceLike`] com o resultado do
//!   `toString` já calculado.
//! - Comprimentos, `padStart`/`padEnd` e `trim` contam unidades UTF-16 e usam o espaço em branco do
//!   JavaScript, como no original.
//! - Divergência: o `node.label.slice(1, 2)` do glifo da raiz sintética pega uma unidade UTF-16, que para
//!   um primeiro caractere fora do plano básico seria meio par substituto (não representável num `String`);
//!   aqui vale o primeiro ponto de código inteiro. Para texto do plano básico é idêntico.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::rc::Rc;
use std::sync::OnceLock;

use zyarr::RegExp;

use crate::dynamic::number_to_js_string;
use crate::js_string::is_js_whitespace;

// ---------------------------------------------------------------------------------------------
// Descrição do histórico
// ---------------------------------------------------------------------------------------------

/// `ExecutionRoot`.
#[derive(Debug)]
pub struct ExecutionRoot {
    pub label: String,
}

/// `ExecutionEvent`.
#[derive(Debug)]
pub struct ExecutionEvent {
    /// Tempo relativo (por exemplo, ms desde o `startTime`). Precisa ser >= 0 e não decrescente na ordem do
    /// histórico.
    pub time: f64,
    pub label: String,
    pub root: Rc<ExecutionRoot>,
    /// `None` significa que o evento é filho direto da raiz.
    pub parent: Option<Rc<ExecutionEvent>>,
    /// Quadro de quem chamou, extraído da pilha de agendamento.
    pub detail: Option<String>,
}

/// `ExecutionHistory`.
#[derive(Debug, Default)]
pub struct ExecutionHistory {
    /// Raízes na ordem da primeira aparição (a ordem das colunas nos renderizadores).
    pub roots: Vec<Rc<ExecutionRoot>>,
    /// Eventos em ordem de tempo.
    pub events: Vec<Rc<ExecutionEvent>>,
}

// ---------------------------------------------------------------------------------------------
// Adaptador: ScheduledTask[] -> ExecutionHistory
// ---------------------------------------------------------------------------------------------

/// A raiz de um `trace` (`TraceLike.root`).
#[derive(Debug)]
pub struct TraceRootLike {
    pub label: String,
}

/// `TraceLike`.
#[derive(Debug)]
pub struct TraceLike {
    pub parent: Option<Rc<TraceLike>>,
    pub root: Rc<TraceRootLike>,
}

/// `ScheduledTaskLike.source`.
#[derive(Debug, Default)]
pub struct TaskSourceLike {
    /// O resultado de `source.toString()`.
    pub text: String,
    pub stack_trace: Option<String>,
}

/// `ScheduledTaskLike`.
#[derive(Debug)]
pub struct ScheduledTaskLike {
    pub time: f64,
    pub source: TaskSourceLike,
    pub trace: Option<Rc<TraceLike>>,
}

/// Uma entrada de log para entrelaçar no histórico junto das tarefas agendadas. Cada log leva o `trace` que
/// era o corrente quando foi emitido.
#[derive(Debug)]
pub struct LogEntryLike {
    pub trace: Rc<TraceLike>,
    pub message: String,
}

/// Procura o primeiro `trace` da cadeia (começando em `start`, sobe pelos pais) que já tem evento.
fn find_event_in_chain(start: Option<Rc<TraceLike>>, event_by_trace: &HashMap<*const TraceLike, Rc<ExecutionEvent>>) -> Option<Rc<ExecutionEvent>> {
    let mut current = start;
    while let Some(trace) = current {
        if let Some(event) = event_by_trace.get(&Rc::as_ptr(&trace)) {
            return Some(event.clone());
        }
        current = trace.parent.clone();
    }
    None
}

/// Converte a lista de tarefas agendadas (cada uma com o seu `trace` causal) num `ExecutionHistory`.
/// Tarefas sem `trace` são descartadas. O evento pai de uma tarefa é a tarefa anterior mais recente cujo
/// `trace` é `task.trace.parent`; se `task.trace.parent` é a própria raiz do `trace`, o evento não tem pai
/// (é filho direto da raiz).
///
/// `logs` entram como eventos sintéticos: o pai de cada log é o evento da tarefa cujo `trace` casa com o
/// `trace` corrente do log na emissão (ou o ancestral mais próximo com evento), e o tempo é herdado desse
/// pai. Dentro de uma mesma tarefa pai os logs ficam na ordem de emissão, logo depois do evento pai.
pub fn build_history_from_tasks(tasks: &[ScheduledTaskLike], start_time: f64, logs: &[LogEntryLike]) -> ExecutionHistory {
    let mut root_by_trace: HashMap<*const TraceRootLike, Rc<ExecutionRoot>> = HashMap::new();
    let mut roots: Vec<Rc<ExecutionRoot>> = Vec::new();
    let mut event_by_trace: HashMap<*const TraceLike, Rc<ExecutionEvent>> = HashMap::new();
    let mut task_events: Vec<Rc<ExecutionEvent>> = Vec::new();

    for task in tasks {
        let Some(trace) = &task.trace else { continue };

        let root = root_by_trace
            .entry(Rc::as_ptr(&trace.root))
            .or_insert_with(|| {
                let root = Rc::new(ExecutionRoot { label: trace.root.label.clone() });
                roots.push(root.clone());
                root
            })
            .clone();

        // Acha o evento pai subindo pela cadeia de `trace` até um cujo evento já se conhece, ou a raiz.
        let parent_event = find_event_in_chain(trace.parent.clone(), &event_by_trace);

        let event = Rc::new(ExecutionEvent {
            time: task.time - start_time,
            label: task.source.text.clone(),
            root,
            parent: parent_event,
            detail: extract_caller_frame(task.source.stack_trace.as_deref()),
        });
        event_by_trace.insert(Rc::as_ptr(trace), event.clone());
        task_events.push(event);
    }

    // Agrupa os logs pelo evento de tarefa pai, preservando a ordem de emissão em cada grupo. Um log sem
    // evento de tarefa que o envolva é descartado (por exemplo, logs emitidos na raiz antes de qualquer
    // tarefa rodar).
    let mut logs_by_parent: HashMap<*const ExecutionEvent, Vec<Rc<ExecutionEvent>>> = HashMap::new();
    for entry in logs {
        let Some(parent_event) = find_event_in_chain(Some(entry.trace.clone()), &event_by_trace) else { continue };

        let log_event = Rc::new(ExecutionEvent {
            time: parent_event.time,
            label: format!("log: {}", entry.message),
            root: parent_event.root.clone(),
            parent: Some(parent_event.clone()),
            detail: None,
        });
        logs_by_parent.entry(Rc::as_ptr(&parent_event)).or_default().push(log_event);
    }

    // Entrelaça: cada evento de tarefa seguido dos seus logs na ordem de emissão.
    let mut events: Vec<Rc<ExecutionEvent>> = Vec::new();
    for event in task_events {
        let logs_of_event = logs_by_parent.remove(&Rc::as_ptr(&event));
        events.push(event);
        if let Some(logs_of_event) = logs_of_event {
            events.extend(logs_of_event);
        }
    }

    ExecutionHistory { roots, events }
}

/// Os padrões dos quadros de pilha que são da infraestrutura de agendamento e rastreio (`_skipFramePatterns`).
const SKIP_FRAME_PATTERNS: [&str; 9] = [
    r"[\\/]virtualScheduling[\\/]",
    r"[\\/]vs[\\/]base[\\/]common[\\/]async\.",
    r"timeTravelScheduler",
    r"RunOnceScheduler\.schedule",
    r"scheduleAtNextAnimationFrame",
    r"TimeoutTimer\.cancelAndSet",
    r"TimeoutTimer\.setIfNotSet",
    r"timeoutDeferred",
    r"createTimeout",
];

const MAX_DETAIL_FRAMES: usize = 5;

fn skip_frame_regexps() -> &'static [RegExp] {
    static REGEXPS: OnceLock<Vec<RegExp>> = OnceLock::new();
    REGEXPS.get_or_init(|| {
        SKIP_FRAME_PATTERNS
            .iter()
            .map(|source| {
                let pattern: Vec<u16> = source.encode_utf16().collect();
                RegExp::new(&pattern, &[]).expect("regex fixa e válida")
            })
            .collect()
    })
}

fn is_js_space(c: char) -> bool {
    u16::try_from(u32::from(c)).is_ok_and(is_js_whitespace)
}

/// `String#trim` do JavaScript.
fn trim_js(text: &str) -> &str {
    text.trim_matches(is_js_space)
}

/// `String#trimEnd` do JavaScript.
fn trim_end_js(text: &str) -> &str {
    text.trim_end_matches(is_js_space)
}

/// `text.length` do JavaScript: unidades UTF-16.
fn js_len(text: &str) -> usize {
    text.encode_utf16().count()
}

/// `text.padStart(target)` com espaço.
fn pad_start(text: &str, target: usize) -> String {
    let len = js_len(text);
    format!("{}{text}", " ".repeat(target.saturating_sub(len)))
}

/// `text.padEnd(target)` com espaço.
fn pad_end(text: &str, target: usize) -> String {
    let len = js_len(text);
    format!("{text}{}", " ".repeat(target.saturating_sub(len)))
}

/// `Math.round`: arredonda meios para cima.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Math.max(...values)`: `NaN` contamina o resultado.
fn js_max(values: impl Iterator<Item = f64>) -> f64 {
    values.fold(f64::NEG_INFINITY, |max, value| if max.is_nan() || value.is_nan() { f64::NAN } else { max.max(value) })
}

/// `+${Math.round(time)}ms`.
fn rounded_time_label(time: f64) -> String {
    format!("+{}ms", number_to_js_string(js_round(time)))
}

/// Extrai até [`MAX_DETAIL_FRAMES`] quadros de pilha que não são da infraestrutura de agendamento e rastreio.
/// Devolve os quadros unidos por quebra de linha (quem chama pode empilhá-los) ou `None` quando não há.
fn extract_caller_frame(stack_trace: Option<&str>) -> Option<String> {
    let stack_trace = stack_trace.filter(|s| !s.is_empty())?;
    let mut frames: Vec<&str> = Vec::new();
    for line in stack_trace.split('\n') {
        let trimmed = trim_js(line);
        if !trimmed.starts_with("at ") {
            continue;
        }
        let units: Vec<u16> = trimmed.encode_utf16().collect();
        if skip_frame_regexps().iter().any(|p| p.exec(&units, 0).is_some()) {
            continue;
        }
        frames.push(&trimmed[3..]);
        if frames.len() >= MAX_DETAIL_FRAMES {
            break;
        }
    }
    if frames.is_empty() { None } else { Some(frames.join("\n")) }
}

// ---------------------------------------------------------------------------------------------
// Renderizador: raias (uma coluna por raiz)
// ---------------------------------------------------------------------------------------------

const CONTINUE_CELL: &str = "│  ";
const BLANK_CELL: &str = "   ";

fn pop_blank_cells(cells: &mut Vec<&'static str>) {
    while cells.last() == Some(&BLANK_CELL) {
        cells.pop();
    }
}

/// Há, na pilha da raia, um ancestral ativo no `slot` (com o último filho ainda por desenhar depois de `i`)?
fn has_active_ancestor(stack: &BTreeSet<usize>, slot_of: &[usize], last_child_of: &[Option<usize>], slot: usize, i: usize) -> bool {
    stack.iter().any(|&a| slot_of[a] == slot && last_child_of[a].is_some_and(|last| last > i))
}

/// A indentação de um evento no `slot`: `│  ` onde há ancestral ativo, três espaços no resto.
fn indent_cells(stack: &BTreeSet<usize>, slot_of: &[usize], last_child_of: &[Option<usize>], slot: usize, i: usize) -> Vec<&'static str> {
    (0..slot)
        .map(|s| if has_active_ancestor(stack, slot_of, last_child_of, s, i) { CONTINUE_CELL } else { BLANK_CELL })
        .collect()
}

/// A continuação de uma raia que não é a do evento `i`: `│` em cada slot ocupado por um ancestral ativo
/// (último filho depois de `i`), sem cortar as células vazias do fim.
fn lane_cells(stack: &BTreeSet<usize>, slot_of: &[usize], last_child_of: &[Option<usize>], i: usize) -> Vec<&'static str> {
    let active_slots: Vec<usize> = stack.iter().filter(|&&a| last_child_of[a].is_some_and(|last| last > i)).map(|&a| slot_of[a]).collect();
    let len = active_slots.iter().max().map_or(0, |max| max + 1);
    let mut cells = vec![BLANK_CELL; len];
    for slot in active_slots {
        cells[slot] = CONTINUE_CELL;
    }
    cells
}

/// Renderiza `history` como diagrama de raias: uma coluna por raiz, os eventos na coluna da raiz deles, a
/// relação pai→filho pela indentação `├─`/`└─` e os ancestrais ativos por linhas de continuação `│`.
///
/// Exemplo:
/// ```text
///                  A           B
///   +0ms ├─ setTimeout
///  +10ms │           ├─ setTimeout
///  +16ms ├─ rAF      │
///  +50ms └─ setTimeout
/// ```
pub fn render_swimlanes(history: &ExecutionHistory) -> String {
    let ExecutionHistory { roots, events } = history;
    if events.is_empty() {
        return "(empty history)".to_string();
    }
    if roots.is_empty() {
        return events.iter().map(|e| format!("[+{}ms] {}", number_to_js_string(e.time), e.label)).collect::<Vec<_>>().join("\n");
    }

    let n = events.len();

    // Índice do pai por evento (`None` = filho direto da raiz).
    let mut parent_of: Vec<Option<usize>> = vec![None; n];
    let mut children_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    let index_of_event: HashMap<*const ExecutionEvent, usize> = events.iter().enumerate().map(|(i, e)| (Rc::as_ptr(e), i)).collect();
    for (i, event) in events.iter().enumerate() {
        if let Some(parent_index) = event.parent.as_ref().and_then(|p| index_of_event.get(&Rc::as_ptr(p)).copied()) {
            parent_of[i] = Some(parent_index);
            children_of[parent_index].push(i);
        }
    }

    // Este evento é o último filho do pai dele?
    let is_last_child: Vec<bool> = (0..n).map(|i| parent_of[i].is_some_and(|p| children_of[p].last() == Some(&i))).collect();

    // Slot = índice visual da coluna de indentação. Por padrão cada filho ganha a sua coluna (slot = slot do
    // pai + 1), para cadeias só de último filho ainda mostrarem a profundidade. Passado o limite de
    // profundidade, os últimos filhos colapsam no slot do pai, para traços muito fundos não saírem da tela.
    const COLLAPSE_DEPTH_THRESHOLD: usize = 6;
    let mut depth_of = vec![0usize; n];
    let mut slot_of = vec![0usize; n];
    for i in 0..n {
        if let Some(p) = parent_of[i] {
            depth_of[i] = depth_of[p] + 1;
            let collapse = is_last_child[i] && depth_of[i] >= COLLAPSE_DEPTH_THRESHOLD;
            slot_of[i] = slot_of[p] + usize::from(!collapse);
        }
    }

    // Rótulo exibido = rótulo mais o quadro de quem chamou, quando há, por exemplo
    // `setTimeout · MyClass.foo (file.ts:42)`. Calculado uma vez para a conta de largura e a linha
    // concordarem. `detail_lines_of` guarda os quadros além do primeiro, desenhados como linhas de
    // continuação.
    let mut display_label_of: Vec<String> = Vec::with_capacity(n);
    let mut detail_lines_of: Vec<Vec<&str>> = Vec::with_capacity(n);
    for event in events {
        let frames: Vec<&str> = event.detail.as_deref().filter(|d| !d.is_empty()).map_or_else(Vec::new, |d| d.split('\n').collect());
        display_label_of.push(match frames.first() {
            Some(first) => format!("{} · {first}", event.label),
            None => event.label.clone(),
        });
        detail_lines_of.push(frames.into_iter().skip(1).collect());
    }

    // Largura da coluna por raiz: a indentação usa slots (os últimos filhos colapsam no slot do pai), então a
    // largura precisa ser por slot para não reservar espaço vazio em cadeias degeneradas de último filho.
    let mut width_of: HashMap<*const ExecutionRoot, usize> = HashMap::new();
    for root in roots {
        width_of.insert(Rc::as_ptr(root), js_len(&root.label));
    }
    for i in 0..n {
        let base_indent = slot_of[i] * 3 + 3;
        let max_len = detail_lines_of[i].iter().map(|l| js_len(l) + 2).fold(js_len(&display_label_of[i]), usize::max);
        let width = base_indent + max_len;
        let current = width_of.get(&Rc::as_ptr(&events[i].root)).copied().unwrap_or(0);
        if width > current {
            width_of.insert(Rc::as_ptr(&events[i].root), width);
        }
    }

    // Largura da coluna de tempo pelo maior tempo (arredondado).
    let max_time = js_max(events.iter().map(|e| js_round(e.time)));
    let time_col_width = js_len(&format!("+{}ms", number_to_js_string(max_time)));

    let mut lines: Vec<String> = Vec::new();

    // Cabeçalho: rótulos das raízes centralizados nas colunas.
    let header: Vec<String> = roots
        .iter()
        .map(|root| {
            let width = width_of[&Rc::as_ptr(root)];
            let label_len = js_len(&root.label);
            pad_end(&pad_start(&root.label, (width + label_len).div_ceil(2)), width)
        })
        .collect();
    lines.push(trim_end_js(&format!("{} {}", " ".repeat(time_col_width), header.join("  "))).to_string());

    // Índice do último filho de cada evento (para desenhar as linhas de continuação).
    let last_child_of: Vec<Option<usize>> = children_of.iter().map(|kids| kids.last().copied()).collect();

    // Por raiz: o conjunto de índices de "ancestrais ativos" (eventos com filhos cujo último filho ainda
    // não foi desenhado, isto é, `last_child_of[a] > i`).
    let mut lane_stacks: HashMap<*const ExecutionRoot, BTreeSet<usize>> = roots.iter().map(|r| (Rc::as_ptr(r), BTreeSet::new())).collect();

    for i in 0..n {
        let event = &events[i];
        let time_str = pad_start(&rounded_time_label(event.time), time_col_width);

        let mut parts: Vec<String> = Vec::new();
        for root in roots {
            let width = width_of[&Rc::as_ptr(root)];
            let stack = &lane_stacks[&Rc::as_ptr(root)];

            if Rc::ptr_eq(root, &event.root) {
                // Linha do evento: indentação por slot, depois `├─`/`└─` e o rótulo. Para cada slot `s` em
                // 0..slot mostra `│  ` se há ancestral ativo no slot `s`, senão três espaços.
                let indent = indent_cells(stack, &slot_of, &last_child_of, slot_of[i], i);
                let prefix = if is_last_child[i] { "└─ " } else { "├─ " };
                parts.push(pad_end(&format!("{}{prefix}{}", indent.concat(), display_label_of[i]), width));
            } else {
                // Continuação entre raias. Desenha `│` em cada slot ocupado por um ancestral ativo (último
                // filho depois de `i`). Mostra também um marcador `|` no slot do próximo evento se ele for
                // um filho que não é o último.
                let mut cells = lane_cells(stack, &slot_of, &last_child_of, i);

                // Próximo evento da raiz `r` estritamente depois de `i`.
                let next_j = (i + 1..n).find(|&j| Rc::ptr_eq(&events[j].root, root));
                if let Some(next_j) = next_j.filter(|&j| parent_of[j].is_some()) {
                    let slot = slot_of[next_j];
                    // Reserva o slot se o próximo evento abrir um ramo novo (`├─`).
                    if !is_last_child[next_j] {
                        while cells.len() <= slot {
                            cells.push(BLANK_CELL);
                        }
                        if cells[slot] == BLANK_CELL {
                            cells[slot] = "|  ";
                        }
                    }
                }

                // Corta as células vazias do fim.
                pop_blank_cells(&mut cells);
                parts.push(pad_end(&cells.concat(), width));
            }
        }

        lines.push(trim_end_js(&format!("{time_str} {}", parts.join("  "))).to_string());

        // Linhas de continuação para os quadros de pilha extras. Indentadas sob o rótulo, sem coluna de
        // tempo, sem o glifo `├─`/`└─` e com `│  ` nas raias de ancestrais ativos (inclusive este evento,
        // quando tem filhos ainda não desenhados).
        let extras = &detail_lines_of[i];
        if !extras.is_empty() {
            let stack_for_extras = &lane_stacks[&Rc::as_ptr(&event.root)];
            // Finge que este evento já está na pilha da raia, para a coluna dele ganhar o glifo de
            // continuação debaixo do `├─`/`└─`.
            let has_open_children = !children_of[i].is_empty();
            let mut extra_indent = indent_cells(stack_for_extras, &slot_of, &last_child_of, slot_of[i], i);
            extra_indent.push(if has_open_children { CONTINUE_CELL } else { BLANK_CELL });
            for extra in extras {
                let mut extras_parts: Vec<String> = Vec::new();
                for root in roots {
                    let width = width_of[&Rc::as_ptr(root)];
                    if Rc::ptr_eq(root, &event.root) {
                        extras_parts.push(pad_end(&format!("{}{extra}", extra_indent.concat()), width));
                    } else {
                        // Mesma lógica de continuação: toda raia ativa nas outras raízes precisa dos
                        // glifos `│`.
                        let mut cells = lane_cells(&lane_stacks[&Rc::as_ptr(root)], &slot_of, &last_child_of, i);
                        pop_blank_cells(&mut cells);
                        extras_parts.push(pad_end(&cells.concat(), width));
                    }
                }
                let time_pad = " ".repeat(time_col_width);
                lines.push(trim_end_js(&format!("{time_pad} {}", extras_parts.join("  "))).to_string());
            }
        }

        // Manutenção da pilha: empilha este evento se tem filhos, depois desempilha os ancestrais cujo
        // último filho acabou de ser desenhado (propagando para cima).
        let stack = lane_stacks.get_mut(&Rc::as_ptr(&event.root)).expect("a raiz do evento está em roots");
        if !children_of[i].is_empty() {
            stack.insert(i);
        }
        let mut current = i;
        while is_last_child[current] {
            let Some(parent) = parent_of[current] else { break };
            stack.remove(&parent);
            current = parent;
        }
    }

    lines.join("\n")
}

// ---------------------------------------------------------------------------------------------
// Renderizador: grafo de raias entrelaçadas (estilo git log)
// ---------------------------------------------------------------------------------------------

/// O nó do grafo de raias: o evento, ou a raiz sintética inserida antes do primeiro filho dela.
struct Node {
    label: String,
    /// Índice do pai em `nodes`.
    parent: Option<usize>,
    is_synthetic: bool,
}

/// Renderiza `history` como grafo entrelaçado "git log". Cada evento pai ganha uma coluna, dispostas da
/// esquerda para a direita na ordem dos eventos. Raízes com pelo menos um filho direto viram linhas
/// sintéticas `+rótulo` inseridas antes do primeiro filho.
///
/// Glifos:
///   `╷`  origem da raia (este nó é pai)
///   `│`  a raia passa
///   `├─` o filho se conecta; a raia continua
///   `└─` o último filho se conecta; a raia fecha
///   `┼─` o conector horizontal cruza uma raia ativa
///   `──` o conector horizontal cruza uma coluna vazia
pub fn render_lane_graph(history: &ExecutionHistory) -> String {
    let events = &history.events;
    if events.is_empty() {
        return String::new();
    }

    // Insere os nós sintéticos das raízes antes do primeiro filho.
    let mut nodes: Vec<Node> = Vec::new();
    let mut synthetic_for_root: HashMap<*const ExecutionRoot, usize> = HashMap::new();
    let mut node_by_event: HashMap<*const ExecutionEvent, usize> = HashMap::new();

    // Quais raízes têm pelo menos um evento filho direto?
    let roots_with_children: HashSet<*const ExecutionRoot> = events.iter().filter(|e| e.parent.is_none()).map(|e| Rc::as_ptr(&e.root)).collect();

    for event in events {
        let root_key = Rc::as_ptr(&event.root);
        if roots_with_children.contains(&root_key) && !synthetic_for_root.contains_key(&root_key) {
            synthetic_for_root.insert(root_key, nodes.len());
            nodes.push(Node { label: format!("+{}", event.root.label), parent: None, is_synthetic: true });
        }
        let time_str = pad_start(&format!("+{}ms", number_to_js_string(event.time)), 7);
        let parent = match &event.parent {
            Some(parent_event) => node_by_event.get(&Rc::as_ptr(parent_event)).copied(),
            None => synthetic_for_root.get(&root_key).copied(),
        };
        node_by_event.insert(Rc::as_ptr(event), nodes.len());
        nodes.push(Node { label: format!("[{time_str}] {}", event.label), parent, is_synthetic: false });
    }

    let n = nodes.len();
    let parent_of: Vec<Option<usize>> = nodes.iter().map(|node| node.parent).collect();
    let mut children_of: Vec<Vec<usize>> = vec![Vec::new(); n];
    for (i, parent) in parent_of.iter().enumerate() {
        if let Some(parent) = parent {
            children_of[*parent].push(i);
        }
    }

    // Atribui colunas: todo nó com filhos ganha a sua.
    let mut col_of: Vec<Option<usize>> = vec![None; n];
    let mut total_cols = 0;
    for i in 0..n {
        if !children_of[i].is_empty() {
            col_of[i] = Some(total_cols);
            total_cols += 1;
        }
    }

    if total_cols == 0 {
        return events
            .iter()
            .map(|e| format!("[+{}] {}", pad_start(&format!("{}ms", number_to_js_string(e.time)), 5), e.label))
            .collect::<Vec<_>>()
            .join("\n");
    }

    let mut active: Vec<Option<usize>> = vec![None; total_cols];
    let mut lines: Vec<String> = Vec::new();

    for i in 0..n {
        let node = &nodes[i];
        let parent_index = parent_of[i];
        let connect_col = parent_index.and_then(|p| col_of[p]);
        let last = parent_index.is_some_and(|p| children_of[p].last() == Some(&i));
        let opens_col = if children_of[i].is_empty() { None } else { col_of[i] };
        let horiz_end = parent_index.map(|_| opens_col.unwrap_or(total_cols));

        let mut chars = String::new();
        for c in 0..total_cols {
            let is_active = active[c].is_some();
            let is_connect = Some(c) == connect_col;
            let is_open = Some(c) == opens_col && !is_connect;
            let in_horiz = connect_col.is_some_and(|cc| c > cc) && horiz_end.is_some_and(|end| c < end);

            let (glyph, suffix) = if is_connect {
                (if last { '└' } else { '├' }, '─')
            } else if is_open && node.is_synthetic {
                ('+', node.label.chars().nth(1).unwrap_or('?'))
            } else if is_open && connect_col.is_some() {
                ('╷', '─')
            } else if is_open {
                ('╷', ' ')
            } else if in_horiz && is_active {
                ('┼', '─')
            } else if in_horiz {
                ('─', '─')
            } else if is_active {
                ('│', ' ')
            } else {
                (' ', ' ')
            };
            chars.push(glyph);
            chars.push(suffix);
        }

        if last && let Some(column) = parent_index.and_then(|p| col_of[p]) {
            active[column] = None;
        }
        if let Some(column) = opens_col {
            active[column] = Some(i);
        }

        if node.is_synthetic {
            lines.push(trim_end_js(&chars).to_string());
        } else {
            lines.push(format!("{chars}{}", node.label));
        }
    }

    lines.join("\n")
}
