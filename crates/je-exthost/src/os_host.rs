//! O `Host` do zjsc sobre o sistema operacional real, em Rust seguro (`unsafe_code = "forbid"`).
//!
//! Os descritores que o motor enxerga são **virtuais**: uma tabela de objetos da `std` (`File`, `TcpListener`,
//! `TcpStream`, pontas de processo filho). Tomar um descritor cru do SO exigiria `unsafe`. As consequências, todas
//! medidas pelo `tests/os_host.rs`:
//!
//! - Rede de loopback sem bloqueio pela `std` (`set_nonblocking`). Como a `std` não tem `poll(2)`, o `net_poll`
//!   confere a prontidão tentando a operação (lê para dentro de um buffer do descritor; aceita para dentro de uma
//!   vaga do ouvinte) e dorme 1 ms entre as rodadas até o prazo.
//! - Pipe e `socketpair` do processo filho: a `std` não deixa um `ChildStdout` sem bloqueio, então cada ponta de
//!   leitura ganha uma thread que lê bloqueando e entrega por canal. A ponta `SocketPair` é um `UnixStream` (só
//!   Unix) e o filho a recebe como `Stdio` por `OwnedFd`.
//! - `setsid` do filho (`detached`) vira `process_group(0)` (só Unix): um grupo novo, sem sessão nova, porque o
//!   `setsid` exige `pre_exec`, que é `unsafe`.
//! - `utimes` com `follow` falso (`lutimes`) e `statfs` não têm API segura na `std`: o primeiro devolve `ENOSYS`
//!   para ligação simbólica e o segundo `ENOSYS`.
//! - Os números de `errno` são os do Linux: no Windows e no macOS o `ErrorKind` do erro da `std` é traduzido.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{ErrorKind, IsTerminal, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, SocketAddrV4, TcpListener, TcpStream};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use zjsc::runtime::host::{
    Errno, FsDirEntry, FsStat, FsStatFs, FsTime, Host, HostFile, HostStat, NetReady, NetWatch, ProcExit, SpawnRequest,
    SpawnStdio, Spawned, ENOSYS,
};

const ENOENT: Errno = 2;
const ESRCH: Errno = 3;
const EIO: Errno = 5;
const EBADF: Errno = 9;
const ECHILD: Errno = 10;
const EAGAIN: Errno = 11;
const EACCES: Errno = 13;
const EEXIST: Errno = 17;
const EINVAL: Errno = 22;
const EPIPE: Errno = 32;
const ECONNREFUSED: Errno = 111;

const O_ACCMODE: u32 = 3;
const O_WRONLY: u32 = 1;
const O_RDWR: u32 = 2;
const O_CREAT: u32 = 0o100;
const O_EXCL: u32 = 0o200;
const O_TRUNC: u32 = 0o1000;
const O_APPEND: u32 = 0o2000;

const S_IFDIR: u32 = 0o040000;
const S_IFREG: u32 = 0o100000;
const S_IFLNK: u32 = 0o120000;

const READ_CHUNK: usize = 64 * 1024;
const POLL_STEP: Duration = Duration::from_millis(1);

/// O `Errno` do Linux para um erro da `std`.
fn errno(error: &std::io::Error) -> Errno {
    #[cfg(unix)]
    if let Some(code) = error.raw_os_error() {
        // No Linux o número já é o do `errno`; no macOS os números 11, 35, 60, 61... diferem, então só o confia no Linux.
        if cfg!(target_os = "linux") {
            return code;
        }
    }
    match error.kind() {
        ErrorKind::NotFound => ENOENT,
        ErrorKind::PermissionDenied => EACCES,
        ErrorKind::AlreadyExists => EEXIST,
        ErrorKind::WouldBlock => EAGAIN,
        ErrorKind::InvalidInput | ErrorKind::InvalidData => EINVAL,
        ErrorKind::BrokenPipe => EPIPE,
        ErrorKind::ConnectionRefused => ECONNREFUSED,
        ErrorKind::NotADirectory => 20,
        ErrorKind::IsADirectory => 21,
        ErrorKind::DirectoryNotEmpty => 39,
        ErrorKind::Unsupported => ENOSYS,
        _ => EIO,
    }
}

fn err(error: std::io::Error) -> Errno {
    errno(&error)
}

struct StdFile(std::fs::File);

impl HostFile for StdFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        self.0.write_all(bytes).and_then(|()| self.0.flush()).map_err(err)
    }
}

/// Lê bloqueando numa thread e entrega os pedaços por canal; um pedaço vazio é o fim do fluxo.
fn pump(mut source: impl Read + Send + 'static) -> Receiver<Vec<u8>> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; READ_CHUNK];
        loop {
            match source.read(&mut buffer) {
                Ok(0) | Err(_) => {
                    let _ = sender.send(Vec::new());
                    break;
                }
                Ok(count) => {
                    if sender.send(buffer[..count].to_vec()).is_err() {
                        break;
                    }
                }
            }
        }
    });
    receiver
}

/// De onde um fluxo de entrada recebe os bytes.
enum Source {
    Tcp(TcpStream),
    Channel(Receiver<Vec<u8>>),
}

/// A metade de leitura de um descritor de fluxo: o que já chegou e se acabou.
struct Incoming {
    source: Source,
    stash: Vec<u8>,
    eof: bool,
}

impl Incoming {
    fn new(source: Source) -> Self {
        Incoming { source, stash: Vec::new(), eof: false }
    }

    /// Traz para o buffer tudo o que o SO já tem, sem bloquear.
    fn fill(&mut self) {
        if self.eof {
            return;
        }
        match &mut self.source {
            Source::Tcp(stream) => {
                let mut buffer = vec![0u8; READ_CHUNK];
                loop {
                    match stream.read(&mut buffer) {
                        Ok(0) => {
                            self.eof = true;
                            break;
                        }
                        Ok(count) => self.stash.extend_from_slice(&buffer[..count]),
                        Err(error) if error.kind() == ErrorKind::WouldBlock => break,
                        Err(error) if error.kind() == ErrorKind::Interrupted => {}
                        Err(_) => {
                            self.eof = true;
                            break;
                        }
                    }
                }
            }
            Source::Channel(receiver) => loop {
                match receiver.try_recv() {
                    Ok(chunk) if chunk.is_empty() => {
                        self.eof = true;
                        break;
                    }
                    Ok(chunk) => self.stash.extend_from_slice(&chunk),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.eof = true;
                        break;
                    }
                }
            },
        }
    }

    fn readable(&mut self) -> bool {
        self.fill();
        !self.stash.is_empty() || self.eof
    }

    /// Até `max` bytes; vazio é o fim e `EAGAIN` é nada por enquanto.
    fn recv(&mut self, max: usize) -> Result<Vec<u8>, Errno> {
        self.fill();
        if !self.stash.is_empty() {
            let take = max.min(self.stash.len());
            return Ok(self.stash.drain(..take).collect());
        }
        if self.eof { Ok(Vec::new()) } else { Err(EAGAIN) }
    }
}

/// Por onde um fluxo envia.
enum Outgoing {
    None,
    Tcp(TcpStream),
    Pipe(Box<dyn Write>),
}

enum Desc {
    File(std::fs::File),
    Listener { listener: TcpListener, pending: Option<(TcpStream, u16)> },
    Stream { incoming: Option<Incoming>, outgoing: Outgoing },
}

impl Desc {
    fn tcp(stream: TcpStream) -> Result<Desc, Errno> {
        stream.set_nonblocking(true).map_err(err)?;
        let writer = stream.try_clone().map_err(err)?;
        Ok(Desc::Stream { incoming: Some(Incoming::new(Source::Tcp(stream))), outgoing: Outgoing::Tcp(writer) })
    }
}

/// O sistema operacional real, visto pelo motor.
#[derive(Default)]
pub struct OsHost {
    table: RefCell<HashMap<i32, Desc>>,
    next_fd: RefCell<i32>,
    children: RefCell<HashMap<i32, Child>>,
}

impl std::fmt::Debug for OsHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("OsHost")
    }
}

impl OsHost {
    pub fn new() -> Self {
        Self::default()
    }

    fn insert(&self, desc: Desc) -> i32 {
        let mut next = self.next_fd.borrow_mut();
        if *next < 3 {
            *next = 3;
        }
        let fd = *next;
        *next += 1;
        self.table.borrow_mut().insert(fd, desc);
        fd
    }

    fn with_file<T>(&self, fd: i32, f: impl FnOnce(&std::fs::File) -> std::io::Result<T>) -> Result<T, Errno> {
        match self.table.borrow().get(&fd) {
            Some(Desc::File(file)) => f(file).map_err(err),
            Some(_) => Err(EINVAL),
            None => Err(EBADF),
        }
    }

    fn with_incoming<T>(&self, fd: i32, f: impl FnOnce(&mut Incoming) -> T) -> Result<T, Errno> {
        match self.table.borrow_mut().get_mut(&fd) {
            Some(Desc::Stream { incoming: Some(incoming), .. }) => Ok(f(incoming)),
            Some(_) => Err(EINVAL),
            None => Err(EBADF),
        }
    }

    fn send(&self, fd: i32, bytes: &[u8]) -> Result<usize, Errno> {
        match self.table.borrow_mut().get_mut(&fd) {
            Some(Desc::Stream { outgoing: Outgoing::Tcp(stream), .. }) => match stream.write(bytes) {
                Ok(count) => Ok(count),
                Err(error) => Err(errno(&error)),
            },
            Some(Desc::Stream { outgoing: Outgoing::Pipe(writer), .. }) => {
                writer.write_all(bytes).and_then(|()| writer.flush()).map(|()| bytes.len()).map_err(err)
            }
            Some(Desc::Stream { .. }) => Err(EBADF),
            Some(_) => Err(EINVAL),
            None => Err(EBADF),
        }
    }
}

#[cfg(unix)]
fn stat_of(metadata: &std::fs::Metadata) -> FsStat {
    use std::os::unix::fs::MetadataExt;
    let birth = metadata.created().ok().and_then(|time| time.duration_since(UNIX_EPOCH).ok()).map(|d| (d.as_secs() as i64, i64::from(d.subsec_nanos())));
    FsStat {
        dev: metadata.dev(),
        ino: metadata.ino(),
        mode: metadata.mode(),
        nlink: metadata.nlink(),
        uid: metadata.uid(),
        gid: metadata.gid(),
        rdev: metadata.rdev(),
        size: metadata.size(),
        blksize: metadata.blksize(),
        blocks: metadata.blocks(),
        atime: (metadata.atime(), metadata.atime_nsec()),
        mtime: (metadata.mtime(), metadata.mtime_nsec()),
        ctime: (metadata.ctime(), metadata.ctime_nsec()),
        birthtime: birth.unwrap_or((metadata.ctime(), metadata.ctime_nsec())),
    }
}

#[cfg(not(unix))]
fn stat_of(metadata: &std::fs::Metadata) -> FsStat {
    fn stamp(time: std::io::Result<SystemTime>) -> (i64, i64) {
        time.ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or((0, 0), |d| (d.as_secs() as i64, i64::from(d.subsec_nanos())))
    }
    let kind = if metadata.is_dir() {
        S_IFDIR | 0o755
    } else if metadata.file_type().is_symlink() {
        S_IFLNK | 0o777
    } else {
        S_IFREG | if metadata.permissions().readonly() { 0o444 } else { 0o644 }
    };
    FsStat {
        mode: kind,
        nlink: 1,
        size: metadata.len(),
        blksize: 4096,
        blocks: metadata.len().div_ceil(512),
        atime: stamp(metadata.accessed()),
        mtime: stamp(metadata.modified()),
        ctime: stamp(metadata.modified()),
        birthtime: stamp(metadata.created()),
        ..FsStat::default()
    }
}

fn mode_type(kind: std::fs::FileType) -> u32 {
    if kind.is_dir() {
        S_IFDIR
    } else if kind.is_symlink() {
        S_IFLNK
    } else if kind.is_file() {
        S_IFREG
    } else {
        0
    }
}

fn time_of(time: FsTime) -> Option<Option<SystemTime>> {
    match time {
        FsTime::Omit => None,
        FsTime::Now => Some(Some(SystemTime::now())),
        FsTime::At(seconds, nanos) => {
            let nanos = u32::try_from(nanos).unwrap_or(0);
            let at = if seconds >= 0 {
                UNIX_EPOCH.checked_add(Duration::new(seconds as u64, nanos))
            } else {
                UNIX_EPOCH.checked_sub(Duration::from_secs(seconds.unsigned_abs())).and_then(|t| t.checked_add(Duration::from_nanos(u64::from(nanos))))
            };
            Some(at)
        }
    }
}

fn set_times(file: &std::fs::File, atime: FsTime, mtime: FsTime) -> Result<(), Errno> {
    let mut times = std::fs::FileTimes::new();
    if let Some(at) = time_of(atime) {
        times = times.set_accessed(at.ok_or(EINVAL)?);
    }
    if let Some(at) = time_of(mtime) {
        times = times.set_modified(at.ok_or(EINVAL)?);
    }
    file.set_times(times).map_err(err)
}

#[cfg(unix)]
fn set_mode(file: &std::fs::File, mode: u32) -> Result<(), Errno> {
    use std::os::unix::fs::PermissionsExt;
    file.set_permissions(std::fs::Permissions::from_mode(mode)).map_err(err)
}

#[cfg(not(unix))]
fn set_mode(file: &std::fs::File, mode: u32) -> Result<(), Errno> {
    let mut permissions = file.metadata().map_err(err)?.permissions();
    permissions.set_readonly(mode & 0o222 == 0);
    file.set_permissions(permissions).map_err(err)
}

fn exit_of(status: std::process::ExitStatus) -> ProcExit {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(signal) = status.signal() {
            return ProcExit::Signaled(signal);
        }
    }
    ProcExit::Exited(status.code().unwrap_or(0))
}

impl Host for OsHost {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, Errno> {
        std::fs::read(path).map_err(err)
    }

    fn stat(&self, path: &str) -> Result<HostStat, Errno> {
        let metadata = std::fs::metadata(path).map_err(err)?;
        let FsStat { uid, gid, .. } = stat_of(&metadata);
        Ok(HostStat { is_dir: metadata.is_dir(), uid, gid })
    }

    fn create_file(&self, path: &str, exclusive: bool) -> Result<Box<dyn HostFile>, Errno> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true);
        if exclusive {
            options.create_new(true);
        } else {
            options.create(true).truncate(true);
        }
        Ok(Box::new(StdFile(options.open(path).map_err(err)?)))
    }

    fn environ(&self) -> Vec<(String, String)> {
        std::env::vars_os().map(|(key, value)| (key.to_string_lossy().into_owned(), value.to_string_lossy().into_owned())).collect()
    }

    fn cwd(&self) -> String {
        std::env::current_dir().map_or_else(|_| String::from("/"), |path| path.to_string_lossy().into_owned())
    }

    fn pid(&self) -> u32 {
        std::process::id()
    }

    fn cpus(&self) -> usize {
        std::thread::available_parallelism().map_or(1, |count| count.get())
    }

    fn random_fill(&self, bytes: &mut [u8]) {
        getrandom::fill(bytes).expect("o SO entrega entropia");
    }

    fn kill(&self, pid: i32, signal: i32) -> Result<(), Errno> {
        if signal == 9 {
            if let Some(child) = self.children.borrow_mut().get_mut(&pid) {
                return child.kill().map_err(err);
            }
        }
        let output = if cfg!(unix) {
            Command::new("kill").arg("-s").arg(signal.to_string()).arg("--").arg(pid.to_string()).output()
        } else {
            Command::new("taskkill").arg("/PID").arg(pid.to_string()).arg("/F").output()
        }
        .map_err(err)?;
        if output.status.success() { Ok(()) } else { Err(ESRCH) }
    }

    fn set_priority(&self, pid: i32, nice: i32) -> Result<(), Errno> {
        if !cfg!(unix) {
            return Err(ENOSYS);
        }
        let output = Command::new("renice").arg("-n").arg(nice.to_string()).arg("-p").arg(pid.to_string()).output().map_err(err)?;
        if output.status.success() { Ok(()) } else { Err(EACCES) }
    }

    fn write_stderr(&self, bytes: &[u8]) {
        let _ = std::io::stderr().write_all(bytes);
    }

    fn exit(&self, code: i32) -> ! {
        std::process::exit(code)
    }

    fn fs_open(&self, path: &str, flags: u32, mode: u32) -> Result<i32, Errno> {
        let mut options = std::fs::OpenOptions::new();
        match flags & O_ACCMODE {
            O_WRONLY => options.write(true),
            O_RDWR => options.read(true).write(true),
            _ => options.read(true),
        };
        if flags & O_APPEND != 0 {
            options.append(true);
        }
        if flags & O_TRUNC != 0 {
            options.truncate(true);
        }
        if flags & O_CREAT != 0 {
            if flags & O_EXCL != 0 {
                options.create_new(true);
            } else {
                options.create(true);
            }
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(mode);
            }
            #[cfg(not(unix))]
            let _ = mode;
        }
        Ok(self.insert(Desc::File(options.open(path).map_err(err)?)))
    }

    fn tty_isatty(&self, fd: i32) -> bool {
        match fd {
            0 => std::io::stdin().is_terminal(),
            1 => std::io::stdout().is_terminal(),
            2 => std::io::stderr().is_terminal(),
            _ => false,
        }
    }

    fn fs_close(&self, fd: i32) -> Result<(), Errno> {
        self.table.borrow_mut().remove(&fd).map(|_| ()).ok_or(EBADF)
    }

    fn fs_read(&self, fd: i32, len: usize, offset: Option<u64>) -> Result<Vec<u8>, Errno> {
        if fd == 0 {
            let mut buffer = vec![0u8; len];
            let read = std::io::stdin().read(&mut buffer).map_err(err)?;
            buffer.truncate(read);
            return Ok(buffer);
        }
        if matches!(self.table.borrow().get(&fd), Some(Desc::Stream { .. })) {
            return self.with_incoming(fd, |incoming| incoming.recv(len))?;
        }
        let mut buffer = vec![0u8; len];
        let read = self.with_file(fd, |mut file| match offset {
            Some(at) => read_at(file, &mut buffer, at),
            None => file.read(&mut buffer),
        })?;
        buffer.truncate(read);
        Ok(buffer)
    }

    fn fs_write(&self, fd: i32, bytes: &[u8], offset: Option<u64>) -> Result<usize, Errno> {
        match fd {
            1 => return std::io::stdout().write_all(bytes).map(|()| bytes.len()).map_err(err),
            2 => return std::io::stderr().write_all(bytes).map(|()| bytes.len()).map_err(err),
            _ => {}
        }
        if matches!(self.table.borrow().get(&fd), Some(Desc::Stream { .. })) {
            return self.send(fd, bytes);
        }
        self.with_file(fd, |mut file| match offset {
            Some(at) => write_at(file, bytes, at),
            None => file.write(bytes),
        })
    }

    fn fs_fstat(&self, fd: i32) -> Result<FsStat, Errno> {
        self.with_file(fd, |file| file.metadata()).map(|metadata| stat_of(&metadata))
    }

    fn fs_stat(&self, path: &str, follow: bool) -> Result<FsStat, Errno> {
        let metadata = if follow { std::fs::metadata(path) } else { std::fs::symlink_metadata(path) };
        metadata.map_err(err).map(|metadata| stat_of(&metadata))
    }

    fn fs_readdir(&self, path: &str) -> Result<Vec<FsDirEntry>, Errno> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path).map_err(err)? {
            let entry = entry.map_err(err)?;
            let kind = entry.file_type().map_err(err)?;
            entries.push(FsDirEntry { name: entry.file_name().into_encoded_bytes(), mode_type: mode_type(kind) });
        }
        Ok(entries)
    }

    fn fs_mkdir(&self, path: &str, mode: u32) -> Result<(), Errno> {
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut builder = std::fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(mode);
        }
        #[cfg(not(unix))]
        let _ = mode;
        builder.create(path).map_err(err)
    }

    fn fs_unlink(&self, path: &str) -> Result<(), Errno> {
        std::fs::remove_file(path).map_err(err)
    }

    fn fs_rmdir(&self, path: &str) -> Result<(), Errno> {
        std::fs::remove_dir(path).map_err(err)
    }

    fn fs_rename(&self, from: &str, to: &str) -> Result<(), Errno> {
        std::fs::rename(from, to).map_err(err)
    }

    fn fs_link(&self, from: &str, to: &str) -> Result<(), Errno> {
        std::fs::hard_link(from, to).map_err(err)
    }

    fn fs_symlink(&self, target: &str, path: &str) -> Result<(), Errno> {
        #[cfg(unix)]
        return std::os::unix::fs::symlink(target, path).map_err(err);
        #[cfg(windows)]
        return if std::fs::metadata(target).is_ok_and(|metadata| metadata.is_dir()) {
            std::os::windows::fs::symlink_dir(target, path)
        } else {
            std::os::windows::fs::symlink_file(target, path)
        }
        .map_err(err);
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, path);
            Err(ENOSYS)
        }
    }

    fn fs_readlink(&self, path: &str) -> Result<String, Errno> {
        std::fs::read_link(path).map_err(err).map(|target| target.to_string_lossy().into_owned())
    }

    fn fs_chmod(&self, path: &str, mode: u32) -> Result<(), Errno> {
        set_mode(&std::fs::File::open(path).map_err(err)?, mode)
    }

    fn fs_fchmod(&self, fd: i32, mode: u32) -> Result<(), Errno> {
        self.with_file(fd, |file| set_mode(file, mode).map_err(std::io::Error::from_raw_os_error))
    }

    fn fs_utimes(&self, path: &str, atime: FsTime, mtime: FsTime, follow: bool) -> Result<(), Errno> {
        if !follow && std::fs::symlink_metadata(path).map_err(err)?.file_type().is_symlink() {
            return Err(ENOSYS);
        }
        set_times(&std::fs::File::open(path).map_err(err)?, atime, mtime)
    }

    fn fs_futimes(&self, fd: i32, atime: FsTime, mtime: FsTime) -> Result<(), Errno> {
        self.with_file(fd, |file| set_times(file, atime, mtime).map_err(std::io::Error::from_raw_os_error))
    }

    fn fs_ftruncate(&self, fd: i32, len: u64) -> Result<(), Errno> {
        self.with_file(fd, |file| file.set_len(len))
    }

    fn fs_fsync(&self, fd: i32) -> Result<(), Errno> {
        self.with_file(fd, |file| file.sync_all())
    }

    fn fs_access(&self, path: &str, mode: u32) -> Result<(), Errno> {
        let metadata = std::fs::metadata(path).map_err(err)?;
        if mode & 4 != 0 {
            if metadata.is_dir() {
                std::fs::read_dir(path).map_err(err)?;
            } else {
                std::fs::File::open(path).map_err(err)?;
            }
        }
        if mode & 2 != 0 {
            if metadata.is_dir() {
                if metadata.permissions().readonly() {
                    return Err(EACCES);
                }
            } else {
                std::fs::OpenOptions::new().write(true).open(path).map_err(err)?;
            }
        }
        if mode & 1 != 0 && cfg!(unix) && stat_of(&metadata).mode & 0o111 == 0 {
            return Err(EACCES);
        }
        Ok(())
    }

    #[cfg(unix)]
    fn fs_chown(&self, path: &str, uid: u32, gid: u32, follow: bool) -> Result<(), Errno> {
        let (uid, gid) = ((uid != u32::MAX).then_some(uid), (gid != u32::MAX).then_some(gid));
        if follow { std::os::unix::fs::chown(path, uid, gid) } else { std::os::unix::fs::lchown(path, uid, gid) }.map_err(err)
    }

    #[cfg(unix)]
    fn fs_fchown(&self, fd: i32, uid: u32, gid: u32) -> Result<(), Errno> {
        let (uid, gid) = ((uid != u32::MAX).then_some(uid), (gid != u32::MAX).then_some(gid));
        self.with_file(fd, |file| std::os::unix::fs::fchown(file, uid, gid))
    }

    fn fs_statfs(&self, _path: &str) -> Result<FsStatFs, Errno> {
        Err(ENOSYS)
    }

    fn fs_realpath(&self, path: &str) -> Result<String, Errno> {
        std::fs::canonicalize(path).map_err(err).map(|resolved| resolved.to_string_lossy().into_owned())
    }

    fn net_listen(&self, port: u16, _backlog: u32) -> Result<(i32, u16), Errno> {
        let listener = TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).map_err(err)?;
        listener.set_nonblocking(true).map_err(err)?;
        let bound = listener.local_addr().map_err(err)?.port();
        Ok((self.insert(Desc::Listener { listener, pending: None }), bound))
    }

    fn net_accept(&self, fd: i32) -> Result<(i32, u16), Errno> {
        let accepted = match self.table.borrow_mut().get_mut(&fd) {
            Some(Desc::Listener { listener, pending }) => match pending.take() {
                Some(ready) => ready,
                None => {
                    let (stream, peer) = listener.accept().map_err(err)?;
                    (stream, peer.port())
                }
            },
            Some(_) => return Err(EINVAL),
            None => return Err(EBADF),
        };
        let (stream, peer_port) = accepted;
        Ok((self.insert(Desc::tcp(stream)?), peer_port))
    }

    fn net_connect(&self, port: u16) -> Result<(i32, u16), Errno> {
        let stream = TcpStream::connect(SocketAddr::from((Ipv4Addr::LOCALHOST, port))).map_err(err)?;
        let local = stream.local_addr().map_err(err)?.port();
        Ok((self.insert(Desc::tcp(stream)?), local))
    }

    fn net_recv(&self, fd: i32, max: usize) -> Result<Vec<u8>, Errno> {
        self.with_incoming(fd, |incoming| incoming.recv(max))?
    }

    fn net_send(&self, fd: i32, bytes: &[u8]) -> Result<usize, Errno> {
        self.send(fd, bytes)
    }

    fn net_shutdown(&self, fd: i32, read: bool, write: bool) -> Result<(), Errno> {
        let how = match (read, write) {
            (true, true) => Shutdown::Both,
            (true, false) => Shutdown::Read,
            (false, true) => Shutdown::Write,
            (false, false) => return Ok(()),
        };
        match self.table.borrow_mut().get_mut(&fd) {
            Some(Desc::Stream { outgoing: Outgoing::Tcp(stream), .. }) => stream.shutdown(how).map_err(err),
            Some(Desc::Stream { outgoing: Outgoing::Pipe(_), .. }) if write => {
                // Fechar a escrita de um pipe é soltar a ponta: o filho vê o fim do arquivo.
                if let Some(Desc::Stream { outgoing, .. }) = self.table.borrow_mut().get_mut(&fd) {
                    *outgoing = Outgoing::None;
                }
                Ok(())
            }
            Some(_) => Ok(()),
            None => Err(EBADF),
        }
    }

    fn net_poll(&self, watch: &[NetWatch], timeout: Option<Duration>) -> Result<Vec<NetReady>, Errno> {
        let deadline = timeout.map(|wait| Instant::now() + wait);
        loop {
            let mut ready = Vec::with_capacity(watch.len());
            for item in watch {
                let mut state = NetReady::default();
                match self.table.borrow_mut().get_mut(&item.fd) {
                    None => state.error = true,
                    Some(Desc::Listener { listener, pending }) => {
                        if item.read {
                            if pending.is_none() {
                                if let Ok((stream, peer)) = listener.accept() {
                                    *pending = Some((stream, peer.port()));
                                }
                            }
                            state.readable = pending.is_some();
                        }
                    }
                    Some(Desc::Stream { incoming, outgoing }) => {
                        if item.read {
                            if let Some(incoming) = incoming {
                                state.readable = incoming.readable();
                                state.hangup = incoming.eof;
                            }
                        }
                        state.writable = item.write && !matches!(outgoing, Outgoing::None);
                    }
                    Some(Desc::File(_)) => {
                        state.readable = item.read;
                        state.writable = item.write;
                    }
                }
                ready.push(state);
            }
            let any = ready.iter().any(NetReady::any);
            let expired = deadline.is_some_and(|limit| Instant::now() >= limit);
            if any || expired || watch.is_empty() && deadline.is_none() {
                return Ok(ready);
            }
            std::thread::sleep(POLL_STEP);
        }
    }

    fn proc_spawn(&self, request: &SpawnRequest) -> Result<Spawned, Errno> {
        let mut command = Command::new(&request.file);
        command.args(request.argv.iter().skip(1));
        if let Some(env) = &request.env {
            command.env_clear().envs(env.iter().map(|(key, value)| (key, value)));
        }
        if let Some(cwd) = &request.cwd {
            command.current_dir(cwd);
        }
        #[cfg(unix)]
        if request.detached {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        // Só os três primeiros descritores existem na `std`; mais que isso só como `Ignore`.
        if request.stdio.iter().skip(3).any(|stdio| !matches!(stdio, SpawnStdio::Ignore)) {
            return Err(EINVAL);
        }
        #[cfg(unix)]
        let mut parent_sockets: Vec<Option<std::os::unix::net::UnixStream>> = Vec::new();
        let mut slots: Vec<Option<Stdio>> = Vec::new();
        for (index, stdio) in request.stdio.iter().take(3).enumerate() {
            #[cfg(unix)]
            parent_sockets.push(None);
            slots.push(Some(match *stdio {
                SpawnStdio::Ignore => Stdio::null(),
                SpawnStdio::Inherit => Stdio::inherit(),
                SpawnStdio::Pipe => Stdio::piped(),
                SpawnStdio::SocketPair => {
                    #[cfg(unix)]
                    {
                        let (parent, child) = std::os::unix::net::UnixStream::pair().map_err(err)?;
                        parent_sockets[index] = Some(parent);
                        Stdio::from(std::os::fd::OwnedFd::from(child))
                    }
                    #[cfg(not(unix))]
                    {
                        let _ = index;
                        return Err(ENOSYS);
                    }
                }
                SpawnStdio::Fd(source) => match source {
                    0..=2 if source as usize == index => Stdio::inherit(),
                    _ => match self.table.borrow().get(&source) {
                        Some(Desc::File(file)) => Stdio::from(file.try_clone().map_err(err)?),
                        Some(_) => return Err(EINVAL),
                        None => return Err(EBADF),
                    },
                },
            }));
        }
        let mut slots = slots.into_iter();
        if let Some(Some(stdin)) = slots.next() {
            command.stdin(stdin);
        }
        if let Some(Some(stdout)) = slots.next() {
            command.stdout(stdout);
        }
        if let Some(Some(stderr)) = slots.next() {
            command.stderr(stderr);
        }
        let mut child = command.spawn().map_err(err)?;
        let pid = child.id() as i32;
        let mut parent_fds: Vec<Option<i32>> = vec![None; request.stdio.len()];
        if let Some(stdin) = child.stdin.take() {
            parent_fds[0] = Some(self.insert(Desc::Stream { incoming: None, outgoing: Outgoing::Pipe(Box::new(stdin)) }));
        }
        if let Some(stdout) = child.stdout.take() {
            parent_fds[1] = Some(self.insert(Desc::Stream { incoming: Some(Incoming::new(Source::Channel(pump(stdout)))), outgoing: Outgoing::None }));
        }
        if let Some(stderr) = child.stderr.take() {
            parent_fds[2] = Some(self.insert(Desc::Stream { incoming: Some(Incoming::new(Source::Channel(pump(stderr)))), outgoing: Outgoing::None }));
        }
        #[cfg(unix)]
        for (index, parent) in parent_sockets.into_iter().enumerate() {
            if let Some(parent) = parent {
                let writer = parent.try_clone().map_err(err)?;
                parent_fds[index] = Some(self.insert(Desc::Stream {
                    incoming: Some(Incoming::new(Source::Channel(pump(parent)))),
                    outgoing: Outgoing::Pipe(Box::new(writer)),
                }));
            }
        }
        self.children.borrow_mut().insert(pid, child);
        Ok(Spawned { pid, parent_fds })
    }

    fn proc_wait(&self, pid: i32, nohang: bool) -> Result<Option<ProcExit>, Errno> {
        let mut children = self.children.borrow_mut();
        let child = children.get_mut(&pid).ok_or(ECHILD)?;
        let status = if nohang {
            match child.try_wait().map_err(err)? {
                Some(status) => status,
                None => return Ok(None),
            }
        } else {
            child.wait().map_err(err)?
        };
        children.remove(&pid);
        Ok(Some(exit_of(status)))
    }
}

#[cfg(unix)]
fn read_at(file: &std::fs::File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buffer, offset)
}

#[cfg(windows)]
fn read_at(file: &std::fs::File, buffer: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buffer, offset)
}

#[cfg(unix)]
fn write_at(file: &std::fs::File, bytes: &[u8], offset: u64) -> std::io::Result<usize> {
    std::os::unix::fs::FileExt::write_at(file, bytes, offset)
}

#[cfg(windows)]
fn write_at(file: &std::fs::File, bytes: &[u8], offset: u64) -> std::io::Result<usize> {
    std::os::windows::fs::FileExt::seek_write(file, bytes, offset)
}
