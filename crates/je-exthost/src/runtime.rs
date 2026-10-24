//! O que o motor JavaScript pede ao hospedeiro além do [`Host`](zjsc::runtime::host::Host): o sistema de arquivos dos
//! módulos (`require` e `import`), o console e a thread com a pilha que o interpretador espera.

use std::io::Write;
use std::rc::Rc;

use zjsc::api::module_probe::{EntryKind, ModuleFs};
use zjsc::runtime::console_host::ConsoleHost;
use zjsc::runtime::vm::VM;

/// A pilha da thread do motor: o interpretador do zjsc recursa fundo (o `typescript.js` pede mais de 100 MB).
pub const STACK_BYTES: usize = 256 * 1024 * 1024;
/// O que sobra para o Rust fora do orçamento do interpretador.
const STACK_RESERVE: usize = 16 * 1024 * 1024;

/// O sistema de arquivos dos módulos sobre o disco.
#[derive(Debug, Default)]
pub struct DiskModuleFs;

impl ModuleFs for DiskModuleFs {
    fn read_file(&self, path: &str) -> Option<String> {
        String::from_utf8(std::fs::read(path).ok()?).ok()
    }

    fn read_bytes(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(path).ok()
    }

    fn stat(&self, path: &str) -> Option<EntryKind> {
        let metadata = std::fs::metadata(path).ok()?;
        if metadata.is_dir() {
            Some(EntryKind::Directory)
        } else if metadata.is_file() {
            Some(EntryKind::File)
        } else {
            None
        }
    }

    fn realpath(&self, path: &str) -> Option<String> {
        Some(std::fs::canonicalize(path).ok()?.to_string_lossy().into_owned())
    }
}

/// O `console` do programa no stdout e no stderr do processo.
#[derive(Debug, Default)]
pub struct OsConsole;

impl ConsoleHost for OsConsole {
    fn write_stdout(&self, bytes: &[u8]) {
        let _ = std::io::stdout().write_all(bytes);
    }

    fn write_stderr(&self, bytes: &[u8]) {
        let _ = std::io::stderr().write_all(bytes);
    }

    fn read_stdin_line(&self) -> Option<String> {
        let mut line = String::new();
        (std::io::stdin().read_line(&mut line).ok()? > 0).then_some(line)
    }

    fn read_stdin_rest(&self) -> Vec<u8> {
        let mut rest = Vec::new();
        let _ = std::io::Read::read_to_end(&mut std::io::stdin(), &mut rest);
        rest
    }
}

/// Roda `body` numa thread com a pilha do motor, com o [`OsHost`](crate::os_host::OsHost) instalado e o orçamento de pilha
/// do interpretador posto, e devolve o resultado. O motor é preso à thread (`Rc` por dentro): tudo o que o programa JavaScript
/// faz acontece dentro de `body`.
pub fn with_runtime_thread<T: Send + 'static>(body: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .name("je-exthost".into())
        .stack_size(STACK_BYTES)
        .spawn(move || {
            zjsc::runtime::host::install(Rc::new(crate::os_host::OsHost::new()));
            VM::set_thread_stack_budget(STACK_BYTES - STACK_RESERVE);
            body()
        })
        .expect("thread do motor")
        .join()
        .unwrap_or_else(|panic| std::panic::resume_unwind(panic))
}
