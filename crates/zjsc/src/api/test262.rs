//! Corredor do test262: um programa (o harness, os `includes` e o teste, montados pelo `scripts/test262-run.py`) roda num realm
//! novo, e o resultado sai como dado: a exceção que escapou (descrita como `Construtor: mensagem`, inclusive `Test262Error`, que
//! não herda de `Error`) e o que o teste imprimiu com `print` (o canal do `doneprintHandle.js` e do marcador de fase).
//!
//! `print` e o objeto `$262` (`global`, `evalScript`, `gc`, `detachArrayBuffer`, `createRealm`) são do prelúdio e de uma função
//! nativa: `createRealm` cria outro `JSGlobalObject` no mesmo `VM` (o que o `ShadowRealm` já faz) com o mesmo prelúdio, e devolve o
//! `$262` dele. O que o motor ou o corredor não têm (`agent`, `IsHTMLDDA`) falta de verdade e o teste falha, que é a medida que se quer.
//! Os módulos que o teste importa (`*_FIXTURE.js`) vêm em memória, pela chave relativa à raiz `test/` do test262.

use crate::api::eval::{define_native, evaluate, named_program_source, new_global_object, program_source, read_global_result, VmFinalizer};
use crate::host_function;
use crate::runtime::cell_registry::run_program;
use crate::runtime::host_call::{HostCall, HostResult};
use crate::runtime::identifier::Identifier;
use crate::runtime::implementation_visibility::ImplementationVisibility;
use crate::runtime::js_global_object::{JSGlobalObject, JSGlobalObjectRef};
use crate::runtime::js_object::JSObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::native_function::NativeFunction;
use crate::runtime::string_regexp_support::get_object_property;
use crate::wtf::text::conversion_mode::ConversionMode;

/// O que um programa do test262 deixou.
pub struct Test262Outcome {
    /// A exceção que escapou, `Construtor: mensagem`; `None` quando o programa terminou.
    pub thrown: Option<String>,
    /// As linhas de `print`, uma por linha.
    pub output: String,
}

const PRELUDE: &str = "var __t262_out = [];
var print = function print() { __t262_out.push(Array.prototype.join.call(arguments, ' ')); };
var $262 = {
  global: globalThis,
  gc: function () {},
  evalScript: function (code) { return (0, eval)(String(code)); },
  detachArrayBuffer: function (buffer) { if (typeof buffer.transfer === 'function') buffer.transfer(); else throw new TypeError('detachArrayBuffer'); },
};";

const DESCRIBE: &str = "(function (e) {
  try {
    if (e === null || (typeof e !== 'object' && typeof e !== 'function')) return typeof e + ': ' + String(e);
    var c = e.constructor;
    return (c && c.name || 'object') + ': ' + String(e.message);
  } catch (x) { return 'unformattable: ' + typeof e; }
})";

fn text_of(value: JSValue) -> Option<String> {
    value.is_string().then(|| String::from_utf8_lossy(&value.as_js_string().value().utf8(ConversionMode::LenientConversion)).into_owned())
}

fn describe(global_object: &JSGlobalObjectRef, exception: JSValue) -> String {
    let text = evaluate(global_object, &program_source(DESCRIBE)).ok().and_then(|function| {
        crate::runtime::call_data::call_with_error_message(global_object, function, JSValue::undefined(), &[exception], "descrição do erro").ok()
    });
    text.and_then(text_of).unwrap_or_else(|| {
        global_object.vm().clear_exception();
        crate::api::eval::describe_exception(&exception)
    })
}

fn output_of(global_object: &JSGlobalObjectRef) -> String {
    read_global_result(global_object, "__t262_out.join('\\n')").ok().and_then(text_of).unwrap_or_default()
}

/// O `$262` do realm `global_object`, depois do prelúdio.
fn host_object(global_object: &JSGlobalObject) -> Option<JSValue> {
    let global_this = global_object.global_this()?;
    get_object_property(global_object, global_this.as_value(), &Identifier::from_span(global_object.vm(), b"$262")).ok()
}

/// O prelúdio (`print`, `$262`) e o `$262.createRealm` nativo no realm `global_object`.
fn install_host(global_object: &JSGlobalObjectRef) {
    let _ = evaluate(global_object, &program_source(PRELUDE));
    if let Some(host) = host_object(global_object).as_ref().and_then(JSObject::from_value) {
        define_native(global_object.vm(), global_object, &host, "createRealm", 0, create_realm as NativeFunction, ImplementationVisibility::Public);
    }
}

/// `$262.createRealm()`: outro `JSGlobalObject` no mesmo `VM`, com o mesmo prelúdio; devolve o `$262` dele.
fn create_realm_body(global_object: &JSGlobalObject, _call: &HostCall) -> HostResult {
    let realm = JSGlobalObject::init(&global_object.vm_rc());
    install_host(&realm);
    Ok(host_object(&realm).unwrap_or_else(JSValue::undefined))
}
host_function!(create_realm, create_realm_body);

/// Roda `source` como script global (o modo estrito vem da diretiva no começo do próprio texto), esvazia as microtarefas e o laço
/// de eventos virtual, e devolve a exceção e as linhas impressas. `url` é a chave do teste em `modules` (os `import()` dele resolvem
/// contra elas).
pub fn run_script(source: &str, url: &str, modules: &[(String, String)]) -> Test262Outcome {
    run_program(|| {
        let (vm, global_object) = new_global_object();
        let _finalizer = VmFinalizer::new(&vm);
        // O referrer do script é `file:///<url>`, absoluto: as chaves dos módulos em memória levam a barra inicial.
        let rooted: Vec<(String, String)> = modules.iter().map(|(key, text)| (format!("/{key}"), text.clone())).collect();
        crate::api::module::install_memory_modules(&global_object, &rooted);
        install_host(&global_object);
        let thrown = evaluate(&global_object, &named_program_source(source, url)).err().map(|exception| describe(&global_object, exception));
        vm.drain_microtasks();
        crate::runtime::timers::run_event_loop(&global_object);
        Test262Outcome { thrown, output: output_of(&global_object) }
    })
}

/// Roda o mapa de módulos `files` (chave, fonte) a partir de `entry`, depois de `global_script` (o harness, que vê o `L` do corredor
/// de módulos: o `print` do harness é `L`, e as linhas impressas são o `log`); `thrown` é o erro que rejeitou a avaliação (parser, ligação ou execução).
pub fn run_module(files: &[(String, String)], entry: &str, global_script: &str) -> Test262Outcome {
    let outcome = crate::api::module::evaluate_module_map_after_script(files, entry, global_script);
    let lines: Vec<String> = log_lines(&outcome.log_json);
    Test262Outcome { thrown: outcome.error, output: lines.join("\n") }
}

/// O `log` (`["a","b"]`) em linhas, sem depender de uma crate de JSON: só strings com os escapes de `JSON.stringify`.
fn log_lines(json: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut chars = json.trim().trim_start_matches('[').trim_end_matches(']').chars().peekable();
    while let Some(c) = chars.next() {
        if c != '"' {
            continue;
        }
        let mut line = String::new();
        while let Some(c) = chars.next() {
            match c {
                '"' => break,
                '\\' => match chars.next() {
                    Some('n') => line.push('\n'),
                    Some('t') => line.push('\t'),
                    Some('r') => line.push('\r'),
                    Some('u') => {
                        let hex: String = chars.by_ref().take(4).collect();
                        line.push(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32).unwrap_or('\u{fffd}'));
                    }
                    Some(other) => line.push(other),
                    None => {}
                },
                other => line.push(other),
            }
        }
        lines.push(line);
    }
    lines
}
