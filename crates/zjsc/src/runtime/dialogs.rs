//! `alert`, `confirm` e `prompt` do global. O JavaScriptCore não os define: quem os instala é o bun (WebCore),
//! como propriedades de dados comuns (`writable`, `enumerable`, `configurable`), `length` 1, sem `prototype`, e
//! `new` é `TypeError`. Na ordem de chaves do bun: `alert` depois de `addEventListener`, `confirm` depois de
//! `clearTimeout`, `prompt` depois de `postMessage`.
//!
//! Sem terminal interativo (stdin em EOF, o único caso do sandbox), medido no bun 1.4.2: `alert` devolve
//! `undefined`, `confirm` devolve `false`, `prompt` devolve `null`. Os argumentos são convertidos com
//! `ToString` (o `toString` do objeto roda, Symbol lança `TypeError`, o erro do `toString` propaga): `alert` e
//! `confirm` só olham o primeiro, `prompt` converte também o segundo (o valor padrão), mesmo com o primeiro
//! `undefined`. Um terceiro argumento é ignorado.
//!
//! O console vem do host (`console_host.rs`). Medido no bun 1.4.2: o convite sai no stdout, sem quebra de linha,
//! depois da conversão dos argumentos, e então uma linha do stdin é lida.
//!
//! - convite: a mensagem (`ToString` do primeiro argumento; sem argumentos, `Alert`, `Confirm` ou `Prompt`) mais
//!   ` [Enter] ` (alert), ` [y/N] ` (confirm) ou ` ` (prompt). O prompt com dois ou mais argumentos põe ` [padrão] `,
//!   com o `ToString` do segundo (`undefined` explícito vira `[undefined]`).
//! - surrogate solto na mensagem ou no padrão: o stdout recebe U+FFFD (`EF BF BD`) por unidade solta (o `Lenient` do
//!   WTF); o par válido vira os 4 bytes UTF-8. O `prompt` devolve o padrão intacto (com o surrogate solto).
//! - a linha acaba em `\n`; um `\r` imediatamente antes é descartado. EOF, inclusive no meio de uma linha, vale
//!   como "sem resposta": `confirm` `false`, `prompt` `null`. `alert` ignora a linha.
//! - `confirm` devolve `true` só para `y` ou `Y` exatos.
//! - `prompt` com linha vazia devolve o próprio segundo argumento (o valor, não o `ToString` dele: `5` volta número,
//!   o objeto volta o objeto) quando há segundo argumento, senão `null`. A mensagem e o espaço saem antes da conversão
//!   do padrão: um `toString` que lança deixa `mensagem ` no stdout.

use crate::host_function;
use crate::runtime::host_call::{pending_or, HostCall, HostResult, Thrown};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_module_loader::rust_string;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::wtf::text::wtf_string::String as WtfString;

/// O texto do argumento `index` (convertido com `ToString`, que roda mesmo para `undefined`), ou `fallback` se faltou.
fn text_argument(global_object: &JSGlobalObject, call: &HostCall, index: usize, fallback: &str) -> Result<String, Thrown> {
    if index >= call.argument_count() {
        return Ok(fallback.to_string());
    }
    Ok(rust_string(&string_argument(global_object, call, index)?))
}

/// O `ToString` do argumento `index`, ainda em UTF-16 (um surrogate solto fica como está).
fn string_argument(global_object: &JSGlobalObject, call: &HostCall, index: usize) -> Result<WtfString, Thrown> {
    pending_or(global_object, call.argument(index).to_wtf_string())
}

/// Escreve o convite e lê a resposta, já sem o `\r` final; `None` em EOF.
fn ask(global_object: &JSGlobalObject, invitation: &str) -> Option<String> {
    let host = global_object.console_host();
    host.as_ref()?.write_stdout(invitation.as_bytes());
    let line = host?.read_stdin_line()?;
    Some(line.strip_suffix('\r').map(str::to_string).unwrap_or(line))
}

fn alert_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let message = text_argument(global_object, call, 0, "Alert")?;
    ask(global_object, &format!("{message} [Enter] "));
    Ok(JSValue::undefined())
}

fn confirm_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let message = text_argument(global_object, call, 0, "Confirm")?;
    let answer = ask(global_object, &format!("{message} [y/N] "));
    Ok(JSValue::Bool(matches!(answer.as_deref(), Some("y" | "Y"))))
}

fn prompt_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let message = text_argument(global_object, call, 0, "Prompt")?;
    // Medido no bun 1.4.2: `mensagem ` sai antes da conversão do padrão (que pode lançar), e `[padrão] ` depois.
    if let Some(host) = global_object.console_host() {
        host.write_stdout(format!("{message} ").as_bytes());
    }
    let default = call.argument(1);
    let shown = if call.argument_count() >= 2 { format!("[{}] ", rust_string(&string_argument(global_object, call, 1)?)) } else { String::new() };
    let Some(answer) = ask(global_object, &shown) else { return Ok(JSValue::null()) };
    match (answer.is_empty(), call.argument_count() >= 2) {
        (true, false) => Ok(JSValue::null()),
        // Linha vazia: o padrão volta como o JS o entregou (o próprio valor, não o texto dele).
        (true, true) => Ok(default),
        (false, _) => Ok(JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf8(answer.as_bytes())))),
    }
}

host_function!(global_func_alert, alert_body);
host_function!(global_func_confirm, confirm_body);
host_function!(global_func_prompt, prompt_body);

/// Instala `alert` no global.
pub fn add_alert(global_object: &JSGlobalObject) {
    crate::runtime::native_class_support::install_global_function(global_object, "alert", 1, global_func_alert);
}

/// Instala `confirm` no global.
pub fn add_confirm(global_object: &JSGlobalObject) {
    crate::runtime::native_class_support::install_global_function(global_object, "confirm", 1, global_func_confirm);
}

/// Instala `prompt` no global.
pub fn add_prompt(global_object: &JSGlobalObject) {
    crate::runtime::native_class_support::install_global_function(global_object, "prompt", 1, global_func_prompt);
}
