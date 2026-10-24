//! O que o runtime precisa do console do processo hospedeiro: escrever no stdout e no stderr e ler uma linha do
//! stdin. `alert`, `confirm` e `prompt` (ver `dialogs.rs`) são os clientes. Como o `ModuleFs` de
//! `api/module_probe.rs`, o runtime não conhece o processo: quem o hospeda fornece a implementação (a do sandbox
//! liga ao VFS e ao terminal da sessão, [`MemoryConsole`] serve aos testes). Sem host instalado, a saída é
//! descartada e o stdin está em EOF.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use crate::runtime::js_global_object::JSGlobalObject;

/// O console do hospedeiro.
pub trait ConsoleHost: std::fmt::Debug {
    /// Escreve bytes crus no stdout, sem acrescentar quebra de linha.
    fn write_stdout(&self, bytes: &[u8]);
    /// Escreve bytes crus no stderr, sem acrescentar quebra de linha.
    fn write_stderr(&self, bytes: &[u8]);
    /// Lê do stdin até o próximo `\n` e devolve a linha sem ele (um `\r` antes fica). `None` em EOF, inclusive
    /// quando o stdin acaba no meio de uma linha sem `\n` final (o resto é descartado).
    fn read_stdin_line(&self) -> Option<String>;
    /// Lê todo o resto do stdin, byte a byte (inclusive a última linha sem `\n`); vazio em EOF. É o que o
    /// `process.stdin` entrega em `data`.
    fn read_stdin_rest(&self) -> Vec<u8>;
}

impl JSGlobalObject {
    /// Instala o console do hospedeiro e o `ConsoleClient` que escreve nele (`console_client.rs`).
    pub fn set_console_host(&self, host: Rc<dyn ConsoleHost>) {
        *self.console_host.borrow_mut() = Some(host);
        crate::runtime::console_client::install(self);
    }

    /// O console instalado, se houver.
    pub fn console_host(&self) -> Option<Rc<dyn ConsoleHost>> {
        self.console_host.borrow().clone()
    }
}

/// Console em memória: stdin fixo, stdout e stderr acumulados.
#[derive(Debug, Default)]
pub struct MemoryConsole {
    stdin: Vec<u8>,
    position: Cell<usize>,
    stdout: RefCell<Vec<u8>>,
    stderr: RefCell<Vec<u8>>,
}

impl MemoryConsole {
    pub fn new(stdin: &[u8]) -> MemoryConsole {
        MemoryConsole { stdin: stdin.to_vec(), ..MemoryConsole::default() }
    }

    /// Tudo o que foi escrito no stdout, byte a byte.
    pub fn stdout_bytes(&self) -> Vec<u8> {
        self.stdout.borrow().clone()
    }

    /// Tudo o que foi escrito no stderr, byte a byte.
    pub fn stderr_bytes(&self) -> Vec<u8> {
        self.stderr.borrow().clone()
    }

    /// O stdout como texto (UTF-8 inválido vira U+FFFD).
    pub fn stdout(&self) -> String {
        String::from_utf8_lossy(&self.stdout.borrow()).into_owned()
    }

    /// O stderr como texto (UTF-8 inválido vira U+FFFD).
    pub fn stderr(&self) -> String {
        String::from_utf8_lossy(&self.stderr.borrow()).into_owned()
    }
}

impl ConsoleHost for MemoryConsole {
    fn write_stdout(&self, bytes: &[u8]) {
        self.stdout.borrow_mut().extend_from_slice(bytes);
    }

    fn write_stderr(&self, bytes: &[u8]) {
        self.stderr.borrow_mut().extend_from_slice(bytes);
    }

    fn read_stdin_line(&self) -> Option<String> {
        let rest = &self.stdin[self.position.get()..];
        let end = rest.iter().position(|&byte| byte == b'\n')?;
        self.position.set(self.position.get() + end + 1);
        Some(String::from_utf8_lossy(&rest[..end]).into_owned())
    }

    fn read_stdin_rest(&self) -> Vec<u8> {
        let rest = self.stdin[self.position.get()..].to_vec();
        self.position.set(self.stdin.len());
        rest
    }
}
