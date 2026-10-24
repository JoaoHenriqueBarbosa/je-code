//! Porte de `WTF/wtf/DataLog.h` e `DataLog.cpp`: o `dataFile()` para onde vão os dumps de
//! depuração (`dumpGeneratedBytecodes` e afins), `stderr` até alguém trocá-lo com `setDataFile`.
//!
//! O `PrintStream` do C++ é um `std::io::Write`. O arquivo é global no C++ (com trava); aqui é por
//! thread, como o resto do estado do `VM` deste porte. `dataLogF` e as variantes com `va_list` não
//! existem: quem chama formata com `format!`.

use std::cell::RefCell;
use std::io::Write;

use crate::runtime::host::{host, HostWriter};

thread_local! {
    static DATA_FILE: RefCell<Option<Box<dyn Write>>> = const { RefCell::new(None) };
}

/// `setDataFile(std::unique_ptr<PrintStream>&&)`.
pub fn set_data_file(file: Box<dyn Write>) {
    DATA_FILE.with(|data_file| *data_file.borrow_mut() = Some(file));
}

/// `setDataFile(const char* path)`: abre (truncando) o arquivo; se não abrir, o C++ avisa em
/// `stderr` e continua com o arquivo anterior.
pub fn set_data_file_path(path: &str) {
    match host().create_file(path, false) {
        Ok(file) => set_data_file(Box::new(HostWriter(file))),
        Err(_) => crate::runtime::host::eprintln(&format!("Warning: Could not open log file {path} for writing.")),
    }
}

/// `dataLog(values...)` com o texto já formatado: escreve no `dataFile()` e dá `flush`.
pub fn data_log(text: &str) {
    DATA_FILE.with(|data_file| match data_file.borrow_mut().as_mut() {
        Some(file) => {
            let _ = file.write_all(text.as_bytes());
            let _ = file.flush();
        }
        None => {
            host().write_stderr(text.as_bytes());
        }
    });
}
