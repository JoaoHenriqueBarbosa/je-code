//! Runner de programa no estilo Node que DEVOLVE o controle ao hospedeiro: o oposto de `evaluate_main_script`, que roda o
//! programa e o laço de eventos até o fim. É o que um host de extensões precisa: o hospedeiro avalia o programa principal
//! (um bootstrap CommonJS com `require`, `process` e os módulos `node:*`), guarda funções JavaScript, chama-as quando chega
//! um evento de fora e avança o laço de eventos em fatias que ele mesmo mede (o relógio do laço é virtual: alimentá-lo
//! com o tempo real que passou dá a semântica de tempo real).
//!
//! MUDANÇA LOCAL do je-code (README da crate): este arquivo não existe no pseudo-linus.
//!
//! A fronteira com o hospedeiro é só texto: uma função global `__je_call(nome, json)` que o programa chama e que o
//! hospedeiro atende com [`NodeRunner::set_bridge`] (devolve o JSON da resposta ou a mensagem de um `TypeError`), mais
//! [`NodeRunner::call`] para o caminho inverso. O JSON dispensa o hospedeiro de conhecer as células do motor.

use std::cell::RefCell;
use std::rc::Rc;

use crate::api::eval::{describe_exception, evaluate, install_cjs_fs, named_program_source, new_global_object, run_main_program, VmFinalizer};
use crate::api::module_probe::ModuleFs;
use crate::host_function;
use crate::runtime::cell_registry::run_program;
use crate::runtime::console_host::ConsoleHost;
use crate::runtime::gc::{Root, RootScope};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::native_class_support::install_global_function_with_visibility;
use crate::runtime::object_to_primitive::call_function;
use crate::runtime::vm::VM;
use crate::wtf::text::wtf_string::String as WtfString;

/// O que atende `__je_call(nome, json)`: devolve o JSON da resposta, ou a mensagem do `TypeError` que o programa vê.
pub type Bridge = Rc<dyn Fn(&str, &str) -> Result<String, String>>;

thread_local! {
    static BRIDGE: RefCell<Option<Bridge>> = const { RefCell::new(None) };
}

fn bridge_call_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let name = rust_string(&call.argument(0).to_wtf_string());
    let payload = rust_string(&call.argument(1).to_wtf_string());
    let Some(bridge) = BRIDGE.with(|slot| slot.borrow().clone()) else {
        return Err(Thrown::type_error("__je_call: o hospedeiro não instalou a ponte"));
    };
    match bridge(&name, &payload) {
        Ok(text) => Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(text.as_bytes())))),
        Err(message) => Err(Thrown::TypeError(message)),
    }
}
host_function!(bridge_call, bridge_call_body);

/// Um valor JavaScript que o hospedeiro guarda (o `Strong` da API C): o coletor não o solta enquanto o handle vive.
pub struct JsHandle(JSValue, #[allow(dead_code)] Root);

impl JsHandle {
    fn new(value: JSValue) -> JsHandle {
        JsHandle(value, Root::new(value))
    }

    /// O texto, quando o valor é uma string; `None` para outro tipo.
    pub fn as_text(&self) -> Option<String> {
        self.0.is_string().then(|| rust_string(&self.0.to_wtf_string()))
    }

    pub fn is_undefined(&self) -> bool {
        self.0.is_undefined()
    }
}

/// Um argumento de [`NodeRunner::call`].
pub enum NodeArg<'a> {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(&'a str),
    Handle(&'a JsHandle),
}

/// O programa em curso.
pub struct NodeRunner {
    vm: Rc<VM>,
    global_object: JSGlobalObjectRef,
}

impl NodeRunner {
    /// Avalia `source` como script clássico de nome `url` e esvazia as microtarefas. Devolve o valor de completude.
    /// `Err` é a exceção não capturada (`Nome: mensagem`).
    pub fn eval(&self, source: &str, url: &str) -> Result<JsHandle, String> {
        let result = evaluate(&self.global_object, &named_program_source(source, url)).map(JsHandle::new);
        self.vm.drain_microtasks();
        result.map_err(|exception| describe_exception(&exception))
    }

    /// Chama a função de `function` com `args` (sem `this`), esvazia as microtarefas e devolve o resultado.
    pub fn call(&self, function: &JsHandle, args: &[NodeArg]) -> Result<JsHandle, String> {
        let values: Vec<JSValue> = args
            .iter()
            .map(|arg| match arg {
                NodeArg::Undefined => JSValue::undefined(),
                NodeArg::Null => JSValue::null(),
                NodeArg::Bool(flag) => js_boolean(*flag),
                NodeArg::Num(number) => js_number(*number),
                NodeArg::Str(text) => JSValue::from_js_string(js_string(&self.vm, &WtfString::from_utf8(text.as_bytes()))),
                NodeArg::Handle(handle) => handle.0,
            })
            .collect();
        // Os argumentos já criados vivem só aqui até a chamada os pôr na pilha do JavaScript.
        let _arguments = RootScope::new(&values);
        let result = call_function(&self.global_object, function.0, JSValue::undefined(), &values).map(JsHandle::new).ok_or_else(|| {
            let message = self.vm.exception().map(|exception| describe_exception(&exception.value())).unwrap_or_default();
            self.vm.clear_exception();
            message
        });
        self.vm.drain_microtasks();
        result
    }

    /// Instala a ponte `__je_call(nome, json)` e o `handler` que a atende.
    pub fn set_bridge(&self, handler: Bridge) {
        BRIDGE.with(|slot| *slot.borrow_mut() = Some(handler));
        install_global_function_with_visibility(&self.global_object, "__je_call", 2, bridge_call, ImplementationVisibility::Public);
    }

    /// Roda o laço de eventos (timers, immediates, microtarefas, rede) por `budget_ms` de tempo do laço, que o
    /// hospedeiro alimenta com o tempo real decorrido. `false` quando o laço parou de vez (exceção fatal).
    pub fn run_event_loop_for(&self, budget_ms: f64) -> bool {
        crate::runtime::timers::run_event_loop_for(&self.global_object, budget_ms)
    }

    /// Só as microtarefas pendentes.
    pub fn drain_microtasks(&self) {
        self.vm.drain_microtasks();
    }
}

/// Cria o programa Node: um global novo com `console` e o sistema de arquivos `fs` dos módulos, roda `main_source`
/// (arquivo `main_url`) como programa principal (ES ou CommonJS, como o bun escolhe) e entrega o [`NodeRunner`] a `body`.
/// O programa principal que lança é `Err`; o que um timer ou uma promessa sem tratador lançam vai ao stderr do `console`
/// e NÃO derruba o programa (um host de extensões segue vivo).
pub fn run_node<T>(
    console: Rc<dyn ConsoleHost>,
    fs: Rc<dyn ModuleFs>,
    main_url: &str,
    main_source: &str,
    body: impl FnOnce(&NodeRunner) -> T,
) -> Result<T, String> {
    run_program(|| {
        crate::runtime::process_system::begin_main_script(main_url);
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        global_object.set_console_host(console);
        install_cjs_fs(Some(fs.clone()));
        global_object.set_module_fs(Some(fs));
        let shared_source: Rc<str> = Rc::from(main_source);
        crate::runtime::uncaught_report::remember_main_source(shared_source.clone());
        let report_source = shared_source.clone();
        let report = Rc::new(move |global: &JSGlobalObject, error: JSValue, with_trace: bool| {
            let mut text = Vec::new();
            crate::runtime::uncaught_report::append_exception_report(&mut text, global, &report_source, error, with_trace);
            if let Some(console) = global.console_host() {
                console.write_stderr(&[text, b"\n".to_vec()].concat());
            }
        });
        let on_error = report.clone();
        vm.set_unhandled_error_reporter(Some(Rc::new(move |global: &JSGlobalObject, error: JSValue| on_error(global, error, true))));
        let on_rejection = report.clone();
        vm.set_unhandled_rejection_reporter(Some(Rc::new(move |global: &JSGlobalObject, _promise: JSValue, reason: JSValue| {
            on_rejection(global, reason, false)
        })));
        let on_exception = report;
        vm.set_uncaught_exception_reporter(Some(Rc::new(move |global: &JSGlobalObject, exception: &Rc<crate::runtime::exception::Exception>| {
            on_exception(global, exception.value(), true)
        })));
        crate::runtime::timers::enable_event_loop();
        if let Err(exception) = run_main_program(&global_object, main_source, main_url) {
            let message = describe_exception(&exception);
            vm.clear_exception();
            return Err(message);
        }
        vm.drain_microtasks();
        let runner = NodeRunner { vm: vm.clone(), global_object };
        Ok(body(&runner))
    })
}
