//! Porte de `src/vs/workbench/api/test/common/extHostTerminalShellIntegration.test.ts` (suíte
//! `InternalTerminalShellIntegration`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; os casos rodam em
//!   `run_with_faked_timers` (o `setup` do mocha fica dentro, porque o `read()` e o `for await` precisam do laço de
//!   microtarefas). `store.add(...)` é o `store` do guarda.
//! - `terminal = Symbol('testTerminal')` é um `Rc<()>` apagado para [`Terminal`] (só a identidade importa).
//! - `Object.freeze({ confidence: High, value, isTrusted: true })` é um [`TerminalShellExecutionCommandLine`]; o
//!   `asCmdLine(string | commandLine)` não precisa de dois ramos, porque todo caso passa texto (`cmd_line`).
//! - `startExecutionAwaitObject` e `endExecutionAwaitObject` são `Promise::new()` cujo resolvedor é chamado pelo ouvinte
//!   (a segunda chamada de um `r(...)` é ignorada, como no JS). `startExecutionAwaitObject(testCommandLine2)` sem `await`
//!   (`no end event`) descarta a promessa.
//! - `emitData`: o `await new Promise(r => queueMicrotask(r))` é a promessa resolvida por `runtime::queue_microtask`.
//! - O ouvinte `async e => { ... for await (const data of stream) ... }` é um `async_fn` disparado pelo ouvinte; o
//!   `DeferredPromise<void>` é o `DeferredPromise<()>`, e `await Promise.all(readIteratorsFlushed)` é `Promise::all`.
//! - `requestNewShellExecution(...)` devolve a `InternalTerminalShellExecution`; como o `value` dela é a própria
//!   [`TerminalShellExecution`] no porte, `apiRequestedExecution.value` é o valor devolvido. `strictEqual` e
//!   `notStrictEqual` entre execuções são `Rc::ptr_eq`.
//! - `deepStrictEqual(trackedEvents, expected)` compara [`TrackedEvent`] (`type`, `commandLine` e `data` opcional).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::any::Any;
use std::cell::RefCell;
use std::ops::ControlFlow;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::async_::{async_fn, for_await};
use je_base::event::Emitter;
use je_base::lifecycle::DisposableStore;
use je_base::promise::{DeferredPromise, Promise};
use je_base::runtime;
use je_base::uri::URI;
use je_workbench::api::common::ext_host_terminal_shell_integration::{
    InternalTerminalShellIntegration, Terminal, TerminalShellExecution, TerminalShellExecutionCommandLine, TerminalShellExecutionStartEvent,
};
use je_workbench::api::common::ext_host_types::TerminalShellExecutionCommandLineConfidence;

/// `cmdLine(value)`.
fn cmd_line(value: &str) -> TerminalShellExecutionCommandLine {
    TerminalShellExecutionCommandLine { confidence: TerminalShellExecutionCommandLineConfidence::High, value: value.to_owned(), is_trusted: true }
}

/// `vsc(data)`.
fn vsc(data: &str) -> String {
    format!("\x1b]633;{data}\x07")
}

const TEST_COMMAND_LINE: &str = "echo hello world";
const TEST_COMMAND_LINE_2: &str = "echo goodbye world";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EventType {
    Start,
    Data,
    End,
}

/// `ITrackedEvent`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct TrackedEvent {
    r#type: EventType,
    command_line: String,
    data: Option<String>,
}

fn tracked(r#type: EventType, command_line: &str, data: Option<&str>) -> TrackedEvent {
    TrackedEvent { r#type, command_line: command_line.to_owned(), data: data.map(str::to_owned) }
}

/// As variáveis do `suite` preenchidas pelo `setup`.
struct Fixture {
    store: Rc<DisposableStore>,
    si: Rc<InternalTerminalShellIntegration>,
    on_did_start_terminal_shell_execution: Rc<Emitter<TerminalShellExecutionStartEvent>>,
    tracked_events: Rc<RefCell<Vec<TrackedEvent>>>,
    read_iterators_flushed: Rc<RefCell<Vec<Promise<()>>>>,
}

impl Fixture {
    /// O `setup`.
    fn new(store: Rc<DisposableStore>) -> Fixture {
        let terminal: Terminal = Rc::new(()) as Rc<dyn Any>;
        let on_did_start_terminal_shell_execution = store.add_rc(Emitter::<TerminalShellExecutionStartEvent>::new());
        let si = store.add_rc(InternalTerminalShellIntegration::new(terminal, true, on_did_start_terminal_shell_execution.clone()));

        let tracked_events: Rc<RefCell<Vec<TrackedEvent>>> = Rc::default();
        let read_iterators_flushed: Rc<RefCell<Vec<Promise<()>>>> = Rc::default();
        store.add(on_did_start_terminal_shell_execution.event().listen({
            let tracked_events = tracked_events.clone();
            let read_iterators_flushed = read_iterators_flushed.clone();
            move |e| {
                tracked_events.borrow_mut().push(tracked(EventType::Start, &e.execution.command_line().value, None));
                let stream = e.execution.read();
                let read_iterators_flushed_deferred = Rc::new(DeferredPromise::<()>::new());
                read_iterators_flushed.borrow_mut().push(read_iterators_flushed_deferred.promise.clone());
                let execution = e.execution.clone();
                let tracked_events = tracked_events.clone();
                drop(async_fn(async move {
                    for_await(&*stream, |data| {
                        tracked_events.borrow_mut().push(tracked(EventType::Data, &execution.command_line().value, Some(&data)));
                        Ok(ControlFlow::Continue(()))
                    })
                    .await?;
                    drop(read_iterators_flushed_deferred.complete(()));
                    Ok(())
                }));
            }
        }));
        store.add(si.on_did_request_end_execution().listen({
            let tracked_events = tracked_events.clone();
            move |e| tracked_events.borrow_mut().push(tracked(EventType::End, &e.execution.command_line().value, None))
        }));

        Fixture { store, si, on_did_start_terminal_shell_execution, tracked_events, read_iterators_flushed }
    }

    /// `startExecutionAwaitObject(commandLine, cwd)`.
    fn start_execution_await_object(&self, command_line: &str, cwd: Option<URI>) -> Promise<Rc<TerminalShellExecution>> {
        let (promise, resolver) = Promise::new();
        self.store.add(self.on_did_start_terminal_shell_execution.event().listen(move |e| resolver.resolve(e.execution.clone())));
        self.si.start_shell_execution(cmd_line(command_line), cwd);
        promise
    }

    /// `endExecutionAwaitObject(commandLine)`.
    fn end_execution_await_object(&self, command_line: &str) -> Promise<Rc<TerminalShellExecution>> {
        let (promise, resolver) = Promise::new();
        self.store.add(self.si.on_did_request_end_execution().listen(move |e| resolver.resolve(e.execution.clone())));
        self.si.end_shell_execution(Some(cmd_line(command_line)), Some(0));
        promise
    }

    /// `emitData(data)`.
    async fn emit_data(&self, data: &str) {
        let (promise, resolver) = Promise::new();
        runtime::queue_microtask(move || resolver.resolve(()));
        promise.await.unwrap();
        self.si.emit_data(data);
    }

    fn tracked_events(&self) -> Vec<TrackedEvent> {
        self.tracked_events.borrow().clone()
    }

    /// `assertTrackedEvents`.
    fn assert_tracked_events(&self, expected: Vec<TrackedEvent>) {
        assert_eq!(self.tracked_events(), expected);
    }

    /// `assertNonDataTrackedEvents`.
    fn assert_non_data_tracked_events(&self, expected: Vec<TrackedEvent>) {
        let actual: Vec<TrackedEvent> = self.tracked_events().into_iter().filter(|e| e.r#type != EventType::Data).collect();
        assert_eq!(actual, expected);
    }

    /// `assertDataTrackedEvents`.
    fn assert_data_tracked_events(&self, expected: Vec<TrackedEvent>) {
        let actual: Vec<TrackedEvent> = self.tracked_events().into_iter().filter(|e| e.r#type == EventType::Data).collect();
        assert_eq!(actual, expected);
    }

    /// `await Promise.all(readIteratorsFlushed)`.
    async fn all_read_iterators_flushed(&self) {
        let promises = self.read_iterators_flushed.borrow().clone();
        Promise::all(promises).await.unwrap();
    }
}

/// Um caso: o guarda, o `setup` e o corpo.
fn run_test<Fut: std::future::Future<Output = ()> + 'static>(body: impl FnOnce(Fixture) -> Fut + 'static) {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let store = guard.store();
    run_with_faked_timers(async move {
        body(Fixture::new(store)).await;
    });
}

#[test]
fn simple_execution() {
    run_test(|f| async move {
        let execution = f.start_execution_await_object(TEST_COMMAND_LINE, None).await.unwrap();
        assert_eq!(execution.command_line().value, TEST_COMMAND_LINE);
        let execution2 = f.end_execution_await_object(TEST_COMMAND_LINE).await.unwrap();
        assert!(Rc::ptr_eq(&execution2, &execution));

        f.assert_tracked_events(vec![tracked(EventType::Start, TEST_COMMAND_LINE, None), tracked(EventType::End, TEST_COMMAND_LINE, None)]);
    });
}

#[test]
fn different_execution_unexpectedly_ended() {
    run_test(|f| async move {
        let execution1 = f.start_execution_await_object(TEST_COMMAND_LINE, None).await.unwrap();
        let execution2 = f.end_execution_await_object(TEST_COMMAND_LINE_2).await.unwrap();
        assert!(Rc::ptr_eq(&execution1, &execution2), "when a different execution is ended, the one that started first should end");

        f.assert_tracked_events(vec![
            tracked(EventType::Start, TEST_COMMAND_LINE, None),
            // This looks weird, but it's the same execution behind the scenes, just the command
            // line was updated
            tracked(EventType::End, TEST_COMMAND_LINE_2, None),
        ]);
    });
}

#[test]
fn no_end_event() {
    run_test(|f| async move {
        let execution1 = f.start_execution_await_object(TEST_COMMAND_LINE, None).await.unwrap();
        let (promise, resolver) = Promise::new();
        f.store.add(f.si.on_did_request_end_execution().listen(move |e| resolver.resolve(e.execution.clone())));
        drop(f.start_execution_await_object(TEST_COMMAND_LINE_2, None));
        let ended_execution = promise.await.unwrap();
        assert!(Rc::ptr_eq(&execution1, &ended_execution), "when no end event is fired, the current execution should end");

        // Clean up disposables
        f.end_execution_await_object(TEST_COMMAND_LINE_2).await.unwrap();
        f.all_read_iterators_flushed().await;

        f.assert_tracked_events(vec![
            tracked(EventType::Start, TEST_COMMAND_LINE, None),
            tracked(EventType::End, TEST_COMMAND_LINE, None),
            tracked(EventType::Start, TEST_COMMAND_LINE_2, None),
            tracked(EventType::End, TEST_COMMAND_LINE_2, None),
        ]);
    });
}

#[test]
fn execute_command_ctrl_c_to_clear_previous_command() {
    run_test(|f| async move {
        let command_line = "foo";
        let api_requested_execution = f.si.request_new_shell_execution(cmd_line(command_line), None);
        let first_execution = f.start_execution_await_object("^C", None).await.unwrap();
        assert!(!Rc::ptr_eq(&first_execution, &api_requested_execution));
        f.si.emit_data("SIGINT");
        f.si.end_shell_execution(Some(cmd_line("^C")), Some(0));
        f.si.start_shell_execution(cmd_line(command_line), None);
        f.emit_data("1").await;
        f.end_execution_await_object(command_line).await.unwrap();
        // IMPORTANT: We cannot reliably assert the order of data events here because flushing
        // of the async iterator is asynchronous and could happen after the execution's end
        // event fires if an execution is started immediately afterwards.
        f.all_read_iterators_flushed().await;

        f.assert_non_data_tracked_events(vec![
            tracked(EventType::Start, "^C", None),
            tracked(EventType::End, "^C", None),
            tracked(EventType::Start, command_line, None),
            tracked(EventType::End, command_line, None),
        ]);
        f.assert_data_tracked_events(vec![tracked(EventType::Data, "^C", Some("SIGINT")), tracked(EventType::Data, command_line, Some("1"))]);
    });
}

#[test]
fn execute_command_multi_line_command_line() {
    run_test(|f| async move {
        let command_line = "foo\nbar";
        let api_requested_execution = f.si.request_new_shell_execution(cmd_line(command_line), None);
        let started_execution = f.start_execution_await_object("foo", None).await.unwrap();
        assert!(Rc::ptr_eq(&started_execution, &api_requested_execution));

        f.si.emit_data("1");
        f.si.emit_data("2");
        f.si.end_shell_execution(Some(cmd_line("foo")), Some(0));
        f.si.start_shell_execution(cmd_line("bar"), None);
        f.si.emit_data("3");
        f.si.emit_data("4");
        let ended_execution = f.end_execution_await_object("bar").await.unwrap();
        assert!(Rc::ptr_eq(&started_execution, &ended_execution));

        f.assert_tracked_events(vec![
            tracked(EventType::Start, command_line, None),
            tracked(EventType::Data, command_line, Some("1")),
            tracked(EventType::Data, command_line, Some("2")),
            tracked(EventType::Data, command_line, Some("3")),
            tracked(EventType::Data, command_line, Some("4")),
            tracked(EventType::End, command_line, None),
        ]);
    });
}

#[test]
fn execute_command_multi_line_command_with_long_second_command() {
    run_test(|f| async move {
        let command_line = "echo foo\ncat << EOT\nline1\nline2\nline3\nEOT";
        let sub_command_line_1 = "echo foo";
        let sub_command_line_2 = "cat << EOT\nline1\nline2\nline3\nEOT";

        let api_requested_execution = f.si.request_new_shell_execution(cmd_line(command_line), None);
        let started_execution = f.start_execution_await_object(sub_command_line_1, None).await.unwrap();
        assert!(Rc::ptr_eq(&started_execution, &api_requested_execution));

        f.si.emit_data(&format!("{}foo", vsc("C")));
        f.si.end_shell_execution(Some(cmd_line(sub_command_line_1)), Some(0));
        f.si.start_shell_execution(cmd_line(sub_command_line_2), None);
        f.si.emit_data(&format!("{}line1", vsc("C")));
        f.si.emit_data("line2");
        f.si.emit_data("line3");
        let ended_execution = f.end_execution_await_object(sub_command_line_2).await.unwrap();
        assert!(Rc::ptr_eq(&started_execution, &ended_execution));

        f.assert_tracked_events(vec![
            tracked(EventType::Start, command_line, None),
            tracked(EventType::Data, command_line, Some(&format!("{}foo", vsc("C")))),
            tracked(EventType::Data, command_line, Some(&format!("{}line1", vsc("C")))),
            tracked(EventType::Data, command_line, Some("line2")),
            tracked(EventType::Data, command_line, Some("line3")),
            tracked(EventType::End, command_line, None),
        ]);
    });
}

#[test]
fn execute_command_multi_line_command_comment_followed_by_long_second_command() {
    run_test(|f| async move {
        let command_line = "# comment: foo\ncat << EOT\nline1\nline2\nline3\nEOT";
        let sub_command_line_1 = "# comment: foo";
        let sub_command_line_2 = "cat << EOT\nline1\nline2\nline3\nEOT";

        let api_requested_execution = f.si.request_new_shell_execution(cmd_line(command_line), None);
        let started_execution = f.start_execution_await_object(sub_command_line_1, None).await.unwrap();
        assert!(Rc::ptr_eq(&started_execution, &api_requested_execution));

        f.si.emit_data(&vsc("C"));
        f.si.end_shell_execution(Some(cmd_line(sub_command_line_1)), Some(0));
        f.si.start_shell_execution(cmd_line(sub_command_line_2), None);
        f.si.emit_data(&format!("{}line1", vsc("C")));
        f.si.emit_data("line2");
        f.si.emit_data("line3");
        let ended_execution = f.end_execution_await_object(sub_command_line_2).await.unwrap();
        assert!(Rc::ptr_eq(&started_execution, &ended_execution));

        f.assert_tracked_events(vec![
            tracked(EventType::Start, command_line, None),
            tracked(EventType::Data, command_line, Some(&vsc("C"))),
            tracked(EventType::Data, command_line, Some(&format!("{}line1", vsc("C")))),
            tracked(EventType::Data, command_line, Some("line2")),
            tracked(EventType::Data, command_line, Some("line3")),
            tracked(EventType::End, command_line, None),
        ]);
    });
}

#[test]
fn execute_command_4_multi_line_commands_with_output() {
    run_test(|f| async move {
        let command_line = "echo \"\nfoo\"\ngit commit -m \"hello\n\nworld\"\ncat << EOT\nline1\nline2\nline3\nEOT\n{\necho \"foo\"\n}";
        let sub_command_line_1 = "echo \"\nfoo\"";
        let sub_command_line_2 = "git commit -m \"hello\n\nworld\"";
        let sub_command_line_3 = "cat << EOT\nline1\nline2\nline3\nEOT";
        let sub_command_line_4 = "{\necho \"foo\"\n}";

        let api_requested_execution = f.si.request_new_shell_execution(cmd_line(command_line), None);
        let started_execution = f.start_execution_await_object(sub_command_line_1, None).await.unwrap();
        assert!(Rc::ptr_eq(&started_execution, &api_requested_execution));

        f.si.emit_data(&format!("{}foo", vsc("C")));
        f.si.end_shell_execution(Some(cmd_line(sub_command_line_1)), Some(0));
        f.si.start_shell_execution(cmd_line(sub_command_line_2), None);
        f.si.emit_data(&format!("{} 2 files changed, 61 insertions(+), 2 deletions(-)", vsc("C")));
        f.si.end_shell_execution(Some(cmd_line(sub_command_line_2)), Some(0));
        f.si.start_shell_execution(cmd_line(sub_command_line_3), None);
        f.si.emit_data(&format!("{}line1", vsc("C")));
        f.si.emit_data("line2");
        f.si.emit_data("line3");
        f.si.end_shell_execution(Some(cmd_line(sub_command_line_3)), Some(0));
        f.si.emit_data(&format!("{}foo", vsc("C")));
        f.si.start_shell_execution(cmd_line(sub_command_line_4), None);
        let ended_execution = f.end_execution_await_object(sub_command_line_4).await.unwrap();
        assert!(Rc::ptr_eq(&started_execution, &ended_execution));

        f.assert_tracked_events(vec![
            tracked(EventType::Start, command_line, None),
            tracked(EventType::Data, command_line, Some(&format!("{}foo", vsc("C")))),
            tracked(EventType::Data, command_line, Some(&format!("{} 2 files changed, 61 insertions(+), 2 deletions(-)", vsc("C")))),
            tracked(EventType::Data, command_line, Some(&format!("{}line1", vsc("C")))),
            tracked(EventType::Data, command_line, Some("line2")),
            tracked(EventType::Data, command_line, Some("line3")),
            tracked(EventType::Data, command_line, Some(&format!("{}foo", vsc("C")))),
            tracked(EventType::End, command_line, None),
        ]);
    });
}
