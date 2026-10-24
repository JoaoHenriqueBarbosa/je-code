//! Caminho de ponta a ponta de `JSC::evaluate` (`runtime/Completion.cpp`) e do `eval` indireto
//! (`globalFuncEval`): fonte JavaScript entra, valor sai. Encadeia o que o porte já tem; os elos que
//! faltam estão em `wip-notes/e2e-gaps.md`.

use std::cell::RefCell;
use std::rc::Rc;

use crate::api::bun_options::apply_bun_options;
use crate::api::main_module_kind::{main_module_kind, MainModuleKind};
use crate::runtime::js_promise::Status as PromiseStatus;
use crate::api::module_probe::{resolve_require, ModuleFs};
use crate::api::module_probe::file_url_path;
use crate::host_function;
use crate::runtime::host_call::{HostCall, HostResult, Thrown};
use crate::runtime::js_module_loader::{rust_string, throw_build_message, throw_require_failure};
use crate::runtime::node_error::{throw_coded_error, throw_coded_type_error, throw_native_type_error};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::intrinsic::Intrinsic;
use crate::runtime::js_array::JSArray;
use crate::runtime::js_function::put_direct_native_function_with_display_name;
use crate::runtime::js_type::JSType;
use crate::runtime::js_typeof::{js_type_string_for_value, js_typeof_is_function, js_typeof_is_object};
use crate::runtime::object_constructor::construct_empty_object;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::operations::strict_equal;
use crate::runtime::property_descriptor::PropertyDescriptor;
use crate::runtime::property_name::PropertyName;
use crate::runtime::string_prototype::code_units;
use crate::runtime::string_regexp_support::{get_object_property, to_string_value};
use crate::llint::LLIntFailure;
use crate::parser::position_map::PositionMap;
use crate::parser::source_code::{make_source, SourceCode};
use crate::parser::source_provider::{SourceProvider, SourceProviderSourceType};
use crate::parser::source_tainted_origin::SourceTaintedOrigin;
use crate::runtime::call_data::{call as call_function, call_returning_exception, get_call_data};
use crate::runtime::cell_registry::run_program;
use crate::runtime::error_instance::ErrorInstance;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::{js_boolean, js_null, JSValue};
use crate::runtime::program_executable::ProgramExecutable;
use crate::runtime::source_origin::SourceOrigin;
use crate::runtime::vm::VM;
use crate::wtf::text::text_position::{OrdinalNumber, TextPosition};
use crate::wtf::text::wtf_string::String as WtfString;
use crate::wtf::url::URL;

/// Um `VM` e um `JSGlobalObject` recém-criados.
pub(crate) fn new_global_object() -> (Rc<VM>, JSGlobalObjectRef) {
    // As opções do JSC que o bun liga precisam estar postas antes do primeiro `VM`.
    apply_bun_options();
    let vm = Rc::new(VM::new());
    // `JSGlobalObject::create(vm, structure)` seguido do `init(vm)` completo: protótipos, construtores,
    // estruturas e `LinkTimeConstant` que o bytecode e os builtins esperam já preenchidos.
    let global_object = JSGlobalObject::init(&vm);
    (vm, global_object)
}

/// Um `JSGlobalObject` a mais no `vm` já criado: o realm de um frame, que divide o `VM` (heap, microtarefas, relógio)
/// com o da página.
pub(crate) fn new_global_object_in(vm: &Rc<VM>) -> JSGlobalObjectRef {
    JSGlobalObject::init(vm)
}

/// A leitura de `result_name` depois do script: `globalThis[result_name]` direto no `globalThis`, sem montar um
/// segundo `Program`. O bun lê o resultado dentro do próprio script; um `Program` novo passaria pelo
/// `ProgramExecutable::initializeGlobalProperties`, que lança `TypeError` (`Proxy is not allowed in the global
/// prototype chain.`) quando o script pôs um `Proxy` na cadeia do global (`Object.prototype.__proto__` com
/// `this = globalThis`), erro que o programa original nunca viu.
pub(crate) fn read_global_result(global_object: &JSGlobalObjectRef, result_name: &str) -> Result<JSValue, JSValue> {
    // Um nome que não é identificador (`__out.join('\n')`) é expressão sobre o global: só ela se avalia como programa.
    let is_identifier = result_name.bytes().enumerate().all(|(i, b)| b == b'_' || b == b'$' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()));
    if result_name.is_empty() || !is_identifier {
        return evaluate(global_object, &program_source(&format!("globalThis.{result_name}")));
    }
    let vm = global_object.vm();
    let global_this = global_object.global_this().expect("JSGlobalObject sem globalThis (o JSGlobalProxy nasce no finishCreation)");
    // Os geradores leem `globalThis.R` pelo nome: o programa que trocou o `globalThis` (`globalThis = 1`) faz a leitura
    // passar pelo valor novo, como no bun.
    let binding = global_this.get(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"globalThis")));
    if !(binding.is_cell() && binding.as_cell() == global_this.as_value().as_cell()) {
        return evaluate(global_object, &program_source(&format!("globalThis.{result_name}")));
    }
    let value = global_this.get(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, result_name.as_bytes())));
    completion(vm, value)
}

/// O `~VM` do C++: ao sair do escopo do programa chama `VM::last_chance_to_finalize`, que quebra os ciclos
/// `Rc` que o `VM` forma com o `JSGlobalObject` (exceções com pilha, tabela de `CodeBlock`s do interpretador,
/// código instalado nos executáveis). Declarado depois do `VM` e do global, cai antes deles.
pub(crate) struct VmFinalizer(Rc<VM>);

impl VmFinalizer {
    pub(crate) fn new(vm: &Rc<VM>) -> VmFinalizer {
        VmFinalizer(Rc::clone(vm))
    }
}

impl Drop for VmFinalizer {
    fn drop(&mut self) {
        self.0.last_chance_to_finalize();
    }
}

/// A conclusão de `evaluate` e de `globalFuncEval`: o valor, ou a exceção pendente (limpa) como `Err`
/// (o `returnedException` de `evaluate`).
fn completion(vm: &VM, result: JSValue) -> Result<JSValue, JSValue> {
    match vm.exception() {
        Some(exception) => {
            vm.clear_exception();
            Err(exception.value())
        }
        None => Ok(result),
    }
}

/// `JSC::evaluate(globalObject, source, thisValue, returnedException)` (`Completion.cpp`): executa
/// `source` como `Program` no `global_object`. O `thisValue` é o global (`thisValue.toThis`), que é o que
/// `Interpreter::execute_program` usa. `Err` leva a exceção (um `SyntaxError` quando o parser recusa).
pub fn evaluate(global_object: &JSGlobalObjectRef, source: &SourceCode) -> Result<JSValue, JSValue> {
    let vm = global_object.vm();
    // `Interpreter::executeProgram`: `ProgramExecutable::initializeGlobalProperties` (parser, cache de
    // código e instanciação das declarações globais), `prepareForExecution` (o `ProgramCodeBlock`) e
    // `vmEntryToJavaScript`.
    let executable = ProgramExecutable::create(global_object, source);
    let result = vm.interpreter().execute_program(&executable, global_object);
    completion(vm, result)
}

/// O texto de uma exceção para diagnóstico: `Nome: mensagem` quando é um `ErrorInstance` (inclusive o `Error`
/// em que `value_or_pending_exception` converte um recurso não portado), senão o formato de depuração do valor.
pub fn describe_exception(value: &JSValue) -> String {
    if let JSValue::Cell(cell_id) = value {
        if let Some(error) = ErrorInstance::from_cell_id(*cell_id) {
            let message = error.message().latin1();
            return format!("{}: {}", error.name(), String::from_utf8_lossy(&message));
        }
    }
    // O bun mostra `null` e `undefined` em minúsculas (`throw null` é `error: null`).
    match value {
        JSValue::Null => return "null".to_owned(),
        JSValue::Undefined => return "undefined".to_owned(),
        _ => {}
    }
    format!("{value:?}")
}

/// O `SourceCode` de um `Program` com a origem e o nome de arquivo vazios.
pub(crate) fn program_source(source: &str) -> SourceCode {
    make_source(
        &WtfString::from_utf8(source.as_bytes()),
        &SourceOrigin::default(),
        SourceTaintedOrigin::Untainted,
        WtfString::default(),
        TextPosition::default(),
        SourceProviderSourceType::Program,
    )
}

/// `evaluate` sobre um VM e um `JSGlobalObject` recém-criados.
pub fn evaluate_script(source: &str) -> Result<JSValue, JSValue> {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        let result = evaluate(&global_object, &program_source(source));
        // `jsc.cpp` (`runWithOptions`): `vm.drainMicrotasks()` depois de cada `evaluate`.
        vm.drain_microtasks();
        result
    })
}

/// Modo de teste dos timers do host: esvazia as microtarefas e, enquanto restarem prazos de
/// `Atomics.waitAsync`, avança o relógio virtual (`waiter_list_manager.rs`) ao menor deles, resolve a
/// promessa com `"timed-out"` e esvazia as microtarefas de novo.
pub fn drain_virtual_timers(global_object: &JSGlobalObjectRef) {
    crate::runtime::timers::run_event_loop(global_object);
}

/// `evaluate_script` que, no fim, deixa os prazos virtuais vencerem (`drain_virtual_timers`).
pub fn evaluate_script_running_timers(source: &str) -> Result<JSValue, JSValue> {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        crate::runtime::timers::enable_event_loop();
        let result = evaluate(&global_object, &program_source(source));
        drain_virtual_timers(&global_object);
        result
    })
}

/// `evaluate` de `source` com o nome de arquivo `url` (o que `Error.stack` mostra) sobre um VM e um
/// `JSGlobalObject` recém-criados; depois de esvaziar as microtarefas devolve o valor da variável global
/// `result_name`, que o programa grava (a pilha de um erro criado dentro de `then` ou de `await`).
pub fn evaluate_named_script_result(source: &str, url: &str, result_name: &str) -> Result<JSValue, JSValue> {
    evaluate_mapped_script_result(source, url, result_name, &[])
}

/// O fonte como unidades UTF-16 exatas: um surrogate solitário do programa chega ao avaliador como está, sem virar U+FFFD.
fn utf16_units(source: &str) -> Vec<u16> {
    source.encode_utf16().collect()
}

/// O `WtfString` de um fonte em UTF-16: ASCII puro fica de 8 bits (como `WtfString::from_utf8`), o resto de 16 bits sem perda.
fn wtf_from_units(units: &[u16]) -> WtfString {
    if units.iter().all(|&unit| unit < 0x80) {
        let bytes: Vec<u8> = units.iter().map(|&unit| unit as u8).collect();
        WtfString::from_utf8(&bytes)
    } else {
        WtfString::from_utf16(units)
    }
}

/// `evaluate_named_script_result` com o fonte em UTF-16 exato (ver `evaluate_mapped_script_result_units`).
pub fn evaluate_named_script_result_units(source: &[u16], url: &str, result_name: &str) -> Result<JSValue, JSValue> {
    evaluate_mapped_script_result_units(source, url, result_name, &[])
}

/// `evaluate_named_script_result` de um programa ESM do golden: o texto é o reimpresso pelo transpilador do bun
/// e `position_runs` (o mapa do golden, `position_map.rs`) leva as posições de `stack`/`line`/`column` de volta ao
/// fonte original, como o `SavedSourceMap` do bun. Vazio, as posições ficam as do texto executado.
pub fn evaluate_mapped_script_result(source: &str, url: &str, result_name: &str, position_runs: &[i64]) -> Result<JSValue, JSValue> {
    evaluate_mapped_script_result_units(&utf16_units(source), url, result_name, position_runs)
}

/// `evaluate_mapped_script_result` com o fonte em unidades UTF-16 (`WtfString` de 16 bits), que preservam o surrogate solitário.
pub fn evaluate_mapped_script_result_units(source: &[u16], url: &str, result_name: &str, position_runs: &[i64]) -> Result<JSValue, JSValue> {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        // Como o bun, a origem é a URL `file://` do arquivo: o `eval` e o `new Function` mostram essa URL nas frames.
        let origin = SourceOrigin::new(URL::from_string(&WtfString::from_latin1(format!("file:///{url}").as_bytes())));
        let named = transpiled_source(&wtf_from_units(source), &origin, WtfString::from_latin1(url.as_bytes()));
        if !position_runs.is_empty() {
            let map = PositionMap::new(position_runs, 0).expect("mapa de posições com tamanho múltiplo de quatro");
            named.provider().expect("SourceCode sem provedor").set_position_map(Rc::new(map));
        }
        // Como o bun ao rodar o arquivo: o SyntaxError ou a exceção não capturada do script não impede a leitura de
        // `result_name`, que fica indefinida quando o script não chegou a gravá-la.
        let _ = evaluate(&global_object, &named);
        vm.drain_microtasks();
        read_global_result(&global_object, result_name)
    })
}

/// `evaluate_named_script_result` como o corredor dos goldens de globais (`scripts/gen-globals-golden.js`): o script
/// roda por `vm.runInThisContext` e, se lançar sem captura, o resultado é só `Uncaught Nome: mensagem` (a variável
/// `result_name` nem é lida). `Err` carrega esse texto; `Ok` é a leitura de `result_name` depois das microtarefas.
pub fn evaluate_named_script_reporting_uncaught(source: &str, url: &str, result_name: &str) -> Result<Result<JSValue, JSValue>, String> {
    evaluate_named_script_with_console(source, url, result_name, None)
}

/// [`evaluate_named_script_reporting_uncaught`] com o console do hospedeiro instalado no global antes do script
/// (`alert`, `confirm` e `prompt` escrevem o convite no stdout dele e leem o stdin dele).
pub fn evaluate_named_script_with_console(
    source: &str,
    url: &str,
    result_name: &str,
    console: Option<Rc<dyn crate::runtime::console_host::ConsoleHost>>,
) -> Result<Result<JSValue, JSValue>, String> {
    evaluate_named_script_inner(source, url, result_name, console, false)
}

/// [`evaluate_named_script_reporting_uncaught`] que, depois do script e das microtarefas, deixa o laço de eventos
/// virtual (`timers.rs`) esvaziar antes de ler `result_name`: o programa pode agendar timers e tarefas do host.
pub fn evaluate_named_script_running_timers_reporting_uncaught(source: &str, url: &str, result_name: &str) -> Result<Result<JSValue, JSValue>, String> {
    evaluate_named_script_inner(source, url, result_name, None, true)
}

/// Avalia `source` como programa do hospedeiro que a página não enxerga (o DOM do navegador escrito em
/// JavaScript): toda função criada por ele se comporta como builtin do JSC (`toString` nativo, frame fora do
/// `Error.stack`). O nome do programa não aparece em lugar nenhum.
pub fn evaluate_hidden_program(global_object: &JSGlobalObjectRef, source: &str) -> Result<JSValue, JSValue> {
    let code = make_source(
        &WtfString::from_utf8(source.as_bytes()),
        &SourceOrigin::default(),
        SourceTaintedOrigin::Untainted,
        WtfString::default(),
        TextPosition::default(),
        SourceProviderSourceType::Program,
    );
    if let Some(provider) = code.provider() {
        provider.set_hidden();
    }
    evaluate(global_object, &code)
}

/// O `SourceCode` de um `Program` com a origem `file:///url` e o nome de arquivo `url`.
pub(crate) fn named_program_source(source: &str, url: &str) -> SourceCode {
    transpiled_source(&WtfString::from_utf8(source.as_bytes()), &file_origin(url), WtfString::from_latin1(url.as_bytes()))
}

/// Como [`named_program_source`] para um fonte já em Latin1 (8 bits): o `createBuiltinExecutable` do JSC só aceita o fonte
/// do módulo embutido nessa forma.
pub(crate) fn latin1_program_source(source: &[u8], url: &str) -> SourceCode {
    transpiled_source(&WtfString::from_latin1(source), &file_origin(url), WtfString::from_latin1(url.as_bytes()))
}

fn file_origin(url: &str) -> SourceOrigin {
    SourceOrigin::new(URL::from_string(&WtfString::from_latin1(format!("file:///{url}").as_bytes())))
}

/// Um arquivo que o bun roda: passou pelo transpilador, que tira os comentários (as diretivas `//# sourceURL=` não
/// valem nele, só no `eval` e no `Function`).
fn transpiled_source(text: &WtfString, origin: &SourceOrigin, url: WtfString) -> SourceCode {
    let code = make_source(text, origin, SourceTaintedOrigin::Untainted, url, TextPosition::default(), SourceProviderSourceType::Program);
    code.provider().expect("SourceCode sem provedor").set_comments_stripped();
    code
}

/// Roda `source` como o `<script src=url>` clássico de uma página do navegador: os modos do V8 que o navegador liga
/// (`Error.stack`, `CallSite` e mensagens; `toString(radix)`), a origem é a própria URL e o fonte não passa por
/// transpilador. Depois de esvaziar as microtarefas devolve a variável global `result_name`.
pub fn evaluate_browser_script_result(source: &str, url: &str, result_name: &str) -> Result<JSValue, JSValue> {
    run_program(|| {
        crate::runtime::stack_frame::set_v8_stack_format(true);
        crate::runtime::number_prototype::set_v8_radix_strings(true);
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        let url_text = WtfString::from_latin1(url.as_bytes());
        let origin = SourceOrigin::new(URL::from_string(&url_text));
        let code = make_source(&wtf_from_units(&utf16_units(source)), &origin, SourceTaintedOrigin::Untainted, url_text, TextPosition::default(), SourceProviderSourceType::Program);
        let _ = evaluate(&global_object, &code);
        vm.drain_microtasks();
        read_global_result(&global_object, result_name)
    })
}

/// O sistema de arquivos de onde o programa principal carrega `import`, `require` e `fetch('file://...')`; vale para o próximo
/// programa (configuração do embedder, como `set_script_arguments`). Sem ele o programa não lê arquivo.
pub fn set_main_module_fs(fs: Option<Rc<dyn ModuleFs>>) {
    MAIN_FS.with(|slot| *slot.borrow_mut() = fs);
}

/// `bun arquivo.js`: roda `source` (arquivo `url`) como o programa principal sobre um global novo com `console`
/// instalado e devolve o código de saída. Uma exceção não capturada do topo é relatada no stderr do console
/// (`uncaught_report.rs`) e dá o código 1; as microtarefas só esvaziam quando o script termina sem lançar.
pub fn evaluate_main_script(source: &str, url: &str, console: Rc<dyn crate::runtime::console_host::ConsoleHost>) -> i32 {
    let (code, held) = evaluate_main_script_reporting_hold(source, url, console);
    if held {
        // O processo do bun continuaria vivo, parado, até um sinal.
        loop {
            std::thread::park();
        }
    }
    code
}

/// `evaluate_main_script` sem parar para sempre: devolve o código de saída e se o laço de eventos ficou segurado por
/// uma fonte `ref` sem trabalho pendente (o bun, nesse caso, só sairia por tempo esgotado ou sinal).
pub fn evaluate_main_script_reporting_hold(source: &str, url: &str, console: Rc<dyn crate::runtime::console_host::ConsoleHost>) -> (i32, bool) {
    let (code, held, _) = evaluate_main_script_reporting_signal(source, url, console);
    (code, held)
}

/// `evaluate_main_script_reporting_hold` que também diz se o programa morreu por um sinal (`process.kill` no próprio pid
/// sem ouvinte, `process.abort()`): o terceiro valor é o número do sinal. O código devolvido segue sendo `128 + n`, como o
/// shell mostra, mas só o sinal distingue essa morte de um `process.exit(128 + n)`.
pub fn evaluate_main_script_reporting_signal(
    source: &str,
    url: &str,
    console: Rc<dyn crate::runtime::console_host::ConsoleHost>,
) -> (i32, bool, Option<i32>) {
    run_program(|| {
        // Dentro do programa: o início de `run_program` solta o anterior e o reset dele apagaria o caminho.
        crate::runtime::process_system::begin_main_script(url);
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        global_object.set_console_host(console);
        let fs = MAIN_FS.with(|slot| slot.borrow().clone());
        CJS_FS.with(|slot| *slot.borrow_mut() = fs.clone());
        global_object.set_module_fs(fs);
        // O relato acumula os blocos de toda exceção fatal (script, microtarefa, timer, rejeição sem tratador) e sai
        // uma vez, com o rodapé; a primeira delas para o laço de eventos.
        let report = Rc::new(RefCell::new(Vec::<u8>::new()));
        let shared_source: Rc<str> = Rc::from(source);
        crate::runtime::uncaught_report::remember_main_source(shared_source.clone());
        let record = {
            let (report, shared_source) = (report.clone(), shared_source.clone());
            Rc::new(move |global: &JSGlobalObject, error: JSValue, with_trace: bool| {
                crate::runtime::uncaught_report::append_exception_report(&mut report.borrow_mut(), global, &shared_source, error, with_trace);
                crate::runtime::timers::halt_event_loop();
            })
        };
        let on_error = record.clone();
        // O relato fatal padrão de um erro de callback (timer, immediate, tick, microtarefa).
        let fatal_error: Rc<dyn Fn(&JSGlobalObject, JSValue)> = Rc::new(move |global: &JSGlobalObject, error: JSValue| {
            crate::runtime::process_exit::mark_fatal();
            on_error(global, error, true)
        });
        let event_loop_error = fatal_error.clone();
        vm.set_unhandled_error_reporter(Some(Rc::new(move |global: &JSGlobalObject, error: JSValue| {
            crate::runtime::process_exit::report_from_event_loop(global, error, &*event_loop_error)
        })));
        // A rejeição sem tratador não conta como fatal para o evento `exit` (medido: ele vê o código 0). Com ouvinte de
        // `unhandledRejection` a rejeição vai a ele; sem, é o relato padrão.
        let on_rejection = record.clone();
        let rejection_fatal = fatal_error.clone();
        vm.set_unhandled_rejection_reporter(Some(Rc::new(move |global: &JSGlobalObject, promise: JSValue, reason: JSValue| {
            if !crate::runtime::process_exit::deliver_unhandled_rejection(global, promise, reason, &*rejection_fatal) {
                on_rejection(global, reason, false)
            }
        })));
        let exception_fatal = fatal_error.clone();
        vm.set_uncaught_exception_reporter(Some(Rc::new(move |global: &JSGlobalObject, exception: &Rc<crate::runtime::exception::Exception>| {
            crate::runtime::process_exit::report_from_event_loop(global, exception.value(), &*exception_fatal)
        })));
        let mut held = false;
        let script_ran = match run_main_program(&global_object, source, url) {
            // `process.exit()` no script principal termina o programa: a exceção de terminação não é um erro.
            Err(_) if crate::runtime::process_exit::exit_requested() => false,
            // O módulo principal roda como promessa: a origem que os ouvintes veem é `unhandledRejection`.
            Err(exception) => match crate::runtime::process_exit::dispatch_uncaught(&global_object, exception, "unhandledRejection") {
                crate::runtime::process_exit::Dispatch::Handled => !crate::runtime::process_exit::exit_requested(),
                crate::runtime::process_exit::Dispatch::NotHandled => {
                    crate::runtime::process_exit::mark_fatal();
                    record(&global_object, exception, false);
                    false
                }
                crate::runtime::process_exit::Dispatch::HandlerThrew(thrown, code) => {
                    crate::runtime::process_exit::mark_handler_failed(code);
                    crate::runtime::process_exit::mark_fatal();
                    record(&global_object, thrown, true);
                    false
                }
            },
            // O módulo ES já esvaziou as microtarefas na avaliação: um `process.exit()` num tick termina ali.
            Ok(_) => !crate::runtime::process_exit::exit_requested(),
        };
        if script_ran {
            vm.drain_microtasks();
            if report.borrow().is_empty() && !crate::runtime::process_exit::exit_requested() {
                held = crate::runtime::timers::run_event_loop_until_held(&global_object);
            }
        }
        // O programa acabou (ou `exit()` já emitiu `exit`): a exceção de terminação pendente não vai adiante.
        vm.clear_exception();
        if !held {
            crate::runtime::process_exit::emit_exit_at_end(&global_object);
        }
        let text = report.borrow().clone();
        if let Some(failure_code) = crate::runtime::process_exit::handler_failure_code().filter(|_| !text.is_empty()) {
            // Ouvinte que lança: o novo erro sai sem rodapé e o código é 7 (1 se foi o callback de captura).
            if let Some(console) = global_object.console_host() {
                console.write_stderr(&text);
            }
            return (failure_code, held, None);
        }
        let report_code = crate::runtime::uncaught_report::finish_report(&global_object, &text);
        let code = if report_code != 0 { report_code } else { crate::runtime::process_exit::resolved_exit_code() };
        (code, held, crate::runtime::process_exit::killing_signal())
    })
}


/// Roda o arquivo principal `url` no modo que o bun escolhe para ele ([`main_module_kind`]): módulo ES pelo carregador
/// de módulos, ou CommonJS no wrapper. `Err` é a exceção (ou a rejeição da avaliação do módulo) que escapou do topo; um
/// módulo com `await` no topo ainda pendente segue no laço de eventos, e uma rejeição tardia dele chega ao relato como
/// rejeição sem tratador, como no bun.
pub(crate) fn run_main_program(global_object: &JSGlobalObjectRef, source: &str, url: &str) -> Result<(), JSValue> {
    // Com transpilador instalado o arquivo principal roda reimpresso, como no bun, e quem decide ESM ou CJS é o
    // parser dele (`exports_kind`); sem ele vale a medição de `main_module_kind` sobre o fonte cru.
    let attempt = crate::api::source_transpiler::attempt_transpile(url, source, false);
    if let Some(Err(messages)) = &attempt {
        // O transpilador recusa o arquivo (erro que o bun dá em tempo de parse, como atribuir a um `const`): o programa
        // não roda, e o erro chega como o `BuildMessage` (ou `AggregateError`) do bun.
        crate::api::module::install_main_module_loader(global_object, source, url, None);
        return Err(crate::runtime::js_module_loader::build_failure_value(global_object, url, messages));
    }
    let transpiled = attempt.and_then(Result::ok);
    let kind = match &transpiled {
        Some(done) if done.is_commonjs => MainModuleKind::Cjs,
        Some(_) => MainModuleKind::Esm,
        None => main_module_kind(source),
    };
    match kind {
        MainModuleKind::Esm => {
            let (text, map) = match &transpiled {
                Some(done) => (done.text.as_str(), Some(done.map.clone())),
                None => (source, None),
            };
            let promise = crate::api::module::start_main_module(global_object, text, url, map);
            // `process.exit()` no corpo do módulo termina o programa ali: nem tick nem microtarefa pendente roda.
            if crate::runtime::process_exit::exit_requested() {
                return Ok(());
            }
            global_object.vm().drain_microtasks();
            match promise.status() {
                PromiseStatus::Rejected => {
                    promise.mark_as_handled();
                    Err(promise.result())
                }
                _ => Ok(()),
            }
        }
        _ => {
            // O cabeçalho do wrapper ocupa a linha 1 e o arquivo começa na 2: o mapa desloca uma linha. A diretiva do topo
            // continua no corpo e torna a função estrita, como no bun.
            let mut body = utf16_units(source);
            if !source.ends_with(['\n', '\r', '\u{2028}', '\u{2029}']) {
                body.push(u16::from(b'\n'));
            }
            let dirname = match url.rfind('/') {
                Some(0) => "/",
                Some(slash) => &url[..slash],
                None => ".",
            };
            // O `import()` do CommonJS principal passa pelo mesmo carregador do principal módulo ES.
            crate::api::module::install_main_module_loader(global_object, source, url, None);
            run_commonjs_wrapper(global_object, &body, url, false, &[], dirname, transpiled.as_ref())
        }
    }
}

fn evaluate_named_script_inner(
    source: &str,
    url: &str,
    result_name: &str,
    console: Option<Rc<dyn crate::runtime::console_host::ConsoleHost>>,
    run_loop: bool,
) -> Result<Result<JSValue, JSValue>, String> {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        if let Some(console) = console {
            global_object.set_console_host(console);
        }
        // O `process.on('uncaughtException', ...)` do gerador: cada exceção que o runtime relata (`Bun__reportUnhandledError`,
        // por exemplo a de um ouvinte de `EventTarget`) entra, como texto, no vetor global `U`, na ordem do relato.
        if let Ok(collector) = evaluate(
            &global_object,
            &program_source("var U = []; (function (e) { U.push(e instanceof Error ? e.name + ': ' + e.message : 'value: ' + String(e)) })"),
        ) {
            let call_data = get_call_data(collector);
            vm.set_unhandled_error_reporter(Some(Rc::new(move |global: &JSGlobalObject, error: JSValue| {
                let _ = call_function(global, collector, &call_data, JSValue::undefined(), &[error]);
            })));
        }
        let named = named_program_source(source, url);
        if let Err(exception) = evaluate(&global_object, &named) {
            // O gerador imprime `String(e && e.name)` e `String(e && e.message)`: uma exceção que não é Error vira
            // `undefined: undefined`.
            let described = match &exception {
                JSValue::Cell(cell_id) if ErrorInstance::from_cell_id(*cell_id).is_some() => describe_exception(&exception),
                _ => "undefined: undefined".to_string(),
            };
            return Err(format!("Uncaught {described}"));
        }
        vm.drain_microtasks();
        if run_loop {
            crate::runtime::timers::run_event_loop(&global_object);
        }
        Ok(read_global_result(&global_object, result_name))
    })
}

/// `evaluate_named_script_result` para um programa que o bun roda como CommonJS (`scripts/golden-prelude.js`,
/// `moduleMode` devolve `cjs` ou `cjs-strict`). `canonical` é o programa gravado no golden: o corpo exato que o
/// runtime do bun põe na função de wrapper (recuado em 2 espaços), precedido de `"use strict";\n` quando o arquivo
/// era CJS estrito (o runtime descarta a diretiva do texto, mas a função é estrita). O programa vira
///
/// ```text
/// [diretiva](function(exports, require, module, __filename, __dirname) {\n<corpo>})
/// ```
///
/// com o cabeçalho na primeira linha (a diretiva, quando há, divide a linha com ele), de modo que
/// `Function.prototype.toString` do topo devolve o mesmo texto do bun. A função é chamada com `this` igual a
/// `exports`, como o carregador de CJS. `position_runs` é o mapa de posições do golden (`position_map.rs`); a
/// diferença de linhas (o golden tem a linha da diretiva, o porte tem a do cabeçalho) entra no deslocamento.
///
/// `module` (`{ exports, require }`) e `require` são JavaScript, não nativos: `require` lança o que o bun lança
/// sem módulo para resolver (TypeError `ERR_INVALID_ARG_TYPE`/`ERR_INVALID_ARG_VALUE`, e `ResolveMessage` com
/// `MODULE_NOT_FOUND` e a "Require stack"), nunca carrega módulo. Os geradores descartam todo programa que cita
/// `require`, `module`, `__filename` ou `__dirname` (`scripts/host-api.js`), então isto só precisa existir. O
/// `__filename` é o `url` (o gerador tira o diretório temporário das saídas) e o `__dirname` é ".".
pub fn evaluate_cjs_program(canonical: &str, url: &str, result_name: &str, position_runs: &[i64]) -> Result<JSValue, JSValue> {
    evaluate_cjs_program_units(&utf16_units(canonical), url, result_name, position_runs)
}

/// `evaluate_cjs_program` com o fonte em unidades UTF-16, que preservam o surrogate solitário.
pub fn evaluate_cjs_program_units(canonical: &[u16], url: &str, result_name: &str, position_runs: &[i64]) -> Result<JSValue, JSValue> {
    evaluate_cjs_program_inner(None, canonical, url, result_name, position_runs, false)
}

/// `evaluate_cjs_program_units` com um sistema de arquivos: `require` resolve (relativo e `node_modules`), lê, compila
/// com `_compile` e grava em `require.cache` (chave: `realpath`). Sem `fs`, `require` só falha como antes. O `fs` é
/// um `Rc` porque a crate proíbe `unsafe` e o `require` vive depois da chamada que o recebeu.
pub fn evaluate_cjs_program_with_fs(
    fs: Rc<dyn ModuleFs>,
    canonical: &[u16],
    url: &str,
    result_name: &str,
    position_runs: &[i64],
) -> Result<JSValue, JSValue> {
    evaluate_cjs_program_inner(Some(fs), canonical, url, result_name, position_runs, false)
}

/// `evaluate_cjs_program_with_fs` que, depois das microtarefas, roda o laço de eventos até não restar nada vivo
/// (`setTimeout`, `setInterval`, `setImmediate`), como o `bun arquivo.js`; só então lê `result_name`.
pub fn evaluate_cjs_program_with_fs_running_timers(
    fs: Rc<dyn ModuleFs>,
    canonical: &[u16],
    url: &str,
    result_name: &str,
    position_runs: &[i64],
) -> Result<JSValue, JSValue> {
    evaluate_cjs_program_inner(Some(fs), canonical, url, result_name, position_runs, true)
}

fn evaluate_cjs_program_inner(
    fs: Option<Rc<dyn ModuleFs>>,
    canonical: &[u16],
    url: &str,
    result_name: &str,
    position_runs: &[i64],
    run_loop: bool,
) -> Result<JSValue, JSValue> {
    run_program(|| {
        CJS_FS.with(|slot| *slot.borrow_mut() = fs.clone());
        let (vm, global_object) = new_global_object();
        global_object.set_module_fs(fs.clone());
        let _finalizer = VmFinalizer::new(&vm);
        if run_loop {
            crate::runtime::timers::enable_event_loop();
        }
        let directive = utf16_units(CJS_STRICT_DIRECTIVE);
        let (strict, body) = match canonical.strip_prefix(directive.as_slice()) {
            Some(rest) => (true, rest),
            None => (false, canonical),
        };
        // Como o bun ao rodar o arquivo: o SyntaxError ou a exceção não capturada não impede a leitura de `result_name`.
        let _ = run_commonjs_wrapper(&global_object, body, url, strict, position_runs, ".", None);
        vm.drain_microtasks();
        if run_loop {
            crate::runtime::timers::run_event_loop(&global_object);
        }
        read_global_result(&global_object, result_name)
    })
}

/// Monta o carregador de CJS do bun no global (`exports`, `require` e `module` do arquivo `url`), embrulha `body` em
/// `function(exports, require, module, __filename, __dirname)` e chama o wrapper com `this` = `exports`. `Err` é o
/// `SyntaxError` do corpo ou a exceção que ele lançou.
/// Com `prebuilt` (o arquivo que o transpilador de fonte reimprimiu) o texto já é a função de wrapper inteira, com o
/// cabeçalho do impressor na linha 1, e o mapa é o do impressor: nada é embrulhado nem deslocado aqui.
/// Monta o estado do carregador de CJS do bun no global (`CJS_STATE`: `exports`, `require` e `module` do arquivo `url`,
/// que vira o módulo 0) e devolve `(exports, require, module)`.
fn boot_cjs_state(global_object: &JSGlobalObjectRef, url: &str) -> (JSValue, JSValue, JSValue) {
    let vm = global_object.vm();
    let global_object = global_object.clone();
    CJS_FILENAME.with(|name| *name.borrow_mut() = url.to_owned());
    let identifier = |name: &str| Identifier::from_span(&vm, name.as_bytes());
    let natives = construct_empty_object(&global_object);
    let getters = construct_empty_object(&global_object);
    let setters = construct_empty_object(&global_object);
    let fns = construct_empty_object(&global_object);
    let proto_fns = construct_empty_object(&global_object);
    let public = ImplementationVisibility::Public;
    let hidden = ImplementationVisibility::Private;
    let require_native = define_native(&vm, &global_object, &natives, "require", 1, cjs_require as NativeFunction, public);
    let resolve_native = define_native(&vm, &global_object, &natives, "resolve", 1, cjs_require_resolve as NativeFunction, public);
    define_native(&vm, &global_object, &proto_fns, "resolve", 1, cjs_require_resolve as NativeFunction, public);
    // `require` e `resolve` dos módulos carregados: o JavaScript do ajudante os amarra (`bind`) ao arquivo e ao módulo.
    let require_from = define_native(&vm, &global_object, &natives, "requireFrom", 3, cjs_require_from as NativeFunction, public);
    let resolve_from = define_native(&vm, &global_object, &natives, "resolveFrom", 3, cjs_resolve_from as NativeFunction, public);
    define_native(&vm, &global_object, &fns, "_compile", 2, cjs_compile as NativeFunction, public);
    define_native(&vm, &global_object, &fns, "paths", 0, cjs_noop as NativeFunction, public);
    // As funções de `require.extensions` são anônimas (nome vazio, `toString` `function () { [native code] }`,
    // `length` 2): cada uma nasce num suporte próprio, porque a chave do suporte é o nome da função.
    let anonymous = |function: NativeFunction| {
        let holder = construct_empty_object(&global_object);
        define_native(&vm, &global_object, &holder, "", 2, function, public).as_value()
    };
    let extension_js = anonymous(cjs_extension_js as NativeFunction);
    let extension_node = anonymous(cjs_extension_node as NativeFunction);
    // No bun `.ts`, `.cts` e `.mts` são a mesma função (`.js` e `.mjs` já são uma só no boot); `.json` é outra.
    let extension_ts = anonymous(cjs_extension_js as NativeFunction);
    for (key, value) in [
        ("js", extension_js),
        ("json", anonymous(cjs_extension_js as NativeFunction)),
        ("node", extension_node),
        ("ts", extension_ts.clone()),
        ("cts", extension_ts.clone()),
        ("mts", extension_ts),
    ] {
        fns.put_direct(&vm, &PropertyName::from_identifier(&identifier(key)), value, 0);
    }
    for (name, getter, setter) in CJS_ACCESSORS {
        // O acessor de `main` nasce nomeado "get main" (getter e setter, sem o redefine do boot) e a chave é esse nome.
        let key = if *name == "main" { "get main" } else { *name };
        define_native(&vm, &global_object, &getters, key, 0, *getter, hidden);
        // O setter de `main` tem `length` 0, como o getter (no bun os dois são o mesmo acessor "get main").
        define_native(&vm, &global_object, &setters, key, if *name == "main" { 0 } else { 1 }, *setter, hidden);
    }

    let boot = evaluate(&global_object, &program_source(CJS_BOOT)).expect("bootstrap do wrapper CJS lançou");
    let boot_data = get_call_data(boot.clone());
    let filename_arg = JSValue::from_js_string(js_string(&vm, &WtfString::from_utf8(url.as_bytes())));
    let args = [
        require_native.as_value(),
        resolve_native.as_value(),
        getters.as_value(),
        setters.as_value(),
        fns.as_value(),
        proto_fns.as_value(),
        filename_arg,
    ];
    let triple = match call_returning_exception(&global_object, boot, &boot_data, JSValue::undefined(), &args) {
        Ok(Ok(triple)) => triple,
        Ok(Err(_)) => panic!("bootstrap do wrapper CJS lançou"),
        Err(unported) => panic!("interpretador: {unported:?} ainda não portado"),
    };
    let triple = JSObject::from_value(&triple).expect("bootstrap do wrapper CJS não devolveu o array");
    let state = JSObject::from_value(&triple.get_by_index(&vm, 3)).expect("bootstrap do wrapper CJS sem os slots");
    let slots: Vec<JSValue> = (0..CJS_ACCESSORS.len() as u32).map(|index| state.get_by_index(&vm, index)).collect();
    let exports = triple.get_by_index(&vm, 0);
    let require = triple.get_by_index(&vm, 1);
    let module = triple.get_by_index(&vm, 2);
    CJS_STATE.with(|cell| {
        *cell.borrow_mut() = Some(CjsState {
            modules: vec![ModuleState { module: module.clone(), require: require.clone(), slots: slots.clone(), parent_unset: true }],
            slots,
            require: require.clone(),
            global_object: global_object.clone(),
            require_from: require_from.as_value(),
            resolve_from: resolve_from.as_value(),
            helper: None,
            wrapper: JSValue::undefined(),
        })
    });
    (exports, require, module)
}

fn run_commonjs_wrapper(
    global_object: &JSGlobalObjectRef,
    body: &[u16],
    url: &str,
    strict: bool,
    position_runs: &[i64],
    dirname: &str,
    prebuilt: Option<&crate::api::source_transpiler::Transpiled>,
) -> Result<(), JSValue> {
    let (exports, require, module) = boot_cjs_state(global_object, url);
    let vm = global_object.vm();
    let global_object = global_object.clone();
    let text = match prebuilt {
        Some(done) => utf16_units(&done.text),
        None => {
            let mut text = utf16_units(&format!(
                "{}(function(exports, require, module, __filename, __dirname) {{\n",
                if strict { CJS_STRICT_DIRECTIVE.trim_end() } else { "" }
            ));
            text.extend_from_slice(body);
            text.extend(utf16_units("})"));
            text
        }
    };
    let origin = SourceOrigin::new(URL::from_string(&WtfString::from_latin1(format!("file:///{url}").as_bytes())));
    // Sem mapa de posições, o bun embrulha o módulo sem mudar linha nem coluna: o corpo está na linha 1. O cabeçalho
    // do porte ocupa a linha 1 e o corpo a 2; o mapa identidade com deslocamento de uma linha acerta tanto as
    // linhas de `stack` quanto as dos executáveis de função (o `SourceCode` de função prende a primeira linha em 1,
    // então um `start_position` negativo não chega a elas).
    let named = transpiled_source(&wtf_from_units(&text), &origin, WtfString::from_latin1(url.as_bytes()));
    if let Some(done) = prebuilt {
        named.provider().expect("SourceCode sem provedor").set_position_map(done.map.clone());
    } else if position_runs.is_empty() && !strict {
        let map = PositionMap::new(&[1, 1, 0, 0], 1).expect("mapa de posições");
        named.provider().expect("SourceCode sem provedor").set_position_map(Rc::new(map));
    } else if !position_runs.is_empty() {
        // Golden: diretiva na linha 1, corpo a partir da 2. Porte: cabeçalho na 1, corpo a partir da 2.
        // Sem diretiva o corpo começa na linha 1 do golden e na 2 do porte.
        let line_shift = if strict { 0 } else { 1 };
        let map = PositionMap::new(position_runs, line_shift).expect("mapa de posições com tamanho múltiplo de quatro");
        named.provider().expect("SourceCode sem provedor").set_position_map(Rc::new(map));
    }
    let wrapper = evaluate(&global_object, &named)?;
    CJS_STATE.with(|cell| {
        if let Some(state) = cell.borrow_mut().as_mut() {
            state.wrapper = wrapper;
        }
    });
    let filename = JSValue::from_js_string(js_string(&vm, &WtfString::from_utf8(url.as_bytes())));
    let dirname = JSValue::from_js_string(js_string(&vm, &WtfString::from_utf8(dirname.as_bytes())));
    let call_data = get_call_data(wrapper.clone());
    match call_returning_exception(&global_object, wrapper, &call_data, exports.clone(), &[exports, require, module, filename, dirname]) {
        Ok(Ok(_)) => Ok(()),
        Ok(Err(thrown)) => Err(thrown.value()),
        Err(unported) => panic!("interpretador: {unported:?} ainda não portado"),
    }
}

/// O `import` do arquivo CommonJS `path` (texto e mapa do transpilador: a função de wrapper inteira): como no bun o
/// arquivo roda na carga e divide com o `require` o cache de módulos (`require.cache[path]`), de modo que um módulo
/// carregado pelos dois roda uma vez. O primeiro CommonJS do programa monta o estado do carregador (o ponto de entrada
/// é um módulo ES, então ele ocupa o lugar do módulo principal). Devolve o `module.exports` que o arquivo deixou.
pub(crate) fn import_commonjs(path: &str, done: &crate::api::source_transpiler::Transpiled) -> HostResult {
    let global_object = crate::runtime::js_module_record::realm();
    let vm = global_object.vm();
    CJS_FS.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = global_object.module_fs();
        }
    });
    let exports_name = Identifier::from_span(vm, b"exports");
    if CJS_STATE.with(|state| state.borrow().is_none()) {
        let (_, _, module) = boot_cjs_state(&global_object, path);
        let cache = JSObject::from_value(&cjs_slot(0)).expect("require.cache não é objeto");
        cache.define_own_property(vm, &key_of(vm, path), &PropertyDescriptor::new(module.clone(), 0), false)?;
        run_prebuilt_wrapper(&global_object, module.clone(), done, path)?;
        return get_object_property(&global_object, module, &exports_name);
    }
    let cache = cjs_slot(0);
    let cached = get_object_property(&global_object, cache, &Identifier::from_span(vm, path.as_bytes()))?;
    if !cached.is_undefined() {
        return get_object_property(&global_object, cached, &exports_name);
    }
    let parent = CJS_STATE.with(|state| state.borrow().as_ref().map(|state| state.modules[0].module.clone())).expect("estado do CJS");
    let (module, cache_object, key) = new_cjs_module(&global_object, path, parent)?;
    if let Err(thrown) = run_prebuilt_wrapper(&global_object, module.clone(), done, path) {
        let _ = cache_object.delete_property(vm, &key, &mut crate::runtime::delete_property_slot::DeletePropertySlot::default());
        return Err(thrown);
    }
    get_object_property(&global_object, module, &exports_name)
}

/// O `import.meta.require` do módulo ES `path`: o `require` do CommonJS (monta o estado do carregador no primeiro uso,
/// com o próprio módulo no lugar do principal) com o pedido `specifier`.
pub(crate) fn import_meta_require(global_object: &JSGlobalObject, path: &str, specifier: JSValue) -> HostResult {
    CJS_FS.with(|slot| {
        if slot.borrow().is_none() {
            *slot.borrow_mut() = global_object.module_fs();
        }
    });
    if CJS_STATE.with(|state| state.borrow().is_none()) {
        let (_, _, module) = boot_cjs_state(&crate::runtime::js_module_record::realm(), path);
        let cache = JSObject::from_value(&cjs_slot(0)).expect("require.cache não é objeto");
        let vm = global_object.vm();
        cache.define_own_property(vm, &key_of(vm, path), &PropertyDescriptor::new(module, 0), false)?;
    }
    let parent = CJS_STATE.with(|state| state.borrow().as_ref().map(|state| state.modules[0].module.clone())).expect("estado do CJS");
    require_module(global_object, specifier, false, path, parent)
}

/// A diretiva que `canonicalSource` preserva no começo do programa CJS estrito.
const CJS_STRICT_DIRECTIVE: &str = "\"use strict\";\n";

/// Monta `exports`, `require` e `module` como o carregador de CJS do bun e devolve `[exports, require, module, slots]`.
/// `require` e `require.resolve` são nativas (`toString` dá `function require() { [native code] }` porque o
/// `toString` de nativa usa o nome do executável; o `name` é redefinido no boot como no bun: "bound require",
/// "bound resolve"). Medido no bun 1.4.2: o protótipo de `require` é um objeto (filho de `Function.prototype`) com os
/// acessores `cache`, `extensions`, `main` e o valor `resolve`; o de `require.resolve` tem só `paths`; o de `module`
/// tem os acessores `_compile`, `children`, `filename`, `id`, `loaded`, `parent`, `path`, `paths` (getter e setter
/// nativos de nome "get X"/"set X", `toString` `function X() { [native code] }`); `module` só tem `exports` e
/// `require` como próprias. Os valores dos acessores ficam em `slots` (`CJS_ACCESSORS`, na ordem).
const CJS_BOOT: &str = r#"(function (require, resolve, getters, setters, fns, protoFns, filename) {
  "use strict";
  var exports = {};
  Object.defineProperty(require, "name", { value: "bound require", configurable: true });
  Object.defineProperty(resolve, "name", { value: "bound resolve", configurable: true });
  var i = filename.lastIndexOf("/");
  var dir = i < 0 ? "." : i === 0 ? "/" : filename.slice(0, i);
  var paths = [];
  for (var d = dir; ; ) {
    paths.push(d === "/" ? "/node_modules" : d + "/node_modules");
    if (d === "/" || d === ".") break;
    var j = d.lastIndexOf("/");
    d = j <= 0 ? "/" : d.slice(0, j);
  }
  function accessors(target, names, enumerable) {
    for (var k = 0; k < names.length; k++) {
      var n = names[k];
      var key = n === "main" ? "get main" : n;
      if (n !== "main") {
        Object.defineProperty(getters[key], "name", { value: "get " + n, configurable: true });
        Object.defineProperty(setters[key], "name", { value: "set " + n, configurable: true });
      }
      Object.defineProperty(target, n, { get: getters[key], set: setters[key], enumerable: enumerable[k], configurable: true });
    }
  }
  var requireProto = Object.create(Function.prototype);
  accessors(requireProto, ["cache", "extensions", "main"], [true, true, true]);
  requireProto.resolve = protoFns.resolve;
  Object.setPrototypeOf(require, requireProto);
  var resolveProto = Object.create(Function.prototype);
  resolveProto.paths = fns.paths;
  Object.defineProperty(resolveProto, Symbol.toStringTag, { value: "resolve", configurable: true });
  Object.setPrototypeOf(resolve, resolveProto);
  require.resolve = resolve;
  var moduleProto = {};
  accessors(moduleProto, ["_compile", "children", "filename", "id", "loaded", "parent", "path", "paths"], [false, false, true, true, true, false, true, true]);
  var module = Object.create(moduleProto);
  module.exports = exports;
  module.require = require;
  var cache = Object.create(null);
  Object.defineProperty(cache, filename, { value: module, writable: false, enumerable: true, configurable: true });
  var extensions = Object.create(null);
  var exts = [".js", ".json", ".node", ".ts", ".cts", ".mjs", ".mts"];
  var extFns = [fns.js, fns.json, fns.node, fns.ts, fns.cts, fns.js, fns.mts];
  for (var e = 0; e < exts.length; e++) extensions[exts[e]] = extFns[e];
  // Os valores iniciais dos slots dos acessores (`CjsState`); a lógica de `require`, `module` e das extensões mora em
  // Rust (nenhum quadro de JavaScript do boot pode aparecer na pilha do programa).
  var state = [cache, extensions, module, fns._compile, [], filename, ".", true, null, dir, paths];
  return [exports, require, module, state];
})"#;

/// O estado de `module` e `require` do programa CJS em curso. Os slots seguem a ordem de `CJS_ACCESSORS` (`cache`,
/// `extensions`, `main`, `_compile`, `children`, `filename`, `id`, `loaded`, `parent`, `path`, `paths`). Nada disto
/// mora em JavaScript: um quadro de JS do boot apareceria no `stack` de quem chamou o acessor.
struct CjsState {
    /// Os slots globais (`cache`, `extensions`, `main`); os demais são por módulo, em `modules`.
    slots: Vec<JSValue>,
    /// O módulo principal primeiro, depois cada módulo carregado por `require`.
    modules: Vec<ModuleState>,
    require: JSValue,
    global_object: JSGlobalObjectRef,
    require_from: JSValue,
    resolve_from: JSValue,
    /// O ajudante em JavaScript (`CJS_HELPER`), criado no primeiro `require` com sistema de arquivos.
    helper: Option<JSValue>,
    /// A função wrapper do módulo principal. No bun ela segue viva depois de devolver (medido com `Bun.gc(true)` num
    /// timer: o erro criado no principal mantém `originalLine`, `originalColumn` e o cabeçalho do tipo, ou seja, o callee
    /// do frame pendente dele não morre), ao contrário do wrapper de um módulo carregado por `require` e da função do
    /// módulo ESM principal, cujos erros caem no caminho de texto. É também o callee do frame que está executando.
    wrapper: JSValue,
}

/// Os slots de um módulo (índices de `CJS_ACCESSORS` a partir de `_compile`).
struct ModuleState {
    module: JSValue,
    /// O `require` que `_compile` passa ao wrapper deste módulo.
    require: JSValue,
    slots: Vec<JSValue>,
    /// `parent` nasce na primeira leitura em que o `id` é "." (então vira null e fica), e é undefined enquanto não for.
    /// Os módulos carregados por `require` nascem com o `parent` definido.
    parent_unset: bool,
}

thread_local! {
    /// O sistema de arquivos do próximo programa principal (`set_main_module_fs`).
    static MAIN_FS: RefCell<Option<Rc<dyn ModuleFs>>> = const { RefCell::new(None) };
    /// O sistema de arquivos do programa CJS em curso (`None`: `require` não carrega módulo).
    static CJS_FS: RefCell<Option<Rc<dyn ModuleFs>>> = const { RefCell::new(None) };
    /// O `__filename` do programa CJS em curso (a "Require stack" das falhas de `require`).
    static CJS_FILENAME: RefCell<String> = const { RefCell::new(String::new()) };
    /// O estado de `module` e `require` (`None` fora de um programa CJS).
    static CJS_STATE: RefCell<Option<CjsState>> = const { RefCell::new(None) };
}

/// O sistema de arquivos do programa CJS em curso (o que `evaluate_main_script` faz com o `MAIN_FS` ao começar).
pub(crate) fn install_cjs_fs(fs: Option<Rc<dyn ModuleFs>>) {
    CJS_FS.with(|slot| *slot.borrow_mut() = fs);
}

/// As raízes do coletor: tudo o que o estado de `module` e `require` guarda (o `CjsState` é a parte do objeto
/// `Module` do bun que aqui mora em Rust).
pub(crate) fn visit_gc_roots(marker: &mut crate::runtime::gc::Marker) {
    CJS_STATE.with(|state| {
        let Ok(state) = state.try_borrow() else {
            marker.abort();
            return;
        };
        let Some(state) = state.as_ref() else { return };
        marker.values(&state.slots);
        marker.values(&[state.require, state.require_from, state.resolve_from]);
        marker.value(state.helper);
        marker.value(state.wrapper);
        for module in &state.modules {
            marker.values(&[module.module, module.require]);
            marker.values(&module.slots);
        }
    });
}

/// Fim do programa (`cell_registry::reset_program_state`): o nome do arquivo e os valores guardados são do programa.
pub(crate) fn reset_for_program() {
    let _ = CJS_FILENAME.try_with(|name| name.borrow_mut().clear());
    let _ = CJS_FS.try_with(|fs| fs.borrow_mut().take());
    crate::api::builtin_modules::reset_for_program();
    let taken = CJS_STATE.try_with(|state| state.borrow_mut().take());
    drop(taken);
}

/// Cria a função nativa `name` em `holder`. `visibility` privada tira o frame nativo da pilha (os acessores de
/// `module` e `require` não têm frame no bun; `_compile` e as extensões têm). Os donos são objetos comuns
/// (`constructEmptyObject`), com a estrutura compartilhada do global: a gravação vai com transição (`putDirect`), como o
/// C++ faz nesses objetos; `putDirectWithoutTransition` só vale em estrutura própria e aqui poria a função em todo `{}`.
pub(crate) fn define_native(
    vm: &VM,
    global_object: &JSGlobalObject,
    holder: &JSObject,
    name: &str,
    length: u32,
    function: NativeFunction,
    visibility: ImplementationVisibility,
) -> crate::runtime::js_function::JSFunctionRef {
    put_direct_native_function_with_display_name(
        vm,
        global_object,
        holder,
        &Identifier::from_span(vm, name.as_bytes()),
        &WtfString::from_utf8(name.as_bytes()),
        length,
        function,
        visibility,
        Intrinsic::NoIntrinsic,
        0,
    )
}

/// Lê um slot do estado (cópia do valor, sem segurar o empréstimo: o código do usuário pode reentrar).
fn cjs_slot(index: usize) -> JSValue {
    CJS_STATE.with(|state| state.borrow().as_ref().map_or(JSValue::undefined(), |state| state.slots[index].clone()))
}

/// Os nomes das propriedades que o setter de `cache` e de `extensions` grava no `this`.
const CJS_OWN_SLOT_NAMES: [&str; 2] = ["cache", "extensions"];

/// O acessor número `id` de `CJS_ACCESSORS` (par: getter, ímpar: setter). O setter de `cache` e de `extensions` grava
/// uma propriedade própria no `this` que for objeto (a do protótipo só é lida quando não há própria; falha de
/// `defineProperty` não lança, medido no bun); o de `main` e o de `parent` não fazem nada; os demais só valem no
/// `module`. `filename`, `id` e `path` convertem para string (o símbolo lança), `loaded` para booleano; o resto guarda cru.
fn cjs_accessor(global_object: &JSGlobalObject, call: &HostCall, id: u32) -> HostResult {
    let vm = global_object.vm();
    let slot = (id >> 1) as usize;
    let this = call.this_value();
    if id & 1 == 1 {
        let value = call.argument(0);
        if slot == 2 {
            // O setter de `main` é o próprio getter: ignora o argumento e devolve o módulo principal.
            return Ok(cjs_slot(2));
        }
        if slot < 2 {
            if let Some(object) = JSObject::from_value(&this) {
                let name = PropertyName::from_identifier(&Identifier::from_span(vm, CJS_OWN_SLOT_NAMES[slot].as_bytes()));
                object.define_own_property(vm, &name, &PropertyDescriptor::new(value, 0), false)?;
            }
        } else if slot != 2 && slot != 8 {
            if let Some(index) = module_index(&this) {
                let stored = match slot {
                    5 | 6 | 9 => JSValue::from_js_string(to_string_value(global_object, value)?),
                    7 => js_boolean(value.to_boolean()),
                    _ => value,
                };
                CJS_STATE.with(|state| {
                    if let Some(state) = state.borrow_mut().as_mut() {
                        state.modules[index].slots[slot] = stored;
                    }
                });
            }
        }
        return Ok(JSValue::undefined());
    }
    if slot < 3 {
        return Ok(cjs_slot(slot));
    }
    let Some(index) = module_index(&this) else {
        return Ok(JSValue::undefined());
    };
    if slot == 8 {
        let unset = CJS_STATE.with(|state| state.borrow().as_ref().is_some_and(|state| state.modules[index].parent_unset));
        if unset {
            let id_value = module_slot(index, 6);
            if !(id_value.is_string() && rust_string(&id_value.to_wtf_string()) == ".") {
                return Ok(JSValue::undefined());
            }
            CJS_STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.modules[index].parent_unset = false;
                    state.modules[index].slots[8] = js_null();
                }
            });
        }
    }
    Ok(module_slot(index, slot))
}

/// O índice de `this` em `CjsState::modules` (`None` se não é um módulo do programa).
fn module_index(this: &JSValue) -> Option<usize> {
    let modules: Vec<JSValue> = CJS_STATE.with(|state| state.borrow().as_ref().map(|state| state.modules.iter().map(|m| m.module.clone()).collect()).unwrap_or_default());
    modules.into_iter().position(|module| strict_equal(this.clone(), module))
}

/// Lê o slot `slot` do módulo número `index` (cópia do valor).
fn module_slot(index: usize, slot: usize) -> JSValue {
    CJS_STATE.with(|state| state.borrow().as_ref().map_or(JSValue::undefined(), |state| state.modules[index].slots[slot].clone()))
}

/// Define a função nativa `$name` (e o corpo `$body`) que roda o acessor número `$id`.
macro_rules! cjs_accessors {
    ($(($name:ident, $body:ident, $id:expr)),* $(,)?) => {
        $(
            fn $body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
                cjs_accessor(global_object, call, $id)
            }
            host_function!($name, $body);
        )*
    };
}
cjs_accessors!(
    (cjs_get_cache, cjs_get_cache_body, 0),
    (cjs_set_cache, cjs_set_cache_body, 1),
    (cjs_get_extensions, cjs_get_extensions_body, 2),
    (cjs_set_extensions, cjs_set_extensions_body, 3),
    (cjs_get_main, cjs_get_main_body, 4),
    (cjs_set_main, cjs_set_main_body, 5),
    (cjs_get_compile, cjs_get_compile_body, 6),
    (cjs_set_compile, cjs_set_compile_body, 7),
    (cjs_get_children, cjs_get_children_body, 8),
    (cjs_set_children, cjs_set_children_body, 9),
    (cjs_get_filename, cjs_get_filename_body, 10),
    (cjs_set_filename, cjs_set_filename_body, 11),
    (cjs_get_id, cjs_get_id_body, 12),
    (cjs_set_id, cjs_set_id_body, 13),
    (cjs_get_loaded, cjs_get_loaded_body, 14),
    (cjs_set_loaded, cjs_set_loaded_body, 15),
    (cjs_get_parent, cjs_get_parent_body, 16),
    (cjs_set_parent, cjs_set_parent_body, 17),
    (cjs_get_path, cjs_get_path_body, 18),
    (cjs_set_path, cjs_set_path_body, 19),
    (cjs_get_paths, cjs_get_paths_body, 20),
    (cjs_set_paths, cjs_set_paths_body, 21),
);

/// O tipo no `got ...` das mensagens de validador de propriedade (`The "x" property must be of type string, got number`):
/// o `typeof`, com `object` para `null` e `array` para arrays.
pub(crate) fn typeof_for_property_error(value: JSValue) -> &'static str {
    if value.is_null() {
        "object"
    } else if JSArray::from_value(&value).is_some() {
        "array"
    } else {
        js_type_string_for_value(value)
    }
}

/// `path.dirname` do bun/node sobre unidades UTF-16 (o `__dirname` que `module._compile` passa ao wrapper).
fn dirname_of(path: &[u16]) -> Vec<u16> {
    const SLASH: u16 = b'/' as u16;
    let dot = vec![b'.' as u16];
    if path.is_empty() {
        return dot;
    }
    let has_root = path[0] == SLASH;
    let mut end = None;
    let mut matched = true;
    for index in (1..path.len()).rev() {
        if path[index] == SLASH {
            if !matched {
                end = Some(index);
                break;
            }
        } else {
            matched = false;
        }
    }
    match end {
        None if has_root => vec![SLASH],
        None => dot,
        Some(1) if has_root => vec![SLASH, SLASH],
        Some(end) => path[..end].to_vec(),
    }
}

/// `module._compile(source, filename)`: só vale no `module`. O texto vira o corpo do wrapper CJS e roda como programa
/// de nome `filename` (a pilha do código mostra `filename`, e logo depois dele o frame nativo `_compile (unknown)`).
fn cjs_compile_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let module = call.this_value();
    if module_index(&module).is_none() {
        return Ok(JSValue::undefined());
    }
    let text = to_string_value(global_object, call.argument(0))?.value();
    let name = call.argument(1);
    let is_string_object = JSObject::from_value(&name).is_some_and(|object| matches!(object.type_(), JSType::StringObjectType | JSType::DerivedStringObjectType));
    if !(name.is_string() || name.is_symbol() || is_string_object) {
        let message = format!("The \"path\" property must be of type string, got {}", typeof_for_property_error(name));
        return Err(throw_coded_type_error(global_object, &message, "ERR_INVALID_ARG_TYPE"));
    }
    let file = to_string_value(global_object, name)?.value();
    compile_module(global_object, module, &text, &file)
}

/// O miolo de `module._compile`: roda `text` no wrapper CJS como o programa `file`, com o `require` do módulo.
fn compile_module(global_object: &JSGlobalObject, module: JSValue, text: &WtfString, file: &WtfString) -> HostResult {
    let vm = global_object.vm();
    let file_units = code_units(file).into_owned();
    let file_text = rust_string(file);
    let mut units = utf16_units("(function(exports, require, module, __filename, __dirname) {");
    units.extend_from_slice(&code_units(text));
    units.extend(utf16_units("\n})"));
    let origin = SourceOrigin::new(URL::from_string(&WtfString::from_latin1(format!("file:///{file_text}").as_bytes())));
    let source = transpiled_source(&wtf_from_units(&units), &origin, file.clone());

    run_module_source(global_object, module, source, &file_text, &file_units, false)
}

/// Roda no wrapper o `source` do arquivo `file` (a função de wrapper já montada), com o `require` do módulo. `this` é o
/// `exports` quando `exports_this`, senão o módulo (o `_compile` do bun).
fn run_module_source(
    global_object: &JSGlobalObject,
    module: JSValue,
    source: crate::parser::source_code::SourceCode,
    file_text: &str,
    file_units: &[u16],
    exports_this: bool,
) -> HostResult {
    let vm = global_object.vm();
    let Some((global, require)) = module_index(&module).and_then(|index| {
        CJS_STATE.with(|state| state.borrow().as_ref().map(|state| (state.global_object.clone(), state.modules[index].require.clone())))
    }) else {
        return Ok(JSValue::undefined());
    };
    let executable = ProgramExecutable::create(&global, &source);
    let wrapper = vm.interpreter().execute_program(&executable, &global);
    if let Some(exception) = vm.exception() {
        // O bun nomeia o frame de parse com o nome dado e a linha do erro no fonte embrulhado: `<parse> (y.js:2)`.
        if let JSValue::Cell(cell_id) = exception.value() {
            if let Some(error) = ErrorInstance::from_cell_id(cell_id).filter(|error| error.is_parse_error()) {
                error.set_parse_frame(file_text, error.line());
            }
        }
        return Err(Thrown::Pending);
    }
    let exports = get_object_property(global_object, module.clone(), &Identifier::from_span(vm, b"exports"))?;
    let call_data = get_call_data(wrapper.clone());
    let dirname = JSValue::from_js_string(js_string(vm, &wtf_from_units(&dirname_of(file_units))));
    let args = [exports, require, module.clone(), JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(file_text.as_bytes()))), dirname];
    match call_function(global_object, wrapper, &call_data, if exports_this { args[0].clone() } else { module }, &args) {
        Ok(_) => Ok(JSValue::undefined()),
        Err(LLIntFailure::Thrown) => Err(Thrown::Pending),
        Err(_) => Err(Thrown::Unported("wrapper de module._compile")),
    }
}

/// O wrapper que o transpilador de fonte reimprimiu para `file` (o texto é a função inteira, com o mapa do impressor),
/// rodado com `this` igual a `exports`, como o bun faz com o CommonJS que um `import` puxa.
fn run_prebuilt_wrapper(global_object: &JSGlobalObject, module: JSValue, done: &crate::api::source_transpiler::Transpiled, file: &str) -> Result<(), Thrown> {
    let origin = SourceOrigin::new(URL::from_string(&WtfString::from_latin1(format!("file:///{file}").as_bytes())));
    let source = transpiled_source(&WtfString::from_utf8(done.text.as_bytes()), &origin, WtfString::from_utf8(file.as_bytes()));
    source.provider().expect("SourceCode sem provedor").set_position_map(done.map.clone());
    run_module_source(global_object, module, source, file, &utf16_units(file), true).map(|_| ())
}
host_function!(cjs_compile, cjs_compile_body);

/// `require.extensions['.js']` (e `.json`, `.ts`, `.cts`, `.mts`): sem módulo para ler, só falha como o bun.
fn cjs_extension_js_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if !strict_equal(call.argument(0), cjs_slot(2)) {
        return Err(throw_native_type_error(global_object, "Module._extensions['.js'] must be called with a CommonJS module object"));
    }
    let file = rust_string(&to_string_value(global_object, call.argument(1))?.value());
    Err(throw_build_message(global_object, format!("ENOENT reading \"{file}\"")))
}
host_function!(cjs_extension_js, cjs_extension_js_body);

/// `require.extensions['.node']`: valida os argumentos de `dlopen` e falha como o bun sem o arquivo.
fn cjs_extension_node_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    if call.argument_count() < 2 {
        return Err(throw_native_type_error(global_object, "dlopen requires 2 arguments"));
    }
    let module = call.argument(0);
    if !(module.is_cell() && (js_typeof_is_object(module.clone()) || js_typeof_is_function(module.clone()))) {
        return Err(throw_native_type_error(global_object, "dlopen requires an object as first argument"));
    }
    if get_object_property(global_object, module, &Identifier::from_span(global_object.vm(), b"exports"))?.is_undefined() {
        return Err(throw_native_type_error(global_object, "dlopen requires an object with an exports property"));
    }
    let file = rust_string(&to_string_value(global_object, call.argument(1))?.value());
    Err(throw_coded_error(global_object, &format!("{file}: cannot open shared object file: No such file or directory"), "ERR_DLOPEN_FAILED"))
}
host_function!(cjs_extension_node, cjs_extension_node_body);

/// Os acessores, na ordem dos slots do boot: (nome, getter, setter).
const CJS_ACCESSORS: &[(&str, NativeFunction, NativeFunction)] = &[
    ("cache", cjs_get_cache as NativeFunction, cjs_set_cache as NativeFunction),
    ("extensions", cjs_get_extensions as NativeFunction, cjs_set_extensions as NativeFunction),
    ("main", cjs_get_main as NativeFunction, cjs_set_main as NativeFunction),
    ("_compile", cjs_get_compile as NativeFunction, cjs_set_compile as NativeFunction),
    ("children", cjs_get_children as NativeFunction, cjs_set_children as NativeFunction),
    ("filename", cjs_get_filename as NativeFunction, cjs_set_filename as NativeFunction),
    ("id", cjs_get_id as NativeFunction, cjs_set_id as NativeFunction),
    ("loaded", cjs_get_loaded as NativeFunction, cjs_set_loaded as NativeFunction),
    ("parent", cjs_get_parent as NativeFunction, cjs_set_parent as NativeFunction),
    ("path", cjs_get_path as NativeFunction, cjs_set_path as NativeFunction),
    ("paths", cjs_get_paths as NativeFunction, cjs_set_paths as NativeFunction),
];

fn cjs_noop_body(_global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    Ok(JSValue::undefined())
}
host_function!(cjs_noop, cjs_noop_body);

/// `require(id)`: sem sistema de arquivos, lança o que o bun lança sem módulo que resolva (ver `throw_require_failure`);
/// com ele, carrega o módulo (`require_module`).
fn cjs_require_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let filename = CJS_FILENAME.with(|name| name.borrow().clone());
    require_module(global_object, call.argument(0), false, &filename, cjs_slot(2))
}
host_function!(cjs_require, cjs_require_body);

/// `require.resolve(request)`: igual a `require`, com o argumento chamado `request` na mensagem de tipo.
fn cjs_require_resolve_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let filename = CJS_FILENAME.with(|name| name.borrow().clone());
    require_module(global_object, call.argument(0), true, &filename, cjs_slot(2))
}
host_function!(cjs_require_resolve, cjs_require_resolve_body);

/// O `require` de um módulo carregado: amarrado (`bind`) ao nome do arquivo e ao módulo, que chegam como os dois
/// primeiros argumentos, e o pedido é o terceiro.
fn cjs_require_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let file = rust_string(&to_string_value(global_object, call.argument(0))?.value());
    require_module(global_object, call.argument(2), false, &file, call.argument(1))
}
host_function!(cjs_require_from, cjs_require_from_body);

fn cjs_resolve_from_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let file = rust_string(&to_string_value(global_object, call.argument(0))?.value());
    require_module(global_object, call.argument(2), true, &file, call.argument(1))
}
host_function!(cjs_resolve_from, cjs_resolve_from_body);

/// O ajudante em JavaScript dos módulos carregados (o corpo tem de ser invisível na pilha de quem os usa, então só
/// monta objetos e nunca roda código do usuário): `make(filename)` devolve `[module, require]` com o protótipo do
/// módulo principal, `exports` e `require` como únicas próprias.
const CJS_HELPER: &str = r#"(function (mainModule, mainRequire, requireFrom, resolveFrom) {
  "use strict";
  var moduleProto = Object.getPrototypeOf(mainModule);
  var requireProto = Object.getPrototypeOf(mainRequire);
  var resolveProto = Object.getPrototypeOf(mainRequire.resolve);
  return {
    make: function (filename) {
      var m = Object.create(moduleProto);
      var req = requireFrom.bind(undefined, filename, m);
      var res = resolveFrom.bind(undefined, filename, m);
      Object.defineProperty(req, "name", { value: "bound require", configurable: true });
      Object.defineProperty(res, "name", { value: "bound resolve", configurable: true });
      Object.setPrototypeOf(req, requireProto);
      Object.setPrototypeOf(res, resolveProto);
      req.resolve = res;
      m.exports = {};
      m.require = req;
      return [m, req];
    },
    push: function (list, item) { list.push(item); },
    array: function () { return []; },
    json: function (text) { return JSON.parse(text); },
  };
})"#;

/// Chama `name` do ajudante (criado no primeiro uso).
fn call_helper(global_object: &JSGlobalObject, name: &str, args: &[JSValue]) -> Result<JSValue, Thrown> {
    let existing = CJS_STATE.with(|state| state.borrow().as_ref().and_then(|state| state.helper.clone()));
    let helper = match existing {
        Some(helper) => helper,
        None => {
            let Some((global, main_module, main_require, require_from, resolve_from)) = CJS_STATE.with(|state| {
                state.borrow().as_ref().map(|state| {
                    (state.global_object.clone(), state.modules[0].module.clone(), state.require.clone(), state.require_from.clone(), state.resolve_from.clone())
                })
            }) else {
                return Ok(JSValue::undefined());
            };
            let factory = evaluate(&global, &program_source(CJS_HELPER)).expect("ajudante do require lançou");
            let data = get_call_data(factory.clone());
            let helper = match call_returning_exception(&global, factory, &data, JSValue::undefined(), &[main_module, main_require, require_from, resolve_from]) {
                Ok(Ok(helper)) => helper,
                Ok(Err(_)) => panic!("ajudante do require lançou"),
                Err(unported) => panic!("interpretador: {unported:?} ainda não portado"),
            };
            CJS_STATE.with(|state| {
                if let Some(state) = state.borrow_mut().as_mut() {
                    state.helper = Some(helper.clone());
                }
            });
            helper
        }
    };
    let function = get_object_property(global_object, helper.clone(), &Identifier::from_span(global_object.vm(), name.as_bytes()))?;
    let data = get_call_data(function.clone());
    match call_function(global_object, function, &data, helper, args) {
        Ok(value) => Ok(value),
        Err(LLIntFailure::Thrown) => Err(Thrown::Pending),
        Err(_) => Err(Thrown::Unported("ajudante do require")),
    }
}

pub(crate) fn js_text(vm: &VM, text: &str) -> JSValue {
    JSValue::from_js_string(js_string(vm, &WtfString::from_utf8(text.as_bytes())))
}

pub(crate) fn key_of(vm: &VM, name: &str) -> PropertyName {
    PropertyName::from_identifier(&Identifier::from_span(vm, name.as_bytes()))
}

/// `path.dirname` de um caminho absoluto.
fn dirname_str(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(end) => &path[..end],
    }
}

/// Os diretórios `node_modules` de `dir` para cima, como `module.paths`.
fn node_modules_paths(dir: &str) -> Vec<String> {
    let mut paths = Vec::new();
    let mut current = dir.to_owned();
    loop {
        paths.push(if current == "/" { "/node_modules".to_owned() } else { format!("{current}/node_modules") });
        if current == "/" || current == "." {
            break;
        }
        current = dirname_str(&current).to_owned();
    }
    paths
}

/// `require(specifier)` e `require.resolve(specifier)` chamados por `importer` (o `module` que chamou é `parent`).
/// Sem sistema de arquivos, ou para `node:`, só falha como o bun sem módulo (`throw_require_failure`). Com ele:
/// resolve (`resolve_require`), devolve o caminho (`resolve`) ou o `exports` do módulo em `require.cache[realpath]`;
/// um módulo novo nasce com `id`, `filename`, `path`, `paths`, `loaded`, `children` e `parent`, entra no cache e na lista
/// `children` do `parent` antes de rodar (ciclos veem o `exports` parcial) e sai do cache se o código lança.
fn require_module(global_object: &JSGlobalObject, specifier: JSValue, resolve: bool, importer: &str, parent: JSValue) -> HostResult {
    // Os embutidos (`vm`, `node:vm`) vêm do registro, antes de qualquer sonda de arquivo e mesmo sem sistema de arquivos.
    if specifier.is_string() {
        let request = rust_string(&specifier.to_wtf_string());
        if let crate::api::builtin_modules::Resolved::Builtin(entry) = crate::api::builtin_modules::resolve(&request) {
            // `require.resolve("vm")` devolve o próprio pedido (medido no bun 1.4.2).
            return if resolve { Ok(js_text(global_object.vm(), &request)) } else { crate::api::builtin_modules::load(global_object, entry) };
        }
    }
    let Some(fs) = CJS_FS.with(|fs| fs.borrow().clone()) else {
        return Err(throw_require_failure(global_object, specifier, resolve, importer));
    };
    if !specifier.is_string() {
        return Err(throw_require_failure(global_object, specifier, resolve, importer));
    }
    let request = rust_string(&specifier.to_wtf_string());
    if request.is_empty() || (request.starts_with("node:") && !resolve) {
        return Err(throw_require_failure(global_object, specifier, resolve, importer));
    }
    let vm = global_object.vm();
    let Some(path) = resolve_require(fs.as_ref(), dirname_str(importer), &request, false) else {
        let reported = file_url_path(&request).map_or(specifier, |path| js_text(global_object.vm(), &path));
        return Err(throw_require_failure(global_object, reported, resolve, importer));
    };
    if resolve {
        return Ok(js_text(vm, &path));
    }
    let cache = cjs_slot(0);
    let cached = get_object_property(global_object, cache.clone(), &Identifier::from_span(vm, path.as_bytes()))?;
    if !cached.is_undefined() {
        return get_object_property(global_object, cached, &Identifier::from_span(vm, b"exports"));
    }
    let Some(text) = fs.read_file(&path) else {
        return Err(throw_build_message(global_object, format!("ENOENT reading \"{path}\"")));
    };
    let (module, cache_object, key) = new_cjs_module(global_object, &path, parent)?;
    let outcome = if path.ends_with(".json") {
        call_helper(global_object, "json", &[js_text(vm, &text)]).and_then(|parsed| {
            let module_object = JSObject::from_value(&module).expect("módulo sem objeto");
            module_object.define_own_property(vm, &key_of(vm, "exports"), &PropertyDescriptor::new(parsed, 0), false)?;
            Ok(JSValue::undefined())
        })
    } else {
        compile_module(global_object, module.clone(), &WtfString::from_utf8(text.as_bytes()), &WtfString::from_utf8(path.as_bytes()))
    };
    if let Err(thrown) = outcome {
        let _ = cache_object.delete_property(vm, &key, &mut crate::runtime::delete_property_slot::DeletePropertySlot::default());
        return Err(thrown);
    }
    get_object_property(global_object, module, &Identifier::from_span(vm, b"exports"))
}


/// O módulo CommonJS novo de `path` (`id`, `filename`, `path`, `paths`, `loaded`, `children` e `parent`): entra na lista
/// `children` do `parent` e em `require.cache` antes de rodar (ciclos veem o `exports` parcial). Devolve o módulo, o
/// objeto do cache e a chave, para quem precisar tirá-lo do cache se o código lançar.
fn new_cjs_module(global_object: &JSGlobalObject, path: &str, parent: JSValue) -> Result<(JSValue, crate::runtime::js_object::JSObjectHandle, PropertyName), Thrown> {
    let vm = global_object.vm();
    let cache = cjs_slot(0);
    let made = call_helper(global_object, "make", &[js_text(vm, path)])?;
    let made = JSObject::from_value(&made).expect("o ajudante do require devolve um array");
    let module = made.get_by_index(vm, 0);
    let module_require = made.get_by_index(vm, 1);
    let children = call_helper(global_object, "array", &[])?;
    let dir = dirname_str(path).to_owned();
    let paths = call_helper(global_object, "array", &[])?;
    for entry in node_modules_paths(&dir) {
        call_helper(global_object, "push", &[paths.clone(), js_text(vm, &entry)])?;
    }
    let compile = cjs_slot(3);
    // Ordem de `CJS_ACCESSORS` a partir do slot 3: _compile, children, filename, id, loaded, parent, path, paths.
    let slots = vec![
        cjs_slot(0),
        cjs_slot(1),
        cjs_slot(2),
        compile,
        children,
        js_text(vm, path),
        js_text(vm, path),
        js_boolean(true),
        parent.clone(),
        js_text(vm, &dir),
        paths,
    ];
    CJS_STATE.with(|state| {
        if let Some(state) = state.borrow_mut().as_mut() {
            state.modules.push(ModuleState { module: module.clone(), require: module_require, slots, parent_unset: false });
        }
    });
    if let Some(index) = module_index(&parent) {
        let siblings = module_slot(index, 4);
        call_helper(global_object, "push", &[siblings, module.clone()])?;
    }
    let cache_object = JSObject::from_value(&cache).expect("require.cache não é objeto");
    let key = key_of(vm, path);
    cache_object.define_own_property(vm, &key, &PropertyDescriptor::new(module.clone(), 0), false)?;
    Ok((module, cache_object, key))
}

/// Literal de string JSON de `text` (só `\`, `"` e quebras de linha precisam de escape nos nomes de arquivo dos goldens).
fn json_quote(text: &str) -> String {
    let mut quoted = String::from("\"");
    for character in text.chars() {
        match character {
            '\\' => quoted.push_str("\\\\"),
            '"' => quoted.push_str("\\\""),
            '\n' => quoted.push_str("\\n"),
            other => quoted.push(other),
        }
    }
    quoted.push('"');
    quoted
}

/// `evaluate_named_script_result` para uma sequência de scripts no mesmo `JSGlobalObject` (o que o host faz com
/// `vm.runInThisContext` várias vezes): cada script roda em ordem e a exceção de um não impede o seguinte. Devolve
/// o texto dos erros (`índice:Nome: mensagem`, na ordem) e o valor da variável global `result_name` no fim.
pub fn evaluate_script_sequence_result(sources: &[&str], url: &str, result_name: &str) -> (Vec<String>, Result<JSValue, JSValue>) {
    run_sequence(sources, url, result_name, false)
}

/// `evaluate_script_sequence_result` que, depois dos scripts, roda o laço de eventos virtual (timers, immediates e
/// microtasks, `drain_virtual_timers`) antes de ler `result_name`.
pub fn evaluate_script_sequence_result_running_timers(sources: &[&str], url: &str, result_name: &str) -> (Vec<String>, Result<JSValue, JSValue>) {
    run_sequence(sources, url, result_name, true)
}

fn run_sequence(sources: &[&str], url: &str, result_name: &str, run_timers: bool) -> (Vec<String>, Result<JSValue, JSValue>) {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        let origin = SourceOrigin::new(URL::from_string(&WtfString::from_latin1(format!("file:///{url}").as_bytes())));
        let mut errors = Vec::new();
        if run_timers {
            crate::runtime::timers::enable_event_loop();
        }
        for (index, source) in sources.iter().enumerate() {
            let named = transpiled_source(&WtfString::from_utf8(source.as_bytes()), &origin, WtfString::from_latin1(url.as_bytes()));
            if let Err(exception) = evaluate(&global_object, &named) {
                errors.push(format!("{index}:{}", describe_exception(&exception)));
            }
        }
        if run_timers {
            drain_virtual_timers(&global_object);
        } else {
            vm.drain_microtasks();
        }
        (errors, evaluate(&global_object, &program_source(result_name)))
    })
}

/// `globalFuncEval` (o `eval` indireto, `eval(source)` chamado do global) sobre um VM e um
/// `JSGlobalObject` recém-criados: `IndirectEvalExecutable::tryCreate` e `Interpreter::executeEval`.
pub fn evaluate_indirect_eval(source: &str) -> Result<JSValue, JSValue> {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        indirect_eval(&vm, &global_object, &utf16_units(source))
    })
}

/// Um `(0, eval)(source)` no `global_object`: o valor de completão, ou a exceção lançada como `Err`.
fn indirect_eval(vm: &VM, global_object: &JSGlobalObjectRef, source: &[u16]) -> Result<JSValue, JSValue> {
    let argument = JSValue::from_js_string(js_string(vm, &wtf_from_units(source)));
    let result = vm.interpreter().global_func_eval(global_object, argument, &SourceOrigin::default(), SourceTaintedOrigin::Untainted, None);
    match result {
        Ok(value) => completion(vm, value),
        Err(LLIntFailure::Thrown) => completion(vm, JSValue::empty()),
        Err(unported) => panic!("interpretador: {unported:?} ainda não portado"),
    }
}

/// O corredor dos geradores de golden que avaliam o programa no bun por `(0, eval)(src)` (o filho que lê o programa do
/// stdin, ou o próprio gerador) e depois leem a global `result_name`: o programa roda como eval indireto num VM recém-criado
/// (`var` e `function` ficam locais ao eval estrito, ou configuráveis no eval sloppy, `let` e `const` não vão para o
/// escopo global de declarações) e uma exceção não impede a leitura, que fica indefinida se o programa não chegou a
/// gravar. `drain_microtasks` espelha o filho que só lê o resultado depois de um `setTimeout(0)` do driver (as
/// microtarefas esvaziam antes da leitura); falso é o filho que lê logo depois do eval.
pub fn evaluate_indirect_eval_result(source: &str, result_name: &str, drain_microtasks: bool) -> Result<JSValue, JSValue> {
    evaluate_indirect_eval_result_units(&utf16_units(source), result_name, drain_microtasks)
}

/// `evaluate_indirect_eval_result` com o fonte em unidades UTF-16, que preservam o surrogate solitário.
pub fn evaluate_indirect_eval_result_units(source: &[u16], result_name: &str, drain_microtasks: bool) -> Result<JSValue, JSValue> {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        let _ = indirect_eval(&vm, &global_object, source);
        if drain_microtasks {
            vm.drain_microtasks();
        }
        evaluate(&global_object, &program_source(&indirect_eval_read_source(result_name, drain_microtasks)))
    })
}

/// A leitura que o filho do gerador faz depois do eval: com o `setTimeout(0)` do driver ele imprime
/// o resultado, então `R` não gravado vira o texto `undefined`. O `RESULT_PRELOAD` dos geradores captura o `globalThis`
/// antes do programa rodar, então um `var globalThis=1` (ou qualquer reatribuição) não muda de onde ele lê `R`: aqui
/// a leitura usa o `this` do topo do `Program`, que é o global e não passa pela propriedade `globalThis`. O filho que
/// lê logo devolve o valor cru.
fn indirect_eval_read_source(result_name: &str, drained: bool) -> String {
    if drained {
        format!("String(this.{result_name})")
    } else {
        format!("globalThis.{result_name}")
    }
}

#[cfg(test)]
mod indirect_eval_read_tests {
    use super::*;

    #[test]
    fn drained_read_stringifies_an_unset_result_like_the_child() {
        assert_eq!(indirect_eval_read_source("R", true), "String(this.R)");
        assert_eq!(indirect_eval_read_source("R", false), "globalThis.R");
    }

    /// O gerador de globais lê `globalThis.R`: com o script recusado pelo parser, `R` nunca foi gravado e a leitura
    /// devolve `undefined`, enquanto o identificador nu `R` lançaria `ReferenceError`.
    #[test]
    fn sequence_read_of_unset_global_property_is_undefined_not_a_throw() {
        let (errors, read) = evaluate_script_sequence_result(&["'use strict'; delete gx"], "x.js", "globalThis.R");
        assert_eq!(errors.len(), 1);
        assert!(read.expect("globalThis.R não lança").is_undefined());
        let (_, bare) = evaluate_script_sequence_result(&["1"], "x.js", "R");
        assert!(bare.is_err());
    }

    /// `Interpreter::executeEval` lança `Can't create duplicate variable in eval` só quando o nome resolve a um
    /// binding léxico (`let`, `const`, `class` do escopo global de declarações); um `var` de script anterior mora
    /// no global object e não colide. Casos medidos no bun 1.4.2 (`tests/golden/global_semantics_bun.tsv`).
    #[test]
    fn eval_var_collides_only_with_global_lexical_bindings() {
        let duplicate = "SyntaxError: Can't create duplicate variable in eval: 'ev'";
        for (script, errors) in [
            ("let ev = 1;\n(0, eval)('var ev = 2')", vec![format!("0:{duplicate}")]),
            ("const ev = 1;\n(0, eval)('var ev')", vec![format!("0:{duplicate}")]),
            ("class ev {}\n(0, eval)('var ev')", vec![format!("0:{duplicate}")]),
            ("let ev = 1;\n(0, eval)('function ev() {}')", vec![format!("0:{duplicate}")]),
            ("var ev = 1;\n(0, eval)('var ev = 2')", vec![]),
            ("(0, eval)('var ev = 1'); (0, eval)('var ev = 2')", vec![]),
        ] {
            let (actual, _) = evaluate_script_sequence_result(&[script], "x.js", "typeof R === 'undefined' ? undefined : R");
            assert_eq!(actual, errors, "{script}");
        }
    }
}

/// O que `runInThisContext` precisa do corredor: o global onde o programa roda e o texto que `readFileSync` devolve.
struct RunInThisContextHost {
    global_object: JSGlobalObjectRef,
    program: Vec<u16>,
}

thread_local! {
    static RUN_IN_THIS_CONTEXT: RefCell<Option<RunInThisContextHost>> = const { RefCell::new(None) };
}

/// O nome de arquivo do script chamador dos geradores (`case.js` no diretório temporário, que o gerador tira das saídas).
const RUN_IN_THIS_CONTEXT_CALLER_FILE: &str = "case.js";

/// `require` do script chamador: os embutidos do registro (`builtin_modules`, hoje `node:vm`) e, para qualquer outro id,
/// o `fs` de teste (só `readFileSync`, que devolve o programa em memória: o corredor não tem arquivo `case_source.js`).
const RUN_IN_THIS_CONTEXT_BOOT: &str =
    "(function (builtin, fs) { return function require(id) { var module = builtin(id); return module === undefined ? fs : module } })";

/// `builtin(id)` do `require` do corredor: o módulo embutido do registro, ou `undefined` se `id` não é embutido.
fn runner_builtin_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let id = call.argument(0);
    if id.is_string() {
        let request = rust_string(&id.to_wtf_string());
        // O `fs` do corredor é o de teste (o programa vem da memória), então `fs` não sai do registro aqui.
        if request.trim_start_matches("node:") == "fs" {
            return Ok(JSValue::undefined());
        }
        if let crate::api::builtin_modules::Resolved::Builtin(entry) = crate::api::builtin_modules::resolve(&request) {
            return crate::api::builtin_modules::load(global_object, entry);
        }
    }
    Ok(JSValue::undefined())
}
host_function!(runner_builtin, runner_builtin_body);

/// Instalador de `node:vm` no registro: só `runInThisContext`, que depende do corredor (`RUN_IN_THIS_CONTEXT`).
pub(crate) fn install_vm_module(global_object: &JSGlobalObject) -> HostResult {
    let vm_object = construct_empty_object(global_object);
    define_native(
        global_object.vm(),
        global_object,
        &vm_object,
        "runInThisContext",
        1,
        vm_run_in_this_context as NativeFunction,
        ImplementationVisibility::Public,
    );
    Ok(vm_object.as_value())
}

/// `fs.readFileSync(caminho, "utf8")` dos geradores: o texto do programa.
fn read_file_sync_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let text = RUN_IN_THIS_CONTEXT.with(|host| host.borrow().as_ref().map(|host| host.program.clone())).unwrap_or_default();
    Ok(JSValue::from_js_string(js_string(global_object.vm(), &wtf_from_units(&text))))
}
host_function!(fs_read_file_sync, read_file_sync_body);

/// `vm.runInThisContext(código, { filename })` do bun: avalia `código` como `Program` no global corrente, com o nome de
/// arquivo da opção (`file:///` quando não há, medido no bun 1.4.2). O fonte não passa pelo transpilador, então as
/// colunas das frames são as cruas do JSC (`set_raw_columns`). A exceção do programa segue pendente para o chamador.
fn run_in_this_context_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let vm = global_object.vm();
    let code = call.argument(0).to_wtf_string();
    let filename = match JSObject::from_value(&call.argument(1)) {
        Some(options) => {
            let value = options.get(vm, &PropertyName::from_identifier(&Identifier::from_span(vm, b"filename")));
            if value.is_string() { Some(String::from_utf8_lossy(&value.to_wtf_string().utf8(crate::wtf::text::conversion_mode::ConversionMode::LenientConversion)).into_owned()) } else { None }
        }
        None => None,
    };
    let Some(global) = RUN_IN_THIS_CONTEXT.with(|host| host.borrow().as_ref().map(|host| host.global_object.clone())) else {
        return Err(Thrown::Unported("runInThisContext fora do corredor"));
    };
    let url = filename.unwrap_or_default();
    let origin = SourceOrigin::new(URL::from_string(&WtfString::from_latin1(format!("file:///{url}").as_bytes())));
    let source = make_source(
        &code,
        &origin,
        SourceTaintedOrigin::Untainted,
        WtfString::from_latin1(if url.is_empty() { b"file:///".as_slice() } else { url.as_bytes() }),
        TextPosition::default(),
        SourceProviderSourceType::Program,
    );
    source.provider().expect("SourceCode sem provedor").set_raw_columns();
    let executable = ProgramExecutable::create(&global, &source);
    let result = vm.interpreter().execute_program(&executable, &global);
    if vm.exception().is_some() {
        return Err(Thrown::Pending);
    }
    Ok(result)
}
host_function!(vm_run_in_this_context, run_in_this_context_body);

/// O corredor dos geradores que avaliam o programa no bun por `vm.runInThisContext` chamado de um arquivo CJS:
///
/// ```text
/// try { require("node:vm").runInThisContext(require("node:fs").readFileSync(<arquivo>, "utf8")[, { filename: "<nome>" }]) } catch (e) {}
/// ```
///
/// O texto sai igual ao dos geradores (coluna 26 da linha 1), dentro do wrapper CJS do bun, e o programa roda por uma
/// função nativa `runInThisContext`. Por isso a pilha tem os frames do hospedeiro que o bun mostra: a frame nativa
/// `runInThisContext (unknown)` e a do chamador `<anonymous> (case.js:1:26)` (anônima, `isToplevel` falso, o `case.js`
/// remapeado como o `SavedSourceMap` do bun), e as frames do programa levam a coluna crua do JSC. `filename` é o da
/// opção (`None`: sem a opção, o nome é `file:///`). Depois esvazia as microtarefas e devolve a global `result_name`.
pub fn evaluate_run_in_this_context(source: &str, filename: Option<&str>, result_name: &str) -> Result<JSValue, JSValue> {
    evaluate_run_in_this_context_units(&utf16_units(source), filename, result_name)
}

/// `evaluate_run_in_this_context` com o fonte em unidades UTF-16, que preservam o surrogate solitário.
pub fn evaluate_run_in_this_context_units(source: &[u16], filename: Option<&str>, result_name: &str) -> Result<JSValue, JSValue> {
    evaluate_run_in_this_context_with_caller_units(source, filename, result_name, RUN_IN_THIS_CONTEXT_DEFAULT_CALLER)
}

/// O texto do chamador que a maioria dos geradores grava em `case.js`; `{options}` vira `, { filename: "<nome>" }` (ou nada).
pub const RUN_IN_THIS_CONTEXT_DEFAULT_CALLER: &str =
    "try { require(\"node:vm\").runInThisContext(require(\"node:fs\").readFileSync(\"case_source.js\", \"utf8\"){options}) } catch (e) {}\n";

/// Igual a `evaluate_run_in_this_context`, com o texto do corpo do chamador à escolha (`caller`, com o marcador `{options}`),
/// para os geradores cujo `case.js` difere do padrão (`catch` que grava `R`, `globalThis.vm = ...` numa linha antes).
pub fn evaluate_run_in_this_context_with_caller(source: &str, filename: Option<&str>, result_name: &str, caller: &str) -> Result<JSValue, JSValue> {
    evaluate_run_in_this_context_with_caller_units(&utf16_units(source), filename, result_name, caller)
}

/// `evaluate_run_in_this_context_with_caller` com o fonte em unidades UTF-16.
pub fn evaluate_run_in_this_context_with_caller_units(source: &[u16], filename: Option<&str>, result_name: &str, caller: &str) -> Result<JSValue, JSValue> {
    let options = filename.map_or(String::new(), |name| format!(", {{ filename: {} }}", json_quote(name)));
    evaluate_with_cjs_caller(source, RUN_IN_THIS_CONTEXT_CALLER_FILE, &caller.replace("{options}", &options), result_name, true)
}

/// O texto do arquivo filho de `scripts/gen-builtin-shape-golden.js` sem as duas últimas linhas (`process.stdout.write` e
/// `process.exit`, que o corredor não precisa: ele lê a global `R` depois).
pub const INDIRECT_EVAL_SHAPE_CALLER: &str = "const fs = require(\"fs\");\n(0, eval)(fs.readFileSync(0, \"utf8\"));\n";

/// O corredor dos geradores que avaliam o programa no bun por `(0, eval)(fs.readFileSync(0, "utf8"))` chamado de um arquivo
/// CJS filho (`caller_path`, o caminho absoluto em que o gerador o grava, com o texto `caller_text` do arquivo). O `eval`
/// é o do próprio global (o builtin, não um atalho): a frame nativa `eval (unknown)` vem dele e o código do programa
/// herda a origem do chamador (`file://<caller_path>`), então o `stack` e o `sourceURL` do programa mostram o arquivo filho
/// e a pilha termina em `<anonymous> (<caller_path>:L:C)`, o chamador remapeado como o `SavedSourceMap` do bun. Depois
/// (se `drain`) esvazia as microtarefas e devolve a global `result_name`.
pub fn evaluate_indirect_eval_with_caller(source: &str, caller_path: &str, caller_text: &str, result_name: &str, drain: bool) -> Result<JSValue, JSValue> {
    evaluate_with_cjs_caller(&utf16_units(source), caller_path, caller_text, result_name, drain)
}

/// O que `runInThisContext` e o eval indireto têm em comum: o corpo do chamador (`caller`, o texto do arquivo CJS) roda
/// dentro do wrapper CJS do bun, num arquivo de nome `caller_file`, com um `require` que devolve o `node:vm` (com
/// `runInThisContext`) para `node:vm` e o `fs` (com `readFileSync`, que devolve `source`) para qualquer outro id.
fn evaluate_with_cjs_caller(source: &[u16], caller_file: &str, caller: &str, result_name: &str, drain: bool) -> Result<JSValue, JSValue> {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        RUN_IN_THIS_CONTEXT.with(|host| *host.borrow_mut() = Some(RunInThisContextHost { global_object: global_object.clone(), program: source.to_vec() }));
        struct ClearHost;
        impl Drop for ClearHost {
            fn drop(&mut self) {
                let taken = RUN_IN_THIS_CONTEXT.try_with(|host| host.borrow_mut().take());
                drop(taken);
            }
        }
        let _clear = ClearHost;
        let identifier = |name: &str| Identifier::from_span(&vm, name.as_bytes());
        let builtin_holder = construct_empty_object(&global_object);
        let builtin = define_native(&vm, &global_object, &builtin_holder, "builtin", 1, runner_builtin as NativeFunction, ImplementationVisibility::Public)
            .as_value();
        let fs_object = construct_empty_object(&global_object);
        define_native(&vm, &global_object, &fs_object, "readFileSync", 1, fs_read_file_sync as NativeFunction, ImplementationVisibility::Public);
        let boot = evaluate(&global_object, &program_source(RUN_IN_THIS_CONTEXT_BOOT)).expect("bootstrap de require lançou");
        let boot_data = get_call_data(boot.clone());
        let require = match call_returning_exception(&global_object, boot, &boot_data, JSValue::undefined(), &[builtin, fs_object.as_value()]) {
            Ok(Ok(require)) => require,
            Ok(Err(_)) => panic!("bootstrap de require lançou"),
            Err(unported) => panic!("interpretador: {unported:?} ainda não portado"),
        };
        let header = "(function(exports, require, module, __filename, __dirname) {\n";
        let text = format!("{header}{caller}}})");
        let origin_path = if caller_file.starts_with('/') { caller_file.to_string() } else { format!("/{caller_file}") };
        let origin = SourceOrigin::new(URL::from_string(&WtfString::from_latin1(format!("file://{origin_path}").as_bytes())));
        let named = transpiled_source(&WtfString::from_utf8(text.as_bytes()), &origin, WtfString::from_utf8(caller_file.as_bytes()));
        // O corpo do chamador começa na linha 1 do arquivo do bun; o porte o tem atrás do cabeçalho do wrapper, então
        // o deslocamento de linha é o número de quebras do cabeçalho, calculado do texto montado. As colunas são as
        // do próprio texto (medido no bun 1.4.2: `globalThis.vm = ...;\ntry { vm.runInThisContext(` dá `case.js:2:10`
        // e o padrão `try { require("node:vm").runInThisContext(` dá `case.js:1:26`, ambos a coluna do nome do método).
        let header_lines = i64::try_from(header.matches('\n').count()).expect("cabeçalho do wrapper");
        let map = PositionMap::new(&[1, 1, 0, 0], header_lines).expect("mapa de posições");
        named.provider().expect("SourceCode sem provedor").set_position_map(Rc::new(map));
        if let Ok(wrapper) = evaluate(&global_object, &named) {
            let exports = construct_empty_object(&global_object).as_value();
            let call_data = get_call_data(wrapper.clone());
            let module = construct_empty_object(&global_object).as_value();
            let filename_value = JSValue::from_js_string(js_string(&vm, &WtfString::from_utf8(caller_file.as_bytes())));
            let dirname_value = JSValue::from_js_string(js_string(&vm, &WtfString::from_utf8(b".")));
            if let Err(unported) =
                call_returning_exception(&global_object, wrapper, &call_data, exports.clone(), &[exports, require, module, filename_value, dirname_value])
            {
                panic!("interpretador: {unported:?} ainda não portado");
            }
        }
        if drain {
            vm.drain_microtasks();
        }
        read_global_result(&global_object, result_name)
    })
}

#[cfg(test)]
mod hidden_program_tests {
    use super::*;
    use crate::wtf::text::conversion_mode::ConversionMode;

    /// Roda `prelude` como programa escondido e `page` como script da página `page.js`; devolve o texto de `page`.
    fn run(prelude: &str, page: &str) -> String {
        run_program(|| {
            let (vm, global_object) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            crate::runtime::native_class_support::install_global_function_with_visibility(
                &global_object,
                "__pl_private",
                1,
                crate::runtime::js_global_object_private_functions::create_private_symbol,
                ImplementationVisibility::Private,
            );
            evaluate_hidden_program(&global_object, prelude).unwrap_or_else(|error| panic!("prelúdio: {}", describe_exception(&error)));
            let value = evaluate(&global_object, &named_program_source(page, "page.js")).unwrap_or_else(|error| panic!("página: {}", describe_exception(&error)));
            String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
        })
    }

    const PRELUDE: &str = "function storeOf(x) { if (x == null) throw new TypeError('bad'); return x; } \
        globalThis.thing = { add(x) { return storeOf(x); }, get size() { return 1; } }; \
        globalThis.make = function make(f) { return f(); };";

    #[test]
    fn functions_of_the_hidden_program_print_as_native() {
        let shown = run(PRELUDE, "[thing.add.toString(), Object.getOwnPropertyDescriptor(thing, 'size').get.toString(), make.toString(), (() => 1).toString()].join('|')");
        assert_eq!(shown, "function add() { [native code] }|function get size() { [native code] }|function make() { [native code] }|() => 1");
    }

    #[test]
    fn frames_of_the_hidden_program_leave_the_stack() {
        let shown = run(PRELUDE, "function page() { try { thing.add(null); } catch (e) { return e.stack; } }\npage()");
        assert!(!shown.contains("storeOf") && !shown.contains("add"), "{shown}");
        assert!(shown.contains("page@") || shown.contains("at page"), "{shown}");
        let callback = run(PRELUDE, "make(function inner() { return new Error('x').stack; })");
        assert!(!callback.contains("make"), "{callback}");
    }

    /// O acesso nomeado da janela, com o caso medido no Chromium 154 (`<div id=h><form name=f><p id=two><p id=two>`
    /// e `var v` com `<div id=v>`): declaração vem antes, ausente segue o `ReferenceError` e o `typeof`.
    #[test]
    fn global_named_lookup_answers_after_the_whole_chain() {
        let shown = run_program(|| {
            let (vm, global_object) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            let hook = evaluate_hidden_program(
                &global_object,
                "const named = { h: { localName: 'div' }, f: { localName: 'form' }, two: [1, 2], v: { localName: 'div' } }; \
                 (function (name) { return Object.hasOwn(named, name) ? named[name] : undefined; })",
            )
            .unwrap_or_else(|error| panic!("prelúdio: {}", describe_exception(&error)));
            crate::runtime::global_named_lookup::set_global_named_lookup(&global_object, hook);
            let page = "var v = 1; let missing; try { missing = nope; } catch (e) { missing = e.name; } \
                        [h.localName, window.f.localName, two.length, v, 'h' in window, typeof nope, missing, globalThis.toString === Object.prototype.toString].join('|')";
            let value = evaluate(&global_object, &named_program_source(&format!("var window = globalThis; {page}"), "page.js"))
                .unwrap_or_else(|error| panic!("página: {}", describe_exception(&error)));
            String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
        });
        assert_eq!(shown, "div|form|2|1|true|undefined|ReferenceError|true");
    }

    /// O `createPrivateSymbol` do JSC entregue ao prelúdio: a chave privada não aparece para a página por nenhum
    /// caminho de enumeração nem de cópia, e o acesso por ela não passa pelas armadilhas de um Proxy.
    #[test]
    fn private_symbols_of_the_host_stay_invisible() {
        let prelude = "const P = __pl_private('shadow'); delete globalThis.__pl_private; \
            globalThis.el = { a: 1 }; el[P] = 42; globalThis.readEl = () => el[P]; \
            globalThis.trapped = []; const t = (name) => function () { trapped.push(name); return Reflect[name](...arguments); }; \
            const proxy = new Proxy({}, { get: t('get'), set: t('set'), has: t('has'), defineProperty: t('defineProperty'), getOwnPropertyDescriptor: t('getOwnPropertyDescriptor') }); \
            proxy[P] = 5; proxy[P]; P in proxy;";
        let page = "[typeof __pl_private, Reflect.ownKeys(el).length, Object.getOwnPropertySymbols(el).length, Object.getOwnPropertyNames(el).length, \
            JSON.stringify(el), Object.keys(Object.getOwnPropertyDescriptors(el)).length, Reflect.ownKeys(structuredClone(el)).length, \
            Object.keys(Object.assign({}, el)).length, Reflect.ownKeys({ ...el }).length, readEl(), trapped.length].join('|')";
        assert_eq!(run(prelude, page), "undefined|1|0|1|{\"a\":1}|1|1|1|1|42|0");
    }

    /// O nome privado mora no próprio objeto: num `Proxy` não chama armadilha nem toca o alvo, e grava em objeto
    /// congelado.
    #[test]
    fn private_symbols_store_on_proxies_and_frozen_objects() {
        let prelude = "const P = __pl_private('x'); delete globalThis.__pl_private; \
            globalThis.trapped = []; const t = (name) => function () { trapped.push(name); return Reflect[name](...arguments); }; \
            globalThis.target = {}; globalThis.proxy = new Proxy(target, { get: t('get'), set: t('set'), has: t('has'), \
              deleteProperty: t('deleteProperty'), defineProperty: t('defineProperty'), getOwnPropertyDescriptor: t('getOwnPropertyDescriptor'), ownKeys: t('ownKeys') }); \
            proxy[P] = 1; globalThis.fromProxy = [proxy[P], P in proxy]; \
            globalThis.frozen = Object.freeze({ k: 0 }); frozen[P] = 2; globalThis.fromFrozen = [frozen[P], P in frozen]; \
            delete proxy[P]; globalThis.afterDelete = [proxy[P], P in proxy];";
        let page = "trapped.length = 0; const keys = Reflect.ownKeys(proxy).length; \
            [fromProxy.join(), fromFrozen.join(), afterDelete.join(), keys, Reflect.ownKeys(target).length, Object.isFrozen(frozen), \
             Reflect.ownKeys(frozen).length, trapped.join()].join('|')";
        assert_eq!(run(prelude, page), "1,true|2,true|,false|0|0|true|1|ownKeys");
    }
}

#[cfg(test)]
mod realm_tests {
    use super::*;
    use crate::wtf::text::conversion_mode::ConversionMode;

    fn show(value: JSValue) -> String {
        String::from_utf8_lossy(&value.to_wtf_string().utf8(ConversionMode::LenientConversion)).into_owned()
    }

    /// Dois globais no mesmo `VM`: cada um avalia o seu script, e o primeiro segue vivo depois do segundo.
    #[test]
    fn two_globals_share_a_vm() {
        let shown = run_program(|| {
            let (vm, first) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            let second = new_global_object_in(&vm);
            let a = evaluate(&first, &named_program_source("globalThis.mark = [1]; typeof Array", "a.js"));
            let b = evaluate(&second, &named_program_source("typeof Array", "b.js"));
            let c = evaluate(&first, &named_program_source("1 + 1", "c.js"));
            (a.map(show), b.map(show), c.map(show))
        });
        assert_eq!(shown.0.ok().as_deref(), Some("function"));
        assert_eq!(shown.1.ok().as_deref(), Some("function"));
        assert_eq!(shown.2.ok().as_deref(), Some("2"));
    }

    /// O frame enxerga o global da página por uma propriedade posta pelo hospedeiro (o `window.parent`), e os
    /// construtores dos dois realms são distintos: `parent.Array !== Array`.
    #[test]
    fn a_second_global_reaches_the_first() {
        let shown = run_program(|| {
            let (vm, first) = new_global_object();
            let _finalizer = VmFinalizer::new(&vm);
            let second = new_global_object_in(&vm);
            let outer = first.global_this().expect("globalThis").as_value();
            let name = PropertyName::from_identifier(&Identifier::from_span(&vm, b"parent"));
            second.put_direct(&vm, &name, outer, 0);
            evaluate(&first, &named_program_source("globalThis.mark = 7", "a.js")).ok();
            let a = evaluate(&second, &named_program_source("parent.mark + '|' + (parent.Array !== Array) + '|' + (parent.eval('[]') instanceof Array)", "b.js"));
            a.map(show)
        });
        assert_eq!(shown.ok().as_deref(), Some("7|true|false"));
    }
}
