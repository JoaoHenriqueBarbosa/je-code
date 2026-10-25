//! Porte de `src/vs/base/test/common/snapshot.ts`: o `assertSnapshot` e o [`SnapshotContext`] que o sustenta. A primeira
//! execução de um teste grava o valor formatado em `__snapshots__`, ao lado do arquivo de teste; as seguintes comparam, e
//! um valor diferente reprova e deixa o `.actual` para comparação.
//!
//! Divergências:
//! - **Valor.** O `unknown` do original é o [`SnapshotValue`], que modela os tipos que o `formatValue` distingue
//!   (primitivos, `bigint`, símbolo, função, `RegExp`, array, objeto, `Map`, `Set` e o objeto com
//!   `Symbol.for('debug.description')`). Array e objeto têm identidade (`Rc`), então um ciclo vira `[Circular]`.
//! - **Mocha.** O original lê `test.file` e `test.fullTitle()` do teste corrente (`setup` e `teardown` globais). Aqui o
//!   teste chama [`begin_snapshot_test`] com a pasta do arquivo de teste e o título completo; o [`SnapshotTest`] devolvido
//!   faz o papel do `teardown`: se o teste não entrou em pânico, apaga os snapshots antigos do título.
//! - **Erro.** O `throw` de snapshot divergente é o [`SnapshotError::Mismatch`], e `assertThrowsAsync` é `is_err()`. A
//!   pilha reescrita do original não existe.
//! - **E/S.** `__readFileInTests` e companhia são `std::fs`. Ler um snapshot que não existe (ou não é legível) cria um novo.
//! - **Números.** `String(number)` do JavaScript vale para inteiros e para os decimais curtos; expoentes (`1e-7`) saem na
//!   forma do Rust.

use std::cell::{Cell, RefCell};
use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// O nome de um snapshot, sem os caracteres fora de `[a-z0-9_-]`.
fn sanitize_name(name: &str) -> String {
    name.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect()
}

fn normalize_crlf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// `ISnapshotOptions`.
#[derive(Clone, Debug, Default)]
pub struct SnapshotOptions {
    /// Nome do arquivo de snapshot, no lugar de um número incrementado.
    pub name: Option<String>,
    /// Extensão do arquivo de snapshot; o padrão é `snap`.
    pub extension: Option<String>,
}

impl SnapshotOptions {
    pub fn named(name: impl Into<String>) -> Self {
        Self { name: Some(name.into()), extension: None }
    }

    pub fn with_extension(mut self, extension: impl Into<String>) -> Self {
        self.extension = Some(extension.into());
        self
    }
}

/// Por que um `assert` de snapshot falhou.
#[derive(Debug)]
pub enum SnapshotError {
    /// `Snapshot #N does not match expected output`.
    Mismatch { name_or_index: String, expected: String, actual: String, snapshot_path: PathBuf },
    Io(io::Error),
}

impl fmt::Display for SnapshotError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SnapshotError::Mismatch { name_or_index, .. } => write!(f, "Snapshot #{name_or_index} does not match expected output"),
            SnapshotError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SnapshotError {}

impl From<io::Error> for SnapshotError {
    fn from(error: io::Error) -> Self {
        SnapshotError::Io(error)
    }
}

/// Um objeto: as propriedades próprias enumeráveis, na ordem de inserção.
pub type SnapshotObject = Rc<RefCell<Vec<(String, SnapshotValue)>>>;
/// Um array.
pub type SnapshotArray = Rc<RefCell<Vec<SnapshotValue>>>;

/// O valor que o `formatValue` do original sabe formatar.
#[derive(Clone)]
pub enum SnapshotValue {
    Undefined,
    Null,
    Bool(bool),
    Number(f64),
    BigInt(i128),
    /// `Symbol(description)`.
    Symbol(String),
    Str(String),
    /// Uma função, com o `name`.
    Function(String),
    /// Um `RegExp`, já na forma de `String(regexp)` (`/hello/g`).
    RegExp(String),
    Array(SnapshotArray),
    Object(SnapshotObject),
    /// `Map`, com as entradas na ordem de inserção.
    Map(Rc<Vec<(SnapshotValue, SnapshotValue)>>),
    /// `Set`, com os valores na ordem de inserção.
    Set(Rc<Vec<SnapshotValue>>),
    /// Um objeto com `[Symbol.for('debug.description')]()`.
    DebugDescription(Rc<dyn Fn() -> String>),
}

impl SnapshotValue {
    pub fn str(text: impl Into<String>) -> Self {
        SnapshotValue::Str(text.into())
    }

    pub fn array(items: Vec<SnapshotValue>) -> Self {
        SnapshotValue::Array(Rc::new(RefCell::new(items)))
    }

    /// `{ key: value, ... }`.
    pub fn object(entries: Vec<(&str, SnapshotValue)>) -> Self {
        SnapshotValue::Object(new_object(entries))
    }

    pub fn map(entries: Vec<(SnapshotValue, SnapshotValue)>) -> Self {
        SnapshotValue::Map(Rc::new(entries))
    }

    pub fn set(values: Vec<SnapshotValue>) -> Self {
        SnapshotValue::Set(Rc::new(values))
    }

    pub fn debug_description(describe: impl Fn() -> String + 'static) -> Self {
        SnapshotValue::DebugDescription(Rc::new(describe))
    }

    /// A identidade do valor, para o `seen.includes(value)`.
    fn identity(&self) -> Option<usize> {
        match self {
            SnapshotValue::Array(array) => Some(Rc::as_ptr(array).cast::<()>() as usize),
            SnapshotValue::Object(object) => Some(Rc::as_ptr(object).cast::<()>() as usize),
            SnapshotValue::Map(map) => Some(Rc::as_ptr(map).cast::<()>() as usize),
            SnapshotValue::Set(set) => Some(Rc::as_ptr(set).cast::<()>() as usize),
            _ => None,
        }
    }
}

/// Um objeto novo; as propriedades acrescentadas depois entram por `borrow_mut().push(...)`.
pub fn new_object(entries: Vec<(&str, SnapshotValue)>) -> SnapshotObject {
    Rc::new(RefCell::new(entries.into_iter().map(|(key, value)| (key.to_owned(), value)).collect()))
}

/// `String(number)`.
fn number_to_string(number: f64) -> String {
    if number.is_nan() {
        "NaN".to_owned()
    } else if number.is_infinite() {
        (if number > 0.0 { "Infinity" } else { "-Infinity" }).to_owned()
    } else if number == number.trunc() && number.abs() < 1e21 {
        (number as i128).to_string()
    } else {
        number.to_string()
    }
}

/// `JSON.stringify(string)`.
fn json_string(text: &str) -> String {
    let mut out = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// A conversão para texto de um template literal (`${k}`), usada nas chaves de `Map` e `Set`.
fn coerce_to_string(value: &SnapshotValue) -> String {
    match value {
        SnapshotValue::Undefined => "undefined".to_owned(),
        SnapshotValue::Null => "null".to_owned(),
        SnapshotValue::Bool(value) => value.to_string(),
        SnapshotValue::Number(number) => number_to_string(*number),
        SnapshotValue::BigInt(value) => value.to_string(),
        SnapshotValue::Symbol(_) => panic!("TypeError: Cannot convert a Symbol value to a string"),
        SnapshotValue::Str(text) | SnapshotValue::RegExp(text) => text.clone(),
        SnapshotValue::Function(name) => format!("function {name}() {{ }}"),
        SnapshotValue::Array(items) => items.borrow().iter().map(coerce_to_string).collect::<Vec<_>>().join(","),
        SnapshotValue::Object(_) | SnapshotValue::Map(_) | SnapshotValue::Set(_) | SnapshotValue::DebugDescription(_) => "[object Object]".to_owned(),
    }
}

/// O `{ ... }` de objeto, `Map` e `Set`: uma linha com uma entrada só, várias linhas com mais.
fn format_block(prefix: &str, lines: &[String], level: usize) -> String {
    let outer_indent = "  ".repeat(level);
    let child_indent = "  ".repeat(level + 1);
    if lines.len() > 1 {
        format!("{prefix}{{\n{child_indent}{}\n{outer_indent}}}", lines.join(&format!(",\n{child_indent}")))
    } else {
        format!("{prefix}{{ {} }}", lines.join(",\n"))
    }
}

/// `formatValue(value, level, seen)`.
fn format_value(value: &SnapshotValue, level: usize, seen: &[usize]) -> String {
    let seen_with = |value: &SnapshotValue| {
        let mut next = seen.to_vec();
        next.extend(value.identity());
        next
    };
    if value.identity().is_some_and(|identity| seen.contains(&identity)) {
        return "[Circular]".to_owned();
    }
    match value {
        SnapshotValue::Undefined => "undefined".to_owned(),
        SnapshotValue::Null => "null".to_owned(),
        SnapshotValue::Bool(value) => value.to_string(),
        SnapshotValue::Number(number) => number_to_string(*number),
        SnapshotValue::BigInt(value) => value.to_string(),
        SnapshotValue::Symbol(description) => format!("Symbol({description})"),
        SnapshotValue::Str(text) => {
            if level == 0 {
                text.clone()
            } else {
                json_string(text)
            }
        }
        SnapshotValue::Function(name) => format!("[Function {name}]"),
        SnapshotValue::RegExp(text) => text.clone(),
        SnapshotValue::DebugDescription(describe) => describe(),
        SnapshotValue::Array(items) => {
            let next_seen = seen_with(value);
            let children: Vec<String> = items.borrow().iter().map(|item| format_value(item, level + 1, &next_seen)).collect();
            let multiline = children.iter().any(|child| child.contains('\n')) || children.join(", ").encode_utf16().count() > 80;
            if multiline {
                let child_indent = "  ".repeat(level + 1);
                format!("[\n{child_indent}{}\n{}]", children.join(&format!(",\n{child_indent}")), "  ".repeat(level))
            } else {
                format!("[ {} ]", children.join(", "))
            }
        }
        SnapshotValue::Map(entries) => {
            let next_seen = seen_with(value);
            let lines: Vec<String> = entries.iter().map(|(key, entry)| format!("{}: {}", coerce_to_string(key), format_value(entry, level + 1, &next_seen))).collect();
            format_block("Map ", &lines, level)
        }
        SnapshotValue::Set(values) => {
            let next_seen = seen_with(value);
            // `[...set.entries()]` entrega pares `[valor, valor]`.
            let lines: Vec<String> = values.iter().map(|entry| format!("{}: {}", coerce_to_string(entry), format_value(entry, level + 1, &next_seen))).collect();
            format_block("Set ", &lines, level)
        }
        SnapshotValue::Object(entries) => {
            let next_seen = seen_with(value);
            let lines: Vec<String> = entries.borrow().iter().map(|(key, entry)| format!("{key}: {}", format_value(entry, level + 1, &next_seen))).collect();
            format_block("", &lines, level)
        }
    }
}

/// O texto que `assertSnapshot` grava e compara para o valor.
pub fn format_snapshot_value(value: &SnapshotValue) -> String {
    format_value(value, 0, &[])
}

/// `SnapshotContext`. Exportado para os testes do próprio snapshot; quem consome usa [`assert_snapshot`].
pub struct SnapshotContext {
    next_index: Cell<u64>,
    snapshots_dir: PathBuf,
    full_title: String,
    name_prefix: String,
    used_names: RefCell<BTreeSet<String>>,
    asserted: Cell<bool>,
}

impl SnapshotContext {
    /// Os snapshots ficam em `__snapshots__`, dentro da pasta do arquivo de teste.
    pub fn new(test_file_dir: &Path, full_title: &str) -> Self {
        Self::with_snapshots_dir(test_file_dir.join("__snapshots__"), full_title)
    }

    /// Com a pasta dos snapshots escolhida (o `this.snapshotsDir = ...` da subclasse dos testes do próprio snapshot).
    pub fn with_snapshots_dir(snapshots_dir: impl Into<PathBuf>, full_title: &str) -> Self {
        Self {
            next_index: Cell::new(0),
            snapshots_dir: snapshots_dir.into(),
            full_title: full_title.to_owned(),
            name_prefix: format!("{}.", sanitize_name(full_title)),
            used_names: RefCell::new(BTreeSet::new()),
            asserted: Cell::new(false),
        }
    }

    pub fn snapshots_dir(&self) -> &Path {
        &self.snapshots_dir
    }

    pub fn assert(&self, value: &SnapshotValue, options: Option<&SnapshotOptions>) -> Result<(), SnapshotError> {
        self.asserted.set(true);
        let name_or_index = match options.and_then(|options| options.name.as_deref()).filter(|name| !name.is_empty()) {
            Some(name) => sanitize_name(name),
            None => {
                let index = self.next_index.get();
                self.next_index.set(index + 1);
                index.to_string()
            }
        };
        let extension = options.and_then(|options| options.extension.as_deref()).filter(|extension| !extension.is_empty()).unwrap_or("snap");
        let file_name = format!("{}{name_or_index}.{extension}", self.name_prefix);
        self.used_names.borrow_mut().insert(file_name.clone());

        let path = self.snapshots_dir.join(&file_name);
        let actual = format_snapshot_value(value);
        let Ok(expected) = fs::read_to_string(&path) else {
            eprintln!("Creating new snapshot in: {}", path.display());
            fs::create_dir_all(&self.snapshots_dir)?;
            fs::write(&path, &actual)?;
            return Ok(());
        };

        if normalize_crlf(&expected) != normalize_crlf(&actual) {
            let mut actual_path = path.clone().into_os_string();
            actual_path.push(".actual");
            fs::write(PathBuf::from(actual_path), &actual)?;
            return Err(SnapshotError::Mismatch { name_or_index, expected, actual, snapshot_path: path });
        }
        Ok(())
    }

    /// Apaga os arquivos do título que a execução não usou (inclusive os `.actual` de divergências antigas).
    pub fn remove_old_snapshots(&self) -> io::Result<()> {
        let mut to_delete = Vec::new();
        for entry in fs::read_dir(&self.snapshots_dir)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            if name.starts_with(&self.name_prefix) && !self.used_names.borrow().contains(&name) {
                to_delete.push(name);
            }
        }
        let old_snapshots = to_delete.iter().filter(|name| !name.ends_with(".actual")).count();
        if old_snapshots > 0 {
            eprintln!("Deleting {old_snapshots} old snapshots for {}", self.full_title);
        }
        for name in to_delete {
            fs::remove_file(self.snapshots_dir.join(name))?;
        }
        Ok(())
    }
}

thread_local! {
    static CONTEXT: RefCell<Option<Rc<SnapshotContext>>> = const { RefCell::new(None) };
}

/// O `setup` e o `teardown` globais do original: enquanto o guarda vive, [`assert_snapshot`] vale para o teste. Ao ser
/// largado sem pânico, apaga os snapshots antigos do título (só se o teste chamou `assert_snapshot`).
pub struct SnapshotTest {
    context: Rc<SnapshotContext>,
}

/// Começa um teste de snapshot: `test_file_dir` é a pasta do arquivo de teste e `full_title` o `fullTitle()` do mocha.
pub fn begin_snapshot_test(test_file_dir: &Path, full_title: &str) -> SnapshotTest {
    let context = Rc::new(SnapshotContext::new(test_file_dir, full_title));
    CONTEXT.with(|slot| *slot.borrow_mut() = Some(context.clone()));
    SnapshotTest { context }
}

impl Drop for SnapshotTest {
    fn drop(&mut self) {
        CONTEXT.with(|slot| *slot.borrow_mut() = None);
        if !std::thread::panicking() && self.context.asserted.get() {
            self.context.remove_old_snapshots().expect("falha ao apagar os snapshots antigos");
        }
    }
}

/// Implementa um utilitário de teste por snapshot.
///
/// A primeira execução de um teste de snapshot grava o valor com que é chamado como o esperado. As seguintes reprovam se o
/// valor diferir, e o snapshot pode ser regenerado à mão. O snapshot fica associado ao teste corrente, em `__snapshots__`
/// ao lado do arquivo de teste.
pub fn assert_snapshot(value: &SnapshotValue, options: Option<&SnapshotOptions>) -> Result<(), SnapshotError> {
    let context = CONTEXT.with(|slot| slot.borrow().clone()).expect("assertSnapshot can only be used in a test");
    context.assert(value, options)
}
