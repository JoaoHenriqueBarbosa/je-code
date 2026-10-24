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

// ---- valores nativos: o `vscode` é construído em Rust, sem prelúdio em JavaScript

use zjsc::api::embed_node::JsData;

fn number(data: &JsData) -> f64 {
    match data {
        JsData::Num(value) => *value,
        _ => panic!("esperava número"),
    }
}

#[test]
fn native_function_receives_and_returns_numbers() {
    with_runner("", |runner, _| {
        let add = runner.make_function("add", 2, Rc::new(|args| Ok(JsData::Num(number(&args[0]) + number(&args[1])))));
        runner.set_global("add", &JsData::Handle(Rc::new(add)));
        assert_eq!(text(runner, "String(add(40, 2)) + ':' + add.name + ':' + add.length"), "42:add:2");
    });
}

#[test]
fn native_function_receives_structured_arguments() {
    with_runner("", |runner, _| {
        let seen: Rc<RefCell<Option<JsData>>> = Rc::default();
        let sink = seen.clone();
        let record = runner.make_function(
            "record",
            1,
            Rc::new(move |args| {
                *sink.borrow_mut() = Some(args[0].clone());
                Ok(JsData::Undefined)
            }),
        );
        runner.set_global("record", &JsData::Handle(Rc::new(record)));
        runner.eval("record({ a: 1, list: [1, 'x', null, true, undefined], nested: { s: 'ç' } })", "t.js").unwrap();
        let JsData::Object(fields) = seen.borrow().clone().unwrap() else { panic!("esperava objeto") };
        let by_name: std::collections::HashMap<_, _> = fields.into_iter().collect();
        assert_eq!(number(&by_name["a"]), 1.0);
        let JsData::Array(list) = &by_name["list"] else { panic!("esperava array") };
        assert!(matches!(list.as_slice(), [JsData::Num(_), JsData::Str(x), JsData::Null, JsData::Bool(true), JsData::Undefined] if x == "x"));
        let JsData::Object(nested) = &by_name["nested"] else { panic!("esperava objeto") };
        assert!(matches!(nested.as_slice(), [(key, JsData::Str(value))] if key == "s" && value == "ç"));
    });
}

#[test]
fn native_function_returns_structured_values_to_javascript() {
    with_runner("", |runner, _| {
        let make = runner.make_function(
            "make",
            0,
            Rc::new(|_| Ok(JsData::Object(vec![("k".into(), JsData::Num(1.0)), ("list".into(), JsData::Array(vec![JsData::Num(1.0), JsData::Str("dois".into()), JsData::Null]))]))),
        );
        runner.set_global("make", &JsData::Handle(Rc::new(make)));
        assert_eq!(text(runner, "JSON.stringify(make())"), r#"{"k":1,"list":[1,"dois",null]}"#);
        assert_eq!(text(runner, "String(Array.isArray(make().list))"), "true");
    });
}

#[test]
fn native_function_errors_become_type_errors() {
    with_runner("", |runner, _| {
        let fail = runner.make_function("fail", 0, Rc::new(|_| Err("não pode".to_owned())));
        runner.set_global("fail", &JsData::Handle(Rc::new(fail)));
        assert_eq!(text(runner, "try { fail(); 'sem erro' } catch (e) { (e instanceof TypeError) + ':' + e.message }"), "true:não pode");
    });
}

#[test]
fn functions_and_promises_cross_as_handles_and_keep_working() {
    with_runner("", |runner, _| {
        let held: Rc<RefCell<Vec<JsData>>> = Rc::default();
        let sink = held.clone();
        let keep = runner.make_function(
            "keep",
            1,
            Rc::new(move |args| {
                sink.borrow_mut().push(args[0].clone());
                Ok(JsData::Undefined)
            }),
        );
        runner.set_global("keep", &JsData::Handle(Rc::new(keep)));
        runner.eval("keep((x) => 'oi ' + x); keep(Promise.resolve(7)); keep(new Date(0));", "t.js").unwrap();
        let held = held.borrow();
        let JsData::Handle(callback) = &held[0] else { panic!("a função devia chegar como handle") };
        let result = runner.call_data(callback, &[JsData::Str("mundo".into())]).unwrap();
        assert!(matches!(result, JsData::Str(ref value) if value == "oi mundo"));
        assert!(matches!(held[1], JsData::Handle(_)), "promessa fica handle, não vira objeto vazio");
        assert!(matches!(held[2], JsData::Handle(_)), "Date fica handle");
    });
}

#[test]
fn native_functions_installed_on_objects_can_be_called_as_methods() {
    with_runner("", |runner, _| {
        let hello = runner.make_function("hello", 1, Rc::new(|args| Ok(JsData::Str(format!("olá, {}", match &args[0] { JsData::Str(name) => name.as_str(), _ => "?" })))));
        let api = JsData::Object(vec![("greet".into(), JsData::Handle(Rc::new(hello)))]);
        runner.set_global("api", &api);
        assert_eq!(text(runner, "api.greet('você')"), "olá, você");
        assert_eq!(text(runner, "typeof api.greet + ':' + Object.keys(api).join()"), "function:greet");
    });
}

// ---- classes nativas: `Position`, `Range`, `Uri`... com `instanceof`, métodos e estáticos

use zjsc::api::embed_node::{ClassSpec, CLASS_KEY};

fn point(x: f64, y: f64) -> JsData {
    JsData::Object(vec![(CLASS_KEY.into(), JsData::Str("Point".into())), ("x".into(), JsData::Num(x)), ("y".into(), JsData::Num(y))])
}

fn field(data: &JsData, name: &str) -> f64 {
    match data {
        JsData::Object(fields) => fields.iter().find(|(key, _)| key == name).map(|(_, value)| number(value)).unwrap_or_else(|| panic!("sem campo {name}")),
        _ => panic!("esperava objeto"),
    }
}

fn define_point(runner: &NodeRunner) {
    let constructor = runner.define_class(ClassSpec {
        name: "Point".into(),
        length: 2,
        construct: Rc::new(|args| Ok(JsData::Object(vec![("x".into(), args[0].clone()), ("y".into(), args[1].clone())]))),
        methods: vec![(
            "translate".into(),
            2,
            Rc::new(|this, args| Ok(point(field(this, "x") + number(&args[0]), field(this, "y") + number(&args[1])))),
        )],
        statics: vec![("origin".into(), 0, Rc::new(|_| Ok(point(0.0, 0.0))))],
    });
    runner.set_global("Point", &JsData::Handle(Rc::new(constructor)));
}

#[test]
fn native_class_instances_have_fields_methods_and_instanceof() {
    with_runner("", |runner, _| {
        define_point(runner);
        assert_eq!(text(runner, "const p = new Point(1, 2); [p.x, p.y, p instanceof Point, p.constructor === Point, typeof Point].join()"), "1,2,true,true,function");
        assert_eq!(text(runner, "const q = new Point(1, 2).translate(10, 20); [q.x, q.y, q instanceof Point].join()"), "11,22,true");
        assert_eq!(text(runner, "String(Point.name) + ':' + Point.length"), "Point:2");
    });
}

#[test]
fn native_class_statics_and_marker_stay_hidden() {
    with_runner("", |runner, _| {
        define_point(runner);
        assert_eq!(text(runner, "const o = Point.origin(); [o instanceof Point, o.x, o.y].join()"), "true,0,0");
        assert_eq!(text(runner, "JSON.stringify(new Point(3, 4))"), r#"{"x":3,"y":4}"#, "o marcador de classe não aparece");
        assert_eq!(text(runner, "Object.keys(new Point(3, 4)).join()"), "x,y");
        assert_eq!(text(runner, "'$$class' in new Point(1, 1) ? 'tem' : 'não tem'"), "tem", "existe, só não é enumerável");
    });
}

#[test]
fn native_class_requires_new_and_can_be_extended_from_javascript() {
    with_runner("", |runner, _| {
        define_point(runner);
        assert_eq!(text(runner, "try { Point(1, 2); 'sem erro' } catch (e) { e instanceof TypeError ? e.message : 'outro' }"), "Class constructor Point cannot be invoked without 'new'");
        assert_eq!(
            text(runner, "class Point3 extends Point { constructor(x, y, z) { super(x, y); this.z = z; } } const r = new Point3(1, 2, 3); [r instanceof Point3, r instanceof Point, r.x, r.z, r.translate(1, 1).x].join()"),
            "true,true,1,3,2"
        );
    });
}

#[test]
fn class_marker_carries_instances_across_the_boundary_in_both_directions() {
    with_runner("", |runner, _| {
        define_point(runner);
        let seen: Rc<RefCell<Option<JsData>>> = Rc::default();
        let sink = seen.clone();
        let give = runner.make_function("give", 0, Rc::new(|_| Ok(point(7.0, 8.0))));
        let take = runner.make_function(
            "take",
            1,
            Rc::new(move |args| {
                *sink.borrow_mut() = Some(args[0].clone());
                Ok(JsData::Undefined)
            }),
        );
        runner.set_global("give", &JsData::Handle(Rc::new(give)));
        runner.set_global("take", &JsData::Handle(Rc::new(take)));
        assert_eq!(text(runner, "const g = give(); [g instanceof Point, g.x, g.y].join()"), "true,7,8", "o Rust devolve um objeto marcado e o JavaScript o recebe como instância");
        runner.eval("take(new Point(5, 6).translate(1, 1))", "t.js").unwrap();
        let received = seen.borrow().clone().unwrap();
        let JsData::Object(fields) = &received else { panic!("esperava objeto") };
        assert!(matches!(fields.first(), Some((key, JsData::Str(class))) if key == CLASS_KEY && class == "Point"), "a instância chega ao Rust com o marcador");
        assert_eq!((field(&received, "x"), field(&received, "y")), (6.0, 7.0));
    });
}

// ---- módulos do hospedeiro: `require('vscode')`

#[test]
fn host_module_is_reachable_from_commonjs_modules_loaded_from_disk() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("extension.js"), "const vscode = require('vscode'); exports.activate = () => vscode.greet('mundo') + ':' + (require('vscode') === vscode) + ':' + require.resolve('vscode');").unwrap();
    let base = dir.path().to_string_lossy().replace('\\', "/");
    let main: &'static str = Box::leak(format!("globalThis.loadExtension = () => require('{base}/extension.js');").into_boxed_str());
    with_runner(main, |runner, _| {
        let greet = runner.make_function("greet", 1, Rc::new(|args| Ok(JsData::Str(format!("olá, {}", match &args[0] { JsData::Str(name) => name.as_str(), _ => "?" })))));
        runner.register_module("vscode", &JsData::Object(vec![("greet".into(), JsData::Handle(Rc::new(greet)))]));
        assert_eq!(text(runner, "loadExtension().activate()"), "olá, mundo:true:vscode");
    });
}

// ---- promessas: `call_method`, `new_promise`, `resolve_promise` e `reject_promise`

#[test]
fn javascript_promises_settle_native_callbacks_through_call_method() {
    with_runner("", |runner, _| {
        let outcomes: Rc<RefCell<Vec<String>>> = Rc::default();
        let (ok, bad) = (outcomes.clone(), outcomes.clone());
        let on_fulfilled = runner.make_function("ok", 1, Rc::new(move |args| {
            ok.borrow_mut().push(format!("ok:{}", match &args[0] { JsData::Num(n) => n.to_string(), JsData::Str(s) => s.clone(), _ => "?".into() }));
            Ok(JsData::Undefined)
        }));
        let on_rejected = runner.make_function("bad", 1, Rc::new(move |args| {
            bad.borrow_mut().push(format!("bad:{}", match &args[0] { JsData::Object(f) => f.iter().find(|(k, _)| k == "message").map(|(_, v)| match v { JsData::Str(s) => s.clone(), _ => "?".into() }).unwrap_or_default(), _ => "?".into() }));
            Ok(JsData::Undefined)
        }));
        let resolved = runner.eval("Promise.resolve(7)", "t.js").unwrap();
        let rejected = runner.eval("Promise.reject(new Error('quebrou'))", "t.js").unwrap();
        let (f, r) = (Rc::new(on_fulfilled), Rc::new(on_rejected));
        runner.call_method(&resolved, "then", &[JsData::Handle(f.clone()), JsData::Handle(r.clone())]).unwrap();
        runner.call_method(&rejected, "then", &[JsData::Handle(f), JsData::Handle(r)]).unwrap();
        assert_eq!(*outcomes.borrow(), ["ok:7", "bad:quebrou"]);
    });
}

#[test]
fn rust_created_promises_are_awaited_by_javascript() {
    with_runner("globalThis.log = [];", |runner, _| {
        let promise = Rc::new(runner.new_promise());
        runner.set_global("pending", &JsData::Handle(promise.clone()));
        runner.eval("pending.then((v) => log.push('then:' + v), (e) => log.push('catch:' + e)); (async () => { log.push('await:' + await pending); })();", "t.js").unwrap();
        assert_eq!(text(runner, "log.join()"), "", "pendente: nada ainda");
        assert!(runner.resolve_promise(&promise, &JsData::Str("pronto".into())));
        assert!(runner.resolve_promise(&promise, &JsData::Str("de novo".into())), "a segunda chamada é aceita e ignorada");
        assert_eq!(text(runner, "log.join()"), "then:pronto,await:pronto");
        let failing = Rc::new(runner.new_promise());
        runner.set_global("failing", &JsData::Handle(failing.clone()));
        runner.eval("failing.catch((e) => log.push('erro:' + e))", "t.js").unwrap();
        runner.reject_promise(&failing, &JsData::Str("falhou".into()));
        assert_eq!(text(runner, "log.join()"), "then:pronto,await:pronto,erro:falhou");
    });
}
