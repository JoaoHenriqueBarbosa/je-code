//! Porte de `src/vs/base/common/console.ts`: o console remoto (extension host e processos filhos mandam
//! `console.*` serializado para quem os criou).
//!
//! O `console` do JavaScript é um efeito colateral sem modelo em Rust. [`format_log`] devolve a chamada que o
//! original faria (o nome do método e os argumentos, com os `%c` de estilo) e [`log`] a entrega a um
//! [`ConsoleSink`]; [`StdConsole`] é o sink que escreve em stdout e stderr sem os estilos.

use std::io::Write;
use std::sync::OnceLock;

use zyarr::RegExp;

use crate::errors::{self, Error};
use crate::json::{self, Value};
use crate::js_string::JsString;
use crate::path::Platform;
use crate::uri::URI;

/// `IRemoteConsoleLog`.
#[derive(Clone, Debug, PartialEq)]
pub struct IRemoteConsoleLog {
    pub r#type: String,
    pub severity: String,
    pub arguments: String,
}

/// `IStackFrame`.
#[derive(Clone, Debug)]
pub struct IStackFrame {
    pub uri: URI,
    pub line: f64,
    pub column: f64,
}

/// `isRemoteConsoleLog`: `entry && typeof entry.type === 'string' && typeof entry.severity === 'string'`.
/// Recebe o valor já lido de um JSON.
pub fn is_remote_console_log(obj: &Value) -> bool {
    match obj {
        Value::Object(o) => matches!(o.get("type"), Some(Value::String(_))) && matches!(o.get("severity"), Some(Value::String(_))),
        _ => false,
    }
}

/// O resultado de `parse`: os argumentos (JSON) e a pilha, se havia.
#[derive(Clone, Debug, PartialEq)]
pub struct ParsedLog {
    pub args: Vec<Value>,
    pub stack: Option<String>,
}

/// `parse`: lê os `arguments` (um array JSON); o último pode ser `{ "__$stack": ... }`, que vira a pilha.
/// JSON inválido, ou que não é um array, dá os dois argumentos de aviso do original.
pub fn parse(entry: &IRemoteConsoleLog) -> ParsedLog {
    let mut errors = Vec::new();
    let parsed = json::parse_str(&entry.arguments, &mut errors).filter(|_| errors.is_empty());
    let Some(Value::Array(mut parsed_arguments)) = parsed else {
        return ParsedLog { args: vec![Value::from("Unable to log remote console arguments"), Value::from(entry.arguments.as_str())], stack: None };
    };

    let mut stack = None;
    // `stackArgument && stackArgument.__$stack`: truthy.
    if let Some(Value::Object(last)) = parsed_arguments.last()
        && let Some(Value::String(s)) = last.get("__$stack").filter(|v| is_truthy(v))
    {
        stack = Some(s.to_string_lossy());
        parsed_arguments.pop();
    }
    ParsedLog { args: parsed_arguments, stack }
}

fn is_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => *n != 0.0 && !n.is_nan(),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn frame_regexp() -> &'static RegExp {
    static RE: OnceLock<RegExp> = OnceLock::new();
    RE.get_or_init(|| {
        let pattern: Vec<u16> = r"at [^\/]*((?:(?:[a-zA-Z]+:)|(?:[\/])|(?:\\\\))(?:.+)):(\d+):(\d+)".encode_utf16().collect();
        RegExp::new(&pattern, &[]).expect("regex fixa e válida")
    })
}

/// `findFirstFrame`: até a primeira quebra de linha.
fn find_first_frame(stack: Option<&str>) -> Option<&str> {
    let stack = stack.filter(|s| !s.is_empty())?;
    Some(stack.split_once('\n').map_or(stack, |(first, _)| first))
}

/// `getFirstFrame(stack)`: a primeira linha da pilha como arquivo, linha e coluna, nos formatos
/// `at x (/caminho:18:17)`, `at /caminho:18:17`, `at c:\caminho:18:17`. `platform` é o sistema do `URI.file`.
pub fn get_first_frame(stack: Option<&str>, platform: Platform) -> Option<IStackFrame> {
    let top_frame = find_first_frame(stack).unwrap_or("");
    let units: Vec<u16> = top_frame.encode_utf16().collect();
    let found = frame_regexp().exec(&units, 0)?;
    let group = |n: usize| found.captures.get(n).and_then(|c| c.clone()).map(|r| JsString::from_units(units[r].to_vec()));
    let (path, line, column) = (group(1)?, group(2)?, group(3)?);
    let number = |s: &JsString| s.to_string_lossy().parse::<f64>().unwrap_or(f64::NAN);
    Some(IStackFrame { uri: URI::file(path, platform), line: number(&line), column: number(&column) })
}

/// `getFirstFrame(entry)`.
pub fn get_first_frame_of_entry(entry: &IRemoteConsoleLog, platform: Platform) -> Option<IStackFrame> {
    get_first_frame(parse(entry).stack.as_deref(), platform)
}

/// Uma chamada de `console.<method>(...args)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ConsoleCall {
    pub method: String,
    pub args: Vec<Value>,
}

/// Os métodos de `console` que são funções.
const CONSOLE_METHODS: [&str; 20] = [
    "assert", "clear", "count", "countReset", "debug", "dir", "dirxml", "error", "group", "groupCollapsed", "groupEnd", "info", "log", "table", "time", "timeEnd",
    "timeLog", "timeStamp", "trace", "warn",
];

fn color(color: &str) -> Value {
    Value::from(format!("color: {color}").as_str())
}

/// A chamada de `console` que `log(entry, label)` faz. Lança `Unknown console method` se a severidade não é
/// uma função do `console`.
pub fn format_log(entry: &IRemoteConsoleLog, label: &str) -> Result<ConsoleCall, Error> {
    let ParsedLog { args, stack } = parse(entry);

    let first_string = match args.first() {
        Some(Value::String(s)) => Some(s.to_string_lossy()),
        _ => None,
    };
    let is_one_string_arg = first_string.is_some() && args.len() == 1;

    let top_frame = find_first_frame(stack.as_deref()).map(|f| format!("({})", JsString::from_units(crate::js_string::trim(&f.encode_utf16().collect::<Vec<_>>()).to_vec()).to_string_lossy()));

    let mut console_args: Vec<Value>;
    if let Some(first) = &first_string {
        // First arg is a string
        if let (Some(top_frame), true) = (&top_frame, is_one_string_arg) {
            console_args = vec![Value::from(format!("%c[{label}] %c{first} %c{top_frame}").as_str()), color("blue"), color(""), color("grey")];
        } else {
            console_args = vec![Value::from(format!("%c[{label}] %c{first}").as_str()), color("blue"), color("")];
            console_args.extend(args[1..].iter().cloned());
        }
    } else {
        // First arg is something else, just apply all
        console_args = vec![Value::from(format!("%c[{label}]%").as_str()), color("blue")];
        console_args.extend(args.iter().cloned());
    }

    // Stack: add to args unless already added
    if let (Some(top_frame), false) = (&top_frame, is_one_string_arg) {
        console_args.push(Value::from(top_frame.as_str()));
    }

    if !CONSOLE_METHODS.contains(&entry.severity.as_str()) {
        return Err(Error::new("Unknown console method"));
    }
    Ok(ConsoleCall { method: entry.severity.clone(), args: console_args })
}

/// Para onde vai o `console.*`.
pub trait ConsoleSink {
    fn call(&self, call: &ConsoleCall);
}

/// `log(entry, label)`. Lança `Unknown console method` (um [`Error`]) como o original.
pub fn log(entry: &IRemoteConsoleLog, label: &str, console: &dyn ConsoleSink) {
    match format_log(entry, label) {
        Ok(call) => console.call(&call),
        Err(error) => errors::throw(error),
    }
}

thread_local! {
    /// As chamadas que o espião instalado por [`WarnSpy::install`] guarda; `None` quando nenhum teste o instalou.
    static WARN_SPY: std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<Vec<String>>>>> = const { std::cell::RefCell::new(None) };
}

/// `console.warn(message)` do código portado: vai para o stderr, a não ser que um teste tenha instalado o espião
/// ([`WarnSpy`], o `sinon.stub(console, 'warn')`), que guarda a chamada em vez de escrever.
pub fn warn(message: &str) {
    let spied = WARN_SPY.with(|spy| match &*spy.borrow() {
        Some(calls) => {
            calls.borrow_mut().push(message.to_owned());
            true
        }
        None => false,
    });
    if !spied {
        let _ = writeln!(std::io::stderr(), "{message}");
    }
}

/// `sinon.stub(console, 'warn')`: conta as chamadas de [`warn`] do thread. `restore` (ou o `Drop`) devolve o
/// `console.warn` ao comportamento normal, como o `consoleWarn.restore()` do original.
pub struct WarnSpy {
    calls: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

impl WarnSpy {
    /// `stub(console, 'warn')`.
    pub fn install() -> Self {
        let calls = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        WARN_SPY.with(|spy| *spy.borrow_mut() = Some(calls.clone()));
        Self { calls }
    }

    /// `consoleWarn.callCount`.
    pub fn call_count(&self) -> usize {
        self.calls.borrow().len()
    }

    /// Os textos das chamadas, na ordem.
    pub fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    /// `consoleWarn.restore()`.
    pub fn restore(&self) {
        WARN_SPY.with(|spy| *spy.borrow_mut() = None);
    }
}

impl Drop for WarnSpy {
    fn drop(&mut self) {
        self.restore();
    }
}

/// O console do processo: `warn` e `error` vão para stderr, o resto para stdout, sem os `%c` nem os estilos.
pub struct StdConsole;

impl ConsoleSink for StdConsole {
    fn call(&self, call: &ConsoleCall) {
        let mut parts: Vec<String> = Vec::new();
        let mut styles_to_skip = 0;
        for (i, arg) in call.args.iter().enumerate() {
            match arg {
                Value::String(s) if i == 0 => {
                    let text = s.to_string_lossy();
                    styles_to_skip = text.matches("%c").count();
                    parts.push(text.replace("%c", ""));
                }
                _ if i >= 1 && i <= styles_to_skip => {}
                Value::String(s) => parts.push(s.to_string_lossy()),
                other => parts.push(format!("{other:?}")),
            }
        }
        let line = parts.join(" ");
        if matches!(call.method.as_str(), "warn" | "error" | "trace") {
            let _ = writeln!(std::io::stderr(), "{line}");
        } else {
            let _ = writeln!(std::io::stdout(), "{line}");
        }
    }
}
