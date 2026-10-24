//! `Ctx`: o que um builtin recebe. É uma alça fina sobre o processo corrente, com os fluxos padrão
//! como `std::io::Read`/`Write` sobre fds do pseudo-linus.

use std::ffi::OsString;
use std::io;
use std::sync::Arc;

use crate::linux::Errno;
use crate::sys::{self, Syscalls};
use crate::types::Fd;

/// Leitor de um fd do pseudo-linus. EINTR é repetido.
#[derive(Clone, Copy, Debug)]
pub struct FdReader(pub Fd);

/// Escritor de um fd do pseudo-linus (sem buffer; embrulhe num `BufWriter` quando fizer sentido).
#[derive(Clone, Copy, Debug)]
pub struct FdWriter(pub Fd);

impl io::Read for FdReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        loop {
            match sys::read(self.0, buf) {
                Err(Errno::EINTR) => continue,
                r => return r.map_err(Errno::to_io),
            }
        }
    }
}

impl io::Write for FdWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        loop {
            match sys::write(self.0, buf) {
                Err(Errno::EINTR) => continue,
                r => return r.map_err(Errno::to_io),
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Contexto de execução de um builtin.
pub struct Ctx {
    sys: Arc<dyn Syscalls>,
    /// Nome com que o programa foi chamado (basename do argv[0]), pras mensagens de erro.
    pub prog: String,
}

impl Ctx {
    /// Contexto do processo corrente da thread.
    pub fn current(argv0: &[u8]) -> Ctx {
        let base = argv0.rsplit(|b| *b == b'/').next().unwrap_or(argv0);
        Ctx { sys: sys::current(), prog: String::from_utf8_lossy(base).into_owned() }
    }

    pub fn sys(&self) -> &Arc<dyn Syscalls> {
        &self.sys
    }

    pub fn stdin(&self) -> FdReader {
        FdReader(Fd::STDIN)
    }

    pub fn stdout(&self) -> FdWriter {
        FdWriter(Fd::STDOUT)
    }

    pub fn stderr(&self) -> FdWriter {
        FdWriter(Fd::STDERR)
    }

    /// `prog: msg` no stderr, como o `error(3)` da glibc.
    pub fn error(&self, msg: impl AsRef<str>) {
        let line = format!("{}: {}\n", self.prog, msg.as_ref());
        let _ = sys::write_all(Fd::STDERR, line.as_bytes());
    }

    pub fn getenv(&self, name: &str) -> Option<Vec<u8>> {
        self.sys.getenv(name.as_bytes())
    }

}

/// Converte o argv de bytes pro formato do `main` dos builtins.
#[cfg(unix)]
pub fn to_os_args(argv: &[Vec<u8>]) -> Vec<OsString> {
    use std::os::unix::ffi::OsStringExt;
    argv.iter().map(|a| OsString::from_vec(a.clone())).collect()
}

/// Fora do Unix o `OsString` não é uma sequência de bytes arbitrária: o argv passa por UTF-8 com substituição.
#[cfg(not(unix))]
pub fn to_os_args(argv: &[Vec<u8>]) -> Vec<OsString> {
    argv.iter().map(|a| OsString::from(String::from_utf8_lossy(a).into_owned())).collect()
}
