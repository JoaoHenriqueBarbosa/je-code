//! O `ConsoleClient` do sandbox: formata os argumentos de `console.log/info/debug` (stdout) e `console.error/warn`
//! (stderr) como o bun 1.4.2 e escreve no `ConsoleHost` do global. Instalado por `set_console_host`.
//!
//! Medido no bun (golden `console_primitive_bun.tsv`), só para argumentos primitivos:
//! - uma exceção (`%s` com Symbol, `%d` com BigInt, `%j` com BigInt) propaga ao chamador de `console.*`, e o texto já
//!   montado até ali (sem o `\n`) é escrito antes (`"[%s]"` com Symbol escreve `[`);
//! - `console.error()` e `console.warn()` sem argumentos escrevem `\n` no stdout, não no stderr;
//! - cada argumento sozinho: string crua, número como `ToString` mas `-0` vira `-0`, BigInt com `n`, Symbol como
//!   `Symbol(descrição)`, `null`, `undefined` e booleanos por extenso; separação por um espaço, `\n` no fim;
//! - toda string com argumentos depois dela vale como formato, não só a primeira (`console.log(1, "%s", 2)` escreve
//!   `1 2`) e consome os argumentos seguintes; enquanto restam argumentos, `%s` é
//!   `ToString` (Symbol lança), `%d` e `%i` são `ToNumber` (Symbol vira `NaN`, BigInt lança) e depois o inteiro lido
//!   do texto do número (`1e21` vira `1`, `1e20` satura em `i64::MAX`, `Infinity` vira `NaN`), `%f` é `ToNumber` e
//!   `ToString` (`-0` vira `0`), `%o` e `%O` são o argumento sozinho, `%j` é `JSON.stringify` (`undefined` vira
//!   vazio), `%c` consome o argumento e não escreve nada;
//! - `%%` vira `%` e o caractere logo depois NUNCA começa uma substituição (`%%%s` com um argumento sai `%%s 1`);
//! - `%` seguido de outra coisa, ou no fim, fica literal; sem argumentos restantes o resto do formato fica como está
//!   (`%s%%` com um argumento sai `1%%`).
//!
//! - `%d`/`%i` de `0 < |n| < 1e-6` não leem o texto: multiplicam por 10 até chegar a 1 e truncam (`5e-324` dá 4);
//! - `count` escreve `rótulo: n` no stdout e `countReset` zera; `time` guarda o instante (um segundo `time` do mesmo
//!   rótulo não reinicia); `timeLog`/`timeEnd` escrevem no stderr `[tempo] rótulo args...` (args crus, sem formato,
//!   nada sem cronômetro, `timeEnd` o apaga); o tempo é `0.00ms` até 1500 ms e `1.60s` depois;
//! - `group` recua 2 espaços por nível só a primeira linha de cada chamada (nada de recuo sem argumentos); a etiqueta
//!   sai no recuo atual, nada sem argumentos, e o nível sobe mesmo se a formatação lançou; `groupCollapsed` escreve
//!   a etiqueta (`undefined` sem recuo se não há argumentos) mas NÃO sobe o nível; `groupEnd` desce sempre, sem passar de 0;
//!   `count`, `time*` e `Assertion failed` não levam recuo; `assert` falso escreve `Assertion failed` sem argumentos,
//!   senão os argumentos como `console.error` (sem o prefixo).
//!
//! LACUNAS: arrays e objetos comuns saem de `console_format.rs` (que tem as próprias lacunas: função, `Date`, `Map`...),
//! `%s` de objeto (`[String: "a"]`, função, classe), `colors: true` de `dir` com `Error` (o relato de erro ainda sai
//! sem cores; o resto do formatador pinta como o `pretty_fmt` do bun), e o tipo `clear`, ... ainda não
//! escreve nada. `table` está em `console_table.rs`. O recuo de grupo é o `indent` inicial do [`Formatter`] (`default_indent` do bun).
//! `dir` (golden `console_dir_bun.tsv`) imprime só o primeiro argumento se há opções, `dirxml` é como `log`, `trace`
//! escreve no stdout os argumentos e um `      at ...` por frame. O estado (contadores, cronômetros, recuo) é uma instância por thread, zerada
//! em cada `install`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::interpreter::call_frame::CallFrame;
use crate::runtime::cell_registry::{self, CellEntry};
use crate::runtime::console_table;
use crate::runtime::host_call::{throw_thrown, Thrown};
use crate::runtime::identifier::Identifier;
use crate::runtime::js_array::JSArray;
use crate::runtime::string_regexp_support::get_object_property;
use crate::runtime::console_format::{visible_len, Formatter};
use crate::runtime::console_object::{ConsoleClient, MessageLevel, MessageType, ScriptArguments};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_value::JSValue;
use crate::runtime::json_object::{json_stringify, throw_json_error};
use crate::runtime::symbol::as_symbol;
use crate::wtf::text::wtf_string::String as WtfString;

/// O estado do console: contadores de `count`, cronômetros de `time` e o nível de `group`.
#[derive(Default)]
struct ConsoleState {
    counts: HashMap<Vec<u16>, u64>,
    timers: HashMap<Vec<u16>, u64>,
    depth: usize,
}

/// O cliente do sandbox: lê o `ConsoleHost` do global a cada chamada; o estado é zerado em cada `install`.
#[derive(Default)]
pub struct HostConsoleClient {
    state: RefCell<ConsoleState>,
}

thread_local! {
    static CLIENT: Rc<HostConsoleClient> = Rc::new(HostConsoleClient::default());
    /// O observador das chamadas de `console` (o `Runtime.consoleAPICalled` do DevTools do navegador).
    static OBSERVER: RefCell<Option<ConsoleObserver>> = const { RefCell::new(None) };
}

/// Recebe o tipo da chamada no vocabulário do protocolo do DevTools (`log`, `warning`, `count`, `timeEnd`...) e os
/// argumentos. `count` chega com o rótulo e o total; `timeEnd` e `timeLog` com o rótulo, os milissegundos e o resto.
pub type ConsoleObserver = Rc<dyn Fn(&JSGlobalObject, &str, &[JSValue])>;

/// Instala (ou tira, com `None`) o observador da thread. A escrita no `ConsoleHost` segue igual.
pub fn set_console_observer(observer: Option<ConsoleObserver>) {
    OBSERVER.with(|slot| *slot.borrow_mut() = observer);
}

fn observe(global_object: &JSGlobalObject, kind: &str, arguments: &[JSValue]) {
    let observer = OBSERVER.with(|slot| slot.borrow().clone());
    if let Some(observer) = observer {
        observer(global_object, kind, arguments);
    }
}

fn observed_arguments(arguments: &ScriptArguments) -> Vec<JSValue> {
    (0..arguments.argument_count()).map(|index| arguments.argument_at(index)).collect()
}

fn label_value(global_object: &JSGlobalObject, label: &WtfString) -> JSValue {
    JSValue::from_js_string(crate::runtime::js_string::js_string(global_object.vm(), label))
}

/// Instala o cliente no global (o `Weak` do global aponta para a instância única da thread). O estado da instância
/// (contadores, cronômetros, recuo) recomeça vazio, como num processo novo.
pub fn install(global_object: &JSGlobalObject) {
    CLIENT.with(|client| {
        *client.state.borrow_mut() = ConsoleState::default();
        let client: Rc<dyn ConsoleClient> = client.clone();
        global_object.set_console_client(Rc::downgrade(&client));
    });
}

/// O tempo de `console.timeLog/timeEnd`: milissegundos com duas casas até 1500 ms, segundos com duas casas depois.
fn format_elapsed(milliseconds: f64) -> String {
    if milliseconds <= 1500.5 {
        format!("{milliseconds:.2}ms")
    } else {
        format!("{:.2}s", milliseconds / 1000.0)
    }
}

fn units(string: &WtfString) -> Vec<u16> {
    string.characters_without_null_termination().unwrap_or_default()
}

fn push_ascii(out: &mut Vec<u16>, text: &str) {
    out.extend(text.bytes().map(u16::from));
}

fn has_exception(global_object: &JSGlobalObject) -> bool {
    global_object.vm().exception().is_some()
}

/// O argumento sozinho (fora de formato, `%o` e `%O`). `false` com exceção pendente.
pub(crate) fn push_plain(global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> bool {
    if value.is_number() {
        let number = value.as_number();
        if number == 0.0 && number.is_sign_negative() {
            push_ascii(out, "-0");
            return true;
        }
    } else if value.is_big_int() {
        let text = value.to_wtf_string();
        out.extend(units(&text));
        out.push(u16::from(b'n'));
        return !has_exception(global_object);
    } else if value.is_symbol() {
        let text = as_symbol(value).try_get_descriptive_string().unwrap_or_default();
        out.extend(units(&text));
        return true;
    }
    let text = value.to_wtf_string();
    out.extend(units(&text));
    !has_exception(global_object)
}

/// Abaixo disto (em módulo) o bun não lê o texto do número: tira o primeiro dígito por multiplicações por 10.
const TINY_INTEGER_LIMIT: f64 = 1e-6;

/// O inteiro de `%d`/`%i` para `0 < |number| < 1e-6`: multiplica por 10 até chegar a 1 ou mais e trunca, com o sinal
/// do número. O erro de arredondamento das multiplicações aparece (`1e-11` dá 9, `5e-324` dá 4, `1e-7` dá 1).
fn tiny_integer(number: f64) -> i64 {
    let mut scaled = number.abs();
    while scaled < 1.0 {
        scaled *= 10.0;
    }
    let digit = scaled as i64;
    if number < 0.0 { -digit } else { digit }
}

/// `%d` e `%i`: o inteiro lido do texto de `number` (`parseInt(String(number))`), impresso com saturação em `i64`.
fn push_integer(out: &mut Vec<u16>, number: f64) {
    if !number.is_finite() {
        push_ascii(out, "NaN");
        return;
    }
    if number != 0.0 && number.abs() < TINY_INTEGER_LIMIT {
        push_ascii(out, &tiny_integer(number).to_string());
        return;
    }
    let text = String::from_utf16_lossy(&units(&WtfString::number_f64(number)));
    let negative = text.starts_with('-');
    let digits: String = text.trim_start_matches('-').chars().take_while(char::is_ascii_digit).collect();
    let magnitude: f64 = digits.parse().unwrap_or(0.0);
    let value = if negative { -magnitude } else { magnitude };
    // Satura em `±i64::MAX` (o bun imprime `-9223372036854775807` para `-1e20`, não `...808`).
    let limit = i64::MAX as f64;
    let integer = if value >= limit { i64::MAX } else if value <= -limit { -i64::MAX } else { value as i64 };
    push_ascii(out, &integer.to_string());
}

/// Um `%d`, `%i` ou `%f` (`symbol` vira `NaN`, o resto passa por `ToNumber`). `false` com exceção pendente.
fn push_numeric(global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue, integer: bool) -> bool {
    let number = if value.is_symbol() { f64::NAN } else { value.to_number() };
    if has_exception(global_object) {
        return false;
    }
    if integer {
        push_integer(out, number);
    } else {
        out.extend(units(&WtfString::number_f64(number)));
    }
    true
}

/// Um `%j`: `JSON.stringify(value)`, `undefined` vira texto vazio. `false` com exceção pendente.
fn push_json(global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> bool {
    match json_stringify(global_object, value, JSValue::undefined(), JSValue::undefined()) {
        Ok(Some(text)) => {
            out.extend(units(&text));
            true
        }
        Ok(None) => true,
        Err(error) => {
            throw_json_error(global_object, error);
            false
        }
    }
}

/// `value` é um `String` encaixotado (`JSType::StringObject`).
pub(crate) fn is_string_object(value: JSValue) -> bool {
    value.is_object() && matches!(cell_registry::get(value.as_cell()), Some(CellEntry::StringObject(_)))
}

/// Imprime o objeto `value` pelo formatador e registra em `counted` o texto que ele já somou à estimativa.
fn push_with_formatter(formatter: &mut Formatter, counted: &mut usize, global_object: &JSGlobalObject, out: &mut Vec<u16>, value: JSValue) -> bool {
    let before = out.len();
    let done = formatter.push_object(global_object, out, value);
    *counted += out.len() - before;
    done
}

/// Aplica o formato de `arguments[start]`, consumindo os argumentos seguintes; devolve o índice do primeiro argumento
/// que sobrou. `None` com exceção pendente (o que já foi montado em `out` continua lá). `counted` acumula as unidades que o
/// `formatter` já somou à estimativa de largura (o objeto de `%s`, `%o` e `%O`).
fn push_formatted(
    global_object: &JSGlobalObject,
    out: &mut Vec<u16>,
    format: &[u16],
    arguments: &ScriptArguments,
    start: usize,
    formatter: &mut Formatter,
    counted: &mut usize,
) -> Option<usize> {
    let count = arguments.argument_count();
    let mut next = start + 1;
    let mut i = 0;
    while i < format.len() {
        let unit = format[i];
        if unit != u16::from(b'%') || next >= count || i + 1 >= format.len() {
            out.push(unit);
            i += 1;
            continue;
        }
        let spec = format[i + 1];
        if spec == u16::from(b'%') {
            // `%%` vira `%`, e o caractere seguinte passa direto, mesmo sendo um `%`.
            out.push(unit);
            i += 2;
            if i < format.len() {
                out.push(format[i]);
                i += 1;
            }
            continue;
        }
        let value = arguments.argument_at(next);
        let done = match u8::try_from(spec).unwrap_or(0) {
            // `print_string` com `quote_strings` falso: a caixa de string leva o rótulo (`[String: "a"]`), o resto é `ToString`.
            b's' if is_string_object(value) => push_with_formatter(formatter, counted, global_object, out, value),
            b's' => {
                let text = value.to_wtf_string();
                out.extend(units(&text));
                !has_exception(global_object)
            }
            b'd' | b'i' => push_numeric(global_object, out, value, true),
            b'f' => push_numeric(global_object, out, value, false),
            // `%o` e `%O` passam pelo formatador de objetos (`Tag::get` e `format`); o primitivo sai como veio.
            b'o' | b'O' if value.is_object() => push_with_formatter(formatter, counted, global_object, out, value),
            b'o' | b'O' => push_plain(global_object, out, value),
            b'j' => push_json(global_object, out, value),
            b'c' => true,
            _ => {
                out.push(unit);
                i += 1;
                continue;
            }
        };
        if !done {
            return None;
        }
        next += 1;
        i += 2;
    }
    Some(next)
}

/// Monta a linha de `console.log(...)` em `out`, sem o `\n`. Toda string que ainda tem argumentos depois dela vale como
/// formato (`console.log(1, "%s", 2)` escreve `1 2`). `false` com exceção pendente; o texto até ali fica em `out`.
fn format_line(global_object: &JSGlobalObject, arguments: &ScriptArguments, out: &mut Vec<u16>, mut formatter: Formatter) -> bool {
    let count = arguments.argument_count();
    let mut index = 0;
    while index < count {
        let start = out.len();
        if index > 0 {
            out.push(u16::from(b' '));
        }
        let value = arguments.argument_at(index);
        if value.is_object() {
            // Objetos, arrays, funções e classes: o formatador soma o próprio texto à estimativa de largura, só falta o espaço.
            formatter.note(out.len() - start);
            if !formatter.push_object(global_object, out, value) {
                return false;
            }
            index += 1;
            continue;
        }
        let mut counted = 0;
        if value.is_string() && index + 1 < count {
            let format = units(&value.to_wtf_string());
            let Some(next) = push_formatted(global_object, out, &format, arguments, index, &mut formatter, &mut counted) else { return false };
            index = next;
        } else {
            let pushed = if formatter.colors() && !value.is_string() { formatter.push_primitive(global_object, out, value).is_some() } else { push_plain(global_object, out, value) };
            if !pushed {
                return false;
            }
            index += 1;
        }
        // O que o formatador contou sozinho (objetos de `%o` e `%O`) não entra duas vezes.
        let written = if formatter.colors() { visible_len(&out[start..]) } else { out.len() - start };
        formatter.note(written - counted);
    }
    true
}

/// O texto que `console.log(...arguments)` escreveria, sem recuo e sem a quebra de linha final (`util.format`). `None` com
/// exceção pendente.
pub(crate) fn format_arguments(global_object: &JSGlobalObject, arguments: &[JSValue]) -> Option<String> {
    let mut out = Vec::new();
    let script_arguments = ScriptArguments::create(arguments.to_vec());
    format_line(global_object, &script_arguments, &mut out, Formatter::new()).then(|| String::from_utf16_lossy(&out))
}

/// O texto de um único `value` no formato do `Bun.inspect` (string entre aspas duplas, como dentro de um objeto), com o
/// `formatter` dado. `None` com exceção pendente.
pub(crate) fn inspect_one(global_object: &JSGlobalObject, mut formatter: Formatter, value: JSValue) -> Option<String> {
    let mut out = Vec::new();
    formatter.push_object(global_object, &mut out, value).then(|| String::from_utf16_lossy(&out))
}

impl HostConsoleClient {
    /// Escreve `text` no stdout ou no stderr do host, se houver.
    fn write(global_object: &JSGlobalObject, to_stderr: bool, text: &[u16]) {
        if let Some(host) = global_object.console_host() {
            let text = String::from_utf16_lossy(text);
            if to_stderr {
                host.write_stderr(text.as_bytes());
            } else {
                host.write_stdout(text.as_bytes());
            }
        }
    }

    /// Escreve a linha de `arguments`; só a primeira linha do texto leva o recuo de `levels` níveis de grupo, e uma
    /// chamada sem argumentos escreve só o `\n`, sem recuo. O que foi montado antes de uma exceção sai também.
    fn write_line(global_object: &JSGlobalObject, arguments: &ScriptArguments, to_stderr: bool, levels: usize) {
        Self::write_line_with(global_object, arguments, to_stderr, levels, Formatter::new());
    }

    /// [`Self::write_line`] com o formatador escolhido (`console.dir` limita a profundidade). O recuo de grupo vira o
    /// `indent` inicial do formatador (`default_indent` do bun), que as linhas seguintes de um objeto multilinha usam.
    fn write_line_with(global_object: &JSGlobalObject, arguments: &ScriptArguments, to_stderr: bool, levels: usize, formatter: Formatter) {
        let formatter = formatter.at_indent(levels);
        let mut line = Vec::new();
        if arguments.argument_count() > 0 {
            line.resize(levels * 2, u16::from(b' '));
        }
        if format_line(global_object, arguments, &mut line, formatter) {
            line.push(u16::from(b'\n'));
        }
        Self::write(global_object, to_stderr, &line);
    }

    /// `console.group`: a etiqueta sai no recuo atual (nada sem argumentos) e o nível sobe, mesmo se a formatação lançou.
    fn start_group(&self, global_object: &JSGlobalObject, arguments: &ScriptArguments) {
        let depth = self.state.borrow().depth;
        if arguments.argument_count() > 0 {
            Self::write_line(global_object, arguments, false, depth);
        }
        self.state.borrow_mut().depth += 1;
    }

    /// `console.groupCollapsed`: a etiqueta sai no recuo atual e o nível NÃO sobe (medido no bun); sem argumentos
    /// escreve `undefined` sem recuo.
    fn start_group_collapsed(&self, global_object: &JSGlobalObject, arguments: &ScriptArguments) {
        if arguments.argument_count() == 0 {
            Self::write_line(global_object, &ScriptArguments::create(vec![JSValue::undefined()]), false, 0);
        } else {
            Self::write_line(global_object, arguments, false, self.state.borrow().depth);
        }
    }

    /// `console.assert` com a condição falsa: `Assertion failed` sem recuo se não há argumentos, senão como `console.error`.
    fn assertion_failed(&self, global_object: &JSGlobalObject, arguments: &ScriptArguments) {
        if arguments.argument_count() == 0 {
            Self::write(global_object, true, &units(&WtfString::from_latin1(b"Assertion failed\n")));
        } else {
            Self::write_line(global_object, arguments, true, self.state.borrow().depth);
        }
    }

    /// A profundidade de `console.dir(valor, { depth })` (ConsoleObject.rs, `message_type == Dir && len >= 2`): número
    /// (`Infinity` e o que passa de `i32` saturam) trunca para `u16` depois de zerar o negativo, BigInt e `null`
    /// é `u16::MAX` e o resto deixa a profundidade padrão. `colors` é lido logo depois (o getter roda). `Err` com a
    /// exceção de um getter pendente.
    fn dir_depth(global_object: &JSGlobalObject, options: JSValue) -> Result<(Option<usize>, bool), ()> {
        let vm = global_object.vm();
        let read = |name: &[u8]| get_object_property(global_object, options, &Identifier::from_string(vm, &WtfString::from_latin1(name))).map_err(|_| ());
        let depth = read(b"depth")?;
        // `colors` só vale se for booleano (`colors_prop.is_boolean()`); o stdout do sandbox não é terminal.
        let colors = read(b"colors")?;
        let colors = colors.is_boolean() && colors.is_true();
        Ok((if depth.is_number() {
            Some(usize::from((depth.as_number() as i32).max(0) as u32 as u16))
        } else if depth.is_big_int() {
            // O bun lê o BigInt com `to_int32()`, que devolve os 32 bits baixos do endereço da célula: com o bit 31
            // ligado o depth vira 0, senão os 16 bits baixos do ponteiro (quase sempre muito acima de qualquer
            // aninhamento). Das duas saídas que o bun produz, o porte fica com a segunda, a mesma de `null`.
            Some(usize::from(u16::MAX))
        } else if depth.is_null() {
            Some(usize::from(u16::MAX))
        } else {
            None
        }, colors))
    }

    /// `console.dir`: com dois ou mais argumentos só o primeiro é impresso, com a profundidade e as cores das opções.
    fn dir(&self, global_object: &JSGlobalObject, arguments: &ScriptArguments) {
        let mut max_depth = None;
        let mut colors = false;
        if arguments.argument_count() >= 2 && arguments.argument_at(1).is_object() {
            match Self::dir_depth(global_object, arguments.argument_at(1)) {
                Ok(options) => (max_depth, colors) = options,
                Err(()) => return,
            }
        }
        let formatter = max_depth.map_or_else(Formatter::new, |depth| Formatter::with_depth(0, depth)).with_colors(colors);
        let first = ScriptArguments::create(vec![arguments.argument_at(0)]);
        Self::write_line_with(global_object, &first, false, self.state.borrow().depth, formatter);
    }

    /// `console.table`: o segundo argumento que não é `undefined` nem array lança `TypeError` (a checagem do
    /// `ConsoleObject::messageWithTypeAndLevel` do C++); dado que não é objeto sai como `console.log`; objeto vira tabela
    /// no stdout, sem o recuo de grupo (só as células o levam).
    fn print_table(&self, global_object: &JSGlobalObject, arguments: &ScriptArguments) {
        let count = arguments.argument_count();
        let depth = self.state.borrow().depth;
        if count >= 2 && !arguments.argument_at(1).is_undefined() && JSArray::from_value(&arguments.argument_at(1)).is_none() {
            throw_thrown(global_object, Thrown::type_error("The \"properties\" argument must be an instance of Array."));
            return;
        }
        let tabular_data = arguments.argument_at(0);
        if !tabular_data.is_object() {
            Self::write_line(global_object, arguments, false, depth);
            return;
        }
        let properties = if count >= 2 { arguments.argument_at(1) } else { JSValue::undefined() };
        if let Some(text) = console_table::render(global_object, tabular_data, properties, depth) {
            Self::write(global_object, false, &units(&WtfString::from_utf8(text.as_bytes())));
        }
    }

    /// `console.trace`: os argumentos como `console.log` (nada sem argumentos) e depois um `      at ...` por frame, no
    /// stdout, sem recuo de grupo. O erro `trace output` do bun conta o frame de `trace` no `Error.stackTraceLimit`
    /// (limite 10 mostra 9 frames; sem limite, nenhum).
    fn trace(&self, global_object: &JSGlobalObject, arguments: &ScriptArguments) {
        if arguments.argument_count() > 0 {
            Self::write_line(global_object, arguments, false, self.state.borrow().depth);
            if has_exception(global_object) {
                return;
            }
        }
        let top = global_object.vm().top_call_frame();
        let limit = global_object.stack_trace_limit.get().map_or(0, |limit| limit.saturating_sub(1) as usize);
        if top == 0 || limit == 0 {
            return;
        }
        let frames = global_object.vm().interpreter().get_stack_trace(global_object.vm(), CallFrame::create(top), limit, None, false);
        let mut text = String::new();
        for line in frames.iter().filter_map(|frame| frame.to_trace_string()) {
            text.push_str("      at ");
            text.push_str(&line);
            text.push('\n');
        }
        Self::write(global_object, false, &units(&WtfString::from_utf8(text.as_bytes())));
    }

    /// A linha de `timeLog` e `timeEnd` no stderr: `[tempo] etiqueta args...`, sem recuo; nada sem o cronômetro.
    fn write_elapsed(&self, global_object: &JSGlobalObject, label: &WtfString, arguments: &ScriptArguments, kind: &str) {
        let label_text = label;
        let label = units(label);
        let Some(started) = self.state.borrow().timers.get(&label).copied() else { return };
        let elapsed = ul_common::time::monotonic_nanos().saturating_sub(started) as f64 / 1e6;
        let mut observed = vec![label_value(global_object, label_text), crate::runtime::js_value::js_number(elapsed)];
        observed.extend(observed_arguments(arguments));
        observe(global_object, kind, &observed);
        let mut line = Vec::new();
        push_ascii(&mut line, &format!("[{}]", format_elapsed(elapsed)));
        if !label.is_empty() {
            line.push(u16::from(b' '));
            line.extend(&label);
        }
        for index in 0..arguments.argument_count() {
            line.push(u16::from(b' '));
            push_plain(global_object, &mut line, arguments.argument_at(index));
        }
        line.push(u16::from(b'\n'));
        Self::write(global_object, true, &line);
    }
}

impl ConsoleClient for HostConsoleClient {
    fn message_with_type_and_level(&self, message_type: MessageType, level: MessageLevel, global_object: &JSGlobalObject, arguments: ScriptArguments) {
        let kind = match (message_type, level) {
            (MessageType::Log, MessageLevel::Log) => Some("log"),
            (MessageType::Log, MessageLevel::Debug) => Some("debug"),
            (MessageType::Log, MessageLevel::Info) => Some("info"),
            (MessageType::Log, MessageLevel::Error) => Some("error"),
            (MessageType::Log, MessageLevel::Warning) => Some("warning"),
            (MessageType::Dir, _) => Some("dir"),
            (MessageType::DirXML, _) => Some("dirxml"),
            (MessageType::Table, _) => Some("table"),
            (MessageType::Trace, _) => Some("trace"),
            (MessageType::Clear, _) => Some("clear"),
            (MessageType::StartGroup, _) => Some("startGroup"),
            (MessageType::StartGroupCollapsed, _) => Some("startGroupCollapsed"),
            (MessageType::EndGroup, _) => Some("endGroup"),
            (MessageType::Assert, _) => Some("assert"),
            _ => None,
        };
        if let Some(kind) = kind {
            observe(global_object, kind, &observed_arguments(&arguments));
        }
        match message_type {
            MessageType::Log => {
                // `console.error()` e `console.warn()` sem argumentos escrevem a linha vazia no stdout (medido no bun).
                let to_stderr = matches!(level, MessageLevel::Warning | MessageLevel::Error) && arguments.argument_count() > 0;
                Self::write_line(global_object, &arguments, to_stderr, self.state.borrow().depth);
            }
            MessageType::StartGroup => self.start_group(global_object, &arguments),
            MessageType::StartGroupCollapsed => self.start_group_collapsed(global_object, &arguments),
            MessageType::EndGroup => {
                let mut state = self.state.borrow_mut();
                state.depth = state.depth.saturating_sub(1);
            }
            MessageType::Assert => self.assertion_failed(global_object, &arguments),
            MessageType::Dir => self.dir(global_object, &arguments),
            MessageType::DirXML => Self::write_line(global_object, &arguments, false, self.state.borrow().depth),
            MessageType::Trace => self.trace(global_object, &arguments),
            MessageType::Table => self.print_table(global_object, &arguments),
            _ => {}
        }
    }

    fn count(&self, global_object: &JSGlobalObject, label: &WtfString) {
        let key = units(label);
        let total = {
            let mut state = self.state.borrow_mut();
            let total = state.counts.entry(key.clone()).or_insert(0);
            *total += 1;
            *total
        };
        observe(global_object, "count", &[label_value(global_object, label), crate::runtime::js_value::js_number(total as f64)]);
        let mut line = key;
        push_ascii(&mut line, &format!(": {total}\n"));
        Self::write(global_object, false, &line);
    }

    fn count_reset(&self, _global_object: &JSGlobalObject, label: &WtfString) {
        self.state.borrow_mut().counts.remove(&units(label));
    }

    fn profile(&self, _global_object: &JSGlobalObject, _title: &WtfString) {}
    fn profile_end(&self, _global_object: &JSGlobalObject, _title: &WtfString) {}
    fn take_heap_snapshot(&self, _global_object: &JSGlobalObject, _title: &WtfString) {}

    fn time(&self, _global_object: &JSGlobalObject, label: &WtfString) {
        self.state.borrow_mut().timers.entry(units(label)).or_insert_with(ul_common::time::monotonic_nanos);
    }

    fn time_log(&self, global_object: &JSGlobalObject, label: &WtfString, arguments: ScriptArguments) {
        self.write_elapsed(global_object, label, &arguments, "timeLog");
    }

    fn time_end(&self, global_object: &JSGlobalObject, label: &WtfString) {
        self.write_elapsed(global_object, label, &ScriptArguments::default(), "timeEnd");
        self.state.borrow_mut().timers.remove(&units(label));
    }

    fn time_stamp(&self, _global_object: &JSGlobalObject, _arguments: ScriptArguments) {}
    fn record(&self, _global_object: &JSGlobalObject, _arguments: ScriptArguments) {}
    fn record_end(&self, _global_object: &JSGlobalObject, _arguments: ScriptArguments) {}
    fn screenshot(&self, _global_object: &JSGlobalObject, _arguments: ScriptArguments) {}
}
