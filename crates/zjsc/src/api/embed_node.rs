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
use std::collections::HashMap;
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
use crate::runtime::identifier::Identifier;
use crate::runtime::js_array::{construct_array, is_js_array};
use crate::runtime::js_function::JSFunction;
use crate::runtime::js_value::{js_boolean, js_number, JSValue};
use crate::runtime::object_constructor::{construct_empty_object, enumerable_own_entries};
use crate::runtime::property_name::PropertyName;
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

/// O que atende uma função nativa criada por [`NodeRunner::make_function`]: recebe os argumentos já lidos e devolve o
/// resultado, ou a mensagem do `TypeError` que o JavaScript vê.
pub type NativeCallback = Rc<dyn Fn(&[JsData]) -> Result<JsData, String>>;

thread_local! {
    /// As funções nativas vivas, pelo id da célula da função.
    static NATIVES: RefCell<HashMap<usize, NativeCallback>> = RefCell::new(HashMap::new());
}

fn native_trampoline_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let Some(callback) = NATIVES.with(|natives| natives.borrow().get(&call.callee()).cloned()) else {
        return Err(Thrown::type_error("função nativa sem implementação registrada"));
    };
    let Some(global) = global_object.to_ref() else { return Ok(JSValue::undefined()) };
    let arguments: Vec<JsData> = call.arguments().iter().map(|argument| read_value(&global, *argument, 0)).collect();
    match callback(&arguments) {
        Ok(data) => Ok(write_value(&global, &data)),
        Err(message) => Err(Thrown::TypeError(message)),
    }
}
host_function!(native_trampoline, native_trampoline_body);

/// Os valores que cruzam a fronteira: o que o JavaScript e o Rust trocam. Objetos e arrays viajam por cópia (só as
/// propriedades próprias enumeráveis, com profundidade limitada); função, promessa e qualquer outra célula viajam como
/// [`JsHandle`], que o coletor não solta.
#[derive(Clone)]
pub enum JsData {
    Undefined,
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Array(Vec<JsData>),
    Object(Vec<(String, JsData)>),
    Handle(Rc<JsHandle>),
}

/// A profundidade máxima de uma cópia (cíclico ou absurdamente fundo vira `undefined` além dela).
const MAX_COPY_DEPTH: usize = 32;

/// Só o objeto comum e o array viajam por cópia; promessa, `Map`, `Date`, erro, tipado, função e o resto ficam como
/// [`JsHandle`] para não perder a identidade nem o comportamento.
fn copies_by_value(value: &JSValue) -> bool {
    use crate::runtime::js_type::JSType;
    match value {
        JSValue::Cell(id) => matches!(crate::runtime::cell_registry::cell_type(*id), Some(JSType::FinalObjectType | JSType::ArrayType)),
        _ => false,
    }
}

fn read_value(global: &JSGlobalObjectRef, value: JSValue, depth: usize) -> JsData {
    if value.is_undefined() {
        JsData::Undefined
    } else if value.is_null() {
        JsData::Null
    } else if value.is_boolean() {
        JsData::Bool(value.as_boolean())
    } else if value.is_number() {
        JsData::Num(value.as_number())
    } else if value.is_string() {
        JsData::Str(rust_string(&value.to_wtf_string()))
    } else if copies_by_value(&value) {
        if depth >= MAX_COPY_DEPTH {
            return JsData::Undefined;
        }
        let Ok(entries) = enumerable_own_entries(global, value) else { return JsData::Undefined };
        if is_js_array(&value) {
            let mut indexed: Vec<(u32, JsData)> = entries
                .iter()
                .filter_map(|(name, item)| String::from_utf8_lossy(&name.utf8()).parse::<u32>().ok().map(|index| (index, read_value(global, *item, depth + 1))))
                .collect();
            indexed.sort_by_key(|(index, _)| *index);
            JsData::Array(indexed.into_iter().map(|(_, item)| item).collect())
        } else {
            JsData::Object(entries.iter().map(|(name, item)| (String::from_utf8_lossy(&name.utf8()).into_owned(), read_value(global, *item, depth + 1))).collect())
        }
    } else {
        JsData::Handle(Rc::new(JsHandle::new(value)))
    }
}

fn write_value(global: &JSGlobalObjectRef, data: &JsData) -> JSValue {
    let vm = global.vm();
    match data {
        JsData::Undefined => JSValue::undefined(),
        JsData::Null => JSValue::null(),
        JsData::Bool(flag) => js_boolean(*flag),
        JsData::Num(number) => js_number(*number),
        JsData::Str(text) => JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes()))),
        JsData::Handle(handle) => handle.0,
        JsData::Array(items) => {
            let values: Vec<JSValue> = items.iter().map(|item| write_value(global, item)).collect();
            let _rooted = RootScope::new(&values);
            construct_array(vm, &global.array_structure(), &values).as_value()
        }
        JsData::Object(fields) => {
            let object = construct_empty_object(global);
            let values: Vec<JSValue> = fields.iter().map(|(_, item)| write_value(global, item)).collect();
            let _rooted = RootScope::new(&values);
            for ((name, _), value) in fields.iter().zip(values) {
                let identifier = Identifier::from_span(vm, name.as_bytes());
                let property = PropertyName::from_identifier(&identifier);
                match property.parse_index() {
                    Some(index) => {
                        let _ = object.put_by_index(vm, index, value, false);
                    }
                    None => {
                        object.put_direct(vm, &property, value, 0);
                    }
                }
            }
            object.as_value()
        }
    }
}

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

    /// Chama `function` com `args` já como [`JsData`] (cada um vira valor JavaScript) e devolve o resultado copiado.
    pub fn call_data(&self, function: &JsHandle, args: &[JsData]) -> Result<JsData, String> {
        let values: Vec<JSValue> = args.iter().map(|arg| write_value(&self.global_object, arg)).collect();
        let _arguments = RootScope::new(&values);
        let result = call_function(&self.global_object, function.0, JSValue::undefined(), &values).ok_or_else(|| {
            let message = self.vm.exception().map(|exception| describe_exception(&exception.value())).unwrap_or_default();
            self.vm.clear_exception();
            message
        });
        self.vm.drain_microtasks();
        result.map(|value| read_value(&self.global_object, value, 0))
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

    /// Cria uma função JavaScript (`name`, `length`) cujo corpo é `callback`.
    pub fn make_function(&self, name: &str, length: u32, callback: NativeCallback) -> JsHandle {
        let function = JSFunction::create_native(
            &self.vm,
            &self.global_object,
            length,
            &WtfString::from_utf8(name.as_bytes()),
            native_trampoline,
            ImplementationVisibility::Public,
            crate::runtime::intrinsic::Intrinsic::NoIntrinsic,
            crate::runtime::js_function::call_host_function_as_constructor,
        );
        let value = function.as_value();
        if let JSValue::Cell(id) = value {
            NATIVES.with(|natives| natives.borrow_mut().insert(id, callback));
        }
        JsHandle::new(value)
    }

    /// Converte `data` em valor JavaScript e o guarda.
    pub fn import(&self, data: &JsData) -> JsHandle {
        JsHandle::new(write_value(&self.global_object, data))
    }

    /// Copia o valor de `handle` para o lado do Rust.
    pub fn export(&self, handle: &JsHandle) -> JsData {
        read_value(&self.global_object, handle.0, 0)
    }

    /// `globalThis[name] = data`.
    pub fn set_global(&self, name: &str, data: &JsData) {
        let value = write_value(&self.global_object, data);
        let _rooted = RootScope::new(&[value]);
        crate::runtime::native_class_support::install_global(&self.global_object, name, value);
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
