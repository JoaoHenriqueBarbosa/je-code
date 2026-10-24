//! Porte de `src/vs/workbench/contrib/testing/common/testResult.ts`: o resultado de uma execução de testes, ao vivo
//! (`LiveTestResult`) ou hidratado do armazenamento (`HydratedTestResult`), com a saída bruta de cada tarefa.
//!
//! Divergências:
//! - **Itens compartilhados.** O `TestResultItem` e o `TestResultItemWithChildren` do original são o mesmo objeto, mutado
//!   por referência pela execução e lido por quem recebe os eventos. Aqui é o [`TestResultNode`] (`Rc`): os dados do
//!   item em `data` e os filhos da execução em `children` (vazio no hidratado), com o mesmo `Rc` em `testById`, nos pais e
//!   nos eventos. `getStateById` devolve o nó.
//! - **`TestResultItemChange`** é a struct [`TestResultItemChange`] com o [`TestResultItemChangeKind`], a união discriminada
//!   do original; [`TestResultItemChange::reason`] é o discriminante (`TestResultItemChangeReason`).
//! - **`ITestRunTaskResults`** é a struct [`TestRunTaskResults`], genérica sobre a saída (`output` por último, para o
//!   `Rc` virar `dyn ITaskRawOutput`). `tasks` do `LiveTestResult` guardam o `TaskRawOutput` concreto; `ITestResult::tasks`
//!   devolve a forma com `dyn ITaskRawOutput`.
//! - **`instanceof LiveTestResult`** é [`ITestResult::as_live`].
//! - **Eventos como métodos.** `onChange`, `onComplete`, `onNewTask` e `onEndTask` são métodos que devolvem o `Event`.
//! - **`protected setAllToState`** é público (Rust não tem herança): o `TestLiveTestResult` do teste o expõe. O
//!   `public disposed` que o teste do original acrescenta é [`LiveTestResult::is_disposed`].
//! - **`toJSON` e `toJSONWithMessages`** devolvem `Option<Rc<ISerializedTestResults>>`, com o `Lazy` do original. O teste
//!   `this.completedAt &&` é verdadeiro para um instante diferente de zero.
//! - **`mustGetTaskIndex`** lança um `Error`, como o original.
//! - **`new Date().toLocaleString(language)`** é o formato `M/D/AAAA, h:mm:ss AM` da locale padrão (`en-US`) do porte, no
//!   fuso do sistema, sobre o `Date.now()` do `runtime`.
//! - **`telemetry.publicLog2('test.outcomes', ...)`** passa o objeto como [`Dynamic`].
//! - **Iteráveis** (`tests`, `resultItemParents`) são `Vec`.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use indexmap::IndexMap;
use je_base::buffer::VSBuffer;
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, throw};
use je_base::event::{Emitter, Event};
use je_base::lazy::Lazy;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::observable::{ISettableObservable, Observable, observable_value};
use je_base::prefix_tree::WellDefinedPrefixTree;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_base::strings::remove_ansi_escape_codes;
use je_platform::telemetry::telemetry::ITelemetryService;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use super::get_computed_state::{ComputedStateNode, IComputedStateAccessor, refresh_computed_state};
use super::test_coverage::TestCoverage;
use super::test_id::TestId;
use super::test_types::{
    ISerializedTestResults, ITestItem, ITestMessage, ITestOutputMessage, ITestRunTask, ITestTaskState, ITestUriCanonicalizer, IRichLocation,
    ResolvedTestRunRequest, SerializedTestResultsTask, TestItemExpandState, TestResultItem, TestResultState, get_mark_id,
};
use super::testing_states::{STATES_IN_ORDER, TestStateCount, make_empty_counts, max_priority, terminal_state_priority};

/// `ITestRunTaskResults`.
pub struct TestRunTaskResults<O: ?Sized = dyn ITaskRawOutput> {
    pub id: String,
    pub name: String,
    pub running: Cell<bool>,
    pub ctrl_id: String,

    /// Contains test coverage for the result, if it's available.
    pub coverage: ISettableObservable<Option<Rc<TestCoverage>>>,

    /// Messages from the task not associated with any specific test.
    pub other_messages: RefCell<Vec<ITestOutputMessage>>,

    /// Test results output for the task. Último campo, para o `Rc<TestRunTaskResults<TaskRawOutput>>` virar
    /// `Rc<TestRunTaskResults>` (com `dyn ITaskRawOutput`).
    pub output: O,
}

pub trait ITestResult {
    /// Count of the number of tests in each run state.
    fn counts(&self) -> TestStateCount;

    /// Unique ID of this set of test results.
    fn id(&self) -> String;

    /// If the test is completed, the unix milliseconds time at which it was
    /// completed. If undefined, the test is still running.
    fn completed_at(&self) -> Option<f64>;

    /// Whether this test result is triggered from an auto run.
    fn request(&self) -> ResolvedTestRunRequest;

    /// Human-readable name of the test result.
    fn name(&self) -> String;

    /// Gets all tests involved in the run.
    fn tests(&self) -> Vec<TestResultItemRef>;

    /// List of this result's subtasks.
    fn tasks(&self) -> Vec<Rc<TestRunTaskResults>>;

    /// Gets the state of the test by its extension-assigned ID.
    fn get_state_by_id(&self, test_ext_id: &str) -> Option<TestResultItemRef>;

    /// Serializes the test result. Used to save and restore results
    /// in the workspace.
    fn to_json(&self) -> Option<Rc<ISerializedTestResults>>;

    /// Serializes the test result, includes messages. Used to send the test states to the extension host.
    fn to_json_with_messages(&self) -> Option<Rc<ISerializedTestResults>>;

    /// `result instanceof LiveTestResult`.
    fn as_live(&self) -> Option<Rc<LiveTestResult>>;
}

/// Output type exposed from live test results.
pub trait ITaskRawOutput {
    fn on_did_write_data(&self) -> Event<VSBuffer>;
    fn end_promise(&self) -> Promise<()>;
    fn buffers(&self) -> Vec<VSBuffer>;
    fn length(&self) -> usize;

    /// Gets a continuous buffer for the desired range
    fn get_range(&self, start: usize, length: usize) -> VSBuffer;
    /// Gets an iterator of buffers for the range; may avoid allocation of getRange()
    fn get_range_iter(&self, start: usize, length: usize) -> Vec<VSBuffer>;
}

/// `emptyRawOutput`.
struct EmptyRawOutput;

impl ITaskRawOutput for EmptyRawOutput {
    fn on_did_write_data(&self) -> Event<VSBuffer> {
        Event::none()
    }

    fn end_promise(&self) -> Promise<()> {
        Promise::resolved(())
    }

    fn buffers(&self) -> Vec<VSBuffer> {
        Vec::new()
    }

    fn length(&self) -> usize {
        0
    }

    fn get_range(&self, _start: usize, _length: usize) -> VSBuffer {
        VSBuffer::alloc(0)
    }

    fn get_range_iter(&self, _start: usize, _length: usize) -> Vec<VSBuffer> {
        Vec::new()
    }
}

/// O retorno de [`TaskRawOutput::append`]: `{ offset, length }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AppendedRange {
    pub offset: usize,
    pub length: usize,
}

pub struct TaskRawOutput {
    write_data_emitter: Rc<Emitter<VSBuffer>>,
    end_deferred: DeferredPromise<()>,
    offset: Cell<usize>,
    buffers: RefCell<Vec<VSBuffer>>,
}

impl Default for TaskRawOutput {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskRawOutput {
    pub fn new() -> Self {
        Self { write_data_emitter: Emitter::new(), end_deferred: DeferredPromise::new(), offset: Cell::new(0), buffers: RefCell::new(Vec::new()) }
    }

    /// Appends data to the output, returning the byte range where the data can be found.
    pub fn append(&self, data: &VSBuffer, marker: Option<u32>) -> AppendedRange {
        let offset = self.offset.get();
        let mut length = data.byte_length();
        let Some(marker) = marker else {
            self.push(data.clone());
            return AppendedRange { offset, length };
        };

        // Bytes that should be 'trimmed' off the end of data. This is done because
        // selections in the terminal are based on the entire line, and commonly
        // the interesting marked range has a trailing new line. We don't want to
        // select the trailing line (which might have other data)
        // so we place the marker before all trailing trimbytes.
        const TRIM_BYTES_CR: u8 = 13;
        const TRIM_BYTES_LF: u8 = 10;

        let start = VSBuffer::from_string(&get_mark_code(marker, true));
        let end = VSBuffer::from_string(&get_mark_code(marker, false));
        length += start.byte_length() + end.byte_length();

        self.push(start);
        let mut trim_len = data.byte_length();
        while trim_len > 0 {
            let last = data.read_u_int8(trim_len - 1);
            if last != TRIM_BYTES_CR && last != TRIM_BYTES_LF {
                break;
            }
            trim_len -= 1;
        }

        self.push(data.slice(Some(0), Some(trim_len as i64)));
        self.push(end);
        self.push(data.slice(Some(trim_len as i64), None));

        AppendedRange { offset, length }
    }

    fn push(&self, data: VSBuffer) {
        if data.byte_length() == 0 {
            return;
        }

        self.buffers.borrow_mut().push(data.clone());
        self.write_data_emitter.fire(data.clone());
        self.offset.set(self.offset.get() + data.byte_length());
    }

    /// Signals the output has ended.
    pub fn end(&self) {
        self.end_deferred.complete(());
    }
}

impl ITaskRawOutput for TaskRawOutput {
    /// @inheritdoc
    fn on_did_write_data(&self) -> Event<VSBuffer> {
        self.write_data_emitter.event()
    }

    /// @inheritdoc
    fn end_promise(&self) -> Promise<()> {
        self.end_deferred.promise.clone()
    }

    /// @inheritdoc
    fn buffers(&self) -> Vec<VSBuffer> {
        self.buffers.borrow().clone()
    }

    /// @inheritdoc
    fn length(&self) -> usize {
        self.offset.get()
    }

    /// @inheritdoc
    fn get_range(&self, start: usize, length: usize) -> VSBuffer {
        let buf = VSBuffer::alloc(length);
        let mut buf_last_write = 0;
        for chunk in self.get_range_iter(start, length) {
            buf.set(&chunk, buf_last_write);
            buf_last_write += chunk.byte_length();
        }

        if buf_last_write < length { buf.slice(Some(0), Some(buf_last_write as i64)) } else { buf }
    }

    /// @inheritdoc
    fn get_range_iter(&self, start: usize, length: usize) -> Vec<VSBuffer> {
        let mut result = Vec::new();
        let mut so_far: i64 = 0;
        let mut internal_last_read: i64 = 0;
        let (start, length) = (start as i64, length as i64);
        for b in self.buffers.borrow().iter() {
            let byte_length = b.byte_length() as i64;
            if internal_last_read + byte_length <= start {
                internal_last_read += byte_length;
                continue;
            }

            let bstart = 0.max(start - internal_last_read);
            let bend = byte_length.min(bstart + length - so_far);

            result.push(b.slice(Some(bstart), Some(bend)));
            so_far += bend - bstart;
            internal_last_read += byte_length;

            if so_far == length {
                break;
            }
        }

        result
    }
}

/// `resultItemParents`: o item e os pais dele até a raiz, na ordem do gerador.
pub fn result_item_parents(results: &dyn ITestResult, item: &TestResultItemRef) -> Vec<TestResultItemRef> {
    let ext_id = item.data.borrow().item.ext_id.clone();
    TestId::from_string(&ext_id)
        .ids_to_root()
        .iter()
        .map(|id| results.get_state_by_id(&id.to_string()).expect("pai do teste presente no resultado"))
        .collect()
}

pub fn max_count_priority(counts: &TestStateCount) -> TestResultState {
    for state in STATES_IN_ORDER {
        if counts[state] > 0 {
            return state;
        }
    }

    TestResultState::Unset
}

fn get_mark_code(marker: u32, start: bool) -> String {
    format!("\x1b]633;SetMark;Id={};Hidden\x07", get_mark_id(marker, start))
}

/// `TestResultItemWithChildren`: o item do resultado e os filhos que a execução conhece.
pub struct TestResultNode {
    /// O `TestResultItem`.
    pub data: RefCell<TestResultItem>,
    /// Children in the run
    pub children: RefCell<Vec<TestResultItemRef>>,
    large_children_states: RefCell<Option<TestStateCount>>,
}

pub type TestResultItemRef = Rc<TestResultNode>;

impl TestResultNode {
    pub fn new(item: TestResultItem) -> TestResultItemRef {
        Rc::new(TestResultNode { data: RefCell::new(item), children: RefCell::new(Vec::new()), large_children_states: RefCell::new(None) })
    }
}

impl ComputedStateNode for Rc<TestResultNode> {
    fn identity(&self) -> usize {
        Rc::as_ptr(self) as usize
    }

    fn large_children_states(&self) -> &RefCell<Option<TestStateCount>> {
        &self.large_children_states
    }
}

fn item_to_node(controller_id: &str, item: &ITestItem, _parent: Option<&str>) -> TestResultItemRef {
    TestResultNode::new(TestResultItem {
        controller_id: controller_id.to_string(),
        expand: TestItemExpandState::NotExpandable,
        item: item.clone(),
        tasks: Vec::new(),
        own_computed_state: TestResultState::Unset,
        computed_state: TestResultState::Unset,
        own_duration: None,
        retired: None,
    })
}

/// `TestResultItemChangeReason`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestResultItemChangeReason {
    ComputedStateChange,
    OwnStateChange,
    NewMessage,
}

/// A parte da união `TestResultItemChange` que depende do `reason`.
#[derive(Clone, Debug)]
pub enum TestResultItemChangeKind {
    ComputedStateChange,
    OwnStateChange { previous_state: TestResultState, previous_own_duration: Option<f64> },
    NewMessage { message: ITestMessage },
}

/// `TestResultItemChange`: `{ item, result } & (...)`.
#[derive(Clone)]
pub struct TestResultItemChange {
    pub item: TestResultItemRef,
    pub result: Rc<dyn ITestResult>,
    pub change: TestResultItemChangeKind,
}

impl TestResultItemChange {
    pub fn reason(&self) -> TestResultItemChangeReason {
        match self.change {
            TestResultItemChangeKind::ComputedStateChange => TestResultItemChangeReason::ComputedStateChange,
            TestResultItemChangeKind::OwnStateChange { .. } => TestResultItemChangeReason::OwnStateChange,
            TestResultItemChangeKind::NewMessage { .. } => TestResultItemChangeReason::NewMessage,
        }
    }
}

/// O acessor de estado calculado do `LiveTestResult` (`computedStateAccessor`).
struct LiveStateAccessor {
    test_by_id: Rc<RefCell<IndexMap<String, TestResultItemRef>>>,
}

impl IComputedStateAccessor<TestResultItemRef> for LiveStateAccessor {
    fn get_own_state(&self, item: &TestResultItemRef) -> Option<TestResultState> {
        Some(item.data.borrow().own_computed_state)
    }

    fn get_current_computed_state(&self, item: &TestResultItemRef) -> Option<TestResultState> {
        Some(item.data.borrow().computed_state)
    }

    fn set_computed_state(&self, item: &TestResultItemRef, state: TestResultState) {
        item.data.borrow_mut().computed_state = state;
    }

    fn get_children(&self, item: &TestResultItemRef) -> Vec<TestResultItemRef> {
        item.children.borrow().clone()
    }

    fn get_parents(&self, item: &TestResultItemRef) -> Vec<TestResultItemRef> {
        let ext_id = item.data.borrow().item.ext_id.clone();
        let Some(parent_id) = TestId::from_string(&ext_id).parent_id() else {
            return Vec::new();
        };

        let test_by_ext_id = self.test_by_id.borrow();
        parent_id
            .ids_to_root()
            .iter()
            .map(|id| test_by_ext_id.get(&id.to_string()).cloned().expect("pai do teste presente na execução"))
            .collect()
    }
}

/// `new Date().toLocaleString(language)` na locale padrão do porte (`en-US`).
fn locale_string(date_now_ms: u64) -> String {
    let Ok(instant) = Timestamp::from_millisecond(i64::try_from(date_now_ms).unwrap_or(i64::MAX)) else {
        return "Invalid Date".to_string();
    };
    let zoned = instant.to_zoned(TimeZone::system());
    let hour = zoned.hour();
    let hour12 = if hour % 12 == 0 { 12 } else { hour % 12 };
    format!(
        "{}/{}/{}, {}:{:02}:{:02} {}",
        zoned.month(),
        zoned.day(),
        zoned.year(),
        hour12,
        zoned.minute(),
        zoned.second(),
        if hour < 12 { "AM" } else { "PM" }
    )
}

/// Results of a test. These are created when the test initially started running
/// and marked as "complete" when the run finishes.
pub struct LiveTestResult {
    this: Weak<LiveTestResult>,
    base: Rc<Disposable>,
    complete_emitter: Rc<Emitter<()>>,
    new_task_emitter: Rc<Emitter<usize>>,
    end_task_emitter: Rc<Emitter<usize>>,
    change_emitter: Rc<Emitter<TestResultItemChange>>,
    /// todo@connor4312: convert to a WellDefinedPrefixTree
    test_by_id: Rc<RefCell<IndexMap<String, TestResultItemRef>>>,
    test_marker_counter: Cell<u32>,
    completed_at: Cell<Option<f64>>,
    disposed: Cell<bool>,
    counts: RefCell<TestStateCount>,
    tasks: RefCell<Vec<Rc<TestRunTaskResults<TaskRawOutput>>>>,
    computed_state_accessor: LiveStateAccessor,
    do_serialize: Lazy<Rc<ISerializedTestResults>>,
    do_serialize_with_messages: Lazy<Rc<ISerializedTestResults>>,

    pub started_at: f64,
    pub name: String,
    pub id: String,
    pub persist: bool,
    pub request: ResolvedTestRunRequest,
    pub insert_order: i64,
    telemetry: Rc<dyn ITelemetryService>,
}

impl LiveTestResult {
    pub fn new(id: &str, persist: bool, request: ResolvedTestRunRequest, insert_order: i64, telemetry: Rc<dyn ITelemetryService>) -> Rc<Self> {
        let base = Disposable::new();
        let complete_emitter = Emitter::new();
        let new_task_emitter = Emitter::new();
        let end_task_emitter = Emitter::new();
        let change_emitter = Emitter::new();
        base.register(complete_emitter.clone());
        base.register(new_task_emitter.clone());
        base.register(end_task_emitter.clone());
        base.register(change_emitter.clone());

        let test_by_id: Rc<RefCell<IndexMap<String, TestResultItemRef>>> = Rc::new(RefCell::new(IndexMap::new()));
        let now = runtime::date_now();

        Rc::new_cyclic(|me: &Weak<LiveTestResult>| {
            let lazy_for = |with_messages: bool| {
                let me = me.clone();
                Lazy::new(move || me.upgrade().expect("o resultado está vivo enquanto é lido").serialize(with_messages))
            };

            Self {
                this: me.clone(),
                base,
                complete_emitter,
                new_task_emitter,
                end_task_emitter,
                change_emitter,
                test_by_id: test_by_id.clone(),
                test_marker_counter: Cell::new(0),
                completed_at: Cell::new(None),
                disposed: Cell::new(false),
                counts: RefCell::new(make_empty_counts()),
                tasks: RefCell::new(Vec::new()),
                computed_state_accessor: LiveStateAccessor { test_by_id },
                do_serialize: lazy_for(false),
                do_serialize_with_messages: lazy_for(true),
                started_at: now as f64,
                name: localize("runFinished", "Test run at {0}", &[locale_string(now).as_str()]),
                id: id.to_string(),
                persist,
                request,
                insert_order,
                telemetry,
            }
        })
    }

    pub fn on_change(&self) -> Event<TestResultItemChange> {
        self.change_emitter.event()
    }

    pub fn on_complete(&self) -> Event<()> {
        self.complete_emitter.event()
    }

    pub fn on_new_task(&self) -> Event<usize> {
        self.new_task_emitter.event()
    }

    pub fn on_end_task(&self) -> Event<usize> {
        self.end_task_emitter.event()
    }

    /// Se o resultado já foi descartado (o `disposed` que o `TestLiveTestResult` do teste do original acrescenta).
    pub fn is_disposed(&self) -> bool {
        self.disposed.get()
    }

    /// As tarefas, com a saída bruta concreta (`(ITestRunTaskResults & { output: TaskRawOutput })[]`).
    pub fn live_tasks(&self) -> Vec<Rc<TestRunTaskResults<TaskRawOutput>>> {
        self.tasks.borrow().clone()
    }

    /// Gets an included test item by ID.
    pub fn get_test_by_id(&self, id: &str) -> Option<ITestItem> {
        self.test_by_id.borrow().get(id).map(|node| node.data.borrow().item.clone())
    }

    fn as_result(&self) -> Rc<dyn ITestResult> {
        self.this.upgrade().expect("o resultado está vivo enquanto é usado")
    }

    /// Appends output that occurred during the test run.
    pub fn append_output(&self, output: &VSBuffer, task_id: &str, location: Option<IRichLocation>, test_id: Option<&str>) {
        let raw_preview = if output.byte_length() > 100 { format!("{}…", output.slice(Some(0), Some(100))) } else { output.to_string() };
        let preview = remove_ansi_escape_codes(&raw_preview.encode_utf16().collect::<Vec<u16>>()).to_string_lossy();
        let test_id = test_id.filter(|id| !id.is_empty());
        let mut marker: Option<u32> = None;

        // currently, the UI only exposes jump-to-message from tests or locations,
        // so no need to mark outputs that don't come from either of those.
        if test_id.is_some() || location.is_some() {
            marker = Some(self.test_marker_counter.get());
            self.test_marker_counter.set(self.test_marker_counter.get() + 1);
        }

        let index = self.must_get_task_index(task_id);
        let task = self.tasks.borrow()[index].clone();

        let AppendedRange { offset, length } = task.output.append(output, marker);
        let message = ITestOutputMessage { location, message: preview, offset, length, marker };

        let test = test_id.and_then(|id| self.test_by_id.borrow().get(id).cloned());
        if let Some(test) = test {
            test.data.borrow_mut().tasks[index].messages.push(ITestMessage::Output(message.clone()));
            self.change_emitter.fire(TestResultItemChange {
                item: test,
                result: self.as_result(),
                change: TestResultItemChangeKind::NewMessage { message: ITestMessage::Output(message) },
            });
        } else {
            task.other_messages.borrow_mut().push(message);
        }
    }

    /// Adds a new run task to the results.
    pub fn add_task(&self, task: ITestRunTask) {
        let task_count = {
            let mut tasks = self.tasks.borrow_mut();
            tasks.push(Rc::new(TestRunTaskResults {
                id: task.id,
                name: task.name,
                running: Cell::new(task.running),
                ctrl_id: task.ctrl_id,
                coverage: observable_value::<Option<Rc<TestCoverage>>, ()>("coverage", None),
                other_messages: RefCell::new(Vec::new()),
                output: TaskRawOutput::new(),
            }));
            tasks.len()
        };

        for test in self.tests() {
            test.data.borrow_mut().tasks.push(ITestTaskState { duration: None, messages: Vec::new(), state: TestResultState::Unset });
        }

        self.new_task_emitter.fire(task_count - 1);
    }

    /// Add the chain of tests to the run. The first test in the chain should
    /// be either a test root, or a previously-known test.
    pub fn add_test_chain_to_run(&self, controller_id: &str, chain: &[ITestItem]) {
        let existing = self.test_by_id.borrow().get(&chain[0].ext_id).cloned();
        let mut parent = match existing {
            Some(parent) => parent,
            None => self.add_test_to_run(controller_id, &chain[0], None), // must be a test root
        };

        for item in &chain[1..] {
            let parent_ext_id = parent.data.borrow().item.ext_id.clone();
            parent = self.add_test_to_run(controller_id, item, Some(&parent_ext_id));
        }
    }

    /// Updates the state of the test by its internal ID.
    pub fn update_state(&self, test_id: &str, task_id: &str, state: TestResultState, duration: Option<f64>) {
        let Some(entry) = self.test_by_id.borrow().get(test_id).cloned() else {
            return;
        };

        let index = self.must_get_task_index(task_id);

        let old_terminal_state_prio = terminal_state_priority(entry.data.borrow().tasks[index].state);
        let new_terminal_state_prio = terminal_state_priority(state);

        // Ignore requests to set the state from one terminal state back to a
        // "lower" one, e.g. from failed back to passed:
        if let Some(old) = old_terminal_state_prio
            && new_terminal_state_prio.is_none_or(|new| new < old)
        {
            return;
        }

        self.fire_update_and_refresh(&entry, index, state, duration);
    }

    /// Appends a message for the test in the run.
    pub fn append_message(&self, test_id: &str, task_id: &str, message: ITestMessage) {
        let Some(entry) = self.test_by_id.borrow().get(test_id).cloned() else {
            return;
        };

        let index = self.must_get_task_index(task_id);
        entry.data.borrow_mut().tasks[index].messages.push(message.clone());
        self.change_emitter.fire(TestResultItemChange { item: entry, result: self.as_result(), change: TestResultItemChangeKind::NewMessage { message } });
    }

    /// Marks the task in the test run complete.
    pub fn mark_task_complete(&self, task_id: &str) {
        let index = self.must_get_task_index(task_id);
        let task = self.tasks.borrow()[index].clone();
        task.running.set(false);
        task.output.end();

        self.set_all_to_state(TestResultState::Skipped, task_id, |t, _| t.state == TestResultState::Queued || t.state == TestResultState::Running);

        self.end_task_emitter.fire(index);
    }

    /// Notifies the service that all tests are complete.
    pub fn mark_complete(&self) {
        if self.completed_at.get().is_some() {
            throw(Error::new("cannot complete a test result multiple times"));
        }

        let running: Vec<String> = self.tasks.borrow().iter().filter(|task| task.running.get()).map(|task| task.id.clone()).collect();
        for task_id in running {
            self.mark_task_complete(&task_id);
        }

        self.completed_at.set(Some(runtime::date_now() as f64));
        self.complete_emitter.fire(());

        let counts = *self.counts.borrow();
        self.telemetry.public_log2(
            "test.outcomes",
            Some(Dynamic::object([
                ("failures", Dynamic::from(f64::from(counts[TestResultState::Errored]) + f64::from(counts[TestResultState::Failed]))),
                ("passes", Dynamic::from(f64::from(counts[TestResultState::Passed]))),
                ("controller", Dynamic::from(self.request.targets.iter().map(|t| t.controller_id.clone()).collect::<Vec<_>>().join(","))),
            ])),
        );
    }

    /// Marks the test and all of its children in the run as retired.
    pub fn mark_retired(&self, test_ids: Option<&WellDefinedPrefixTree<()>>) {
        let entries: Vec<(String, TestResultItemRef)> = self.test_by_id.borrow().iter().map(|(id, test)| (id.clone(), test.clone())).collect();
        for (id, test) in entries {
            let retired = test.data.borrow().retired.unwrap_or(false);
            if !retired && test_ids.is_none_or(|ids| ids.has_key_or_parent(TestId::from_string(&id).path())) {
                test.data.borrow_mut().retired = Some(true);
                self.change_emitter.fire(TestResultItemChange { item: test, result: self.as_result(), change: TestResultItemChangeKind::ComputedStateChange });
            }
        }
    }

    /// Updates all tests in the collection to the given state.
    pub fn set_all_to_state(&self, state: TestResultState, task_id: &str, when: impl Fn(&ITestTaskState, &TestResultItem) -> bool) {
        let index = self.must_get_task_index(task_id);
        let tests: Vec<TestResultItemRef> = self.test_by_id.borrow().values().cloned().collect();
        for test in tests {
            let matches = {
                let data = test.data.borrow();
                when(&data.tasks[index], &data)
            };
            if matches {
                self.fire_update_and_refresh(&test, index, state, None);
            }
        }
    }

    fn fire_update_and_refresh(&self, entry: &TestResultItemRef, task_index: usize, new_state: TestResultState, new_own_duration: Option<f64>) {
        let (previous_own_computed, previous_own_duration) = {
            let data = entry.data.borrow();
            (data.own_computed_state, data.own_duration)
        };
        let change_event = TestResultItemChange {
            item: entry.clone(),
            result: self.as_result(),
            change: TestResultItemChangeKind::OwnStateChange { previous_state: previous_own_computed, previous_own_duration },
        };

        let new_own_computed = {
            let mut data = entry.data.borrow_mut();
            data.tasks[task_index].state = new_state;
            if let Some(new_own_duration) = new_own_duration {
                data.tasks[task_index].duration = Some(new_own_duration);
                data.own_duration = Some(data.own_duration.unwrap_or(0.0).max(new_own_duration));
            }

            let states: Vec<TestResultState> = data.tasks.iter().map(|t| t.state).collect();
            max_priority(&states)
        };
        if new_own_computed == previous_own_computed {
            if new_own_duration != previous_own_duration {
                self.change_emitter.fire(change_event); // fire manually since state change won't do it
            }
            return;
        }

        entry.data.borrow_mut().own_computed_state = new_own_computed;
        {
            let mut counts = self.counts.borrow_mut();
            counts.decrement(previous_own_computed);
            counts.increment(new_own_computed);
        }
        for t in refresh_computed_state(&self.computed_state_accessor, entry, None, true) {
            if Rc::ptr_eq(&t, entry) {
                self.change_emitter.fire(change_event.clone());
            } else {
                self.change_emitter.fire(TestResultItemChange { item: t, result: self.as_result(), change: TestResultItemChangeKind::ComputedStateChange });
            }
        }
    }

    fn add_test_to_run(&self, controller_id: &str, item: &ITestItem, parent: Option<&str>) -> TestResultItemRef {
        let node = item_to_node(controller_id, item, parent);
        self.test_by_id.borrow_mut().insert(item.ext_id.clone(), node.clone());
        self.counts.borrow_mut().increment(TestResultState::Unset);

        if let Some(parent) = parent {
            let parent_node = self.test_by_id.borrow().get(parent).cloned();
            if let Some(parent_node) = parent_node {
                parent_node.children.borrow_mut().push(node.clone());
            }
        }

        let task_count = self.tasks.borrow().len();
        for _ in 0..task_count {
            node.data.borrow_mut().tasks.push(ITestTaskState { duration: None, messages: Vec::new(), state: TestResultState::Unset });
        }

        node
    }

    fn must_get_task_index(&self, task_id: &str) -> usize {
        match self.tasks.borrow().iter().position(|t| t.id == task_id) {
            Some(index) => index,
            None => throw(Error::new(format!("Unknown task {task_id} in updateState"))),
        }
    }

    fn serialize(&self, with_messages: bool) -> Rc<ISerializedTestResults> {
        let items = self
            .test_by_id
            .borrow()
            .values()
            .map(|node| {
                let data = node.data.borrow();
                if with_messages { TestResultItem::serialize(&data) } else { TestResultItem::serialize_without_messages(&data) }
            })
            .collect();
        Rc::new(ISerializedTestResults {
            id: self.id.clone(),
            completed_at: self.completed_at.get().unwrap_or(0.0),
            tasks: self
                .tasks
                .borrow()
                .iter()
                .map(|t| SerializedTestResultsTask { id: t.id.clone(), name: Some(t.name.clone()), ctrl_id: t.ctrl_id.clone(), has_coverage: t.coverage.get().is_some() })
                .collect(),
            name: self.name.clone(),
            request: self.request.clone(),
            items,
        })
    }

    fn is_persisted_and_complete(&self) -> bool {
        self.completed_at.get().is_some_and(|completed_at| completed_at != 0.0) && self.persist
    }
}

impl ITestResult for LiveTestResult {
    /// @inheritdoc
    fn counts(&self) -> TestStateCount {
        *self.counts.borrow()
    }

    fn id(&self) -> String {
        self.id.clone()
    }

    /// @inheritdoc
    fn completed_at(&self) -> Option<f64> {
        self.completed_at.get()
    }

    fn request(&self) -> ResolvedTestRunRequest {
        self.request.clone()
    }

    fn name(&self) -> String {
        self.name.clone()
    }

    /// @inheritdoc
    fn tests(&self) -> Vec<TestResultItemRef> {
        self.test_by_id.borrow().values().cloned().collect()
    }

    fn tasks(&self) -> Vec<Rc<TestRunTaskResults>> {
        self.tasks.borrow().iter().map(|task| task.clone() as Rc<TestRunTaskResults>).collect()
    }

    /// @inheritdoc
    fn get_state_by_id(&self, ext_test_id: &str) -> Option<TestResultItemRef> {
        self.test_by_id.borrow().get(ext_test_id).cloned()
    }

    /// @inheritdoc
    fn to_json(&self) -> Option<Rc<ISerializedTestResults>> {
        if self.is_persisted_and_complete() { Some(self.do_serialize.value()) } else { None }
    }

    fn to_json_with_messages(&self) -> Option<Rc<ISerializedTestResults>> {
        if self.is_persisted_and_complete() { Some(self.do_serialize_with_messages.value()) } else { None }
    }

    fn as_live(&self) -> Option<Rc<LiveTestResult>> {
        self.this.upgrade()
    }
}

impl IDisposable for LiveTestResult {
    fn dispose(&self) {
        self.disposed.set(true);
        self.base.dispose();
    }
}

/// Test results hydrated from a previously-serialized test run.
pub struct HydratedTestResult {
    counts: TestStateCount,
    id: String,
    completed_at: f64,
    tasks: Vec<Rc<TestRunTaskResults>>,
    name: String,
    request: ResolvedTestRunRequest,
    test_by_id: IndexMap<String, TestResultItemRef>,
    serialized: Rc<ISerializedTestResults>,
    persist: bool,
}

impl HydratedTestResult {
    /// `new HydratedTestResult(identity, serialized)` (com `persist = true`).
    pub fn new(identity: &(impl ITestUriCanonicalizer + ?Sized), serialized: Rc<ISerializedTestResults>) -> Rc<Self> {
        Self::with_persist(identity, serialized, true)
    }

    pub fn with_persist(identity: &(impl ITestUriCanonicalizer + ?Sized), serialized: Rc<ISerializedTestResults>, persist: bool) -> Rc<Self> {
        let tasks = serialized
            .tasks
            .iter()
            .map(|task| {
                Rc::new(TestRunTaskResults {
                    id: task.id.clone(),
                    name: task.name.clone().filter(|name| !name.is_empty()).unwrap_or_else(|| localize("testUnnamedTask", "Unnamed Task", &[])),
                    ctrl_id: task.ctrl_id.clone(),
                    running: Cell::new(false),
                    coverage: observable_value::<Option<Rc<TestCoverage>>, ()>("coverage", None),
                    other_messages: RefCell::new(Vec::new()),
                    output: EmptyRawOutput,
                }) as Rc<TestRunTaskResults>
            })
            .collect();

        let mut counts = make_empty_counts();
        let mut test_by_id = IndexMap::new();
        for item in &serialized.items {
            let de = TestResultItem::deserialize(identity, item);
            counts.increment(de.own_computed_state);
            test_by_id.insert(item.item.ext_id.clone(), TestResultNode::new(de));
        }

        Rc::new(Self {
            counts,
            id: serialized.id.clone(),
            completed_at: serialized.completed_at,
            tasks,
            name: serialized.name.clone(),
            request: serialized.request.clone(),
            test_by_id,
            serialized,
            persist,
        })
    }
}

impl ITestResult for HydratedTestResult {
    /// @inheritdoc
    fn counts(&self) -> TestStateCount {
        self.counts
    }

    /// @inheritdoc
    fn id(&self) -> String {
        self.id.clone()
    }

    /// @inheritdoc
    fn completed_at(&self) -> Option<f64> {
        Some(self.completed_at)
    }

    /// @inheritdoc
    fn request(&self) -> ResolvedTestRunRequest {
        self.request.clone()
    }

    /// @inheritdoc
    fn name(&self) -> String {
        self.name.clone()
    }

    /// @inheritdoc
    fn tests(&self) -> Vec<TestResultItemRef> {
        self.test_by_id.values().cloned().collect()
    }

    /// @inheritdoc
    fn tasks(&self) -> Vec<Rc<TestRunTaskResults>> {
        self.tasks.clone()
    }

    /// @inheritdoc
    fn get_state_by_id(&self, ext_test_id: &str) -> Option<TestResultItemRef> {
        self.test_by_id.get(ext_test_id).cloned()
    }

    /// @inheritdoc
    fn to_json(&self) -> Option<Rc<ISerializedTestResults>> {
        if self.persist { Some(self.serialized.clone()) } else { None }
    }

    /// @inheritdoc
    fn to_json_with_messages(&self) -> Option<Rc<ISerializedTestResults>> {
        self.to_json()
    }

    fn as_live(&self) -> Option<Rc<LiveTestResult>> {
        None
    }
}
