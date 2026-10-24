//! Utilitários de E/S dos programas sobre o `sysabi`: argv em bytes, mensagens de erro no formato do
//! `error(3)`, saída com buffer, leitura de arquivo inteiro (com `-` = entrada padrão) e `stat`.
//!
//! Nada aqui toca o host: tudo passa por `sys`.

use std::ffi::OsString;

use crate::sys::{self, SysResult};
use crate::{AtFlags, Errno, Fd, Mode, OFlags, Stat};

/// argv do `main` em bytes.
#[cfg(unix)]
pub fn args_bytes(args: &[OsString]) -> Vec<Vec<u8>> {
    use std::os::unix::ffi::OsStrExt;
    args.iter().map(|a| a.as_bytes().to_vec()).collect()
}

/// Fora do Unix o `OsString` é WTF-8: o argv sai em UTF-8 com substituição.
#[cfg(not(unix))]
pub fn args_bytes(args: &[OsString]) -> Vec<Vec<u8>> {
    args.iter().map(|a| a.to_string_lossy().into_owned().into_bytes()).collect()
}

/// argv[0] como foi chamado (os programas GNU usam o caminho completo nas mensagens:
/// `/usr/bin/diff: invalid option -- 'k'`).
pub fn argv0(args: &[Vec<u8>]) -> String {
    args.first().map(|a| String::from_utf8_lossy(a).into_owned()).unwrap_or_default()
}

/// Escreve no stderr, ignorando erro (não há pra onde reportar).
pub fn eprint(msg: impl AsRef<[u8]>) {
    let _ = sys::write_all(Fd::STDERR, msg.as_ref());
}

/// `argv0: msg\n` no stderr, como o `error(3)`.
pub fn error(argv0: &str, msg: impl AsRef<[u8]>) {
    let mut line = Vec::with_capacity(argv0.len() + 2 + msg.as_ref().len() + 1);
    line.extend_from_slice(argv0.as_bytes());
    line.extend_from_slice(b": ");
    line.extend_from_slice(msg.as_ref());
    line.push(b'\n');
    eprint(line);
}

/// `argv0: nome: strerror\n`.
pub fn error_path(argv0: &str, path: &[u8], e: Errno) {
    let mut msg = path.to_vec();
    msg.extend_from_slice(b": ");
    msg.extend_from_slice(e.message().as_bytes());
    error(argv0, msg);
}

/// Saída com buffer sobre um fd, com os pontos de descarga do stdio da glibc: em bloco de
/// `st_blksize` (4096 em pipe, tmpfs e ext4) quando o fd não é terminal, por linha quando é. Isso
/// importa quando stdout e stderr vão pro mesmo lugar (`2>&1`): a ordem das linhas fica igual à do
/// programa GNU. Quem imita o `error(3)` do gnulib deve chamar [`Output::flush`] antes de escrever no
/// stderr, como ele faz com o `fflush(stdout)`. Guarda o primeiro erro de escrita; `finish` devolve
/// ele.
pub struct Output {
    fd: Fd,
    buf: Vec<u8>,
    err: Option<Errno>,
    block: usize,
    line_buffered: bool,
}

impl Output {
    pub fn new(fd: Fd) -> Output {
        let (block, line_buffered) = match sys::try_current() {
            Some(s) => {
                let blk = s.fstat(fd).map(|st| st.blksize as usize).unwrap_or(0);
                (if blk > 0 { blk } else { 4096 }, s.isatty(fd))
            }
            None => (4096, false),
        };
        Output { fd, buf: Vec::new(), err: None, block, line_buffered }
    }

    pub fn stdout() -> Output {
        Output::new(Fd::STDOUT)
    }

    fn raw_write(&mut self, data: &[u8]) {
        if self.err.is_none() && !data.is_empty()
            && let Err(e) = sys::write_all(self.fd, data) {
                self.err = Some(e);
            }
    }

    pub fn write(&mut self, data: &[u8]) {
        if self.err.is_some() {
            return;
        }
        if self.line_buffered {
            self.buf.extend_from_slice(data);
            if let Some(nl) = data.iter().rposition(|&b| b == b'\n') {
                let keep = data.len() - nl - 1;
                let upto = self.buf.len() - keep;
                let head: Vec<u8> = self.buf.drain(..upto).collect();
                self.raw_write(&head);
            }
            return;
        }
        let room = self.block - self.buf.len();
        if data.len() < room {
            self.buf.extend_from_slice(data);
            return;
        }
        // Como o `fwrite` da glibc: completa o bloco e descarrega, escreve direto os blocos inteiros
        // que sobrarem e guarda o resto.
        let (first, rest) = data.split_at(room);
        self.buf.extend_from_slice(first);
        let full = std::mem::take(&mut self.buf);
        self.raw_write(&full);
        let direct = rest.len() - rest.len() % self.block;
        self.raw_write(&rest[..direct]);
        self.buf.extend_from_slice(&rest[direct..]);
    }

    pub fn write_str(&mut self, s: &str) {
        self.write(s.as_bytes());
    }

    pub fn flush(&mut self) {
        let pending = std::mem::take(&mut self.buf);
        self.raw_write(&pending);
    }

    /// Esvazia o buffer e devolve o primeiro erro de escrita, se houve.
    pub fn finish(&mut self) -> Result<(), Errno> {
        self.flush();
        match self.err.take() {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }

    pub fn error(&self) -> Option<Errno> {
        self.err
    }
}

impl std::io::Write for Output {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Output::write(self, buf);
        match self.err {
            Some(e) => Err(e.to_io()),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Output::flush(self);
        match self.err {
            Some(e) => Err(e.to_io()),
            None => Ok(()),
        }
    }
}

impl Drop for Output {
    fn drop(&mut self) {
        self.flush();
    }
}

pub use crate::sys::{lstat, read_to_end as read_fd, stat};

/// Lê um arquivo inteiro; `-` é a entrada padrão.
pub fn read_path(path: &[u8]) -> SysResult<Vec<u8>> {
    if path == b"-" {
        return read_fd(Fd::STDIN);
    }
    let fd = sys::open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
    let r = read_fd(fd);
    let _ = sys::close(fd);
    r
}

/// `open(path, O_RDONLY)`.
pub fn open_read(path: &[u8]) -> SysResult<Fd> {
    sys::open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0)
}

/// Cria (ou trunca) um arquivo pra escrita com `mode` (a umask do processo se aplica).
pub fn create(path: &[u8], mode: Mode) -> SysResult<Fd> {
    sys::open(path, OFlags::WRONLY | OFlags::CREAT | OFlags::TRUNC | OFlags::CLOEXEC, mode)
}

/// Escreve um arquivo inteiro (cria ou trunca).
pub fn write_file(path: &[u8], data: &[u8], mode: Mode) -> SysResult<()> {
    let fd = create(path, mode)?;
    let r = sys::write_all(fd, data);
    let c = sys::close(fd);
    r.and(c)
}

pub fn fstat(fd: Fd) -> SysResult<Stat> {
    sys::current().fstat(fd)
}

pub fn exists(path: &[u8]) -> bool {
    sys::current().fstatat(Fd::CWD, path, AtFlags::SYMLINK_NOFOLLOW).is_ok()
}

/// Junta diretório e nome com uma barra só.
pub fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"/") && !p.is_empty() {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

/// Último componente (sem barras finais), como o `basename` do GNU usa.
pub fn basename(path: &[u8]) -> &[u8] {
    let mut end = path.len();
    while end > 1 && path[end - 1] == b'/' {
        end -= 1;
    }
    let p = &path[..end];
    match p.iter().rposition(|&b| b == b'/') {
        Some(i) if i + 1 < p.len() => &p[i + 1..],
        _ => p,
    }
}

/// Valor de uma variável de ambiente do processo corrente.
pub fn getenv(name: &str) -> Option<Vec<u8>> {
    sys::try_current().and_then(|s| s.getenv(name.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_helpers() {
        assert_eq!(join(b"a", b"b"), b"a/b");
        assert_eq!(join(b"a/", b"b"), b"a/b");
        assert_eq!(basename(b"a/b/"), b"b");
        assert_eq!(basename(b"/"), b"/");
        assert_eq!(basename(b"x"), b"x");
    }
}
