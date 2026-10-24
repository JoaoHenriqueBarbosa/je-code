//! Ponto de entrada do hospedeiro de páginas (o navegador headless do sandbox): um programa JavaScript de vida longa
//! sobre um global novo, com o console e o DOM do hospedeiro instalados, onde o hospedeiro avalia scripts em ordem,
//! dispara os eventos do documento e roda o laço de eventos.

use std::cell::RefCell;
use std::rc::Rc;

use crate::api::browser_modules::{evaluate_page_module, install_url_module_loader, ModuleFetch, UrlModuleHost};
use crate::api::eval::{describe_exception, evaluate, named_program_source, new_global_object, new_global_object_in, VmFinalizer};
use crate::host_function;
use crate::runtime::cell_registry::run_program;
use crate::runtime::console_host::ConsoleHost;
use crate::runtime::dom_bridge::{self, DomHost};
use crate::runtime::gc::{Root, RootScope};
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_promise::Status;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_number, JSValue};
use crate::runtime::object_to_primitive::call_function;
use crate::wtf::text::wtf_string::String as WtfString;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::native_class_support::install_global_function_with_visibility;
use crate::runtime::program_executable::ProgramExecutable;
use crate::runtime::vm::VM;

/// `__pl_eval(fonte, url)`: avalia `fonte` como script clássico no global que chamou e devolve o valor de completude
/// (o que o `Runtime.evaluate` do DevTools entrega). A exceção do script segue pendente para o chamador JavaScript,
/// que a relata como erro de página.
fn pl_eval_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let source = rust_string(&call.argument(0).to_wtf_string());
    let url = rust_string(&call.argument(1).to_wtf_string());
    let Some(global) = global_object.to_ref() else { return Ok(JSValue::undefined()) };
    let executable = ProgramExecutable::create(&global, &named_program_source(&source, &url));
    let vm = global_object.vm();
    let completion = vm.interpreter().execute_program(&executable, &global);
    if vm.exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(completion)
}
host_function!(pl_eval, pl_eval_body);

/// `__pl_set_code_guard(função)`: registra a função que o motor consulta antes de compilar texto (`eval` e
/// `Function`), ver `runtime::dynamic_code_guard`. Sem argumento função, remove o gancho.
fn pl_set_code_guard_body(_global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let function = call.argument(0);
    crate::runtime::dynamic_code_guard::set_code_guard(function.is_object().then_some(function));
    Ok(JSValue::undefined())
}
host_function!(pl_set_code_guard, pl_set_code_guard_body);

/// `__pl_microtask_checkpoint()`: o "perform a microtask checkpoint" do HTML, que o navegador roda quando a pilha de
/// JavaScript fica vazia (depois de cada ouvinte de um evento despachado por uma tarefa do navegador, por exemplo).
/// Como no HTML, não faz nada se um checkpoint já está em curso.
fn pl_microtask_checkpoint_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    if !vm.default_microtask_queue.is_performing_microtask_checkpoint() {
        vm.drain_microtasks();
    }
    Ok(JSValue::undefined())
}
host_function!(pl_microtask_checkpoint, pl_microtask_checkpoint_body);

/// Um valor do programa escondido que o hospedeiro guarda para chamar depois ([`PageRunner::call_hidden`]). É um
/// `Strong<Unknown>` (o `JSValueProtect` da API C): o coletor não solta o valor enquanto o hospedeiro o segura.
pub struct HostHandle(JSValue, #[allow(dead_code)] Root);

impl HostHandle {
    fn new(value: JSValue) -> HostHandle {
        HostHandle(value, Root::new(value))
    }

    /// O texto de um valor string (o que o gancho devolve ao hospedeiro, como a caixa de saída do DevTools); `None`
    /// para outro tipo.
    pub fn as_text(&self) -> Option<String> {
        self.0.is_string().then(|| rust_string(&self.0.to_wtf_string()))
    }
}

/// Argumento de [`PageRunner::call_hidden`].
#[derive(Clone, Copy)]
pub enum HostArg<'a> {
    Str(&'a str),
    Num(f64),
    Null,
    /// Um valor que o hospedeiro guardou (o elemento `<iframe>` que o gancho devolveu).
    Handle(&'a HostHandle),
    /// O `globalThis` de outro programa do mesmo `VM` (a janela de um frame, ou a do pai).
    Global(&'a PageRunner),
}

/// O programa em curso: avalia scripts no global da página.
pub struct PageRunner {
    vm: Rc<VM>,
    global_object: JSGlobalObjectRef,
    modules: Rc<UrlModuleHost>,
}

impl PageRunner {
    /// O programa de um frame: um `JSGlobalObject` novo no `VM` desta página (heap, microtarefas, timers e relógio
    /// compartilhados), com o DOM `dom` e o módulo de URL `document_url` próprios.
    pub fn new_frame(&self, console: Rc<dyn ConsoleHost>, dom: Rc<dyn DomHost>, document_url: &str, fetch: ModuleFetch) -> PageRunner {
        let global_object = new_global_object_in(&self.vm);
        let modules = configure_global(&global_object, console, dom, document_url, fetch);
        PageRunner { vm: self.vm.clone(), global_object, modules }
    }

    /// Avalia um `<script type=module>`: inline (`source`, com `url` `None`) ou externo (`url` absoluta). Esvazia as
    /// microtarefas; `Err` é a rejeição da avaliação já conhecida (`Nome: mensagem`). Um módulo que ainda espera
    /// (`await` de topo, `import` pela rede) segue no laço de eventos.
    pub fn eval_module(&self, source: Option<&str>, url: Option<&str>) -> Result<(), String> {
        let promise = evaluate_page_module(&self.global_object, &self.modules, source, url);
        self.vm.drain_microtasks();
        match promise.status() {
            Status::Rejected => Err(describe_exception(&promise.result())),
            _ => Ok(()),
        }
    }

    /// Avalia `source` como script clássico de nome `url` e esvazia as microtarefas. `Err` é a exceção não capturada
    /// (`Nome: mensagem`).
    pub fn eval(&self, source: &str, url: &str) -> Result<(), String> {
        let result = evaluate(&self.global_object, &named_program_source(source, url));
        self.vm.drain_microtasks();
        result.map(|_| ()).map_err(|exception| describe_exception(&exception))
    }

    /// Como [`PageRunner::eval`], para o código do hospedeiro (o DOM em JavaScript e as chamadas ao gancho dele):
    /// as funções criadas são builtins para a página, sem nome de arquivo nem frame no `Error.stack`.
    pub fn eval_hidden(&self, source: &str) -> Result<(), String> {
        self.eval_hidden_value(source).map(|_| ())
    }

    /// Como [`PageRunner::eval_hidden`], guardando o valor de completude do programa (a função gancho do DOM, que
    /// assim não precisa de binding global que a página enxergue). O valor fica enraizado já antes das microtarefas.
    pub fn eval_hidden_value(&self, source: &str) -> Result<HostHandle, String> {
        let result = crate::api::eval::evaluate_hidden_program(&self.global_object, source).map(HostHandle::new);
        self.vm.drain_microtasks();
        result.map_err(|exception| describe_exception(&exception))
    }

    /// Chama a função de `handle` com `args` a partir do hospedeiro (sem frame da página), esvazia as microtarefas e
    /// devolve o resultado.
    pub fn call_hidden(&self, handle: &HostHandle, args: &[HostArg]) -> Result<HostHandle, String> {
        let args: Vec<JSValue> = args
            .iter()
            .map(|arg| match arg {
                HostArg::Str(text) => JSValue::from_js_string(js_string(&self.vm, &WtfString::from_utf8(text.as_bytes()))),
                HostArg::Num(number) => js_number(*number),
                HostArg::Null => JSValue::null(),
                HostArg::Handle(handle) => handle.0,
                HostArg::Global(runner) => runner.global_object.global_this().map_or_else(JSValue::undefined, |object| object.as_value()),
            })
            .collect();
        // Os argumentos já criados vivem só aqui até a chamada os pôr na pilha do JavaScript.
        let _arguments = RootScope::new(&args);
        let result = call_function(&self.global_object, handle.0, JSValue::undefined(), &args)
            .map(HostHandle::new)
            .ok_or_else(|| {
                let exception = self.vm.exception().map(|exception| describe_exception(&exception.value())).unwrap_or_default();
                self.vm.clear_exception();
                exception
            });
        self.vm.drain_microtasks();
        result
    }

    /// Roda o laço de eventos (timers, immediates, microtarefas) no relógio virtual do motor até esvaziar ou até
    /// passarem `budget_ms` de tempo virtual (o `--virtual-time-budget` do Chromium). Pode ser chamado de novo: o
    /// relógio virtual continua de onde parou.
    pub fn run_event_loop_for(&self, budget_ms: f64) {
        crate::runtime::timers::run_event_loop_for(&self.global_object, budget_ms);
    }

    /// A latência de rede do `import()` dinâmico de módulo ainda não carregado.
    pub fn set_network_latency_ms(&self, milliseconds: f64) {
        self.modules.set_network_latency_ms(milliseconds);
    }

    /// Troca o `Math.random` da página pelo do V8 com a semente dada (o `--js-flags=--random-seed` do Chromium).
    pub fn set_random_seed(&self, seed: u64) {
        self.global_object.set_v8_random_seed(seed);
    }

    /// Acesso nomeado da janela: `handle` recebe o nome que a busca no global e em toda a cadeia não achou e devolve
    /// o valor, ou `undefined` para ausente.
    pub fn set_global_named_lookup(&self, handle: &HostHandle) {
        crate::runtime::global_named_lookup::set_global_named_lookup(&self.global_object, handle.0);
    }

    /// Entrega ao hospedeiro as rejeições sem tratador: `handle` recebe `("unhandled", promise, reason)` ao fim de
    /// cada checkpoint e `("handled", promise)` quando o tratador chega depois. Tira o tratamento padrão do bun.
    pub fn set_rejection_hook(&self, handle: &HostHandle) {
        let function = handle.0;
        let root = Root::new(function);
        let notify = move |global_object: &JSGlobalObject, args: &[JSValue]| {
            let _ = &root;
            let vm = global_object.vm();
            if call_function(global_object, function, JSValue::undefined(), args).is_none() {
                vm.clear_exception();
            }
        };
        let notify = Rc::new(notify);
        let unhandled = notify.clone();
        self.vm.set_unhandled_rejection_reporter(Some(Rc::new(move |global_object: &JSGlobalObject, promise, reason| {
            let kind = JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(b"unhandled")));
            unhandled(global_object, &[kind, promise, reason]);
        })));
        self.vm.set_rejection_handled_reporter(Some(Rc::new(move |global_object: &JSGlobalObject, promise| {
            let kind = JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(b"handled")));
            notify(global_object, &[kind, promise]);
        })));
    }

    /// Só as microtarefas pendentes.
    pub fn drain_microtasks(&self) {
        self.vm.drain_microtasks();
    }

    /// Entrega cada chamada de `console` a `handle` como `(tipo, ...argumentos)`, com o tipo no vocabulário do
    /// `Runtime.consoleAPICalled` (ver `console_client::ConsoleObserver`). A saída no `ConsoleHost` não muda.
    pub fn set_console_hook(&self, handle: &HostHandle) {
        let function = handle.0;
        let root = Root::new(function);
        crate::runtime::console_client::set_console_observer(Some(Rc::new(move |global_object: &JSGlobalObject, kind: &str, args: &[JSValue]| {
            let _ = &root;
            let vm = global_object.vm();
            let mut values = vec![JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(kind.as_bytes())))];
            values.extend_from_slice(args);
            // Uma exceção do observador não pode vazar para a chamada de `console` da página.
            let pending = vm.exception();
            if call_function(global_object, function, JSValue::undefined(), &values).is_none() && pending.is_none() {
                vm.clear_exception();
            }
        })));
    }
}

/// O que cada global de página (a janela e as dos frames) recebe do hospedeiro: console, DOM, as funções privadas
/// e o carregador de módulos de URL.
fn configure_global(global_object: &JSGlobalObjectRef, console: Rc<dyn ConsoleHost>, dom: Rc<dyn DomHost>, document_url: &str, fetch: ModuleFetch) -> Rc<UrlModuleHost> {
    global_object.set_console_host(console);
    dom_bridge::install(global_object, dom);
    install_global_function_with_visibility(global_object, "__pl_eval", 2, pl_eval, ImplementationVisibility::Private);
    install_global_function_with_visibility(global_object, "__pl_set_code_guard", 1, pl_set_code_guard, ImplementationVisibility::Private);
    install_global_function_with_visibility(global_object, "__pl_microtask_checkpoint", 0, pl_microtask_checkpoint, ImplementationVisibility::Private);
    install_global_function_with_visibility(
        global_object,
        "__pl_private",
        1,
        crate::runtime::js_global_object_private_functions::create_private_symbol,
        ImplementationVisibility::Private,
    );
    let modules = Rc::new(UrlModuleHost::new(document_url, fetch));
    install_url_module_loader(global_object, modules.clone());
    modules
}

/// Cria o programa da página e entrega o [`PageRunner`] a `body`. `document_url` e `fetch` alimentam os módulos ES
/// (`<script type=module>` e `import()`): o texto de cada URL absoluta, ou `None` para erro de rede.
pub fn run_page<T>(console: Rc<dyn ConsoleHost>, dom: Rc<dyn DomHost>, document_url: &str, fetch: ModuleFetch, body: impl FnOnce(&PageRunner) -> T) -> T {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        let modules = configure_global(&global_object, console, dom, document_url, fetch);
        crate::runtime::timers::enable_event_loop();
        let runner = PageRunner { vm, global_object, modules };
        let result = body(&runner);
        crate::runtime::console_client::set_console_observer(None);
        result
    })
}
