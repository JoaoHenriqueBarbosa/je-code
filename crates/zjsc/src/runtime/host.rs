//! A fronteira do zjsc com a máquina onde roda. O motor não conhece o host: tudo o que ele lê do mundo (arquivos de
//! `/proc` e `/etc`, o ambiente, o diretório de trabalho, o pid, a entropia, a saída de diagnóstico, a entrega de
//! sinais) passa por este trait, instalado por thread por quem hospeda o programa. Dentro do pseudo-linus o
//! hospedeiro é o `ul-bun`, que responde com o `sysabi` do processo; o zjsc nunca vê o sistema de arquivos, o
//! ambiente nem os processos da máquina real. É o mesmo desenho do [`ConsoleHost`](crate::runtime::console_host) e do
//! [`ModuleFs`](crate::runtime::module_fs), só que ambiente (de thread) em vez de pendurado no global, porque os
//! chamadores (coletor, opções, `WTF::dataLog`) não têm um global à mão.
//!
//! Os relógios ficam fora do trait de propósito: `ul_common::time::realtime_nanos`, `monotonic_nanos` e `sleep` já
//! respondem com o relógio do pseudo-processo e só caem no do host fora de um sandbox.
//!
//! Sem host instalado vale o [`Detached`]: uma máquina vazia (nenhum arquivo, nenhuma variável, pid 1, uma CPU,
//! diagnóstico descartado), que não olha o sistema real. Os testes que comparam com o bun na máquina real instalam o
//! `StdHost` de `tests/common/std_host.rs`.

use std::cell::RefCell;
use std::rc::Rc;

/// Número de erro do Linux (`ENOENT` é 2), como o `sysabi` devolve.
pub type Errno = i32;

pub const ENOENT: Errno = 2;
pub const ESRCH: Errno = 3;
pub const EACCES: Errno = 13;
pub const ENOSYS: Errno = 38;
pub const EAGAIN: Errno = 11;
pub const ENOTTY: Errno = 25;

/// O `struct stat` inteiro, com os tempos em (segundos, nanossegundos).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FsStat {
    pub dev: u64,
    pub ino: u64,
    pub mode: u32,
    pub nlink: u64,
    pub uid: u32,
    pub gid: u32,
    pub rdev: u64,
    pub size: u64,
    pub blksize: u64,
    pub blocks: u64,
    pub atime: (i64, i64),
    pub mtime: (i64, i64),
    pub ctime: (i64, i64),
    pub birthtime: (i64, i64),
}

/// O `struct statfs` (os campos que o `fs.statfs` do Node mostra).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FsStatFs {
    pub fs_type: u64,
    pub bsize: u64,
    pub frsize: u64,
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
}

/// Uma entrada de diretório (sem `.` e `..`); `mode_type` são os bits `S_IFMT` do tipo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FsDirEntry {
    pub name: Vec<u8>,
    pub mode_type: u32,
}

/// Como um tempo do `utimes` é dado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FsTime {
    Now,
    Omit,
    At(i64, i64),
}

/// O que o motor pergunta de um caminho.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostStat {
    pub is_dir: bool,
    pub uid: u32,
    pub gid: u32,
}

/// Um arquivo aberto para escrita.
pub trait HostFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Errno>;
}

/// O [`HostFile`] como `std::io::Write`, para quem escreve por esse trait (o `dataFile` do `WTF::dataLog`).
pub struct HostWriter(pub Box<dyn HostFile>);

impl std::io::Write for HostWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write_all(buf).map_err(std::io::Error::from_raw_os_error)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// O que se espera de um descritor de rede em [`Host::net_poll`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetWatch {
    pub fd: i32,
    /// Pronto para `net_recv` (ou `net_accept`, num ouvinte).
    pub read: bool,
    /// Pronto para `net_send`.
    pub write: bool,
}

/// O que o `poll(2)` achou de um descritor: um [`NetWatch`] respondido, na mesma posição.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NetReady {
    pub readable: bool,
    pub writable: bool,
    /// O outro lado fechou (`POLLHUP`).
    pub hangup: bool,
    /// `POLLERR` ou `POLLNVAL`.
    pub error: bool,
}

impl NetReady {
    /// Algum evento aconteceu.
    pub fn any(&self) -> bool {
        self.readable || self.writable || self.hangup || self.error
    }
}

/// O que um descritor do filho recebe em [`Host::proc_spawn`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpawnStdio {
    /// `/dev/null`.
    Ignore,
    /// O descritor de mesmo número do processo.
    Inherit,
    /// Um `pipe(2)`: o filho fica com uma ponta, o processo com a outra (sem bloqueio).
    Pipe,
    /// Um `socketpair(AF_UNIX)`: a mesma ideia, com os dois sentidos (o `socket-fd` do bun).
    SocketPair,
    /// O descritor `n` do processo (um `dup2`).
    Fd(i32),
}

/// O pedido de [`Host::proc_spawn`]: o `posix_spawn` que o `Bun.spawn` faz.
#[derive(Clone, Debug, Default)]
pub struct SpawnRequest {
    /// O programa, como o chamador escreveu (com `/` vale o caminho, sem ele a busca no `PATH` é do host).
    pub file: String,
    pub argv: Vec<String>,
    /// O ambiente inteiro do filho; `None` herda o do processo.
    pub env: Option<Vec<(String, String)>>,
    pub cwd: Option<String>,
    /// Um item por descritor do filho, a partir do 0.
    pub stdio: Vec<SpawnStdio>,
    /// `setsid()` no filho.
    pub detached: bool,
}

/// O filho criado: o pid e, por descritor pedido como `Pipe` ou `SocketPair`, a ponta que ficou com o processo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Spawned {
    pub pid: i32,
    pub parent_fds: Vec<Option<i32>>,
}

/// Como um filho terminou.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcExit {
    Exited(i32),
    Signaled(i32),
}

/// A máquina vista pelo motor.
pub trait Host {
    /// Conteúdo inteiro do arquivo (`/proc/self/stat`, `/etc/hostname`, o arquivo de `function overrides`).
    fn read_file(&self, path: &str) -> Result<Vec<u8>, Errno>;
    /// `stat` seguindo ligações simbólicas.
    fn stat(&self, path: &str) -> Result<HostStat, Errno>;
    /// Abre para escrita, truncando; com `exclusive`, falha se já existir (`O_EXCL`).
    fn create_file(&self, path: &str, exclusive: bool) -> Result<Box<dyn HostFile>, Errno>;
    /// O ambiente do processo, na ordem do `environ`.
    fn environ(&self) -> Vec<(String, String)>;
    /// O valor de uma variável de ambiente.
    fn getenv(&self, name: &str) -> Option<String> {
        self.environ().into_iter().find(|(key, _)| key == name).map(|(_, value)| value)
    }
    /// O diretório de trabalho do processo.
    fn cwd(&self) -> String;
    fn pid(&self) -> u32;
    /// As CPUs em que o processo pode rodar.
    fn cpus(&self) -> usize;
    /// Bytes aleatórios do sistema (`getrandom`).
    fn random_fill(&self, bytes: &mut [u8]);
    /// `kill(pid, signal)` de um processo que não é o próprio.
    fn kill(&self, pid: i32, signal: i32) -> Result<(), Errno>;
    /// `setpriority(PRIO_PROCESS, pid, nice)`.
    fn set_priority(&self, pid: i32, nice: i32) -> Result<(), Errno>;
    /// Escreve no stderr do processo (diagnósticos do motor, sem passar pelo `console`).
    fn write_stderr(&self, bytes: &[u8]);
    /// Termina o processo.
    fn exit(&self, code: i32) -> !;

    // ---- sistema de arquivos (o que o `node:fs` pede, no nível das chamadas de sistema) ----
    /// `open(2)`: `flags` e `mode` como no Linux x86_64. Devolve o descritor.
    fn fs_open(&self, _path: &str, _flags: u32, _mode: u32) -> Result<i32, Errno> {
        Err(ENOSYS)
    }
    /// `isatty(3)`: o descritor é um terminal.
    fn tty_isatty(&self, _fd: i32) -> bool {
        false
    }
    /// `TIOCGWINSZ`: `(colunas, linhas)` do terminal em `fd`.
    fn tty_window_size(&self, _fd: i32) -> Result<(u16, u16), Errno> {
        Err(ENOTTY)
    }
    /// Liga ou desliga o modo cru do terminal em `fd` (o `uv_tty_set_mode` do libuv).
    fn tty_set_raw_mode(&self, _fd: i32, _raw: bool) -> Result<(), Errno> {
        Err(ENOTTY)
    }
    fn fs_close(&self, _fd: i32) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    /// `read(2)` de até `len` bytes (`pread(2)` quando há `offset`); vazio é o fim do arquivo.
    fn fs_read(&self, _fd: i32, _len: usize, _offset: Option<u64>) -> Result<Vec<u8>, Errno> {
        Err(ENOSYS)
    }
    /// `write(2)` (`pwrite(2)` quando há `offset`); devolve os bytes escritos.
    fn fs_write(&self, _fd: i32, _bytes: &[u8], _offset: Option<u64>) -> Result<usize, Errno> {
        Err(ENOSYS)
    }
    fn fs_fstat(&self, _fd: i32) -> Result<FsStat, Errno> {
        Err(ENOSYS)
    }
    /// `stat(2)` (`follow`) ou `lstat(2)`.
    fn fs_stat(&self, _path: &str, _follow: bool) -> Result<FsStat, Errno> {
        Err(ENOSYS)
    }
    /// As entradas do diretório, na ordem do sistema de arquivos, sem `.` e `..`.
    fn fs_readdir(&self, _path: &str) -> Result<Vec<FsDirEntry>, Errno> {
        Err(ENOSYS)
    }
    fn fs_mkdir(&self, _path: &str, _mode: u32) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_unlink(&self, _path: &str) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_rmdir(&self, _path: &str) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_rename(&self, _from: &str, _to: &str) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_link(&self, _from: &str, _to: &str) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_symlink(&self, _target: &str, _path: &str) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_readlink(&self, _path: &str) -> Result<String, Errno> {
        Err(ENOSYS)
    }
    fn fs_chmod(&self, _path: &str, _mode: u32) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_fchmod(&self, _fd: i32, _mode: u32) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    /// `utimensat(2)`; `follow` falso é o `lutimes`.
    fn fs_utimes(&self, _path: &str, _atime: FsTime, _mtime: FsTime, _follow: bool) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_futimes(&self, _fd: i32, _atime: FsTime, _mtime: FsTime) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_ftruncate(&self, _fd: i32, _len: u64) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_fsync(&self, _fd: i32) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    /// `access(2)` com `mode` (`F_OK` 0, `R_OK` 4, `W_OK` 2, `X_OK` 1).
    fn fs_access(&self, _path: &str, _mode: u32) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    /// `chown(2)` (`follow`) ou `lchown(2)`; `u32::MAX` deixa o dono (ou o grupo) como está.
    fn fs_chown(&self, _path: &str, _uid: u32, _gid: u32, _follow: bool) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_fchown(&self, _fd: i32, _uid: u32, _gid: u32) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    fn fs_statfs(&self, _path: &str) -> Result<FsStatFs, Errno> {
        Err(ENOSYS)
    }
    /// `realpath(3)`.
    fn fs_realpath(&self, _path: &str) -> Result<String, Errno> {
        Err(ENOSYS)
    }

    // ---- rede (TCP de loopback, sem bloqueio; o descritor fecha com `fs_close`) ----
    /// `socket` + `bind` + `listen` em `127.0.0.1`; a porta 0 sorteia uma efêmera. Devolve o descritor e a porta.
    fn net_listen(&self, _port: u16, _backlog: u32) -> Result<(i32, u16), Errno> {
        Err(ENOSYS)
    }
    /// `accept4`: a conexão e a porta de quem conectou; `EAGAIN` sem conexão na fila.
    fn net_accept(&self, _fd: i32) -> Result<(i32, u16), Errno> {
        Err(ENOSYS)
    }
    /// `socket` + `connect` a `127.0.0.1:port`: a conexão e a porta local; `ECONNREFUSED` sem ouvinte.
    fn net_connect(&self, _port: u16) -> Result<(i32, u16), Errno> {
        Err(ENOSYS)
    }
    /// `recv` de até `max` bytes; vazio é o fim da conexão e `EAGAIN` é nada a ler por enquanto.
    fn net_recv(&self, _fd: i32, _max: usize) -> Result<Vec<u8>, Errno> {
        Err(ENOSYS)
    }
    /// `send`; devolve os bytes aceitos (`EAGAIN` com o buffer cheio, `EPIPE` com o outro lado fechado).
    fn net_send(&self, _fd: i32, _bytes: &[u8]) -> Result<usize, Errno> {
        Err(ENOSYS)
    }
    /// `shutdown(2)` da leitura, da escrita ou das duas.
    fn net_shutdown(&self, _fd: i32, _read: bool, _write: bool) -> Result<(), Errno> {
        Err(ENOSYS)
    }
    /// `poll(2)` de `watch`, esperando até `timeout` (`None` espera sem prazo); a resposta tem o tamanho de `watch`.
    fn net_poll(&self, _watch: &[NetWatch], _timeout: Option<std::time::Duration>) -> Result<Vec<NetReady>, Errno> {
        Err(ENOSYS)
    }

    // ---- processos filhos ----
    /// `posix_spawn`: cria o filho com os descritores de `request.stdio`. O `ENOENT` de um programa que não existe vem aqui.
    fn proc_spawn(&self, _request: &SpawnRequest) -> Result<Spawned, Errno> {
        Err(ENOSYS)
    }
    /// `wait4(pid)`: como o filho terminou; `None` quando `nohang` e ele ainda roda.
    fn proc_wait(&self, _pid: i32, _nohang: bool) -> Result<Option<ProcExit>, Errno> {
        Err(ENOSYS)
    }
}

/// A máquina vazia de quando ninguém instalou um host.
#[derive(Debug, Default)]
pub struct Detached;

impl Host for Detached {
    fn read_file(&self, _path: &str) -> Result<Vec<u8>, Errno> {
        Err(ENOENT)
    }
    fn stat(&self, _path: &str) -> Result<HostStat, Errno> {
        Err(ENOENT)
    }
    fn create_file(&self, _path: &str, _exclusive: bool) -> Result<Box<dyn HostFile>, Errno> {
        Err(EACCES)
    }
    fn environ(&self) -> Vec<(String, String)> {
        Vec::new()
    }
    fn cwd(&self) -> String {
        String::from("/")
    }
    fn pid(&self) -> u32 {
        1
    }
    fn cpus(&self) -> usize {
        1
    }
    fn random_fill(&self, bytes: &mut [u8]) {
        use std::hash::{BuildHasher, Hasher};
        for chunk in bytes.chunks_mut(8) {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_u8(0);
            chunk.copy_from_slice(&hasher.finish().to_le_bytes()[..chunk.len()]);
        }
    }
    fn kill(&self, _pid: i32, _signal: i32) -> Result<(), Errno> {
        Err(ESRCH)
    }
    fn set_priority(&self, _pid: i32, _nice: i32) -> Result<(), Errno> {
        Err(ESRCH)
    }
    fn write_stderr(&self, _bytes: &[u8]) {}
    fn exit(&self, code: i32) -> ! {
        panic!("exit({code}) sem host instalado")
    }
}

thread_local! {
    static HOST: RefCell<Option<Rc<dyn Host>>> = const { RefCell::new(None) };
}

/// Instala o host desta thread e devolve o anterior.
pub fn install(host: Rc<dyn Host>) -> Option<Rc<dyn Host>> {
    HOST.with(|slot| slot.borrow_mut().replace(host))
}

/// Tira o host desta thread (volta ao [`Detached`]).
pub fn uninstall() -> Option<Rc<dyn Host>> {
    HOST.with(|slot| slot.borrow_mut().take())
}

/// O host desta thread, ou o [`Detached`] (também quando a thread já está desfazendo os locais, e um `Drop` de outro
/// local ainda pergunta ao host).
pub fn host() -> Rc<dyn Host> {
    HOST.try_with(|slot| slot.borrow().clone()).ok().flatten().unwrap_or_else(|| Rc::new(Detached))
}

/// Escreve `text` no stderr do host, com a quebra de linha do `eprintln`.
pub fn eprintln(text: &str) {
    host().write_stderr(format!("{text}\n").as_bytes());
}

/// O arquivo como texto, sem a quebra de linha final (o formato de `/proc` e `/etc` que o motor lê).
pub fn read_text(path: &str) -> Option<String> {
    let bytes = host().read_file(path).ok()?;
    Some(String::from_utf8(bytes).ok()?.trim_end_matches('\n').to_string())
}

#[cfg(test)]
#[path = "../../tests/common/std_host.rs"]
pub mod std_host;
