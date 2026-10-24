//! O `NodeRunner` do zjsc pelo je-exthost: ponte `__je_call`, chamada de função JavaScript pelo Rust e laço de eventos em
//! tempo real.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use je_exthost::runtime::{with_runtime_thread, DiskModuleFs};
use zjsc::api::embed_node::{run_node, NodeArg, NodeRunner};
use zjsc::runtime::console_host::ConsoleHost;

#[derive(Debug, Default)]
struct Captured {
    stdout: RefCell<Vec<u8>>,
    stderr: RefCell<Vec<u8>>,
}

impl ConsoleHost for Captured {
    fn write_stdout(&self, bytes: &[u8]) {
        self.stdout.borrow_mut().extend_from_slice(bytes);
    }
    fn write_stderr(&self, bytes: &[u8]) {
        self.stderr.borrow_mut().extend_from_slice(bytes);
    }
    fn read_stdin_line(&self) -> Option<String> {
        None
    }
    fn read_stdin_rest(&self) -> Vec<u8> {
        Vec::new()
    }
}

fn text(runner: &NodeRunner, source: &str) -> String {
    runner.eval(source, "test.js").unwrap().as_text().expect("o programa devolve string")
}

fn with_runner<T: Send + 'static>(main_source: &'static str, body: impl FnOnce(&NodeRunner, &Captured) -> T + Send + 'static) -> T {
    with_runtime_thread(move || {
        let console = Rc::new(Captured::default());
        let shared = console.clone();
        run_node(console, Rc::new(DiskModuleFs), "/je-test/main.js", main_source, move |runner| body(runner, &shared)).unwrap()
    })
}

#[test]
fn evaluates_scripts_and_reads_back_text() {
    with_runner("globalThis.fromMain = 40;", |runner, _| {
        assert_eq!(text(runner, "String(fromMain + 2)"), "42");
        assert!(runner.eval("throw new Error('boom')", "t.js").err().unwrap().contains("boom"));
    });
}

#[test]
fn main_program_failure_is_reported_as_error() {
    let outcome = with_runtime_thread(|| run_node(Rc::new(Captured::default()), Rc::new(DiskModuleFs), "/je-test/bad.js", "throw new Error('bootstrap falhou');", |_| ()));
    assert!(outcome.unwrap_err().contains("bootstrap falhou"));
}

#[test]
fn bridge_carries_json_both_ways() {
    with_runner("", |runner, _| {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let log = seen.clone();
        runner.set_bridge(Rc::new(move |name, json| {
            log.borrow_mut().push((name.to_owned(), json.to_owned()));
            match name {
                "add" => Ok("3".to_owned()),
                _ => Err(format!("comando desconhecido: {name}")),
            }
        }));
        assert_eq!(text(runner, "__je_call('add', JSON.stringify([1, 2]))"), "3");
        assert_eq!(text(runner, "try { __je_call('nope', '{}') } catch (e) { e instanceof TypeError ? e.message : 'outro' }"), "comando desconhecido: nope");
        assert_eq!(*seen.borrow(), [("add".to_owned(), "[1,2]".to_owned()), ("nope".to_owned(), "{}".to_owned())]);
    });
}

#[test]
fn rust_calls_javascript_functions_with_arguments() {
    with_runner("", |runner, _| {
        let join = runner.eval("(function (a, b, c, d) { return [typeof a, a, b, c, d].join('|'); })", "t.js").unwrap();
        let held = runner.eval("'guardado'", "t.js").unwrap();
        let result = runner.call(&join, &[NodeArg::Num(1.5), NodeArg::Str("x"), NodeArg::Bool(true), NodeArg::Handle(&held)]).unwrap();
        assert_eq!(result.as_text().unwrap(), "number|1.5|x|true|guardado");
        let failing = runner.eval("(function () { throw new RangeError('falhou'); })", "t.js").unwrap();
        assert!(runner.call(&failing, &[]).err().unwrap().contains("falhou"));
    });
}

#[test]
fn event_loop_follows_the_clock_the_host_feeds_it() {
    with_runner("globalThis.fired = 0;", |runner, _| {
        runner.eval("setTimeout(() => { fired += 1; }, 50); setTimeout(() => { fired += 10; }, 500);", "t.js").unwrap();
        runner.run_event_loop_for(10.0);
        assert_eq!(text(runner, "String(fired)"), "0", "10 ms não vence o timer de 50");
        runner.run_event_loop_for(60.0);
        assert_eq!(text(runner, "String(fired)"), "1", "70 ms vence o de 50 e não o de 500");
        runner.run_event_loop_for(500.0);
        assert_eq!(text(runner, "String(fired)"), "11");
    });
}

#[test]
fn promises_resolve_without_advancing_the_clock() {
    with_runner("", |runner, _| {
        runner.eval("globalThis.log = []; Promise.resolve(1).then((v) => log.push('then' + v)); queueMicrotask(() => log.push('micro')); log.push('sync');", "t.js").unwrap();
        assert_eq!(text(runner, "log.join(',')"), "sync,then1,micro");
    });
}

#[test]
fn node_modules_work_inside_the_program() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("dep.js"), "module.exports = { answer: 42 };").unwrap();
    std::fs::write(dir.path().join("note.txt"), "do disco").unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    let main = format!("globalThis.dep = require('{base}/dep.js'); globalThis.note = require('fs').readFileSync('{base}/note.txt', 'utf8'); globalThis.sep = require('path').sep;");
    let main: &'static str = Box::leak(main.into_boxed_str());
    with_runner(main, |runner, _| {
        assert_eq!(text(runner, "String(dep.answer) + '/' + note + '/' + sep"), "42/do disco//");
    });
}

#[test]
fn uncaught_errors_in_timers_are_reported_without_stopping_the_program() {
    with_runner("globalThis.alive = 0;", |runner, console| {
        runner.eval("setTimeout(() => { throw new Error('de um timer'); }, 5); setTimeout(() => { alive = 1; }, 20);", "t.js").unwrap();
        runner.run_event_loop_for(50.0);
        assert_eq!(text(runner, "String(alive)"), "1", "o programa segue depois do erro do timer");
        let stderr = String::from_utf8(console.stderr.borrow().clone()).unwrap();
        assert!(stderr.contains("de um timer"), "o erro vai ao stderr do console: {stderr:?}");
    });
}

#[test]
fn real_time_slicing_matches_wall_clock() {
    with_runner("globalThis.fired = 0;", |runner, _| {
        runner.eval("setTimeout(() => { fired = 1; }, 30);", "t.js").unwrap();
        let started = Instant::now();
        let mut last = started;
        while text(runner, "String(fired)") == "0" && started.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(2));
            let now = Instant::now();
            runner.run_event_loop_for(now.duration_since(last).as_secs_f64() * 1000.0);
            last = now;
        }
        let waited = started.elapsed();
        assert_eq!(text(runner, "String(fired)"), "1");
        assert!(waited >= Duration::from_millis(28) && waited < Duration::from_millis(400), "disparou em tempo real: {waited:?}");
    });
}
