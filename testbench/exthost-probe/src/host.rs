//! O `Host` do zjsc sobre o sistema real (`std::fs`, `std::env`), em Rust seguro: a fronteira que o pseudo-linus
//! preenche com o `sysabi` e que o je-code preenche com o SO de verdade. Os descritores são virtuais (uma tabela de
//! `std::fs::File`), porque tomar um descritor cru do SO exigiria `unsafe`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::unix::fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt};

use zjsc::runtime::host::{Errno, FsDirEntry, FsStat, Host, HostFile, HostStat};

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

fn errno(error: std::io::Error) -> Errno {
    error.raw_os_error().unwrap_or(5)
}

struct StdFile(std::fs::File);

impl HostFile for StdFile {
    fn write_all(&mut self, bytes: &[u8]) -> Result<(), Errno> {
        self.0.write_all(bytes).and_then(|()| self.0.flush()).map_err(errno)
    }
}

fn stat_of(metadata: &std::fs::Metadata) -> FsStat {
    let birth = metadata.created().ok().and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| (d.as_secs() as i64, i64::from(d.subsec_nanos())));
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

#[derive(Default)]
pub struct ProbeHost {
    files: RefCell<HashMap<i32, std::fs::File>>,
    next_fd: RefCell<i32>,
}

impl ProbeHost {
    fn with_file<T>(&self, fd: i32, f: impl FnOnce(&std::fs::File) -> std::io::Result<T>) -> Result<T, Errno> {
        let files = self.files.borrow();
        let file = files.get(&fd).ok_or(9)?;
        f(file).map_err(errno)
    }
}

impl std::fmt::Debug for ProbeHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProbeHost")
    }
}

impl Host for ProbeHost {
    fn read_file(&self, path: &str) -> Result<Vec<u8>, Errno> {
        std::fs::read(path).map_err(errno)
    }
    fn stat(&self, path: &str) -> Result<HostStat, Errno> {
        let metadata = std::fs::metadata(path).map_err(errno)?;
        Ok(HostStat { is_dir: metadata.is_dir(), uid: metadata.uid(), gid: metadata.gid() })
    }
    fn create_file(&self, path: &str, exclusive: bool) -> Result<Box<dyn HostFile>, Errno> {
        let mut options = std::fs::OpenOptions::new();
        options.write(true);
        if exclusive {
            options.create_new(true);
        } else {
            options.create(true).truncate(true);
        }
        Ok(Box::new(StdFile(options.open(path).map_err(errno)?)))
    }
    fn environ(&self) -> Vec<(String, String)> {
        std::env::vars_os().map(|(k, v)| (k.to_string_lossy().into_owned(), v.to_string_lossy().into_owned())).collect()
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
        std::fs::File::open("/dev/urandom").and_then(|mut source| source.read_exact(bytes)).expect("/dev/urandom");
    }
    fn kill(&self, _pid: i32, _signal: i32) -> Result<(), Errno> {
        Err(38)
    }
    fn set_priority(&self, _pid: i32, _nice: i32) -> Result<(), Errno> {
        Err(38)
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
            options.mode(mode);
        }
        let file = options.open(path).map_err(errno)?;
        let mut next = self.next_fd.borrow_mut();
        if *next < 3 {
            *next = 3;
        }
        let fd = *next;
        *next += 1;
        self.files.borrow_mut().insert(fd, file);
        Ok(fd)
    }
    fn fs_close(&self, fd: i32) -> Result<(), Errno> {
        self.files.borrow_mut().remove(&fd).map(|_| ()).ok_or(9)
    }
    fn fs_read(&self, fd: i32, len: usize, offset: Option<u64>) -> Result<Vec<u8>, Errno> {
        let mut buffer = vec![0u8; len];
        let read = self.with_file(fd, |mut file| match offset {
            Some(at) => file.read_at(&mut buffer, at),
            None => file.read(&mut buffer),
        })?;
        buffer.truncate(read);
        Ok(buffer)
    }
    fn fs_write(&self, fd: i32, bytes: &[u8], offset: Option<u64>) -> Result<usize, Errno> {
        match fd {
            1 => return std::io::stdout().write_all(bytes).map(|()| bytes.len()).map_err(errno),
            2 => return std::io::stderr().write_all(bytes).map(|()| bytes.len()).map_err(errno),
            _ => {}
        }
        self.with_file(fd, |mut file| match offset {
            Some(at) => file.write_at(bytes, at),
            None => file.write(bytes),
        })
    }
    fn fs_fstat(&self, fd: i32) -> Result<FsStat, Errno> {
        self.with_file(fd, |file| file.metadata()).map(|m| stat_of(&m))
    }
    fn fs_stat(&self, path: &str, follow: bool) -> Result<FsStat, Errno> {
        let metadata = if follow { std::fs::metadata(path) } else { std::fs::symlink_metadata(path) };
        metadata.map_err(errno).map(|m| stat_of(&m))
    }
    fn fs_readdir(&self, path: &str) -> Result<Vec<FsDirEntry>, Errno> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(path).map_err(errno)? {
            let entry = entry.map_err(errno)?;
            let kind = entry.file_type().map_err(errno)?;
            let mode_type = if kind.is_dir() {
                S_IFDIR
            } else if kind.is_symlink() {
                S_IFLNK
            } else if kind.is_file() {
                S_IFREG
            } else {
                0
            };
            entries.push(FsDirEntry { name: entry.file_name().into_encoded_bytes(), mode_type });
        }
        Ok(entries)
    }
    fn fs_mkdir(&self, path: &str, _mode: u32) -> Result<(), Errno> {
        std::fs::create_dir(path).map_err(errno)
    }
    fn fs_unlink(&self, path: &str) -> Result<(), Errno> {
        std::fs::remove_file(path).map_err(errno)
    }
    fn fs_rmdir(&self, path: &str) -> Result<(), Errno> {
        std::fs::remove_dir(path).map_err(errno)
    }
    fn fs_rename(&self, from: &str, to: &str) -> Result<(), Errno> {
        std::fs::rename(from, to).map_err(errno)
    }
    fn fs_link(&self, from: &str, to: &str) -> Result<(), Errno> {
        std::fs::hard_link(from, to).map_err(errno)
    }
    fn fs_symlink(&self, target: &str, path: &str) -> Result<(), Errno> {
        std::os::unix::fs::symlink(target, path).map_err(errno)
    }
    fn fs_readlink(&self, path: &str) -> Result<String, Errno> {
        std::fs::read_link(path).map_err(errno).map(|p| p.to_string_lossy().into_owned())
    }
    fn fs_chmod(&self, path: &str, mode: u32) -> Result<(), Errno> {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(errno)
    }
    fn fs_ftruncate(&self, fd: i32, len: u64) -> Result<(), Errno> {
        self.with_file(fd, |file| file.set_len(len))
    }
    fn fs_fsync(&self, fd: i32) -> Result<(), Errno> {
        self.with_file(fd, |file| file.sync_all())
    }
    fn fs_access(&self, path: &str, mode: u32) -> Result<(), Errno> {
        let metadata = std::fs::metadata(path).map_err(errno)?;
        let bits = metadata.mode();
        let want = (if mode & 4 != 0 { 0o444 } else { 0 }) | (if mode & 2 != 0 { 0o222 } else { 0 }) | (if mode & 1 != 0 { 0o111 } else { 0 });
        if want == 0 || bits & want != 0 { Ok(()) } else { Err(13) }
    }
    fn fs_realpath(&self, path: &str) -> Result<String, Errno> {
        std::fs::canonicalize(path).map_err(errno).map(|p| p.to_string_lossy().into_owned())
    }
}
