//! Kernel de teste em memória (feature `testkit`).
//!
//! Serve pra testar programas antes do kernel real. É **síncrono e de uma thread**: `spawn` e
//! `spawn_fn` rodam o filho até o fim antes de voltar, e `wait4` só colhe quem já terminou. Com isso:
//!
//! - pipes não bloqueiam: escrita acumula até 1 MiB (aí o escritor leva EPIPE/SIGPIPE, que é o que
//!   acontece com `yes | head` no Linux, só que antes) e leitura de pipe vazio é EOF;
//! - sinais pra outros processos quase sempre chegam tarde (eles já terminaram);
//! - não há escalonador, rlimits, procfs nem permissões (todo mundo é root).
//!
//! O resto segue o Linux: inodes, hardlinks, symlinks com limite de 40, `..`, barra final, errnos,
//! open file descriptions compartilhadas por `dup`, CLOEXEC, `O_APPEND`, `readdir` na ordem do tmpfs
//! (mais novo primeiro), `execve` de builtin e de `#!`.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::ctx::{Ctx, to_os_args};
use crate::itimer::{Itimer, ItimerSlot, Itimerval};
use crate::linux::{DefaultAction, Errno, Signal};
use crate::program::{Main, Program};
use crate::sched::{self, SchedAttr, SchedCaller, SchedParam, SchedState};
use crate::sys::{self, ExecUnwind, ExitUnwind, KillUnwind, SysResult, Syscalls};
use crate::types::*;

/// 2026-01-15T12:00:00Z, o mesmo instante das fixtures da bancada.
pub const DEFAULT_TIME: i64 = 1_768_478_400;
const PIPE_LIMIT: usize = 1 << 20;
const MAXSYMLINKS: usize = 40;
const PATH_MAX: usize = 4096;
const NAME_MAX: usize = 255;

/// Ambiente padrão, o mesmo do oráculo da bancada.
pub const BASE_ENV: &[(&str, &str)] = &[
    ("PATH", "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin"),
    ("HOME", "/root"),
    ("USER", "root"),
    ("LOGNAME", "root"),
    ("SHELL", "/bin/bash"),
    ("LC_ALL", "C.UTF-8"),
    ("TZ", "UTC"),
];

type Ino = u64;

#[derive(Clone, Debug)]
enum Kind {
    File(Vec<u8>),
    Dir(BTreeMap<Vec<u8>, (Ino, u64)>),
    Symlink(Vec<u8>),
    Builtin(String),
    Device(Dev),
    Fifo,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dev {
    Null,
    Zero,
    Full,
    Random,
    Tty,
}

#[derive(Clone, Debug)]
struct Node {
    kind: Kind,
    mode: Mode,
    uid: Uid,
    gid: Gid,
    nlink: u64,
    atime: TimeSpec,
    mtime: TimeSpec,
    ctime: TimeSpec,
}

impl Node {
    fn type_bits(&self) -> Mode {
        match &self.kind {
            Kind::File(_) | Kind::Builtin(_) => mode::S_IFREG,
            Kind::Dir(_) => mode::S_IFDIR,
            Kind::Symlink(_) => mode::S_IFLNK,
            Kind::Device(_) => mode::S_IFCHR,
            Kind::Fifo => mode::S_IFIFO,
        }
    }

    fn file_type(&self) -> FileType {
        FileType::from_mode(self.type_bits())
    }
}

#[derive(Debug)]
struct PipeBuf {
    data: VecDeque<u8>,
    readers: usize,
    writers: usize,
}

/// Conexão de rede de teste (ver [`TestKit::net`]).
pub trait NetStream: std::io::Read + std::io::Write + Send {
    fn peer(&self) -> std::net::SocketAddr;
    fn local(&self) -> std::net::SocketAddr;
    fn set_read_timeout(&self, d: Option<Duration>) -> std::io::Result<()>;
}

/// Quem atende `net_connect` no testkit: recebe host e porta, devolve a conexão ou o errno.
pub type NetHandler = Arc<dyn Fn(&[u8], u16) -> SysResult<Box<dyn NetStream>> + Send + Sync>;

struct Socket {
    stream: Box<dyn NetStream>,
}

impl std::fmt::Debug for Socket {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Socket({})", self.stream.peer())
    }
}

#[derive(Debug)]
enum OpenKind {
    Inode(Ino),
    Dir { ino: Ino, listing: Option<Vec<DirEntry>> },
    PipeRead(Arc<Mutex<PipeBuf>>),
    PipeWrite(Arc<Mutex<PipeBuf>>),
    Socket(Arc<Mutex<Socket>>),
}

#[derive(Debug)]
struct OpenFile {
    kind: OpenKind,
    flags: OFlags,
    offset: u64,
}

impl Drop for OpenFile {
    fn drop(&mut self) {
        match &self.kind {
            OpenKind::PipeRead(p) => lock(p).readers -= 1,
            OpenKind::PipeWrite(p) => lock(p).writers -= 1,
            _ => {}
        }
    }
}

#[derive(Clone, Debug)]
struct FdEntry {
    file: Arc<Mutex<OpenFile>>,
    cloexec: bool,
}

#[derive(Clone, Debug)]
struct Proc {
    pid: Pid,
    ppid: Pid,
    pgid: Pid,
    sid: Pid,
    cwd: Vec<u8>,
    env: Vec<Vec<u8>>,
    argv: Vec<Vec<u8>>,
    umask: Mode,
    fds: BTreeMap<i32, FdEntry>,
    sigs: BTreeMap<i32, SigDisposition>,
    caught: Vec<Signal>,
    pending_fatal: Option<Signal>,
    status: Option<WaitStatus>,
    reaped: bool,
    rlimits: BTreeMap<Resource, Rlimit>,
    nice: i32,
    sched: SchedState,
    /// `io_context->ioprio` (0 = classe NONE, o padrão).
    ioprio: i32,
    persona: u32,
    /// Máscara gravada por `sched_setaffinity`; `None` são todas as CPUs.
    cpus: Option<Vec<usize>>,
    /// Os timers de `setitimer`, pelo relógio virtual (só o de parede vence: não há CPU a contar).
    itimers: [ItimerSlot; 3],
}

struct World {
    nodes: BTreeMap<Ino, Node>,
    next_ino: Ino,
    seq: u64,
    programs: BTreeMap<String, Main>,
    procs: BTreeMap<Pid, Proc>,
    next_pid: Pid,
    now: TimeSpec,
    hostname: Vec<u8>,
    domainname: Vec<u8>,
    rng: u64,
    net: Option<NetHandler>,
    /// Travas OFD por inode; o dono é a open file description (fraca: some sozinha quando o último fd
    /// que a referencia fecha).
    locks: BTreeMap<Ino, Vec<OfdLock>>,
    ncpus: usize,
    next_tid: Tid,
    /// Threads ainda não juntadas. Cada uma roda numa thread do hospedeiro, com o processo instalado,
    /// e devolve o desenrolar de processo (`exit`, sinal fatal, `execve`) que saiu dela, se saiu.
    threads: BTreeMap<Tid, std::thread::JoinHandle<Option<Box<dyn std::any::Any + Send>>>>,
}

/// Trava OFD: a open file description dona (fraca) e a trava em si.
type OfdLock = (std::sync::Weak<Mutex<OpenFile>>, FileLock);

const ROOT: Ino = 1;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Copia de `data` a partir de `off` para `buf`; offset além do fim lê 0 bytes, como no Linux.
fn copy_out(data: &[u8], off: usize, buf: &mut [u8]) -> usize {
    let start = off.min(data.len());
    let n = (data.len() - start).min(buf.len());
    buf[..n].copy_from_slice(&data[start..start + n]);
    n
}

/// Grava `buf` em `data` a partir de `off`, estendendo com zeros o que faltar; devolve o fim da escrita.
fn copy_in(data: &mut Vec<u8>, off: usize, buf: &[u8]) -> usize {
    let end = off + buf.len();
    if data.len() < end {
        data.resize(end, 0);
    }
    data[off..end].copy_from_slice(buf);
    end
}

/// `read`, `write`, `pread` e `pwrite` diferem só no sentido da cópia e no deslocamento explícito.
enum Io<'a> {
    Read(&'a mut [u8]),
    Write(&'a [u8]),
}

macro_rules! io_syscall {
    ($name:ident($buf:ident: $ty:ty) => $io:ident) => {
        fn $name(&self, fd: Fd, $buf: $ty) -> SysResult<usize> {
            self.file_io(fd, None, Io::$io($buf))
        }
    };
    ($name:ident($buf:ident: $ty:ty, at) => $io:ident) => {
        fn $name(&self, fd: Fd, $buf: $ty, offset: u64) -> SysResult<usize> {
            self.file_io(fd, Some(offset), Io::$io($buf))
        }
    };
}

/// `getpgid` e `getsid`: um campo do processo `pid` (0 é o chamador); `ESRCH` se não existe.
macro_rules! proc_getter {
    ($name:ident => $field:ident) => {
        fn $name(&self, pid: Pid) -> SysResult<Pid> {
            let pid = if pid == 0 { self.pid } else { pid };
            self.w().procs.get(&pid).map(|p| p.$field).ok_or(Errno::ESRCH)
        }
    };
}

fn split(path: &[u8]) -> Vec<&[u8]> {
    path.split(|b| *b == b'/').filter(|c| !c.is_empty()).collect()
}

/// Resultado de resolver um caminho até o último componente.
struct Resolved {
    parent: Ino,
    name: Vec<u8>,
    ino: Option<Ino>,
    /// Caminho canônico do pai (pra `chdir`/`getcwd`).
    parent_path: Vec<u8>,
    trailing_slash: bool,
}

impl World {
    /// O relógio virtual em nanossegundos desde a época.
    fn now_ns(&self) -> u64 {
        self.now.sec as u64 * 1_000_000_000 + u64::from(self.now.nsec)
    }

    fn set_now_ns(&mut self, ns: u64) {
        self.now = TimeSpec { sec: (ns / 1_000_000_000) as i64, nsec: (ns % 1_000_000_000) as u32 };
    }

    fn new() -> World {
        let mut w = World {
            nodes: BTreeMap::new(),
            next_ino: ROOT,
            seq: 0,
            programs: BTreeMap::new(),
            procs: BTreeMap::new(),
            next_pid: 100,
            now: TimeSpec { sec: DEFAULT_TIME, nsec: 0 },
            hostname: b"pseudo-linus".to_vec(),
            domainname: b"(none)".to_vec(),
            rng: 0x9E37_79B9_7F4A_7C15,
            net: None,
            locks: BTreeMap::new(),
            ncpus: 1,
            next_tid: 1_000_000,
            threads: BTreeMap::new(),
        };
        let root = w.alloc(Kind::Dir(BTreeMap::new()), 0o755);
        debug_assert_eq!(root, ROOT);
        for (dir, m) in [
            ("/usr", 0o755), ("/usr/bin", 0o755), ("/usr/sbin", 0o755), ("/usr/lib", 0o755),
            ("/usr/share", 0o755), ("/etc", 0o755), ("/tmp", 0o1777), ("/root", 0o700), ("/home", 0o755),
            ("/work", 0o755), ("/dev", 0o755), ("/var", 0o755), ("/var/tmp", 0o1777), ("/proc", 0o555),
        ] {
            w.mkdir_p(dir.as_bytes(), m);
        }
        for (link, target) in [("/bin", "usr/bin"), ("/sbin", "usr/sbin"), ("/lib", "usr/lib")] {
            w.put(link.as_bytes(), Kind::Symlink(target.as_bytes().to_vec()), 0o777);
        }
        for (name, dev, m) in [
            ("null", Dev::Null, 0o666), ("zero", Dev::Zero, 0o666), ("full", Dev::Full, 0o666),
            ("random", Dev::Random, 0o666), ("urandom", Dev::Random, 0o666), ("tty", Dev::Tty, 0o666),
        ] {
            w.put(format!("/dev/{name}").as_bytes(), Kind::Device(dev), m);
        }
        for (name, n) in [("stdin", 0), ("stdout", 1), ("stderr", 2)] {
            w.put(format!("/dev/{name}").as_bytes(), Kind::Symlink(format!("/proc/self/fd/{n}").into_bytes()), 0o777);
        }
        w.put(b"/dev/fd", Kind::Symlink(b"/proc/self/fd".to_vec()), 0o777);
        let files: &[(&str, &str)] = &[
            ("/etc/passwd", "root:x:0:0:root:/root:/bin/bash\ndaemon:x:1:1:daemon:/usr/sbin:/usr/sbin/nologin\nnobody:x:65534:65534:nobody:/nonexistent:/usr/sbin/nologin\n"),
            ("/etc/group", "root:x:0:\ndaemon:x:1:\nnogroup:x:65534:\n"),
            ("/etc/hostname", "pseudo-linus\n"),
            ("/etc/os-release", "PRETTY_NAME=\"pseudo-linus\"\nNAME=\"pseudo-linus\"\nID=pseudo-linus\nID_LIKE=debian\n"),
        ];
        for (p, c) in files {
            w.put(p.as_bytes(), Kind::File(c.as_bytes().to_vec()), 0o644);
        }
        w
    }

    fn alloc(&mut self, kind: Kind, perm: Mode) -> Ino {
        let ino = self.next_ino;
        self.next_ino += 1;
        let now = self.now;
        self.nodes.insert(ino, Node { kind, mode: perm & 0o7777, uid: 0, gid: 0, nlink: 1, atime: now, mtime: now, ctime: now });
        ino
    }

    fn node(&self, ino: Ino) -> &Node {
        self.nodes.get(&ino).expect("inode existe")
    }

    fn node_mut(&mut self, ino: Ino) -> &mut Node {
        self.nodes.get_mut(&ino).expect("inode existe")
    }

    fn dir_lookup(&self, dir: Ino, name: &[u8]) -> Option<Ino> {
        match &self.node(dir).kind {
            Kind::Dir(m) => m.get(name).map(|e| e.0),
            _ => None,
        }
    }

    fn link_into(&mut self, dir: Ino, name: &[u8], ino: Ino) {
        self.seq += 1;
        let seq = self.seq;
        let now = self.now;
        let d = self.node_mut(dir);
        if let Kind::Dir(m) = &mut d.kind {
            m.insert(name.to_vec(), (ino, seq));
        }
        d.mtime = now;
        d.ctime = now;
    }

    fn unlink_from(&mut self, dir: Ino, name: &[u8]) -> Option<Ino> {
        let now = self.now;
        let d = self.node_mut(dir);
        d.mtime = now;
        d.ctime = now;
        match &mut d.kind {
            Kind::Dir(m) => m.remove(name).map(|e| e.0),
            _ => None,
        }
    }

    /// Cria (ou substitui) um nó num caminho absoluto, criando os diretórios que faltarem.
    fn put(&mut self, path: &[u8], kind: Kind, perm: Mode) -> Ino {
        let comps = split(path);
        let (last, dirs) = comps.split_last().expect("caminho não vazio");
        let mut cur = ROOT;
        for c in dirs {
            cur = match self.dir_lookup(cur, c) {
                Some(i) => i,
                None => {
                    let i = self.alloc(Kind::Dir(BTreeMap::new()), 0o755);
                    self.link_into(cur, c, i);
                    i
                }
            };
        }
        if let Some(old) = self.unlink_from(cur, last) {
            self.node_mut(old).nlink = self.node(old).nlink.saturating_sub(1);
        }
        let ino = self.alloc(kind, perm);
        self.link_into(cur, last, ino);
        ino
    }

    fn mkdir_p(&mut self, path: &[u8], perm: Mode) {
        let mut cur = ROOT;
        for c in split(path) {
            cur = match self.dir_lookup(cur, c) {
                Some(i) => i,
                None => {
                    let i = self.alloc(Kind::Dir(BTreeMap::new()), perm);
                    self.link_into(cur, c, i);
                    i
                }
            };
        }
        self.node_mut(cur).mode = perm & 0o7777;
    }

    /// namei: resolve `path` a partir de `cwd`. Segue symlinks intermediários sempre e o último se
    /// `follow_last`. Devolve o pai e o nome do último componente (que pode não existir).
    fn resolve(&self, cwd: &[u8], path: &[u8], follow_last: bool) -> SysResult<Resolved> {
        if path.is_empty() {
            return Err(Errno::ENOENT);
        }
        if path.len() >= PATH_MAX {
            return Err(Errno::ENAMETOOLONG);
        }
        let trailing_slash = path.ends_with(b"/");
        let mut queue: VecDeque<Vec<u8>> = VecDeque::new();
        if !path.starts_with(b"/") {
            for c in split(cwd) {
                queue.push_back(c.to_vec());
            }
        }
        for c in split(path) {
            queue.push_back(c.to_vec());
        }
        // Pilha de (ino, nome) do caminho canônico.
        let mut stack: Vec<(Ino, Vec<u8>)> = Vec::new();
        let mut links = 0;
        let cur_ino = |stack: &Vec<(Ino, Vec<u8>)>| stack.last().map(|e| e.0).unwrap_or(ROOT);
        if queue.is_empty() {
            return Ok(Resolved { parent: ROOT, name: Vec::new(), ino: Some(ROOT), parent_path: b"/".to_vec(), trailing_slash });
        }
        loop {
            let c = queue.pop_front().expect("fila não vazia");
            let last = queue.is_empty();
            if c.len() > NAME_MAX {
                return Err(Errno::ENAMETOOLONG);
            }
            let dir = cur_ino(&stack);
            if !matches!(self.node(dir).kind, Kind::Dir(_)) {
                return Err(Errno::ENOTDIR);
            }
            if c == b"." || c == b".." {
                if c == b".." {
                    stack.pop();
                }
                if last {
                    let ino = cur_ino(&stack);
                    let (parent, name, parent_path) = match stack.split_last() {
                        Some(((_, n), rest)) => (
                            rest.last().map(|e| e.0).unwrap_or(ROOT),
                            n.clone(),
                            canonical(rest),
                        ),
                        None => (ROOT, Vec::new(), b"/".to_vec()),
                    };
                    return Ok(Resolved { parent, name, ino: Some(ino), parent_path, trailing_slash });
                }
                continue;
            }
            let child = self.dir_lookup(dir, &c);
            match child {
                None => {
                    if last {
                        return Ok(Resolved { parent: dir, name: c, ino: None, parent_path: canonical(&stack), trailing_slash });
                    }
                    return Err(Errno::ENOENT);
                }
                Some(ino) => {
                    if let Kind::Symlink(target) = &self.node(ino).kind
                        && (!last || follow_last || trailing_slash)
                    {
                        {
                            links += 1;
                            if links > MAXSYMLINKS {
                                return Err(Errno::ELOOP);
                            }
                            if target.starts_with(b"/proc/self/fd") {
                                // /dev/fd e /dev/std*: o testkit não tem procfs; o chamador trata.
                                let mut rest: Vec<Vec<u8>> = queue.drain(..).collect();
                                let mut t = target.clone();
                                for r in rest.drain(..) {
                                    t.push(b'/');
                                    t.extend(r);
                                }
                                return Ok(Resolved { parent: dir, name: t, ino: None, parent_path: b"\0fd".to_vec(), trailing_slash });
                            }
                            let mut next: VecDeque<Vec<u8>> = split(target).into_iter().map(<[u8]>::to_vec).collect();
                            if target.starts_with(b"/") {
                                stack.clear();
                            }
                            next.extend(queue.drain(..));
                            queue = next;
                            if queue.is_empty() {
                                let ino = cur_ino(&stack);
                                return Ok(Resolved { parent: ino, name: Vec::new(), ino: Some(ino), parent_path: canonical(&stack), trailing_slash });
                            }
                            continue;
                        }
                    }
                    if last {
                        return Ok(Resolved { parent: dir, name: c, ino: Some(ino), parent_path: canonical(&stack), trailing_slash });
                    }
                    stack.push((ino, c));
                }
            }
        }
    }

    fn stat_of(&self, ino: Ino) -> Stat {
        let n = self.node(ino);
        let size = match &n.kind {
            Kind::File(d) => d.len() as u64,
            Kind::Symlink(t) => t.len() as u64,
            Kind::Dir(m) => 40 + 20 * m.len() as u64,
            _ => 0,
        };
        Stat {
            dev: 0x2a,
            ino,
            mode: n.type_bits() | n.mode,
            nlink: if let Kind::Dir(m) = &n.kind {
                2 + m.values().filter(|(i, _)| matches!(self.node(*i).kind, Kind::Dir(_))).count() as u64
            } else {
                n.nlink
            },
            uid: n.uid,
            gid: n.gid,
            rdev: if let Kind::Device(d) = n.kind { dev_number(d) } else { 0 },
            size,
            blksize: 4096,
            blocks: size.div_ceil(4096) * 8,
            atime: n.atime,
            mtime: n.mtime,
            ctime: n.ctime,
            btime: Some(n.ctime),
        }
    }

    fn rand(&mut self) -> u64 {
        // xorshift64*: determinístico, suficiente pra testes.
        let mut x = self.rng;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.rng = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
}

fn dev_number(d: Dev) -> u64 {
    match d {
        Dev::Null => (1 << 8) | 3,
        Dev::Zero => (1 << 8) | 5,
        Dev::Full => (1 << 8) | 7,
        Dev::Random => (1 << 8) | 9,
        Dev::Tty => 5 << 8,
    }
}

fn canonical(stack: &[(Ino, Vec<u8>)]) -> Vec<u8> {
    if stack.is_empty() {
        return b"/".to_vec();
    }
    let mut out = Vec::new();
    for (_, n) in stack {
        out.push(b'/');
        out.extend_from_slice(n);
    }
    out
}

fn join(dir: &[u8], name: &[u8]) -> Vec<u8> {
    let mut p = dir.to_vec();
    if !p.ends_with(b"/") {
        p.push(b'/');
    }
    p.extend_from_slice(name);
    p
}

/// O processo de teste: implementa `Syscalls` sobre o mundo compartilhado.
struct ProcHandle {
    world: Arc<Mutex<World>>,
    pid: Pid,
}

impl ProcHandle {
    fn w(&self) -> MutexGuard<'_, World> {
        lock(&self.world)
    }

    /// O corpo do `wait4` e do `waitid`: o primeiro filho que casa e já terminou (colhido se `reap`).
    fn wait_scan(&self, target: WaitTarget, reap: bool) -> SysResult<Option<WaitInfo>> {
        self.check_pending();
        let mut w = self.w();
        let my_pgid = w.procs[&self.pid].pgid;
        let mine: Vec<Pid> = w
            .procs
            .values()
            .filter(|p| p.ppid == self.pid && !p.reaped && p.pid != self.pid)
            .filter(|p| match target {
                WaitTarget::Any => true,
                WaitTarget::Pid(x) => p.pid == x,
                WaitTarget::Group(0) => p.pgid == my_pgid,
                WaitTarget::Group(g) => p.pgid == g,
            })
            .map(|p| p.pid)
            .collect();
        if mine.is_empty() {
            return Err(Errno::ECHILD);
        }
        for pid in mine {
            let p = w.procs.get_mut(&pid).expect("filho");
            if let Some(status) = p.status {
                p.reaped |= reap;
                // Todo processo do testkit é do uid 0.
                return Ok(Some(WaitInfo { pid, uid: 0, status, rusage: Rusage::default() }));
            }
        }
        // Síncrono: um filho vivo aqui só pode ser este processo esperando ele mesmo; não bloqueia.
        Ok(None)
    }

    fn with_proc<R>(&self, f: impl FnOnce(&mut World, Pid) -> R) -> R {
        let mut w = self.w();
        f(&mut w, self.pid)
    }

    /// `read`, `write`, `pread` e `pwrite`: `at` é o deslocamento explícito (None usa e avança o do descritor).
    fn file_io(&self, fd: Fd, at: Option<u64>, io: Io<'_>) -> SysResult<usize> {
        if at.is_none() {
            self.check_pending();
        }
        let e = self.fd_entry(fd)?;
        let mut f = lock(&e.file);
        let allowed = match io {
            Io::Read(_) => f.flags.readable(),
            Io::Write(_) => f.flags.writable(),
        };
        if at.is_none() && !allowed {
            return Err(Errno::EBADF);
        }
        match &f.kind {
            OpenKind::Dir { .. } => return Err(Errno::EISDIR),
            OpenKind::Inode(ino) => {
                let ino = *ino;
                return self.inode_io(ino, &mut f, at, io);
            }
            _ if at.is_some() => return Err(Errno::ESPIPE),
            _ => {}
        }
        match (&f.kind, io) {
            (OpenKind::PipeRead(p), Io::Read(buf)) => {
                let mut p = lock(p);
                let n = buf.len().min(p.data.len());
                for (i, b) in p.data.drain(..n).enumerate() {
                    buf[i] = b;
                }
                Ok(n)
            }
            (OpenKind::PipeWrite(p), Io::Write(buf)) => {
                let full = {
                    let mut p = lock(p);
                    if p.readers == 0 || p.data.len() + buf.len() > PIPE_LIMIT {
                        true
                    } else {
                        p.data.extend(buf.iter().copied());
                        false
                    }
                };
                if full {
                    drop(f);
                    self.deliver(Signal::SIGPIPE)?;
                    return Err(Errno::EPIPE);
                }
                Ok(buf.len())
            }
            (OpenKind::Socket(s), Io::Read(buf)) => lock(s).stream.read(buf).map_err(|e| Errno::from_io(&e)),
            (OpenKind::Socket(s), Io::Write(buf)) => lock(s).stream.write(buf).map_err(|e| Errno::from_io(&e)),
            _ => Err(Errno::EBADF),
        }
    }

    /// A parte de `file_io` que toca o inode: arquivo regular e dispositivos.
    fn inode_io(&self, ino: Ino, f: &mut OpenFile, at: Option<u64>, io: Io<'_>) -> SysResult<usize> {
        let off = at.unwrap_or(f.offset) as usize;
        let append = at.is_none() && f.flags.contains(OFlags::APPEND);
        let mut w = self.w();
        let now = w.now;
        let (n, advance) = match io {
            Io::Read(buf) => {
                let n = match &w.node(ino).kind {
                    Kind::File(d) => copy_out(d, off, buf),
                    Kind::Device(Dev::Null) => 0,
                    Kind::Device(Dev::Zero | Dev::Full) => {
                        buf.fill(0);
                        buf.len()
                    }
                    Kind::Device(Dev::Random) => {
                        for b in buf.iter_mut() {
                            *b = w.rand() as u8;
                        }
                        buf.len()
                    }
                    Kind::Device(Dev::Tty) => return Err(Errno::ENXIO),
                    _ if at.is_some() => return Err(Errno::ESPIPE),
                    _ => return Err(Errno::EINVAL),
                };
                w.node_mut(ino).atime = now;
                (n, Some((off + n) as u64))
            }
            Io::Write(buf) => {
                let node = w.node_mut(ino);
                match &mut node.kind {
                    Kind::File(d) => {
                        let start = if append { d.len() } else { off };
                        let end = copy_in(d, start, buf);
                        node.mtime = now;
                        node.ctime = now;
                        (buf.len(), Some(end as u64))
                    }
                    Kind::Device(Dev::Null | Dev::Zero | Dev::Random) => (buf.len(), None),
                    Kind::Device(Dev::Full) => return Err(Errno::ENOSPC),
                    _ if at.is_some() => return Err(Errno::ESPIPE),
                    _ => return Err(Errno::EINVAL),
                }
            }
        };
        drop(w);
        if let (None, Some(end)) = (at, advance) {
            f.offset = end;
        }
        Ok(n)
    }

    fn fd_entry(&self, fd: Fd) -> SysResult<FdEntry> {
        let w = self.w();
        w.procs.get(&self.pid).and_then(|p| p.fds.get(&fd.0)).cloned().ok_or(Errno::EBADF)
    }

    fn cwd(&self) -> Vec<u8> {
        self.w().procs[&self.pid].cwd.clone()
    }

    /// O processo `pid` (0 é o corrente) pras syscalls de escalonamento, afinidade e prioridade.
    fn sched_pid(&self, w: &World, pid: Pid) -> SysResult<Pid> {
        let pid = if pid == 0 { self.pid } else { pid };
        match w.procs.get(&pid) {
            Some(p) if !p.reaped => Ok(pid),
            _ => Err(Errno::ESRCH),
        }
    }

    /// Quem muda o escalonamento de `p`: todo processo do testkit é root e do mesmo dono, e os rlimits
    /// `RTPRIO` e `NICE` que ninguém ajustou valem 0, como no contêiner padrão (o `getrlimit` do
    /// testkit os devolve ilimitados, que é só o padrão genérico dele).
    fn sched_caller(p: &Proc) -> SchedCaller {
        let rl = |r: Resource| p.rlimits.get(&r).map_or(0, |l| l.cur);
        SchedCaller { same_owner: true, rlim_rtprio: rl(Resource::Rtprio), rlim_nice: rl(Resource::Nice) }
    }

    /// Alvos de `ioprio_get`/`ioprio_set`. `which` inválido é EINVAL; sem alvo a lista vem vazia.
    fn ioprio_targets(&self, w: &World, which: i32, who: i32) -> SysResult<Vec<Pid>> {
        let live = |p: &&Proc| !p.reaped && p.status.is_none();
        match which {
            sched::IOPRIO_WHO_PROCESS => {
                let pid = if who == 0 { self.pid } else { who };
                Ok(w.procs.get(&pid).filter(|p| !p.reaped).map(|p| vec![p.pid]).unwrap_or_default())
            }
            sched::IOPRIO_WHO_PGRP => {
                let pgid = if who == 0 { w.procs[&self.pid].pgid } else { who };
                Ok(w.procs.values().filter(live).filter(|p| p.pgid == pgid).map(|p| p.pid).collect())
            }
            // Todo processo do testkit é do uid 0.
            sched::IOPRIO_WHO_USER => Ok(if who == 0 { w.procs.values().filter(live).map(|p| p.pid).collect() } else { Vec::new() }),
            _ => Err(Errno::EINVAL),
        }
    }

    /// Caminho base pra uma syscall `*at`.
    fn base(&self, dirfd: Fd, path: &[u8]) -> SysResult<Vec<u8>> {
        if path.starts_with(b"/") || dirfd == Fd::CWD {
            return Ok(self.cwd());
        }
        let e = self.fd_entry(dirfd)?;
        let f = lock(&e.file);
        match &f.kind {
            OpenKind::Dir { ino, .. } | OpenKind::Inode(ino) => {
                let w = self.w();
                path_of(&w, *ino).ok_or(Errno::ENOENT)
            }
            _ => Err(Errno::ENOTDIR),
        }
    }

    fn resolve_at(&self, dirfd: Fd, path: &[u8], follow: bool) -> SysResult<Resolved> {
        let base = self.base(dirfd, path)?;
        self.w().resolve(&base, path, follow)
    }

    fn alloc_fd(&self, w: &mut World, entry: FdEntry, min: i32) -> SysResult<Fd> {
        let p = w.procs.get_mut(&self.pid).expect("processo");
        let limit = p.rlimits.get(&Resource::Nofile).map(|l| l.cur).unwrap_or(1024) as i32;
        let mut n = min;
        while p.fds.contains_key(&n) {
            n += 1;
        }
        if n >= limit {
            return Err(Errno::EMFILE);
        }
        p.fds.insert(n, entry);
        Ok(Fd(n))
    }

    /// Entrega a ação padrão de um sinal fatal ao próprio processo.
    fn deliver(&self, sig: Signal) -> SysResult<()> {
        let disp = self.w().procs[&self.pid].sigs.get(&sig.0).copied().unwrap_or(SigDisposition::Default);
        match disp {
            SigDisposition::Ignore => Ok(()),
            SigDisposition::Catch => {
                self.w().procs.get_mut(&self.pid).expect("processo").caught.push(sig);
                Ok(())
            }
            SigDisposition::Default => match sig.default_action() {
                DefaultAction::Terminate | DefaultAction::CoreDump => std::panic::resume_unwind(Box::new(KillUnwind(sig))),
                _ => Ok(()),
            },
        }
    }

    fn check_pending(&self) {
        let pending = self.w().procs.get_mut(&self.pid).and_then(|p| p.pending_fatal.take());
        if let Some(sig) = pending {
            let _ = self.deliver(sig);
        }
    }

    fn new_child(&self, attrs: &ProcAttrs, argv: Vec<Vec<u8>>) -> SysResult<Pid> {
        let mut w = self.w();
        let parent = w.procs[&self.pid].clone();
        let pid = w.next_pid;
        w.next_pid += 1;
        let mut child = parent.clone();
        child.pid = pid;
        child.ppid = self.pid;
        child.argv = argv;
        child.caught.clear();
        child.pending_fatal = None;
        child.itimers = [ItimerSlot::default(); 3];
        child.status = None;
        child.reaped = false;
        // `sched_fork`: com reset_on_fork o filho perde tempo real e nice negativa.
        let (sched, nice) = child.sched.fork(child.nice);
        child.sched = sched;
        child.nice = nice;
        if let Some(env) = &attrs.env {
            child.env = env.clone();
        }
        if let Some(cwd) = &attrs.cwd {
            let r = w.resolve(&parent.cwd, cwd, true)?;
            let ino = r.ino.ok_or(Errno::ENOENT)?;
            if !matches!(w.node(ino).kind, Kind::Dir(_)) {
                return Err(Errno::ENOTDIR);
            }
            child.cwd = path_of(&w, ino).unwrap_or_else(|| cwd.clone());
        }
        match attrs.group {
            ProcessGroup::Inherit => {}
            ProcessGroup::New => child.pgid = pid,
            ProcessGroup::Join(g) => child.pgid = g,
        }
        if attrs.new_session {
            child.sid = pid;
            child.pgid = pid;
        }
        for s in &attrs.reset_signals {
            child.sigs.remove(&s.0);
        }
        for s in &attrs.ignore_signals {
            child.sigs.insert(s.0, SigDisposition::Ignore);
        }
        // Catch vira Default no filho de exec; spawn_fn mantém (como fork).
        w.procs.insert(pid, child);
        drop(w);
        // Ações de fd, aplicadas no contexto do filho.
        let child_handle = ProcHandle { world: self.world.clone(), pid };
        for a in &attrs.fd_actions {
            let r = match a {
                FdAction::Dup2 { from, to } => {
                    if from == to {
                        child_handle.set_cloexec(*from, false)
                    } else {
                        child_handle.dup3(*from, *to, false).map(|_| ())
                    }
                }
                FdAction::Close(fd) => {
                    let _ = child_handle.close(*fd);
                    Ok(())
                }
                FdAction::CloseFrom(from) => {
                    let open: Vec<i32> = self.w().procs.get(&pid).map(|p| p.fds.keys().copied().filter(|fd| *fd >= from.0).collect()).unwrap_or_default();
                    for fd in open {
                        let _ = child_handle.close(Fd(fd));
                    }
                    Ok(())
                }
                FdAction::Open { fd, path, flags, mode } => child_handle.openat(Fd::CWD, path, *flags, *mode).and_then(|got| {
                    if got != *fd {
                        child_handle.dup3(got, *fd, false)?;
                        child_handle.close(got)?;
                    }
                    Ok(())
                }),
            };
            if let Err(e) = r {
                self.w().procs.remove(&pid);
                return Err(e);
            }
        }
        Ok(pid)
    }

    /// Roda o corpo de um processo filho já criado, até o fim, nesta thread.
    fn run_child(&self, pid: Pid, body: ProcessFn) {
        let handle: Arc<dyn Syscalls> = Arc::new(ProcHandle { world: self.world.clone(), pid });
        let prev = sys::install(handle.clone());
        let mut body: Option<ProcessFn> = Some(body);
        let status = loop {
            let f = body.take().expect("corpo");
            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
                Ok(code) => break WaitStatus::Exited(code & 0xff),
                Err(payload) => {
                    if let Some(e) = payload.downcast_ref::<ExitUnwind>() {
                        break WaitStatus::Exited(e.0 & 0xff);
                    }
                    if let Some(k) = payload.downcast_ref::<KillUnwind>() {
                        break WaitStatus::Signaled { signal: k.0, core_dumped: false };
                    }
                    if let Ok(exec) = payload.downcast::<ExecUnwind>() {
                        let h = ProcHandle { world: self.world.clone(), pid };
                        match h.prepare_exec(&exec.path, exec.argv.clone(), exec.env.clone()) {
                            Ok(next) => {
                                body = Some(next);
                                continue;
                            }
                            Err(_) => break WaitStatus::Exited(127),
                        }
                    }
                    let msg = b"testkit: panic no processo\n";
                    let _ = handle.write(Fd::STDERR, msg);
                    break WaitStatus::Signaled { signal: Signal::SIGABRT, core_dumped: true };
                }
            }
        };
        sys::uninstall();
        if let Some(p) = prev {
            sys::install(p);
        }
        let mut w = self.w();
        let mut fds = BTreeMap::new();
        if let Some(p) = w.procs.get_mut(&pid) {
            std::mem::swap(&mut fds, &mut p.fds);
            p.status = Some(status);
        }
        // Órfãos do filho viram filhos do processo raiz do teste (no Linux, do init).
        let root_pid = w.procs.keys().next().copied().unwrap_or(pid);
        for p in w.procs.values_mut() {
            if p.ppid == pid {
                p.ppid = root_pid;
            }
        }
        drop(w);
        drop(fds);
    }

    /// Prepara o corpo de um `execve`: builtin, `#!` ou erro.
    fn prepare_exec(&self, path: &[u8], argv: Vec<Vec<u8>>, env: Option<Vec<Vec<u8>>>) -> SysResult<ProcessFn> {
        let mut path = path.to_vec();
        let mut argv = argv;
        for _ in 0..4 {
            let r = self.resolve_at(Fd::CWD, &path, true)?;
            let ino = r.ino.ok_or(Errno::ENOENT)?;
            let (kind, mode, owner) = {
                let w = self.w();
                let n = w.node(ino);
                (n.kind.clone(), n.mode, (n.uid, n.gid))
            };
            match kind {
                Kind::Dir(_) => return Err(Errno::EACCES),
                Kind::Builtin(name) => {
                    let main = self.w().programs.get(&name).copied().ok_or(Errno::ENOENT)?;
                    {
                        let mut w = self.w();
                        let p = w.procs.get_mut(&self.pid).expect("processo");
                        p.argv = argv.clone();
                        if let Some(e) = &env {
                            p.env = e.clone();
                        }
                        // Exec setuid/setgid que troca o dono efetivo (todo processo aqui é root): a
                        // personality perde os bits de `PER_CLEAR_ON_SETID`.
                        let secure = (mode & crate::types::mode::S_ISUID != 0 && owner.0 != 0)
                            || (mode & crate::types::mode::S_ISGID != 0 && owner.1 != 0);
                        p.persona = sched::personality::after_exec(p.persona, secure);
                        // CLOEXEC fecha, disposições capturadas voltam ao padrão.
                        let close: Vec<i32> = p.fds.iter().filter(|(_, e)| e.cloexec).map(|(k, _)| *k).collect();
                        for k in close {
                            p.fds.remove(&k);
                        }
                        p.sigs.retain(|_, d| *d == SigDisposition::Ignore);
                        p.caught.clear();
                    }
                    let args = to_os_args(&argv);
                    let argv0 = argv.first().cloned().unwrap_or_default();
                    return Ok(Box::new(move || run_main(main, &argv0, &args)));
                }
                Kind::File(data) => {
                    if mode & 0o111 == 0 {
                        return Err(Errno::EACCES);
                    }
                    if !data.starts_with(b"#!") {
                        return Err(Errno::ENOEXEC);
                    }
                    let line = data[2..].split(|b| *b == b'\n').next().unwrap_or(b"");
                    let line = line.strip_suffix(b"\r").unwrap_or(line);
                    let trimmed: Vec<u8> = line.iter().copied().skip_while(|b| *b == b' ' || *b == b'\t').collect();
                    let mut parts = trimmed.splitn(2, |b| *b == b' ' || *b == b'\t');
                    let interp = parts.next().unwrap_or(b"").to_vec();
                    let arg: Vec<u8> = parts.next().map(|a| a.iter().copied().skip_while(|b| *b == b' ').collect()).unwrap_or_default();
                    if interp.is_empty() {
                        return Err(Errno::ENOEXEC);
                    }
                    let mut new_argv = vec![interp.clone()];
                    if !arg.is_empty() {
                        new_argv.push(arg);
                    }
                    new_argv.push(path.clone());
                    new_argv.extend(argv.into_iter().skip(1));
                    argv = new_argv;
                    path = interp;
                }
                _ => return Err(Errno::EACCES),
            }
        }
        Err(Errno::ELOOP)
    }

    fn open_special(&self, target: &[u8], flags: OFlags) -> SysResult<Fd> {
        // /proc/self/fd/N: duplica o fd N.
        let n: i32 = std::str::from_utf8(target.strip_prefix(b"/proc/self/fd/").ok_or(Errno::ENOENT)?)
            .ok()
            .and_then(|s| s.parse().ok())
            .ok_or(Errno::ENOENT)?;
        let fd = self.dup(Fd(n))?;
        self.set_cloexec(fd, flags.contains(OFlags::CLOEXEC))?;
        Ok(fd)
    }
}

fn path_of(w: &World, target: Ino) -> Option<Vec<u8>> {
    if target == ROOT {
        return Some(b"/".to_vec());
    }
    // Busca em largura a partir da raiz (testkit: árvores pequenas).
    let mut queue: VecDeque<(Ino, Vec<u8>)> = VecDeque::from([(ROOT, Vec::new())]);
    while let Some((ino, path)) = queue.pop_front() {
        if let Kind::Dir(m) = &w.node(ino).kind {
            for (name, (child, _)) in m {
                let mut p = path.clone();
                p.push(b'/');
                p.extend_from_slice(name);
                if *child == target {
                    return Some(p);
                }
                if matches!(w.node(*child).kind, Kind::Dir(_)) {
                    queue.push_back((*child, p));
                }
            }
        }
    }
    None
}

/// Chama o `main` de um builtin com o `Ctx` do processo corrente.
pub fn run_main(main: Main, argv0: &[u8], args: &[OsString]) -> i32 {
    let mut ctx = Ctx::current(argv0);
    main(&mut ctx, args)
}

impl Syscalls for ProcHandle {
    fn openat(&self, dirfd: Fd, path: &[u8], flags: OFlags, mode: Mode) -> SysResult<Fd> {
        self.check_pending();
        let follow = !flags.contains(OFlags::NOFOLLOW);
        let r = self.resolve_at(dirfd, path, follow)?;
        if r.parent_path == b"\0fd" {
            return self.open_special(&r.name, flags);
        }
        let mut w = self.w();
        let umask = w.procs[&self.pid].umask;
        let ino = match r.ino {
            Some(ino) => {
                if flags.contains(OFlags::CREAT) && flags.contains(OFlags::EXCL) {
                    return Err(Errno::EEXIST);
                }
                ino
            }
            None => {
                if !flags.contains(OFlags::CREAT) {
                    return Err(Errno::ENOENT);
                }
                if r.trailing_slash {
                    return Err(Errno::EISDIR);
                }
                let ino = w.alloc(Kind::File(Vec::new()), mode & !umask);
                w.link_into(r.parent, &r.name, ino);
                ino
            }
        };
        let is_dir = matches!(w.node(ino).kind, Kind::Dir(_));
        if let Kind::Symlink(_) = w.node(ino).kind
            && flags.contains(OFlags::NOFOLLOW)
            && !flags.contains(OFlags::PATH)
        {
            return Err(Errno::ELOOP);
        }
        if flags.contains(OFlags::DIRECTORY) && !is_dir {
            return Err(Errno::ENOTDIR);
        }
        if is_dir && flags.writable() {
            return Err(Errno::EISDIR);
        }
        if r.trailing_slash && !is_dir {
            return Err(Errno::ENOTDIR);
        }
        if flags.contains(OFlags::TRUNC) && flags.writable() {
            let now = w.now;
            let n = w.node_mut(ino);
            if let Kind::File(d) = &mut n.kind {
                d.clear();
                n.mtime = now;
                n.ctime = now;
            }
        }
        let kind = if is_dir { OpenKind::Dir { ino, listing: None } } else { OpenKind::Inode(ino) };
        let entry = FdEntry {
            file: Arc::new(Mutex::new(OpenFile { kind, flags, offset: 0 })),
            cloexec: flags.contains(OFlags::CLOEXEC),
        };
        self.alloc_fd(&mut w, entry, 0)
    }

    fn close(&self, fd: Fd) -> SysResult<()> {
        let removed = self.with_proc(|w, pid| w.procs.get_mut(&pid).and_then(|p| p.fds.remove(&fd.0)));
        removed.map(drop).ok_or(Errno::EBADF)
    }

    io_syscall!(read(buf: &mut [u8]) => Read);
    io_syscall!(write(buf: &[u8]) => Write);
    io_syscall!(pread(buf: &mut [u8], at) => Read);
    io_syscall!(pwrite(buf: &[u8], at) => Write);

    fn lseek(&self, fd: Fd, offset: i64, whence: Whence) -> SysResult<u64> {
        let e = self.fd_entry(fd)?;
        let mut f = lock(&e.file);
        let size = match &f.kind {
            OpenKind::Inode(ino) => match &self.w().node(*ino).kind {
                Kind::File(d) => d.len() as i64,
                Kind::Device(_) => 0,
                _ => return Err(Errno::ESPIPE),
            },
            OpenKind::Dir { .. } => 0,
            _ => return Err(Errno::ESPIPE),
        };
        let base = match whence {
            Whence::Set => 0,
            Whence::Cur => f.offset as i64,
            Whence::End => size,
            Whence::Data => offset,
            Whence::Hole => size,
        };
        let new = if matches!(whence, Whence::Data | Whence::Hole) { base } else { base + offset };
        if new < 0 {
            return Err(Errno::EINVAL);
        }
        if let OpenKind::Dir { listing, .. } = &mut f.kind
            && new == 0
        {
            *listing = None;
        }
        f.offset = new as u64;
        Ok(new as u64)
    }

    fn fstat(&self, fd: Fd) -> SysResult<Stat> {
        let e = self.fd_entry(fd)?;
        let f = lock(&e.file);
        match &f.kind {
            OpenKind::Inode(ino) | OpenKind::Dir { ino, .. } => Ok(self.w().stat_of(*ino)),
            OpenKind::Socket(_) => Ok(Stat {
                mode: mode::S_IFSOCK | 0o777,
                nlink: 1,
                blksize: 4096,
                ino: 0x2000 + fd.0 as u64,
                dev: 0x8,
                ..Stat::default()
            }),
            OpenKind::PipeRead(_) | OpenKind::PipeWrite(_) => Ok(Stat {
                mode: mode::S_IFIFO | 0o600,
                nlink: 1,
                blksize: 4096,
                ino: 0x1000 + fd.0 as u64,
                dev: 0xc,
                ..Stat::default()
            }),
        }
    }

    fn fstatat(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<Stat> {
        if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            return self.fstat(dirfd);
        }
        let r = self.resolve_at(dirfd, path, !flags.contains(AtFlags::SYMLINK_NOFOLLOW))?;
        if r.parent_path == b"\0fd" {
            let fd = self.open_special(&r.name, OFlags::CLOEXEC)?;
            let st = self.fstat(fd);
            let _ = self.close(fd);
            return st;
        }
        let ino = r.ino.ok_or(Errno::ENOENT)?;
        let w = self.w();
        if r.trailing_slash && !matches!(w.node(ino).kind, Kind::Dir(_)) {
            return Err(Errno::ENOTDIR);
        }
        Ok(w.stat_of(ino))
    }

    fn faccessat(&self, dirfd: Fd, path: &[u8], mode: AccessMode, _flags: AtFlags) -> SysResult<()> {
        let st = self.fstatat(dirfd, path, AtFlags::empty())?;
        // Root: leitura e escrita sempre; execução só se algum bit x estiver ligado (ou diretório).
        if mode.contains(AccessMode::X_OK) && st.perm() & 0o111 == 0 && st.file_type() != FileType::Directory {
            return Err(Errno::EACCES);
        }
        Ok(())
    }

    fn mkdirat(&self, dirfd: Fd, path: &[u8], mode: Mode) -> SysResult<()> {
        let r = self.resolve_at(dirfd, path, false)?;
        if r.ino.is_some() {
            return Err(Errno::EEXIST);
        }
        let mut w = self.w();
        let umask = w.procs[&self.pid].umask;
        let ino = w.alloc(Kind::Dir(BTreeMap::new()), mode & !umask & 0o7777);
        w.link_into(r.parent, &r.name, ino);
        Ok(())
    }

    fn unlinkat(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<()> {
        let r = self.resolve_at(dirfd, path, false)?;
        let ino = r.ino.ok_or(Errno::ENOENT)?;
        let mut w = self.w();
        let is_dir = matches!(w.node(ino).kind, Kind::Dir(_));
        if flags.contains(AtFlags::REMOVEDIR) {
            if !is_dir {
                return Err(Errno::ENOTDIR);
            }
            if r.name.is_empty() || r.name == b"." {
                return Err(Errno::EINVAL);
            }
            if let Kind::Dir(m) = &w.node(ino).kind
                && !m.is_empty() {
                    return Err(Errno::ENOTEMPTY);
                }
        } else {
            if is_dir {
                return Err(Errno::EISDIR);
            }
            if r.trailing_slash {
                return Err(Errno::ENOTDIR);
            }
        }
        w.unlink_from(r.parent, &r.name);
        let n = w.node_mut(ino);
        n.nlink = n.nlink.saturating_sub(1);
        Ok(())
    }

    fn renameat2(&self, olddir: Fd, old: &[u8], newdir: Fd, new: &[u8], flags: RenameFlags) -> SysResult<()> {
        let src = self.resolve_at(olddir, old, false)?;
        let src_ino = src.ino.ok_or(Errno::ENOENT)?;
        let dst = self.resolve_at(newdir, new, false)?;
        let mut w = self.w();
        let src_dir = matches!(w.node(src_ino).kind, Kind::Dir(_));
        if let Some(d) = dst.ino {
            if d == src_ino {
                return Ok(());
            }
            if flags.contains(RenameFlags::NOREPLACE) {
                return Err(Errno::EEXIST);
            }
            let dst_dir = matches!(w.node(d).kind, Kind::Dir(_));
            match (src_dir, dst_dir) {
                (true, false) => return Err(Errno::ENOTDIR),
                (false, true) => return Err(Errno::EISDIR),
                (true, true) => {
                    if let Kind::Dir(m) = &w.node(d).kind
                        && !m.is_empty() {
                            return Err(Errno::ENOTEMPTY);
                        }
                }
                _ => {}
            }
            w.unlink_from(dst.parent, &dst.name);
            let n = w.node_mut(d);
            n.nlink = n.nlink.saturating_sub(1);
        } else if dst.trailing_slash && !src_dir {
            return Err(Errno::ENOTDIR);
        }
        // Não pode mover um diretório pra dentro dele mesmo.
        if src_dir {
            let src_path = path_of(&w, src_ino).unwrap_or_default();
            let dst_parent = path_of(&w, dst.parent).unwrap_or_default();
            if dst_parent == src_path || dst_parent.starts_with(&join(&src_path, b"")) {
                return Err(Errno::EINVAL);
            }
        }
        w.unlink_from(src.parent, &src.name);
        w.link_into(dst.parent, &dst.name, src_ino);
        let now = w.now;
        w.node_mut(src_ino).ctime = now;
        Ok(())
    }

    fn linkat(&self, olddir: Fd, old: &[u8], newdir: Fd, new: &[u8], flags: AtFlags) -> SysResult<()> {
        let src = self.resolve_at(olddir, old, flags.contains(AtFlags::SYMLINK_FOLLOW))?;
        let ino = src.ino.ok_or(Errno::ENOENT)?;
        let dst = self.resolve_at(newdir, new, false)?;
        if dst.ino.is_some() {
            return Err(Errno::EEXIST);
        }
        let mut w = self.w();
        if matches!(w.node(ino).kind, Kind::Dir(_)) {
            return Err(Errno::EPERM);
        }
        w.link_into(dst.parent, &dst.name, ino);
        let now = w.now;
        let n = w.node_mut(ino);
        n.nlink += 1;
        n.ctime = now;
        Ok(())
    }

    fn symlinkat(&self, target: &[u8], dirfd: Fd, path: &[u8]) -> SysResult<()> {
        if target.is_empty() {
            return Err(Errno::ENOENT);
        }
        let r = self.resolve_at(dirfd, path, false)?;
        if r.ino.is_some() {
            return Err(Errno::EEXIST);
        }
        let mut w = self.w();
        let ino = w.alloc(Kind::Symlink(target.to_vec()), 0o777);
        w.link_into(r.parent, &r.name, ino);
        Ok(())
    }

    fn readlinkat(&self, dirfd: Fd, path: &[u8]) -> SysResult<Vec<u8>> {
        let r = self.resolve_at(dirfd, path, false)?;
        let ino = r.ino.ok_or(Errno::ENOENT)?;
        match &self.w().node(ino).kind {
            Kind::Symlink(t) => Ok(t.clone()),
            _ => Err(Errno::EINVAL),
        }
    }

    fn mknodat(&self, dirfd: Fd, path: &[u8], mode: Mode, _dev: u64) -> SysResult<()> {
        let r = self.resolve_at(dirfd, path, false)?;
        if r.ino.is_some() {
            return Err(Errno::EEXIST);
        }
        if FileType::from_mode(mode) != FileType::Fifo {
            return Err(Errno::EPERM);
        }
        let mut w = self.w();
        let umask = w.procs[&self.pid].umask;
        let ino = w.alloc(Kind::Fifo, mode & !umask & 0o7777);
        w.link_into(r.parent, &r.name, ino);
        Ok(())
    }

    fn fchmodat(&self, dirfd: Fd, path: &[u8], mode: Mode, flags: AtFlags) -> SysResult<()> {
        let r = self.resolve_at(dirfd, path, !flags.contains(AtFlags::SYMLINK_NOFOLLOW))?;
        let ino = r.ino.ok_or(Errno::ENOENT)?;
        let mut w = self.w();
        let now = w.now;
        let n = w.node_mut(ino);
        n.mode = mode & 0o7777;
        n.ctime = now;
        Ok(())
    }

    fn fchmod(&self, fd: Fd, mode: Mode) -> SysResult<()> {
        let e = self.fd_entry(fd)?;
        let f = lock(&e.file);
        if let OpenKind::Inode(ino) | OpenKind::Dir { ino, .. } = &f.kind {
            let mut w = self.w();
            w.node_mut(*ino).mode = mode & 0o7777;
        }
        Ok(())
    }

    fn fchownat(&self, dirfd: Fd, path: &[u8], uid: Option<Uid>, gid: Option<Gid>, flags: AtFlags) -> SysResult<()> {
        let ino = if path.is_empty() && flags.contains(AtFlags::EMPTY_PATH) {
            match &lock(&self.fd_entry(dirfd)?.file).kind {
                OpenKind::Inode(ino) | OpenKind::Dir { ino, .. } => *ino,
                _ => return Ok(()),
            }
        } else {
            self.resolve_at(dirfd, path, !flags.contains(AtFlags::SYMLINK_NOFOLLOW))?.ino.ok_or(Errno::ENOENT)?
        };
        let mut w = self.w();
        let n = w.node_mut(ino);
        if let Some(u) = uid {
            n.uid = u;
        }
        if let Some(g) = gid {
            n.gid = g;
        }
        Ok(())
    }

    fn utimensat(&self, dirfd: Fd, path: &[u8], atime: SetTime, mtime: SetTime, flags: AtFlags) -> SysResult<()> {
        let r = self.resolve_at(dirfd, path, !flags.contains(AtFlags::SYMLINK_NOFOLLOW))?;
        let ino = r.ino.ok_or(Errno::ENOENT)?;
        let mut w = self.w();
        let now = w.now;
        let n = w.node_mut(ino);
        match atime {
            SetTime::Now => n.atime = now,
            SetTime::At(t) => n.atime = t,
            SetTime::Omit => {}
        }
        match mtime {
            SetTime::Now => n.mtime = now,
            SetTime::At(t) => n.mtime = t,
            SetTime::Omit => {}
        }
        n.ctime = now;
        Ok(())
    }

    fn futimens(&self, fd: Fd, atime: SetTime, mtime: SetTime) -> SysResult<()> {
        let e = self.fd_entry(fd)?;
        let f = lock(&e.file);
        if let OpenKind::Inode(ino) | OpenKind::Dir { ino, .. } = &f.kind {
            let mut w = self.w();
            let now = w.now;
            let n = w.node_mut(*ino);
            if let SetTime::At(t) = atime {
                n.atime = t;
            } else if atime == SetTime::Now {
                n.atime = now;
            }
            if let SetTime::At(t) = mtime {
                n.mtime = t;
            } else if mtime == SetTime::Now {
                n.mtime = now;
            }
        }
        Ok(())
    }

    fn ftruncate(&self, fd: Fd, len: u64) -> SysResult<()> {
        let e = self.fd_entry(fd)?;
        let f = lock(&e.file);
        if !f.flags.writable() {
            return Err(Errno::EINVAL);
        }
        match &f.kind {
            OpenKind::Inode(ino) => {
                let mut w = self.w();
                if let Kind::File(d) = &mut w.node_mut(*ino).kind {
                    d.resize(len as usize, 0);
                    Ok(())
                } else {
                    Err(Errno::EINVAL)
                }
            }
            _ => Err(Errno::EINVAL),
        }
    }

    fn fallocate(&self, fd: Fd, mode: FallocFlags, offset: i64, len: i64) -> SysResult<()> {
        mode.validate(offset, len)?;
        let e = self.fd_entry(fd)?;
        let f = lock(&e.file);
        if !f.flags.writable() {
            return Err(Errno::EBADF);
        }
        let ino = match &f.kind {
            OpenKind::Inode(ino) => *ino,
            OpenKind::Dir { .. } => return Err(Errno::EISDIR),
            OpenKind::PipeRead(_) | OpenKind::PipeWrite(_) => return Err(Errno::ESPIPE),
            OpenKind::Socket(_) => return Err(Errno::ENODEV),
        };
        let mut w = self.w();
        let now = w.now;
        let n = w.node_mut(ino);
        let d = match &mut n.kind {
            Kind::File(d) => d,
            Kind::Fifo => return Err(Errno::ESPIPE),
            _ => return Err(Errno::ENODEV),
        };
        let end = offset.checked_add(len).ok_or(Errno::EFBIG)? as u64;
        // O testkit modela só o tmpfs: os outros modos não existem nele.
        if mode.bits() & !(FallocFlags::KEEP_SIZE | FallocFlags::PUNCH_HOLE).bits() != 0 {
            return Err(Errno::EOPNOTSUPP);
        }
        let start = offset as usize;
        if mode.contains(FallocFlags::PUNCH_HOLE) {
            let stop = (end as usize).min(d.len());
            if start < stop {
                d[start..stop].fill(0);
            }
        } else if !mode.contains(FallocFlags::KEEP_SIZE) && end as usize > d.len() {
            d.resize(end as usize, 0);
        }
        n.mtime = now;
        n.ctime = now;
        Ok(())
    }

    fn fsync(&self, fd: Fd) -> SysResult<()> {
        self.fd_entry(fd).map(|_| ())
    }

    /// O kit roda um processo por vez, então nenhuma trava POSIX jamais conflita: `F_SETLK` concede e
    /// `F_GETLK` responde `F_UNLCK`.
    fn fcntl_lock(&self, fd: Fd, cmd: LockCmd, mut flock: Flock) -> SysResult<Flock> {
        self.fd_entry(fd)?;
        if matches!(cmd, LockCmd::Get | LockCmd::OfdGet) {
            flock.l_type = crate::fcntl::F_UNLCK;
        }
        Ok(flock)
    }

    fn getdents(&self, fd: Fd) -> SysResult<Vec<DirEntry>> {
        let e = self.fd_entry(fd)?;
        let mut f = lock(&e.file);
        let OpenKind::Dir { ino, listing } = &mut f.kind else {
            return Err(Errno::ENOTDIR);
        };
        if listing.is_some() {
            return Ok(Vec::new());
        }
        let w = self.w();
        let parent = {
            let p = path_of(&w, *ino).unwrap_or_else(|| b"/".to_vec());
            let up = if p == b"/" { b"/".to_vec() } else { p[..p.iter().rposition(|b| *b == b'/').unwrap_or(0).max(1)].to_vec() };
            w.resolve(b"/", &up, true).ok().and_then(|r| r.ino).unwrap_or(ROOT)
        };
        let mut out = vec![
            DirEntry { ino: *ino, kind: FileType::Directory, name: b".".to_vec() },
            DirEntry { ino: parent, kind: FileType::Directory, name: b"..".to_vec() },
        ];
        if let Kind::Dir(m) = &w.node(*ino).kind {
            // Ordem do tmpfs: mais novo primeiro.
            let mut items: Vec<(&Vec<u8>, &(Ino, u64))> = m.iter().collect();
            items.sort_by_key(|item| std::cmp::Reverse(item.1.1));
            for (name, (child, _)) in items {
                out.push(DirEntry { ino: *child, kind: w.node(*child).file_type(), name: name.clone() });
            }
        }
        drop(w);
        *listing = Some(Vec::new());
        Ok(out)
    }

    fn statfs(&self, path: &[u8]) -> SysResult<StatFs> {
        self.fstatat(Fd::CWD, path, AtFlags::empty())?;
        Ok(StatFs { fs_type: 0x0102_1994, bsize: 4096, blocks: 262_144, bfree: 262_000, bavail: 262_000, files: 65_536, ffree: 65_000, namelen: 255, frsize: 4096, flags: 0 })
    }

    fn fstatfs(&self, fd: Fd) -> SysResult<StatFs> {
        self.fd_entry(fd)?;
        self.statfs(b"/")
    }

    fn dup(&self, fd: Fd) -> SysResult<Fd> {
        self.dup_min(fd, Fd(0), false)
    }

    fn dup3(&self, old: Fd, new: Fd, cloexec: bool) -> SysResult<Fd> {
        if old == new {
            return Err(Errno::EINVAL);
        }
        let e = self.fd_entry(old)?;
        let removed = self.with_proc(|w, pid| {
            let p = w.procs.get_mut(&pid).expect("processo");
            
            p.fds.insert(new.0, FdEntry { file: e.file.clone(), cloexec })
        });
        drop(removed);
        Ok(new)
    }

    fn dup_min(&self, fd: Fd, min: Fd, cloexec: bool) -> SysResult<Fd> {
        let e = self.fd_entry(fd)?;
        let mut w = self.w();
        self.alloc_fd(&mut w, FdEntry { file: e.file, cloexec }, min.0)
    }

    fn get_cloexec(&self, fd: Fd) -> SysResult<bool> {
        Ok(self.fd_entry(fd)?.cloexec)
    }

    fn set_cloexec(&self, fd: Fd, on: bool) -> SysResult<()> {
        self.with_proc(|w, pid| {
            let e = w.procs.get_mut(&pid).and_then(|p| p.fds.get_mut(&fd.0)).ok_or(Errno::EBADF)?;
            e.cloexec = on;
            Ok(())
        })
    }

    fn get_status_flags(&self, fd: Fd) -> SysResult<OFlags> {
        Ok(lock(&self.fd_entry(fd)?.file).flags)
    }

    fn set_status_flags(&self, fd: Fd, flags: OFlags) -> SysResult<()> {
        let e = self.fd_entry(fd)?;
        let mut f = lock(&e.file);
        let keep = f.flags - (OFlags::APPEND | OFlags::NONBLOCK);
        f.flags = keep | (flags & (OFlags::APPEND | OFlags::NONBLOCK));
        Ok(())
    }

    fn pipe2(&self, flags: OFlags) -> SysResult<(Fd, Fd)> {
        let buf = Arc::new(Mutex::new(PipeBuf { data: VecDeque::new(), readers: 1, writers: 1 }));
        let cloexec = flags.contains(OFlags::CLOEXEC);
        let r = FdEntry { file: Arc::new(Mutex::new(OpenFile { kind: OpenKind::PipeRead(buf.clone()), flags: OFlags::RDONLY, offset: 0 })), cloexec };
        let wr = FdEntry { file: Arc::new(Mutex::new(OpenFile { kind: OpenKind::PipeWrite(buf), flags: OFlags::WRONLY, offset: 0 })), cloexec };
        let mut w = self.w();
        let a = self.alloc_fd(&mut w, r, 0)?;
        let b = self.alloc_fd(&mut w, wr, 0)?;
        Ok((a, b))
    }

    fn isatty(&self, _fd: Fd) -> bool {
        false
    }

    fn tcgetwinsize(&self, fd: Fd) -> SysResult<Winsize> {
        self.fd_entry(fd)?;
        Err(Errno::ENOTTY)
    }

    // O testkit não tem terminal (o /dev/tty dá ENXIO): todo fd válido é ENOTTY.
    fn tcgetattr(&self, fd: Fd) -> SysResult<Termios> {
        self.fd_entry(fd)?;
        Err(Errno::ENOTTY)
    }

    fn tcsetattr(&self, fd: Fd, _when: SetAttrWhen, _termios: &Termios) -> SysResult<()> {
        self.fd_entry(fd)?;
        Err(Errno::ENOTTY)
    }

    fn tcsetwinsize(&self, fd: Fd, _ws: Winsize) -> SysResult<()> {
        self.fd_entry(fd)?;
        Err(Errno::ENOTTY)
    }

    fn tcflush(&self, fd: Fd, _queue: i32) -> SysResult<()> {
        self.fd_entry(fd)?;
        Err(Errno::ENOTTY)
    }

    fn tcflow(&self, fd: Fd, _action: i32) -> SysResult<()> {
        self.fd_entry(fd)?;
        Err(Errno::ENOTTY)
    }

    fn open_fds(&self) -> Vec<Fd> {
        self.w().procs[&self.pid].fds.keys().map(|k| Fd(*k)).collect()
    }

    fn chdir(&self, path: &[u8]) -> SysResult<()> {
        let r = self.resolve_at(Fd::CWD, path, true)?;
        let ino = r.ino.ok_or(Errno::ENOENT)?;
        let mut w = self.w();
        if !matches!(w.node(ino).kind, Kind::Dir(_)) {
            return Err(Errno::ENOTDIR);
        }
        let p = path_of(&w, ino).ok_or(Errno::ENOENT)?;
        w.procs.get_mut(&self.pid).expect("processo").cwd = p;
        Ok(())
    }

    fn fchdir(&self, fd: Fd) -> SysResult<()> {
        let e = self.fd_entry(fd)?;
        let f = lock(&e.file);
        let OpenKind::Dir { ino, .. } = &f.kind else { return Err(Errno::ENOTDIR) };
        let mut w = self.w();
        let p = path_of(&w, *ino).ok_or(Errno::ENOENT)?;
        w.procs.get_mut(&self.pid).expect("processo").cwd = p;
        Ok(())
    }

    fn getcwd(&self) -> SysResult<Vec<u8>> {
        Ok(self.cwd())
    }

    fn umask(&self, mask: Mode) -> Mode {
        self.with_proc(|w, pid| std::mem::replace(&mut w.procs.get_mut(&pid).expect("processo").umask, mask & 0o777))
    }

    fn getpid(&self) -> Pid {
        self.pid
    }

    fn getppid(&self) -> Pid {
        self.w().procs[&self.pid].ppid
    }

    proc_getter!(getpgid => pgid);

    fn setpgid(&self, pid: Pid, pgid: Pid) -> SysResult<()> {
        let pid = if pid == 0 { self.pid } else { pid };
        let mut w = self.w();
        let p = w.procs.get_mut(&pid).ok_or(Errno::ESRCH)?;
        p.pgid = if pgid == 0 { pid } else { pgid };
        Ok(())
    }

    proc_getter!(getsid => sid);

    fn setsid(&self) -> SysResult<Pid> {
        let mut w = self.w();
        let p = w.procs.get_mut(&self.pid).expect("processo");
        if p.pgid == p.pid {
            return Err(Errno::EPERM);
        }
        p.sid = p.pid;
        p.pgid = p.pid;
        Ok(p.pid)
    }

    fn spawn(&self, spec: SpawnSpec) -> SysResult<Pid> {
        self.check_pending();
        // Valida o alvo antes de criar o processo (posix_spawn reporta ENOENT/EACCES ao chamador).
        {
            let r = self.resolve_at(Fd::CWD, &spec.path, true)?;
            let ino = r.ino.ok_or(Errno::ENOENT)?;
            let w = self.w();
            let n = w.node(ino);
            match &n.kind {
                Kind::Dir(_) => return Err(Errno::EACCES),
                Kind::File(d) if n.mode & 0o111 == 0 => {
                    let _ = d;
                    return Err(Errno::EACCES);
                }
                Kind::File(d) if !d.starts_with(b"#!") => return Err(Errno::ENOEXEC),
                _ => {}
            }
        }
        let pid = self.new_child(&spec.attrs, spec.argv.clone())?;
        let child = ProcHandle { world: self.world.clone(), pid };
        // Exec no filho: disposições Catch voltam ao padrão.
        {
            let mut w = self.w();
            w.procs.get_mut(&pid).expect("filho").sigs.retain(|_, d| *d == SigDisposition::Ignore);
        }
        let body = match child.prepare_exec(&spec.path, spec.argv, spec.attrs.env.clone()) {
            Ok(b) => b,
            Err(e) => {
                self.w().procs.remove(&pid);
                return Err(e);
            }
        };
        self.run_child(pid, body);
        Ok(pid)
    }

    fn spawn_fn(&self, attrs: ProcAttrs, name: Vec<u8>, body: ProcessFn) -> SysResult<Pid> {
        self.check_pending();
        let argv = self.w().procs[&self.pid].argv.clone();
        let pid = self.new_child(&attrs, argv)?;
        let _ = name;
        self.run_child(pid, body);
        Ok(pid)
    }

    fn execve(&self, path: &[u8], argv: &[Vec<u8>], env: Option<&[Vec<u8>]>) -> Errno {
        // Valida antes de desenrolar: se falhar, o processo continua (o execve volta com erro).
        let r = match self.resolve_at(Fd::CWD, path, true) {
            Ok(r) => r,
            Err(e) => return e,
        };
        let Some(ino) = r.ino else { return Errno::ENOENT };
        {
            let w = self.w();
            let n = w.node(ino);
            match &n.kind {
                Kind::Dir(_) => return Errno::EACCES,
                Kind::File(_) if n.mode & 0o111 == 0 => return Errno::EACCES,
                Kind::File(d) if !d.starts_with(b"#!") => return Errno::ENOEXEC,
                _ => {}
            }
        }
        std::panic::resume_unwind(Box::new(ExecUnwind { path: path.to_vec(), argv: argv.to_vec(), env: env.map(<[Vec<u8>]>::to_vec) }))
    }

    fn wait4_info(&self, target: WaitTarget, _options: WaitOptions) -> SysResult<Option<WaitInfo>> {
        self.wait_scan(target, true)
    }

    fn waitid(&self, target: WaitIdTarget, options: WaitOptions) -> SysResult<Option<WaitInfo>> {
        let target = match target {
            WaitIdTarget::All => WaitTarget::Any,
            WaitIdTarget::Pid(p) if p > 0 => WaitTarget::Pid(p),
            WaitIdTarget::Group(g) if g >= 0 => WaitTarget::Group(g),
            _ => return Err(Errno::EINVAL),
        };
        if !options.intersects(WaitOptions::EXITED | WaitOptions::UNTRACED | WaitOptions::CONTINUED) {
            return Err(Errno::EINVAL);
        }
        // O testkit só tem término: parada e continuação não existem aqui.
        if !options.contains(WaitOptions::EXITED) {
            return Ok(None);
        }
        self.wait_scan(target, !options.contains(WaitOptions::NOWAIT))
    }

    fn kill(&self, target: KillTarget, sig: Signal) -> SysResult<()> {
        let targets: Vec<Pid> = {
            let w = self.w();
            let my_pgid = w.procs[&self.pid].pgid;
            match target {
                KillTarget::Pid(p) => {
                    if !w.procs.contains_key(&p) {
                        return Err(Errno::ESRCH);
                    }
                    vec![p]
                }
                KillTarget::Group(g) => {
                    let g = if g == 0 { my_pgid } else { g };
                    let v: Vec<Pid> = w.procs.values().filter(|p| p.pgid == g && p.status.is_none()).map(|p| p.pid).collect();
                    if v.is_empty() {
                        return Err(Errno::ESRCH);
                    }
                    v
                }
                KillTarget::All => w.procs.values().filter(|p| p.pid != self.pid && p.status.is_none()).map(|p| p.pid).collect(),
            }
        };
        if sig.0 == 0 {
            return Ok(());
        }
        if !sig.is_valid() {
            return Err(Errno::EINVAL);
        }
        for pid in targets {
            if pid == self.pid {
                self.deliver(sig)?;
            } else {
                let mut w = self.w();
                if let Some(p) = w.procs.get_mut(&pid)
                    && p.status.is_none() {
                        match p.sigs.get(&sig.0).copied().unwrap_or(SigDisposition::Default) {
                            SigDisposition::Catch => p.caught.push(sig),
                            SigDisposition::Ignore => {}
                            SigDisposition::Default => p.pending_fatal = Some(sig),
                        }
                    }
            }
        }
        Ok(())
    }

    fn sigaction(&self, sig: Signal, disposition: SigDisposition) -> SysResult<SigDisposition> {
        if !sig.is_valid() || sig.is_uncatchable() {
            return Err(Errno::EINVAL);
        }
        self.with_proc(|w, pid| {
            let p = w.procs.get_mut(&pid).expect("processo");
            Ok(p.sigs.insert(sig.0, disposition).unwrap_or(SigDisposition::Default))
        })
    }

    fn take_caught_signals(&self) -> Vec<Signal> {
        self.with_proc(|w, pid| std::mem::take(&mut w.procs.get_mut(&pid).expect("processo").caught))
    }

    fn checkpoint(&self) {
        self.check_pending();
    }

    fn sched_yield(&self) {}

    fn getpriority(&self, pid: Pid) -> SysResult<i32> {
        let w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        Ok(w.procs[&pid].nice)
    }

    fn setpriority(&self, pid: Pid, nice: i32) -> SysResult<()> {
        // O testkit não tem permissões: todo mundo é root e a nice só é guardada.
        let mut w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        w.procs.get_mut(&pid).expect("processo").nice = nice.clamp(-20, 19);
        Ok(())
    }

    fn getrlimit(&self, res: Resource) -> SysResult<Rlimit> {
        let w = self.w();
        Ok(w.procs[&self.pid].rlimits.get(&res).copied().unwrap_or(match res {
            Resource::Nofile => Rlimit { cur: 1024, max: 1_048_576 },
            Resource::Stack => Rlimit { cur: 8 << 20, max: RLIM_INFINITY },
            Resource::Core => Rlimit { cur: 0, max: RLIM_INFINITY },
            _ => Rlimit { cur: RLIM_INFINITY, max: RLIM_INFINITY },
        }))
    }

    fn setrlimit(&self, res: Resource, lim: Rlimit) -> SysResult<()> {
        if lim.cur > lim.max {
            return Err(Errno::EINVAL);
        }
        self.with_proc(|w, pid| {
            w.procs.get_mut(&pid).expect("processo").rlimits.insert(res, lim);
        });
        Ok(())
    }

    fn getrusage(&self, _who: RusageWho) -> SysResult<Rusage> {
        Ok(Rusage::default())
    }

    fn list_processes(&self) -> Vec<ProcInfo> {
        self.w()
            .procs
            .values()
            .filter(|p| !p.reaped)
            .map(|p| ProcInfo {
                pid: p.pid,
                ppid: p.ppid,
                pgid: p.pgid,
                sid: p.sid,
                state: if p.status.is_some() { 'Z' } else { 'R' },
                comm: p.argv.first().map(|a| a.rsplit(|b| *b == b'/').next().unwrap_or(a).to_vec()).unwrap_or_default(),
            })
            .collect()
    }

    fn spawn_thread(&self, body: ThreadFn) -> SysResult<Tid> {
        let tid = {
            let mut w = self.w();
            let t = w.next_tid;
            w.next_tid += 1;
            t
        };
        // Concorrente, numa thread do hospedeiro com o mesmo processo instalado: threads que conversam
        // por canal (o leitor e o ordenador do `sort`) travariam se uma rodasse até o fim antes da
        // outra começar. Um `exit` dentro dela volta no `join_thread` e desenrola o chamador, que é o
        // mais perto do `exit_group` que o kit consegue sem kernel.
        let handle: Arc<dyn Syscalls> = Arc::new(ProcHandle { world: Arc::clone(&self.world), pid: self.pid });
        let h = std::thread::Builder::new()
            .name(format!("testkit-{tid}"))
            .spawn(move || {
                sys::install(handle);
                let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body));
                sys::uninstall();
                r.err().filter(|p| p.is::<ExitUnwind>() || p.is::<KillUnwind>() || p.is::<ExecUnwind>())
            })
            .map_err(|_| Errno::EAGAIN)?;
        self.w().threads.insert(tid, h);
        Ok(tid)
    }

    fn reserve_task(&self) -> SysResult<()> {
        Ok(())
    }

    fn release_task(&self) {}

    fn join_thread(&self, tid: Tid) -> SysResult<()> {
        if tid == self.pid {
            return Err(Errno::EDEADLK);
        }
        let h = {
            let mut w = self.w();
            match w.threads.remove(&tid) {
                Some(h) => h,
                None if tid < w.next_tid && tid >= 1_000_000 => return Err(Errno::EINVAL),
                None => return Err(Errno::ESRCH),
            }
        };
        if let Ok(Some(unwind)) = h.join() {
            std::panic::resume_unwind(unwind);
        }
        Ok(())
    }

    fn gettid(&self) -> Tid {
        self.pid
    }

    fn sched_getaffinity(&self) -> Vec<usize> {
        let w = self.w();
        sched::effective_affinity(w.ncpus, w.procs[&self.pid].cpus.as_deref())
    }

    fn sched_getaffinity_of(&self, pid: Pid) -> SysResult<Vec<usize>> {
        let w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        Ok(sched::effective_affinity(w.ncpus, w.procs[&pid].cpus.as_deref()))
    }

    fn sched_setaffinity(&self, pid: Pid, cpus: &[usize]) -> SysResult<()> {
        let mut w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        let mask = sched::normalize_affinity(w.ncpus, cpus)?;
        w.procs.get_mut(&pid).expect("processo").cpus = Some(mask);
        Ok(())
    }

    fn personality(&self, persona: u32) -> SysResult<u32> {
        let mut w = self.w();
        let p = w.procs.get_mut(&self.pid).expect("processo");
        let (old, new) = sched::personality::change(p.persona, persona)?;
        p.persona = new;
        Ok(old)
    }

    fn sched_getscheduler(&self, pid: Pid) -> SysResult<i32> {
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        Ok(w.procs[&pid].sched.scheduler())
    }

    fn sched_setscheduler(&self, pid: Pid, policy: i32, param: SchedParam) -> SysResult<()> {
        if pid < 0 || policy < 0 {
            return Err(Errno::EINVAL);
        }
        let mut w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        let p = w.procs.get_mut(&pid).expect("processo");
        let attr = sched::attr_for_setscheduler(policy, param, p.nice)?;
        let (s, n) = p.sched.set(p.nice, &attr, false, &Self::sched_caller(p))?;
        p.sched = s;
        p.nice = n;
        Ok(())
    }

    fn sched_getparam(&self, pid: Pid) -> SysResult<SchedParam> {
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        Ok(SchedParam { priority: w.procs[&pid].sched.rt_priority as i32 })
    }

    fn sched_setparam(&self, pid: Pid, param: SchedParam) -> SysResult<()> {
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let mut w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        let p = w.procs.get_mut(&pid).expect("processo");
        let attr = sched::attr_for_setparam(param, p.nice);
        let (s, n) = p.sched.set(p.nice, &attr, true, &Self::sched_caller(p))?;
        p.sched = s;
        p.nice = n;
        Ok(())
    }

    fn sched_rr_get_interval(&self, pid: Pid) -> SysResult<Duration> {
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        Ok(w.procs[&pid].sched.rr_interval(w.ncpus))
    }

    fn sched_getattr(&self, pid: Pid, size: u32, flags: u32) -> SysResult<SchedAttr> {
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        sched::check_getattr(size, flags)?;
        let w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        let p = &w.procs[&pid];
        Ok(p.sched.attr(p.nice, size, w.ncpus))
    }

    fn sched_setattr(&self, pid: Pid, attr: &SchedAttr, flags: u32) -> SysResult<()> {
        if pid < 0 {
            return Err(Errno::EINVAL);
        }
        let attr = sched::check_setattr(attr, flags)?;
        let mut w = self.w();
        let pid = self.sched_pid(&w, pid)?;
        let p = w.procs.get_mut(&pid).expect("processo");
        let (s, n) = p.sched.set(p.nice, &attr, false, &Self::sched_caller(p))?;
        p.sched = s;
        p.nice = n;
        Ok(())
    }

    fn ioprio_get(&self, which: i32, who: i32) -> SysResult<i32> {
        let w = self.w();
        let pids = self.ioprio_targets(&w, which, who)?;
        let mut best: Option<i32> = None;
        for pid in pids {
            let p = &w.procs[&pid];
            let v = sched::ioprio_effective(p.ioprio, p.sched.policy, p.nice);
            best = Some(best.map_or(v, |b| sched::ioprio_best(b, v)));
        }
        best.ok_or(Errno::ESRCH)
    }

    fn ioprio_set(&self, which: i32, who: i32, ioprio: i32) -> SysResult<()> {
        sched::ioprio_check(ioprio)?;
        let mut w = self.w();
        let pids = self.ioprio_targets(&w, which, who)?;
        if pids.is_empty() {
            return Err(Errno::ESRCH);
        }
        for pid in pids {
            w.procs.get_mut(&pid).expect("processo").ioprio = ioprio;
        }
        Ok(())
    }

    fn getuid(&self) -> Uid {
        0
    }

    fn geteuid(&self) -> Uid {
        0
    }

    fn getgid(&self) -> Gid {
        0
    }

    fn getegid(&self) -> Gid {
        0
    }

    fn getgroups(&self) -> Vec<Gid> {
        vec![0]
    }

    fn argv(&self) -> Vec<Vec<u8>> {
        self.w().procs[&self.pid].argv.clone()
    }

    fn environ(&self) -> Vec<Vec<u8>> {
        self.w().procs[&self.pid].env.clone()
    }

    fn getenv(&self, name: &[u8]) -> Option<Vec<u8>> {
        let w = self.w();
        w.procs[&self.pid].env.iter().find_map(|kv| {
            let eq = kv.iter().position(|b| *b == b'=')?;
            (&kv[..eq] == name).then(|| kv[eq + 1..].to_vec())
        })
    }

    fn setenv(&self, name: &[u8], value: &[u8]) -> SysResult<()> {
        if name.is_empty() || name.contains(&b'=') {
            return Err(Errno::EINVAL);
        }
        self.with_proc(|w, pid| {
            let p = w.procs.get_mut(&pid).expect("processo");
            p.env.retain(|kv| !(kv.starts_with(name) && kv.get(name.len()) == Some(&b'=')));
            let mut kv = name.to_vec();
            kv.push(b'=');
            kv.extend_from_slice(value);
            p.env.push(kv);
        });
        Ok(())
    }

    fn unsetenv(&self, name: &[u8]) -> SysResult<()> {
        self.with_proc(|w, pid| {
            w.procs.get_mut(&pid).expect("processo").env.retain(|kv| !(kv.starts_with(name) && kv.get(name.len()) == Some(&b'=')));
        });
        Ok(())
    }

    fn uname(&self) -> Utsname {
        let w = self.w();
        let mut u = Utsname {
            sysname: b"Linux".to_vec(),
            nodename: w.hostname.clone(),
            release: b"6.12.101+deb13-amd64".to_vec(),
            version: b"#1 SMP PREEMPT_DYNAMIC Debian 6.12.101-1".to_vec(),
            machine: b"x86_64".to_vec(),
            domainname: w.domainname.clone(),
        };
        sched::personality::apply_to_uname(w.procs[&self.pid].persona, &mut u);
        u
    }

    fn sethostname(&self, name: &[u8]) -> SysResult<()> {
        self.w().hostname = name.to_vec();
        Ok(())
    }

    fn setdomainname(&self, name: &[u8]) -> SysResult<()> {
        self.w().domainname = name.to_vec();
        Ok(())
    }

    fn clock_gettime(&self, clock: Clock) -> SysResult<TimeSpec> {
        let now = self.w().now;
        Ok(match clock {
            Clock::Realtime => now,
            _ => TimeSpec { sec: now.sec - DEFAULT_TIME + 1000, nsec: now.nsec },
        })
    }

    fn nanosleep(&self, d: Duration) -> SysResult<()> {
        self.check_pending();
        let target = self.w().now_ns() + d.as_nanos() as u64;
        loop {
            // O alarme de parede que vence antes do fim do sono interrompe o sono se o `SIGALRM` é capturado.
            let due = self.w().procs[&self.pid].itimers[Itimer::Real as usize].expiry().filter(|at| *at <= target);
            let stop = {
                let mut w = self.w();
                let stop = due.unwrap_or(target).max(w.now_ns());
                w.set_now_ns(stop);
                stop
            };
            let Some(_) = due else { return Ok(()) };
            let fired = self.with_proc(|w, pid| w.procs.get_mut(&pid).expect("processo").itimers[Itimer::Real as usize].fire(stop));
            if fired {
                let catches = self.w().procs[&self.pid].sigs.get(&Signal::SIGALRM.0) == Some(&SigDisposition::Catch);
                self.deliver(Signal::SIGALRM)?;
                if catches {
                    return Err(Errno::EINTR);
                }
            }
        }
    }

    fn setitimer(&self, which: i32, new: Itimerval) -> SysResult<Itimerval> {
        let which = Itimer::from_raw(which)?;
        let (value, interval) = new.to_ns()?;
        self.with_proc(|w, pid| {
            let now = w.now_ns();
            let (old_value, old_interval) = w.procs.get_mut(&pid).expect("processo").itimers[which as usize].set(which, now, value, interval);
            Ok(Itimerval::from_ns(old_value, old_interval))
        })
    }

    fn getitimer(&self, which: i32) -> SysResult<Itimerval> {
        let which = Itimer::from_raw(which)?;
        self.with_proc(|w, pid| {
            let (value, interval) = w.procs[&pid].itimers[which as usize].get(which, w.now_ns());
            Ok(Itimerval::from_ns(value, interval))
        })
    }

    fn getrandom(&self, buf: &mut [u8]) -> SysResult<usize> {
        let mut w = self.w();
        for b in buf.iter_mut() {
            *b = w.rand() as u8;
        }
        Ok(buf.len())
    }

    fn local_timezone(&self) -> Vec<u8> {
        self.getenv(b"TZ").unwrap_or_else(|| b"UTC".to_vec())
    }

    fn poll(&self, fds: &mut [PollFd], timeout: Option<Duration>) -> SysResult<usize> {
        self.check_pending();
        let mut ready = 0;
        for p in fds.iter_mut() {
            p.revents = PollEvents::empty();
            let Ok(e) = self.fd_entry(p.fd) else {
                p.revents = PollEvents::NVAL;
                ready += 1;
                continue;
            };
            let f = lock(&e.file);
            let mut rev = match &f.kind {
                OpenKind::Inode(_) | OpenKind::Dir { .. } => PollEvents::IN | PollEvents::OUT,
                OpenKind::PipeRead(b) => {
                    let b = lock(b);
                    let mut r = PollEvents::empty();
                    if !b.data.is_empty() {
                        r |= PollEvents::IN;
                    }
                    if b.writers == 0 {
                        r |= PollEvents::HUP;
                    }
                    r
                }
                OpenKind::PipeWrite(b) => {
                    if lock(b).readers == 0 { PollEvents::ERR } else { PollEvents::OUT }
                }
                // Síncrono: não dá pra saber sem bloquear; o socket de teste é tratado como pronto.
                OpenKind::Socket(_) => PollEvents::IN | PollEvents::OUT,
            };
            rev &= p.events | PollEvents::ERR | PollEvents::HUP | PollEvents::NVAL;
            p.revents = rev;
            if !rev.is_empty() {
                ready += 1;
            }
        }
        if ready == 0 {
            // Nada fica pronto num mundo síncrono: o tempo passa e o poll expira.
            if let Some(d) = timeout {
                self.nanosleep(d)?;
            }
        }
        Ok(ready)
    }

    fn ofd_setlk(&self, fd: Fd, lk: FileLock, wait: bool) -> SysResult<()> {
        let e = self.fd_entry(fd)?;
        let (ino, flags) = {
            let f = lock(&e.file);
            match &f.kind {
                OpenKind::Inode(ino) => (*ino, f.flags),
                _ => return Err(Errno::EINVAL),
            }
        };
        match lk.kind {
            LockKind::Read if !flags.readable() => return Err(Errno::EBADF),
            LockKind::Write if !flags.writable() => return Err(Errno::EBADF),
            _ => {}
        }
        let me = Arc::downgrade(&e.file);
        let mut w = self.w();
        let table = w.locks.entry(ino).or_default();
        table.retain(|(owner, _)| owner.strong_count() > 0);
        if lk.kind != LockKind::Unlock {
            let conflict = table.iter().any(|(owner, other)| {
                !owner.ptr_eq(&me) && other.overlaps(&lk) && (lk.kind == LockKind::Write || other.kind == LockKind::Write)
            });
            if conflict {
                // Síncrono: esperar nunca terminaria.
                return Err(if wait { Errno::EDEADLK } else { Errno::EAGAIN });
            }
        }
        // Remove a faixa deste dono e insere a nova (sem fusão fina: suficiente pros testes).
        let mut kept = Vec::new();
        for (owner, other) in table.drain(..) {
            if owner.ptr_eq(&me) && other.overlaps(&lk) {
                let end = |l: &FileLock| if l.len == 0 { u64::MAX } else { l.start + l.len };
                if other.start < lk.start {
                    kept.push((owner.clone(), FileLock { kind: other.kind, start: other.start, len: lk.start - other.start }));
                }
                if end(&other) > end(&lk) && lk.len != 0 {
                    let s = end(&lk);
                    let len = if other.len == 0 { 0 } else { end(&other) - s };
                    kept.push((owner, FileLock { kind: other.kind, start: s, len }));
                }
            } else {
                kept.push((owner, other));
            }
        }
        if lk.kind != LockKind::Unlock {
            kept.push((me, lk));
        }
        *table = kept;
        Ok(())
    }

    fn ofd_getlk(&self, fd: Fd, lk: FileLock) -> SysResult<Option<FileLock>> {
        let e = self.fd_entry(fd)?;
        let ino = match &lock(&e.file).kind {
            OpenKind::Inode(ino) => *ino,
            _ => return Err(Errno::EINVAL),
        };
        let me = Arc::downgrade(&e.file);
        let w = self.w();
        Ok(w.locks.get(&ino).and_then(|t| {
            t.iter()
                .find(|(owner, other)| {
                    owner.strong_count() > 0
                        && !owner.ptr_eq(&me)
                        && other.overlaps(&lk)
                        && (lk.kind == LockKind::Write || other.kind == LockKind::Write)
                })
                .map(|(_, l)| *l)
        }))
    }

    fn net_connect(&self, host: &[u8], port: u16, timeout: Option<Duration>) -> SysResult<NetConn> {
        self.check_pending();
        let handler = self.w().net.clone().ok_or(Errno::ENETUNREACH)?;
        let stream = handler(host, port)?;
        if let Some(t) = timeout {
            let _ = stream.set_read_timeout(Some(t));
        }
        let (peer, local) = (stream.peer(), stream.local());
        let entry = FdEntry {
            file: Arc::new(Mutex::new(OpenFile { kind: OpenKind::Socket(Arc::new(Mutex::new(Socket { stream }))), flags: OFlags::RDWR, offset: 0 })),
            cloexec: false,
        };
        let mut w = self.w();
        let fd = self.alloc_fd(&mut w, entry, 0)?;
        Ok(NetConn { fd, peer, local })
    }
}

/// Resultado de [`TestKit::run`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunResult {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: WaitStatus,
}

impl RunResult {

    pub fn stdout_str(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }

    pub fn stderr_str(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

/// Entrada de [`TestKit::tree`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TreeEntry {
    File { data: Vec<u8>, mode: Mode },
    Dir { mode: Mode },
    Symlink { target: Vec<u8> },
    Other { mode: Mode },
}

/// O kernel de teste. Cada `TestKit` é um sistema independente.
pub struct TestKit {
    world: Arc<Mutex<World>>,
    cwd: Vec<u8>,
    env: Vec<Vec<u8>>,
}

impl Default for TestKit {
    fn default() -> Self {
        Self::new()
    }
}

impl TestKit {
    pub fn new() -> TestKit {
        TestKit {
            world: Arc::new(Mutex::new(World::new())),
            cwd: b"/work".to_vec(),
            env: BASE_ENV.iter().map(|(k, v)| format!("{k}={v}").into_bytes()).collect(),
        }
    }

    /// Registra programas embutidos em `dir/name` (ex.: /usr/bin/grep).
    pub fn programs(self, programs: impl IntoIterator<Item = Program>) -> TestKit {
        {
            let mut w = lock(&self.world);
            for p in programs {
                w.programs.insert(p.name.to_string(), p.main);
                w.put(p.path().as_bytes(), Kind::Builtin(p.name.to_string()), 0o755);
            }
        }
        self
    }

    pub fn file(self, path: &str, data: impl AsRef<[u8]>, mode: Mode) -> TestKit {
        self.put_file(path.as_bytes(), data.as_ref(), mode);
        self
    }

    pub fn put_file(&self, path: &[u8], data: &[u8], mode: Mode) {
        lock(&self.world).put(path, Kind::File(data.to_vec()), mode);
    }

    pub fn dir(self, path: &str, mode: Mode) -> TestKit {
        lock(&self.world).mkdir_p(path.as_bytes(), mode);
        self
    }

    pub fn put_dir(&self, path: &[u8], mode: Mode) {
        lock(&self.world).mkdir_p(path, mode);
    }

    pub fn symlink(self, path: &str, target: &str) -> TestKit {
        self.put_symlink(path.as_bytes(), target.as_bytes());
        self
    }

    pub fn put_symlink(&self, path: &[u8], target: &[u8]) {
        lock(&self.world).put(path, Kind::Symlink(target.to_vec()), 0o777);
    }

    /// Ajusta o mtime (e atime) de um caminho já existente.
    pub fn set_mtime(&self, path: &[u8], sec: i64) {
        let mut w = lock(&self.world);
        if let Ok(r) = w.resolve(b"/", path, false)
            && let Some(ino) = r.ino {
                let n = w.node_mut(ino);
                n.mtime = TimeSpec { sec, nsec: 0 };
                n.atime = TimeSpec { sec, nsec: 0 };
            }
    }

    pub fn cwd(mut self, path: &str) -> TestKit {
        self.cwd = path.as_bytes().to_vec();
        self
    }

    pub fn env(mut self, key: &str, value: &str) -> TestKit {
        let prefix = format!("{key}=");
        self.env.retain(|kv| !kv.starts_with(prefix.as_bytes()));
        self.env.push(format!("{key}={value}").into_bytes());
        self
    }

    /// Número de CPUs virtuais que `sched_getaffinity` informa (padrão: 1).
    pub fn cpus(self, n: usize) -> TestKit {
        lock(&self.world).ncpus = n.max(1);
        self
    }

    /// Liga a rede de teste: `net_connect` passa a chamar `handler(host, porta)`.
    pub fn net(self, handler: NetHandler) -> TestKit {
        lock(&self.world).net = Some(handler);
        self
    }

    /// Relógio do sistema (segundos desde a época).
    pub fn time(self, sec: i64) -> TestKit {
        lock(&self.world).now = TimeSpec { sec, nsec: 0 };
        self
    }

    pub fn read_file(&self, path: &str) -> Option<Vec<u8>> {
        let w = lock(&self.world);
        let r = w.resolve(b"/", path.as_bytes(), true).ok()?;
        match &w.node(r.ino?).kind {
            Kind::File(d) => Some(d.clone()),
            _ => None,
        }
    }

    /// Retrato recursivo de um diretório (caminhos relativos a `root`, ordenados).
    pub fn tree(&self, root: &str) -> Vec<(Vec<u8>, TreeEntry)> {
        let w = lock(&self.world);
        let mut out = Vec::new();
        let Ok(r) = w.resolve(b"/", root.as_bytes(), true) else { return out };
        let Some(start) = r.ino else { return out };
        let mut stack: Vec<(Ino, Vec<u8>)> = vec![(start, Vec::new())];
        while let Some((ino, prefix)) = stack.pop() {
            if let Kind::Dir(m) = &w.node(ino).kind {
                for (name, (child, _)) in m {
                    let rel = if prefix.is_empty() { name.clone() } else { join(&prefix, name) };
                    let n = w.node(*child);
                    let entry = match &n.kind {
                        Kind::File(d) => TreeEntry::File { data: d.clone(), mode: n.mode },
                        Kind::Dir(_) => {
                            stack.push((*child, rel.clone()));
                            TreeEntry::Dir { mode: n.mode }
                        }
                        Kind::Symlink(t) => TreeEntry::Symlink { target: t.clone() },
                        _ => TreeEntry::Other { mode: n.mode },
                    };
                    out.push((rel, entry));
                }
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    /// Roda `argv` (argv[0] é procurado no PATH se não tiver `/`) com `stdin`, como um processo filho
    /// do "sistema", e devolve stdout, stderr e status.
    pub fn run(&self, argv: &[&str], stdin: &[u8]) -> RunResult {
        let argv: Vec<Vec<u8>> = argv.iter().map(|a| a.as_bytes().to_vec()).collect();
        self.run_bytes(&argv, stdin)
    }

    pub fn run_bytes(&self, argv: &[Vec<u8>], stdin: &[u8]) -> RunResult {
        let root_pid = {
            let mut w = lock(&self.world);
            let pid = w.next_pid;
            w.next_pid += 1;
            w.procs.insert(
                pid,
                Proc {
                    pid,
                    ppid: 1,
                    pgid: pid,
                    sid: pid,
                    cwd: self.cwd.clone(),
                    env: self.env.clone(),
                    argv: vec![b"testkit".to_vec()],
                    umask: 0o022,
                    fds: BTreeMap::new(),
                    sigs: BTreeMap::new(),
                    caught: Vec::new(),
                    pending_fatal: None,
                    status: None,
                    reaped: false,
                    rlimits: BTreeMap::new(),
                    nice: 0,
                    sched: SchedState::default(),
                    ioprio: 0,
                    persona: 0,
                    cpus: None,
                    itimers: [ItimerSlot::default(); 3],
                },
            );
            pid
        };
        let root = ProcHandle { world: self.world.clone(), pid: root_pid };
        // O processo raiz ocupa 0, 1 e 2 com /dev/null, pra que os pipes de captura ganhem números
        // acima de 2 e as ações de fd do filho não se atropelem.
        for _ in 0..3 {
            root.openat(Fd::CWD, b"/dev/null", OFlags::RDWR, 0).expect("/dev/null");
        }
        let (in_r, in_w) = root.pipe2(OFlags::empty()).expect("pipe stdin");
        root.write(in_w, stdin).expect("stdin cabe no pipe");
        root.close(in_w).expect("fechar stdin");
        let (out_r, out_w) = root.pipe2(OFlags::empty()).expect("pipe stdout");
        let (err_r, err_w) = root.pipe2(OFlags::empty()).expect("pipe stderr");
        let path = resolve_in_path(&root, &argv[0]);
        let attrs = ProcAttrs {
            fd_actions: vec![
                FdAction::Dup2 { from: in_r, to: Fd::STDIN },
                FdAction::Dup2 { from: out_w, to: Fd::STDOUT },
                FdAction::Dup2 { from: err_w, to: Fd::STDERR },
                FdAction::Close(in_r),
                FdAction::Close(out_w),
                FdAction::Close(err_w),
                FdAction::Close(out_r),
                FdAction::Close(err_r),
            ],
            ..ProcAttrs::default()
        };
        let status = match root.spawn(SpawnSpec { path, argv: argv.to_vec(), attrs }) {
            Ok(pid) => match root.wait4(WaitTarget::Pid(pid), WaitOptions::empty()) {
                Ok(Some((_, st))) => st,
                _ => WaitStatus::Exited(255),
            },
            Err(e) => {
                let name = String::from_utf8_lossy(&argv[0]).into_owned();
                let msg = if e == Errno::ENOENT { format!("{name}: command not found\n") } else { format!("{name}: {}\n", e.message()) };
                root.write(err_w, msg.as_bytes()).ok();
                WaitStatus::Exited(if e == Errno::ENOENT { 127 } else { 126 })
            }
        };
        let _ = root.close(out_w);
        let _ = root.close(err_w);
        let drain = |fd: Fd| {
            let mut out = Vec::new();
            let mut buf = vec![0u8; 65536];
            while let Ok(n) = root.read(fd, &mut buf) {
                if n == 0 {
                    break;
                }
                out.extend_from_slice(&buf[..n]);
            }
            out
        };
        let stdout = drain(out_r);
        let stderr = drain(err_r);
        lock(&self.world).procs.remove(&root_pid);
        RunResult { stdout, stderr, status }
    }
}

fn resolve_in_path(root: &ProcHandle, name: &[u8]) -> Vec<u8> {
    if name.contains(&b'/') {
        return name.to_vec();
    }
    let path = root.getenv(b"PATH").unwrap_or_default();
    for dir in path.split(|b| *b == b':') {
        let candidate = join(if dir.is_empty() { b"." } else { dir }, name);
        if root.fstatat(Fd::CWD, &candidate, AtFlags::empty()).is_ok() {
            return candidate;
        }
    }
    name.to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    fn cat(ctx: &mut Ctx, args: &[OsString]) -> i32 {
        use std::os::unix::ffi::OsStrExt;
        let mut out = ctx.stdout();
        if args.len() <= 1 {
            let mut data = Vec::new();
            ctx.stdin().read_to_end(&mut data).ok();
            out.write_all(&data).ok();
            return 0;
        }
        let mut code = 0;
        for a in &args[1..] {
            match sys::read_file(a.as_bytes()) {
                Ok(d) => {
                    out.write_all(&d).ok();
                }
                Err(e) => {
                    ctx.error(format!("{}: {}", a.to_string_lossy(), e.message()));
                    code = 1;
                }
            }
        }
        code
    }

    fn yes(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let mut out = ctx.stdout();
        loop {
            if out.write_all(b"y\n").is_err() {
                return 1;
            }
        }
    }

    fn head1(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let mut b = [0u8; 2];
        let n = ctx.stdin().read(&mut b).unwrap_or(0);
        ctx.stdout().write_all(&b[..n]).ok();
        0
    }

    fn pipeline(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        // Simula `yes | head1` com spawn e pipe, como um shell faria.
        let sys = ctx.sys().clone();
        let (r, w) = sys.pipe2(OFlags::CLOEXEC).unwrap();
        let a = sys
            .spawn(SpawnSpec { path: b"/usr/bin/yes".to_vec(), argv: vec![b"yes".to_vec()], attrs: ProcAttrs { fd_actions: vec![FdAction::Dup2 { from: w, to: Fd::STDOUT }], ..ProcAttrs::default() } })
            .unwrap();
        sys.close(w).unwrap();
        let b = sys
            .spawn(SpawnSpec { path: b"/usr/bin/head1".to_vec(), argv: vec![b"head1".to_vec()], attrs: ProcAttrs { fd_actions: vec![FdAction::Dup2 { from: r, to: Fd::STDIN }], ..ProcAttrs::default() } })
            .unwrap();
        sys.close(r).unwrap();
        let (_, sa) = sys.wait4(WaitTarget::Pid(a), WaitOptions::empty()).unwrap().unwrap();
        let (_, sb) = sys.wait4(WaitTarget::Pid(b), WaitOptions::empty()).unwrap().unwrap();
        let msg = format!("{} {}\n", sa.shell_status(), sb.shell_status());
        ctx.stdout().write_all(msg.as_bytes()).ok();
        0
    }

    fn kit() -> TestKit {
        TestKit::new().programs([
            Program::bin("cat", cat),
            Program::bin("yes", yes),
            Program::bin("head1", head1),
            Program::bin("pipeline", pipeline),
        ])
    }

    #[test]
    fn runs_programs_with_files_stdin_and_errors() {
        let k = kit().file("/work/in.txt", "abc\n", 0o644);
        let r = k.run(&["cat", "in.txt", "missing"], b"");
        assert_eq!(r.stdout, b"abc\n");
        assert_eq!(r.stderr_str(), "cat: missing: No such file or directory\n");
        assert_eq!(r.status.shell_status(), 1);
        let r = k.run(&["cat"], b"from stdin");
        assert_eq!(r.stdout, b"from stdin");
        let r = k.run(&["nope"], b"");
        assert_eq!(r.status.shell_status(), 127);
    }

    fn read_past_end(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let sys = ctx.sys().clone();
        let fd = sys.openat(Fd::CWD, b"small", OFlags::RDONLY, 0).unwrap();
        sys.lseek(fd, 1000, Whence::Set).unwrap();
        let mut buf = [0u8; 16];
        let n = sys.read(fd, &mut buf).unwrap();
        let p = sys.pread(fd, &mut buf, 5000).unwrap();
        ctx.stdout().write_all(format!("{n} {p}\n").as_bytes()).ok();
        0
    }

    #[test]
    fn read_past_end_returns_zero() {
        let k = kit().programs([Program::bin("rpe", read_past_end)]).file("/work/small", "abc", 0o644);
        assert_eq!(k.run(&["rpe"], b"").stdout_str(), "0 0\n");
    }

    #[test]
    fn pipeline_gets_sigpipe_like_linux() {
        let r = kit().run(&["pipeline"], b"");
        // yes morre de SIGPIPE (141) depois que o leitor some; head1 sai com 0.
        assert_eq!(r.stdout_str(), "y\n141 0\n");
    }

    #[test]
    fn namei_symlinks_and_tmpfs_order() {
        let k = kit().file("/work/a", "1", 0o644).file("/work/b", "2", 0o644).symlink("/work/l", "a");
        let tree = k.tree("/work");
        assert_eq!(tree.len(), 3);
        let r = k.run(&["cat", "l", "/usr/../work/b"], b"");
        assert_eq!(r.stdout, b"12");
        // Como no Linux: /bin é symlink pra usr/bin, então /bin/.. é /usr, não /.
        let r = k.run(&["cat", "/bin/../work/b"], b"");
        assert_eq!(r.stderr_str(), "cat: /bin/../work/b: No such file or directory\n");
        // tmpfs: readdir do mais novo pro mais antigo.
        let names: Vec<Vec<u8>> = k.tree("/work").into_iter().map(|e| e.0).collect();
        assert_eq!(names, vec![b"a".to_vec(), b"b".to_vec(), b"l".to_vec()]);
    }

    fn termios_probe(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let sys = ctx.sys().clone();
        let t = Termios::default();
        let ws = Winsize { rows: 24, cols: 80, xpixel: 0, ypixel: 0 };
        let mut out = String::new();
        for (fd, want) in [(Fd::STDIN, Errno::ENOTTY), (Fd(42), Errno::EBADF)] {
            let g = sys.tcgetattr(fd).err() == Some(want);
            let s = sys.tcsetattr(fd, SetAttrWhen::Now, &t).err() == Some(want);
            let w = sys.tcsetwinsize(fd, ws).err() == Some(want);
            out.push_str(&format!("{g} {s} {w}\n"));
        }
        ctx.stdout().write_all(out.as_bytes()).ok();
        0
    }

    #[test]
    fn termios_calls_give_enotty_or_ebadf() {
        let r = kit().programs([Program::bin("tprobe", termios_probe)]).run(&["tprobe"], b"");
        assert_eq!(r.stdout_str(), "true true true\ntrue true true\n");
    }

    #[test]
    fn default_termios_matches_tty_std_termios() {
        use crate::types::termios::*;
        let t = Termios::default();
        assert_eq!(t.c_iflag, 0o2400);
        assert_eq!(t.c_oflag, 0o5);
        assert_eq!(t.c_cflag, 0o2277);
        assert_eq!(t.c_lflag, 0o105073);
        assert_eq!(t.c_cc[..17], [3, 0x1c, 0x7f, 0x15, 4, 0, 1, 0, 0x11, 0x13, 0x1a, 0, 0x12, 0x0f, 0x17, 0x16, 0]);
        assert_eq!(baud_of(t.c_cflag), Some(38400));
        assert_eq!(baud_of(B115200), Some(115200));
        assert_eq!(baud_of(BOTHER), None);
    }

    fn print_line(s: String) {
        sys::write_all(Fd::STDOUT, s.as_bytes()).ok();
    }

    fn umachine(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let u = ctx.sys().uname();
        let line = format!("{} {}\n", String::from_utf8_lossy(&u.machine), String::from_utf8_lossy(&u.release));
        ctx.stdout().write_all(line.as_bytes()).ok();
        0
    }

    fn persona_probe(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let sys = ctx.sys().clone();
        let mut out = format!("{:?} {:?} {:?}\n", sys.personality(0xffff_ffff), sys.personality(0x40000), sys.personality(0x20008));
        let u = sys.uname();
        out.push_str(&format!("{} {}\n", String::from_utf8_lossy(&u.machine), String::from_utf8_lossy(&u.release)));
        // O exec herda a personality (é o que faz `setarch i686 prog` funcionar).
        let pid = sys
            .spawn(SpawnSpec { path: b"/usr/bin/umachine".to_vec(), argv: vec![b"umachine".to_vec()], attrs: ProcAttrs::default() })
            .unwrap();
        sys.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap();
        // O fork também.
        let body: ProcessFn = Box::new(|| {
            print_line(format!("{}\n", String::from_utf8_lossy(&sys::current().uname().machine)));
            0
        });
        let pid = sys.spawn_fn(ProcAttrs::default(), b"child".to_vec(), body).unwrap();
        sys.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap();
        // Voltar a 0 desfaz o `uname`.
        sys.personality(0).unwrap();
        let u = sys.uname();
        out.push_str(&format!("{} {}\n", String::from_utf8_lossy(&u.machine), String::from_utf8_lossy(&u.release)));
        ctx.stdout().write_all(out.as_bytes()).ok();
        0
    }

    #[test]
    fn personality_follows_docker_seccomp_and_uname() {
        let r = kit().programs([Program::bin("pprobe", persona_probe), Program::bin("umachine", umachine)]).run(&["pprobe"], b"");
        assert_eq!(
            r.stdout_str(),
            "i686 2.6.72+deb13-amd64\ni686\nOk(0) Err(EPERM) Ok(0)\ni686 2.6.72+deb13-amd64\nx86_64 6.12.101+deb13-amd64\n"
        );
    }

    // Threads que conversam por canal: o `sort` lê numa e ordena na outra. Com thread síncrona, a
    // primeira esperava pra sempre pela segunda, que só começaria depois dela.
    fn channel_probe(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let sys = ctx.sys().clone();
        let (to_worker, from_main) = std::sync::mpsc::sync_channel::<u32>(1);
        let (to_main, from_worker) = std::sync::mpsc::sync_channel::<u32>(1);
        let worker = sys
            .spawn_thread(Box::new(move || {
                while let Ok(n) = from_main.recv() {
                    let pid = sys::current().getpid();
                    to_main.send(n * 10 + u32::from(pid > 0)).unwrap();
                }
            }))
            .unwrap();
        let mut out = String::new();
        for n in 1..=3 {
            to_worker.send(n).unwrap();
            out.push_str(&format!("{}\n", from_worker.recv().unwrap()));
        }
        drop(to_worker);
        out.push_str(&format!("{:?} {:?}\n", sys.join_thread(worker), sys.join_thread(worker)));
        ctx.stdout().write_all(out.as_bytes()).ok();
        0
    }

    #[test]
    fn threads_run_concurrently_with_the_spawner() {
        let r = kit().programs([Program::bin("chan", channel_probe)]).run(&["chan"], b"");
        assert_eq!(r.stdout_str(), "11\n21\n31\nOk(()) Err(EINVAL)\n");
    }

    fn sched_probe(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let sys = ctx.sys().clone();
        let mut o = String::new();
        let mut line = |s: String| o.push_str(&format!("{s}\n"));
        line(format!("{:?}", sys.sched_getscheduler(0)));
        line(format!("{:?}", sys.sched_setscheduler(0, 1, SchedParam { priority: 10 })));
        line(format!("{:?}", sys.sched_setscheduler(0, 2, SchedParam { priority: 0 })));
        line(format!("{:?}", sys.sched_setscheduler(0, 3, SchedParam { priority: 7 })));
        line(format!("{:?}", sys.sched_setscheduler(0, 3, SchedParam { priority: 0 })));
        line(format!("{:?}", sys.sched_getscheduler(0)));
        line(format!("{:?} {:?}", sys.sched_getscheduler(99999), sys.sched_getscheduler(-1)));
        line(format!("{:?} {:?}", sys.sched_getparam(0), sys.sched_setparam(0, SchedParam { priority: 5 })));
        let a = sys.sched_getattr(0, 56, 0).unwrap();
        line(format!("{} {} {} {} {} {}", a.size, a.policy, a.nice, a.priority, a.runtime, a.util_max));
        line(format!("{:?} {:?}", sys.sched_getattr(0, 47, 0).err(), sys.sched_getattr(0, 56, 1).err()));
        line(format!(
            "{:?} {:?} {:?} {:?}",
            crate::sched::priority_min(1),
            crate::sched::priority_max(2),
            crate::sched::priority_max(5),
            crate::sched::priority_max(4)
        ));
        let set = SchedAttr { size: 56, policy: 0, nice: 5, ..SchedAttr::default() };
        line(format!("{:?} {:?}", sys.sched_setattr(0, &set, 0), sys.getpriority(0)));
        let low = SchedAttr { nice: -1, ..set };
        line(format!("{:?}", sys.sched_setattr(0, &low, 0)));
        line(format!("{:?}", sys.sched_setattr(0, &SchedAttr { size: 8, ..set }, 0)));
        line(format!("{:?}", sys.sched_setattr(424242, &set, 0)));
        line(format!("{:?}", sys.sched_rr_get_interval(0)));
        line(format!("{:?}", sys.sched_setscheduler(0, 5 | sched::SCHED_RESET_ON_FORK, SchedParam::default())));
        line(format!("{:?}", sys.sched_getscheduler(0)));
        let body: ProcessFn = Box::new(|| {
            print_line(format!("{:?}\n", sys::sched_getscheduler(0)));
            0
        });
        let pid = sys.spawn_fn(ProcAttrs::default(), b"child".to_vec(), body).unwrap();
        sys.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap();
        // IDLE não volta a OTHER sem RLIMIT_NICE, como no Linux.
        line(format!("{:?}", sys.sched_setscheduler(0, 0, SchedParam::default())));
        ctx.stdout().write_all(o.as_bytes()).ok();
        0
    }

    #[test]
    fn scheduler_policies_follow_the_default_container() {
        let r = kit().programs([Program::bin("sprobe", sched_probe)]).run(&["sprobe"], b"");
        // A primeira linha é a do filho, que escreve antes de o pai despejar o relatório.
        let want = [
            "Ok(5)",
            "Ok(0)",
            "Err(EPERM)",
            "Err(EINVAL)",
            "Err(EINVAL)",
            "Ok(())",
            "Ok(3)",
            "Err(ESRCH) Err(EINVAL)",
            "Ok(SchedParam { priority: 0 }) Err(EINVAL)",
            "56 3 0 0 700000 1024",
            "Some(EINVAL) Some(EINVAL)",
            "Ok(1) Ok(99) Ok(0) Err(EINVAL)",
            "Ok(()) Ok(5)",
            "Err(EPERM)",
            "Err(E2BIG)",
            "Err(ESRCH)",
            "Ok(0ns)",
            "Ok(())",
            "Ok(1073741829)",
            "Err(EPERM)",
        ];
        assert_eq!(r.stdout_str(), want.join("\n") + "\n");
    }

    fn affinity_probe(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let sys = ctx.sys().clone();
        let mut o = String::new();
        let mut line = |s: String| o.push_str(&format!("{s}\n"));
        line(format!("{:?}", sys.sched_getaffinity_of(0)));
        line(format!("{:?} {:?}", sys.sched_setaffinity(0, &[2, 9]), sys.sched_getaffinity_of(0)));
        line(format!("{:?} {:?}", sys.sched_setaffinity(0, &[]), sys.sched_setaffinity(0, &[7])));
        line(format!("{:?} {:?}", sys.sched_setaffinity(424242, &[0]), sys.sched_getaffinity_of(424242)));
        line(format!("{:?}", sys.sched_getaffinity()));
        let body: ProcessFn = Box::new(|| {
            print_line(format!("{:?}\n", sys::sched_getaffinity_of(0)));
            0
        });
        let pid = sys.spawn_fn(ProcAttrs::default(), b"child".to_vec(), body).unwrap();
        sys.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap();
        ctx.stdout().write_all(o.as_bytes()).ok();
        0
    }

    #[test]
    fn affinity_is_per_process_validated_and_inherited() {
        let r = kit().cpus(4).programs([Program::bin("aprobe", affinity_probe)]).run(&["aprobe"], b"");
        assert_eq!(
            r.stdout_str(),
            "Ok([2])\nOk([0, 1, 2, 3])\nOk(()) Ok([2])\nErr(EINVAL) Err(EINVAL)\nErr(ESRCH) Err(ESRCH)\n[2]\n"
        );
    }

    fn ioprio_probe(ctx: &mut Ctx, _args: &[OsString]) -> i32 {
        let sys = ctx.sys().clone();
        let mut o = String::new();
        let mut line = |s: String| o.push_str(&format!("{s}\n"));
        let be = |n: i32| sched::ioprio_value(sched::IOPRIO_CLASS_BE, n);
        line(format!("{:?}", sys.ioprio_get(sched::IOPRIO_WHO_PROCESS, 0)));
        sys.setpriority(0, 10).unwrap();
        line(format!("{:?}", sys.ioprio_get(sched::IOPRIO_WHO_PROCESS, 0)));
        line(format!("{:?}", sys.ioprio_set(1, 0, sched::ioprio_value(sched::IOPRIO_CLASS_RT, 0))));
        line(format!("{:?}", sys.ioprio_set(1, 0, be(8))));
        line(format!("{:?}", sys.ioprio_set(1, 0, sched::ioprio_value(sched::IOPRIO_CLASS_NONE, 1))));
        line(format!("{:?} {:?}", sys.ioprio_set(1, 0, be(1)), sys.ioprio_get(1, 0)));
        line(format!("{:?} {:?}", sys.ioprio_get(4, 0), sys.ioprio_get(1, 77777)));
        line(format!("{:?}", sys.ioprio_set(sched::IOPRIO_WHO_PGRP, 0, sched::ioprio_value(sched::IOPRIO_CLASS_IDLE, 0))));
        line(format!("{:?} {:?}", sys.ioprio_get(sched::IOPRIO_WHO_PGRP, 0), sys.ioprio_get(sched::IOPRIO_WHO_USER, 0)));
        let body: ProcessFn = Box::new(|| {
            print_line(format!("{:?}\n", sys::ioprio_get(1, 0)));
            0
        });
        let pid = sys.spawn_fn(ProcAttrs::default(), b"child".to_vec(), body).unwrap();
        sys.wait4(WaitTarget::Pid(pid), WaitOptions::empty()).unwrap();
        ctx.stdout().write_all(o.as_bytes()).ok();
        0
    }

    #[test]
    fn ioprio_defaults_from_nice_and_rejects_realtime() {
        let r = kit().programs([Program::bin("iprobe", ioprio_probe)]).run(&["iprobe"], b"");
        // BE nível 4 (nice 0) e 6 (nice 10); IDLE é classe 3 (3 << 13 = 24576).
        assert_eq!(
            r.stdout_str(),
            "Ok(24576)\nOk(16388)\nOk(16390)\nErr(EPERM)\nOk(())\nErr(EINVAL)\nOk(()) Ok(16385)\nErr(EINVAL) Err(ESRCH)\nOk(())\nOk(24576) Ok(24576)\n"
        );
    }
}
