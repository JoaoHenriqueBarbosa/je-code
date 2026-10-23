//! Sonda H15 e H16: o zjsc (JavaScriptCore em Rust do pseudo-linus) como motor do extension host.
//!
//! Uso: `exthost-probe run <arquivo.js> [args...]` roda o arquivo como programa principal CommonJS, com o
//! `ModuleFs` sobre `std::fs` e o console no stdio do processo (o mesmo contrato do `bench_run` do zjsc).

mod host;

use std::cell::RefCell;
use std::io::Write;
use std::rc::Rc;

use zjsc::api::eval::{evaluate_main_script, set_main_module_fs};
use zjsc::runtime::console_host::ConsoleHost;
use zjsc::runtime::module_fs::{EntryKind, ModuleFs};
use zjsc::runtime::process_system::set_script_arguments;
use zjsc::runtime::vm::VM;

#[derive(Debug, Default)]
struct StdConsole {
    pending: RefCell<Vec<u8>>,
}

impl ConsoleHost for StdConsole {
    fn write_stdout(&self, bytes: &[u8]) {
        let _ = std::io::stdout().write_all(bytes);
    }
    fn write_stderr(&self, bytes: &[u8]) {
        let _ = std::io::stderr().write_all(bytes);
    }
    fn read_stdin_line(&self) -> Option<String> {
        None
    }
    fn read_stdin_rest(&self) -> Vec<u8> {
        std::mem::take(&mut *self.pending.borrow_mut())
    }
}

#[derive(Debug)]
struct DiskFs;

impl ModuleFs for DiskFs {
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

const STACK_BYTES: usize = 256 * 1024 * 1024;

fn main() {
    let code = std::thread::Builder::new().stack_size(STACK_BYTES).spawn(run).expect("thread").join().unwrap_or(70);
    std::process::exit(code);
}

fn run() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(file) = args.first() else {
        eprintln!("uso: exthost-probe <arquivo.js> [args...]");
        return 2;
    };
    let path = std::fs::canonicalize(file).map_or_else(|_| file.clone(), |p| p.to_string_lossy().into_owned());
    let Ok(source) = std::fs::read_to_string(&path) else {
        eprintln!("{file}: não leu");
        return 2;
    };
    zjsc::runtime::host::install(Rc::new(host::ProbeHost::default()));
    VM::set_thread_stack_budget(STACK_BYTES - 16 * 1024 * 1024);
    let rest: Vec<&str> = args[1..].iter().map(String::as_str).collect();
    set_script_arguments(&rest);
    set_main_module_fs(Some(Rc::new(DiskFs)));
    evaluate_main_script(&source, &path, Rc::new(StdConsole::default()))
}
