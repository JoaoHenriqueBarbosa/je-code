//! Tipos das syscalls, com os mesmos números e bits do Linux x86_64.

use std::time::Duration;

use bitflags::bitflags;

use crate::linux::Signal;

/// Descritor de arquivo de um pseudo-processo.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fd(pub i32);

impl Fd {
    pub const STDIN: Fd = Fd(0);
    pub const STDOUT: Fd = Fd(1);
    pub const STDERR: Fd = Fd(2);
    /// `AT_FDCWD`: caminhos relativos resolvem a partir do cwd.
    pub const CWD: Fd = Fd(-100);
}

pub type Pid = i32;
/// Id de thread (`gettid`); a thread principal tem tid = pid.
pub type Tid = i32;
/// Corpo de uma thread criada por [`crate::Syscalls::spawn_thread`].
pub type ThreadFn = Box<dyn FnOnce() + Send + 'static>;
pub type Uid = u32;
/// O `-1` de `setresuid`/`setreuid`/`chown`: não muda aquele id.
pub const ID_UNCHANGED: u32 = u32::MAX;
pub type Gid = u32;
/// Bits de permissão e tipo (`st_mode`).
pub type Mode = u32;

bitflags! {
    /// Flags do `open(2)`, valores do Linux x86_64.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct OFlags: u32 {
        const WRONLY = 0o1;
        const RDWR = 0o2;
        const CREAT = 0o100;
        const EXCL = 0o200;
        const NOCTTY = 0o400;
        const TRUNC = 0o1000;
        const APPEND = 0o2000;
        const NONBLOCK = 0o4000;
        const DSYNC = 0o10000;
        const DIRECTORY = 0o200000;
        const NOFOLLOW = 0o400000;
        const NOATIME = 0o1000000;
        const CLOEXEC = 0o2000000;
        const SYNC = 0o4010000;
        const PATH = 0o10000000;
        const TMPFILE = 0o20200000;
    }
}

bitflags! {
    /// Modo do `fallocate(2)` (`FALLOC_FL_*` de `linux/falloc.h`). Zero é a alocação comum, que estende o
    /// tamanho. Use `from_bits_retain` pra passar bits desconhecidos: o kernel responde EOPNOTSUPP.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct FallocFlags: u32 {
        const KEEP_SIZE = 0x01;
        const PUNCH_HOLE = 0x02;
        const NO_HIDE_STALE = 0x04;
        const COLLAPSE_RANGE = 0x08;
        const ZERO_RANGE = 0x10;
        const INSERT_RANGE = 0x20;
        const UNSHARE_RANGE = 0x40;
    }
}

impl FallocFlags {
    /// `FALLOC_FL_SUPPORTED_MASK` do `fs/open.c`.
    pub const SUPPORTED_MASK: u32 = 0x7f;

    /// As checagens genéricas do `vfs_fallocate` (Linux 6.12) que só dependem de `mode`, `offset` e
    /// `len`, na ordem do kernel: EINVAL pra `offset < 0` ou `len <= 0`, EOPNOTSUPP pra bit desconhecido,
    /// pra PUNCH_HOLE junto com ZERO_RANGE e pra PUNCH_HOLE sem KEEP_SIZE, EINVAL pra COLLAPSE_RANGE ou
    /// INSERT_RANGE combinados com outra flag e pra UNSHARE_RANGE com algo além de KEEP_SIZE.
    pub fn validate(self, offset: i64, len: i64) -> Result<(), crate::linux::Errno> {
        use crate::linux::Errno;
        if offset < 0 || len <= 0 {
            return Err(Errno::EINVAL);
        }
        let m = self.bits();
        if m & !Self::SUPPORTED_MASK != 0 {
            return Err(Errno::EOPNOTSUPP);
        }
        if self.contains(Self::PUNCH_HOLE | Self::ZERO_RANGE) {
            return Err(Errno::EOPNOTSUPP);
        }
        if self.contains(Self::PUNCH_HOLE) && !self.contains(Self::KEEP_SIZE) {
            return Err(Errno::EOPNOTSUPP);
        }
        if self.contains(Self::COLLAPSE_RANGE) && m & !Self::COLLAPSE_RANGE.bits() != 0 {
            return Err(Errno::EINVAL);
        }
        if self.contains(Self::INSERT_RANGE) && m & !Self::INSERT_RANGE.bits() != 0 {
            return Err(Errno::EINVAL);
        }
        if self.contains(Self::UNSHARE_RANGE) && m & !(Self::UNSHARE_RANGE | Self::KEEP_SIZE).bits() != 0 {
            return Err(Errno::EINVAL);
        }
        Ok(())
    }
}

impl OFlags {
    /// `O_RDONLY` é zero; ficam aqui pra deixar a intenção explícita.
    pub const RDONLY: OFlags = OFlags::empty();
    pub const ACCMODE: u32 = 0o3;

    pub fn readable(self) -> bool {
        self.bits() & Self::ACCMODE != 1
    }

    pub fn writable(self) -> bool {
        matches!(self.bits() & Self::ACCMODE, 1 | 2)
    }
}

bitflags! {
    /// Flags `AT_*` das syscalls `*at`.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct AtFlags: u32 {
        const SYMLINK_NOFOLLOW = 0x100;
        /// Pra `unlinkat` é `AT_REMOVEDIR`; pra `faccessat` o mesmo bit é `AT_EACCESS`.
        const REMOVEDIR = 0x200;
        const SYMLINK_FOLLOW = 0x400;
        const NO_AUTOMOUNT = 0x800;
        const EMPTY_PATH = 0x1000;
    }
}

bitflags! {
    /// `renameat2`.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct RenameFlags: u32 {
        const NOREPLACE = 1;
        const EXCHANGE = 2;
        const WHITEOUT = 4;
    }
}

bitflags! {
    /// Modo do `access(2)`: `F_OK` é vazio.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct AccessMode: u32 {
        const X_OK = 1;
        const W_OK = 2;
        const R_OK = 4;
    }
}

/// Bits de `st_mode`.
pub mod mode {
    use super::Mode;
    pub const S_IFMT: Mode = 0o170000;
    pub const S_IFSOCK: Mode = 0o140000;
    pub const S_IFLNK: Mode = 0o120000;
    pub const S_IFREG: Mode = 0o100000;
    pub const S_IFBLK: Mode = 0o060000;
    pub const S_IFDIR: Mode = 0o040000;
    pub const S_IFCHR: Mode = 0o020000;
    pub const S_IFIFO: Mode = 0o010000;
    pub const S_ISUID: Mode = 0o4000;
    pub const S_ISGID: Mode = 0o2000;
    pub const S_ISVTX: Mode = 0o1000;
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum FileType {
    Regular,
    Directory,
    Symlink,
    CharDevice,
    BlockDevice,
    Fifo,
    Socket,
}

impl FileType {
    pub fn from_mode(m: Mode) -> FileType {
        match m & mode::S_IFMT {
            mode::S_IFDIR => FileType::Directory,
            mode::S_IFLNK => FileType::Symlink,
            mode::S_IFCHR => FileType::CharDevice,
            mode::S_IFBLK => FileType::BlockDevice,
            mode::S_IFIFO => FileType::Fifo,
            mode::S_IFSOCK => FileType::Socket,
            _ => FileType::Regular,
        }
    }

    pub fn mode_bits(self) -> Mode {
        match self {
            FileType::Regular => mode::S_IFREG,
            FileType::Directory => mode::S_IFDIR,
            FileType::Symlink => mode::S_IFLNK,
            FileType::CharDevice => mode::S_IFCHR,
            FileType::BlockDevice => mode::S_IFBLK,
            FileType::Fifo => mode::S_IFIFO,
            FileType::Socket => mode::S_IFSOCK,
        }
    }

    /// `d_type` do `getdents64`.
    pub fn dirent_type(self) -> u8 {
        match self {
            FileType::Fifo => 1,
            FileType::CharDevice => 2,
            FileType::Directory => 4,
            FileType::BlockDevice => 6,
            FileType::Regular => 8,
            FileType::Symlink => 10,
            FileType::Socket => 12,
        }
    }
}

/// Instante com nanossegundos, como `struct timespec`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimeSpec {
    pub sec: i64,
    pub nsec: u32,
}

impl TimeSpec {
    pub fn from_duration_since_epoch(d: Duration) -> TimeSpec {
        TimeSpec { sec: d.as_secs() as i64, nsec: d.subsec_nanos() }
    }
}

/// Alvo de `utimensat`: um instante, "agora" (`UTIME_NOW`) ou "não mexe" (`UTIME_OMIT`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SetTime {
    Now,
    Omit,
    At(TimeSpec),
}

/// `struct stat` (com `btime` do statx).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Stat {
    pub dev: u64,
    pub ino: u64,
    /// Tipo e permissões (`S_IFREG | 0o644`...).
    pub mode: Mode,
    pub nlink: u64,
    pub uid: Uid,
    pub gid: Gid,
    pub rdev: u64,
    pub size: u64,
    pub blksize: u64,
    /// Em blocos de 512 bytes.
    pub blocks: u64,
    pub atime: TimeSpec,
    pub mtime: TimeSpec,
    pub ctime: TimeSpec,
    pub btime: Option<TimeSpec>,
}

impl Stat {
    pub fn file_type(&self) -> FileType {
        FileType::from_mode(self.mode)
    }

    pub fn perm(&self) -> Mode {
        self.mode & 0o7777
    }
}

/// Entrada de diretório.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DirEntry {
    pub ino: u64,
    pub kind: FileType,
    pub name: Vec<u8>,
}

/// `struct statfs`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StatFs {
    /// `f_type` (TMPFS_MAGIC = 0x01021994, PROC_SUPER_MAGIC = 0x9fa0...).
    pub fs_type: u64,
    pub bsize: u64,
    pub blocks: u64,
    pub bfree: u64,
    pub bavail: u64,
    pub files: u64,
    pub ffree: u64,
    pub namelen: u64,
    pub frsize: u64,
    pub flags: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Whence {
    Set,
    Cur,
    End,
    /// `SEEK_DATA` e `SEEK_HOLE`.
    Data,
    Hole,
}

/// `struct winsize`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Winsize {
    pub rows: u16,
    pub cols: u16,
    pub xpixel: u16,
    pub ypixel: u16,
}

/// Atributos de terminal, no formato do `struct termios2` do kernel Linux x86_64
/// (`include/uapi/asm-generic/termbits.h`).
///
/// Decisão sobre `c_cc`: usamos os 19 bytes do kernel (`NCCS = 19`), não os 32 da glibc. A glibc só
/// copia os 19 primeiros no `TCGETS`/`TCSETS` e os 13 restantes nunca chegam ao kernel, então o que
/// existe de verdade no tty são estes 19. As velocidades ficam em `c_ispeed`/`c_ospeed` como no
/// `termios2` (`TCGETS2`); com `CBAUD` diferente de `BOTHER` elas espelham a taxa codificada em
/// `c_cflag`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Termios {
    pub c_iflag: u32,
    pub c_oflag: u32,
    pub c_cflag: u32,
    pub c_lflag: u32,
    pub c_line: u8,
    pub c_cc: [u8; termios::NCCS],
    pub c_ispeed: u32,
    pub c_ospeed: u32,
}

impl Default for Termios {
    /// O `tty_std_termios` do kernel (`drivers/tty/tty_io.c`): o estado de um pty recém-aberto.
    fn default() -> Self {
        use termios::*;
        let mut c_cc = [0u8; NCCS];
        c_cc[VINTR] = 0x03;
        c_cc[VQUIT] = 0x1c;
        c_cc[VERASE] = 0x7f;
        c_cc[VKILL] = 0x15;
        c_cc[VEOF] = 0x04;
        c_cc[VTIME] = 0;
        c_cc[VMIN] = 1;
        c_cc[VSWTC] = 0;
        c_cc[VSTART] = 0x11;
        c_cc[VSTOP] = 0x13;
        c_cc[VSUSP] = 0x1a;
        c_cc[VEOL] = 0;
        c_cc[VREPRINT] = 0x12;
        c_cc[VDISCARD] = 0x0f;
        c_cc[VWERASE] = 0x17;
        c_cc[VLNEXT] = 0x16;
        c_cc[VEOL2] = 0;
        Termios {
            c_iflag: ICRNL | IXON,
            c_oflag: OPOST | ONLCR,
            c_cflag: B38400 | CS8 | CREAD | HUPCL,
            c_lflag: ISIG | ICANON | ECHO | ECHOE | ECHOK | ECHOCTL | ECHOKE | IEXTEN,
            c_line: 0,
            c_cc,
            c_ispeed: 38400,
            c_ospeed: 38400,
        }
    }
}

/// Quando o `tcsetattr` aplica a mudança (`TCSANOW`, `TCSADRAIN`, `TCSAFLUSH`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SetAttrWhen {
    /// `TCSANOW` (0).
    Now,
    /// `TCSADRAIN` (1): espera a saída pendente ser transmitida.
    Drain,
    /// `TCSAFLUSH` (2): espera a saída e descarta a entrada ainda não lida.
    Flush,
}

/// Flags e índices do `termios`, com os valores do Linux x86_64 (`asm-generic/termbits.h`).
pub mod termios {
    pub const NCCS: usize = 19;

    // Argumentos do `TCFLSH` (fila a descartar) e do `TCXONC` (ação de controle de fluxo).
    pub const TCIFLUSH: i32 = 0;
    pub const TCOFLUSH: i32 = 1;
    pub const TCIOFLUSH: i32 = 2;
    pub const TCOOFF: i32 = 0;
    pub const TCOON: i32 = 1;
    pub const TCIOFF: i32 = 2;
    pub const TCION: i32 = 3;

    // Índices de c_cc.
    pub const VINTR: usize = 0;
    pub const VQUIT: usize = 1;
    pub const VERASE: usize = 2;
    pub const VKILL: usize = 3;
    pub const VEOF: usize = 4;
    pub const VTIME: usize = 5;
    pub const VMIN: usize = 6;
    pub const VSWTC: usize = 7;
    pub const VSTART: usize = 8;
    pub const VSTOP: usize = 9;
    pub const VSUSP: usize = 10;
    pub const VEOL: usize = 11;
    pub const VREPRINT: usize = 12;
    pub const VDISCARD: usize = 13;
    pub const VWERASE: usize = 14;
    pub const VLNEXT: usize = 15;
    pub const VEOL2: usize = 16;

    // c_iflag.
    pub const IGNBRK: u32 = 0o000001;
    pub const BRKINT: u32 = 0o000002;
    pub const IGNPAR: u32 = 0o000004;
    pub const PARMRK: u32 = 0o000010;
    pub const INPCK: u32 = 0o000020;
    pub const ISTRIP: u32 = 0o000040;
    pub const INLCR: u32 = 0o000100;
    pub const IGNCR: u32 = 0o000200;
    pub const ICRNL: u32 = 0o000400;
    pub const IUCLC: u32 = 0o001000;
    pub const IXON: u32 = 0o002000;
    pub const IXANY: u32 = 0o004000;
    pub const IXOFF: u32 = 0o010000;
    pub const IMAXBEL: u32 = 0o020000;
    pub const IUTF8: u32 = 0o040000;

    // c_oflag.
    pub const OPOST: u32 = 0o000001;
    pub const OLCUC: u32 = 0o000002;
    pub const ONLCR: u32 = 0o000004;
    pub const OCRNL: u32 = 0o000010;
    pub const ONOCR: u32 = 0o000020;
    pub const ONLRET: u32 = 0o000040;
    pub const OFILL: u32 = 0o000100;
    pub const OFDEL: u32 = 0o000200;
    pub const NLDLY: u32 = 0o000400;
    pub const NL0: u32 = 0o000000;
    pub const NL1: u32 = 0o000400;
    pub const CRDLY: u32 = 0o003000;
    pub const CR0: u32 = 0o000000;
    pub const CR1: u32 = 0o001000;
    pub const CR2: u32 = 0o002000;
    pub const CR3: u32 = 0o003000;
    pub const TABDLY: u32 = 0o014000;
    pub const TAB0: u32 = 0o000000;
    pub const TAB1: u32 = 0o004000;
    pub const TAB2: u32 = 0o010000;
    pub const TAB3: u32 = 0o014000;
    pub const XTABS: u32 = 0o014000;
    pub const BSDLY: u32 = 0o020000;
    pub const BS0: u32 = 0o000000;
    pub const BS1: u32 = 0o020000;
    pub const VTDLY: u32 = 0o040000;
    pub const VT0: u32 = 0o000000;
    pub const VT1: u32 = 0o040000;
    pub const FFDLY: u32 = 0o100000;
    pub const FF0: u32 = 0o000000;
    pub const FF1: u32 = 0o100000;

    // c_cflag.
    pub const CBAUD: u32 = 0o010017;
    pub const B0: u32 = 0o000000;
    pub const B50: u32 = 0o000001;
    pub const B75: u32 = 0o000002;
    pub const B110: u32 = 0o000003;
    pub const B134: u32 = 0o000004;
    pub const B150: u32 = 0o000005;
    pub const B200: u32 = 0o000006;
    pub const B300: u32 = 0o000007;
    pub const B600: u32 = 0o000010;
    pub const B1200: u32 = 0o000011;
    pub const B1800: u32 = 0o000012;
    pub const B2400: u32 = 0o000013;
    pub const B4800: u32 = 0o000014;
    pub const B9600: u32 = 0o000015;
    pub const B19200: u32 = 0o000016;
    pub const B38400: u32 = 0o000017;
    pub const EXTA: u32 = B19200;
    pub const EXTB: u32 = B38400;
    pub const CSIZE: u32 = 0o000060;
    pub const CS5: u32 = 0o000000;
    pub const CS6: u32 = 0o000020;
    pub const CS7: u32 = 0o000040;
    pub const CS8: u32 = 0o000060;
    pub const CSTOPB: u32 = 0o000100;
    pub const CREAD: u32 = 0o000200;
    pub const PARENB: u32 = 0o000400;
    pub const PARODD: u32 = 0o001000;
    pub const HUPCL: u32 = 0o002000;
    pub const CLOCAL: u32 = 0o004000;
    pub const CBAUDEX: u32 = 0o010000;
    pub const BOTHER: u32 = 0o010000;
    pub const B57600: u32 = 0o010001;
    pub const B115200: u32 = 0o010002;
    pub const B230400: u32 = 0o010003;
    pub const B460800: u32 = 0o010004;
    pub const B500000: u32 = 0o010005;
    pub const B576000: u32 = 0o010006;
    pub const B921600: u32 = 0o010007;
    pub const B1000000: u32 = 0o010010;
    pub const B1152000: u32 = 0o010011;
    pub const B1500000: u32 = 0o010012;
    pub const B2000000: u32 = 0o010013;
    pub const B2500000: u32 = 0o010014;
    pub const B3000000: u32 = 0o010015;
    pub const B3500000: u32 = 0o010016;
    pub const B4000000: u32 = 0o010017;
    pub const CIBAUD: u32 = 0o02003600000;
    pub const IBSHIFT: u32 = 16;
    pub const CMSPAR: u32 = 0o10000000000;
    pub const CRTSCTS: u32 = 0o20000000000;

    // c_lflag.
    pub const ISIG: u32 = 0o000001;
    pub const ICANON: u32 = 0o000002;
    pub const XCASE: u32 = 0o000004;
    pub const ECHO: u32 = 0o000010;
    pub const ECHOE: u32 = 0o000020;
    pub const ECHOK: u32 = 0o000040;
    pub const ECHONL: u32 = 0o000100;
    pub const NOFLSH: u32 = 0o000200;
    pub const TOSTOP: u32 = 0o000400;
    pub const ECHOCTL: u32 = 0o001000;
    pub const ECHOPRT: u32 = 0o002000;
    pub const ECHOKE: u32 = 0o004000;
    pub const FLUSHO: u32 = 0o010000;
    pub const PENDIN: u32 = 0o040000;
    pub const IEXTEN: u32 = 0o100000;
    pub const EXTPROC: u32 = 0o200000;

    /// Taxa em bauds de um código `Bnnn` (o `tty_termios_baud_rate` sem `BOTHER`); `None` pra código
    /// inválido ou `BOTHER`.
    pub fn baud_of(code: u32) -> Option<u32> {
        const LOW: [u32; 16] = [0, 50, 75, 110, 134, 150, 200, 300, 600, 1200, 1800, 2400, 4800, 9600, 19200, 38400];
        const HIGH: [u32; 15] =
            [57600, 115200, 230400, 460800, 500000, 576000, 921600, 1000000, 1152000, 1500000, 2000000, 2500000, 3000000, 3500000, 4000000];
        let code = code & CBAUD;
        if code & CBAUDEX == 0 {
            Some(LOW[code as usize])
        } else {
            let i = (code & !CBAUDEX) as usize;
            if i == 0 { None } else { HIGH.get(i - 1).copied() }
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Clock {
    Realtime,
    Monotonic,
    Boottime,
    ProcessCpuTime,
    ThreadCpuTime,
}

/// `getrusage`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Rusage {
    pub utime: Duration,
    pub stime: Duration,
    pub maxrss_kib: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RusageWho {
    SelfProcess,
    Children,
}

/// Recursos de `getrlimit`/`setrlimit`, com os números do Linux.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Resource {
    Cpu = 0,
    Fsize = 1,
    Data = 2,
    Stack = 3,
    Core = 4,
    Rss = 5,
    Nproc = 6,
    Nofile = 7,
    Memlock = 8,
    As = 9,
    Locks = 10,
    Sigpending = 11,
    Msgqueue = 12,
    Nice = 13,
    Rtprio = 14,
    Rttime = 15,
}

pub const RLIM_INFINITY: u64 = u64::MAX;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct Rlimit {
    pub cur: u64,
    pub max: u64,
}

/// `uname(2)`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Utsname {
    pub sysname: Vec<u8>,
    pub nodename: Vec<u8>,
    pub release: Vec<u8>,
    pub version: Vec<u8>,
    pub machine: Vec<u8>,
    pub domainname: Vec<u8>,
}

/// Disposição de um sinal no processo.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum SigDisposition {
    Default,
    Ignore,
    /// O processo quer ser avisado: o kernel enfileira e o programa consulta com
    /// [`crate::Syscalls::take_caught_signals`] (é assim que o `trap` do shell funciona).
    Catch,
}

/// Como um processo terminou ou mudou de estado, como o `wait4` informa.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WaitStatus {
    Exited(i32),
    Signaled { signal: Signal, core_dumped: bool },
    Stopped(Signal),
    Continued,
}

impl WaitStatus {
    /// Código de saída visto pelo shell (`$?`): 128 + sinal quando morto por sinal.
    pub fn shell_status(self) -> i32 {
        match self {
            WaitStatus::Exited(c) => c & 0xff,
            WaitStatus::Signaled { signal, .. } => 128 + signal.0,
            WaitStatus::Stopped(s) => 128 + s.0,
            WaitStatus::Continued => 0,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WaitTarget {
    /// `-1`: qualquer filho.
    Any,
    Pid(Pid),
    /// Qualquer filho do grupo (`-pgid`; `0` = grupo do chamador).
    Group(Pid),
}

bitflags! {
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct WaitOptions: u32 {
        const NOHANG = 1;
        const UNTRACED = 2;
        /// `WEXITED`: só vale no `waitid` (o `wait4` sempre espera o término).
        const EXITED = 4;
        const CONTINUED = 8;
        /// `WNOWAIT`: só vale no `waitid`; o filho continua esperável.
        const NOWAIT = 0x0100_0000;
    }
}

/// Alvo do `waitid(idtype, id, ...)`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum WaitIdTarget {
    /// `P_ALL`.
    All,
    /// `P_PID`.
    Pid(Pid),
    /// `P_PGID` (`0` = grupo do chamador).
    Group(Pid),
    /// `P_PIDFD`.
    Pidfd(Fd),
}

/// O `siginfo_t` que o `waitid` preenche: o filho, o dono real dele e o que aconteceu. O `rusage` é o
/// que o `wait4` entrega: o uso do filho colhido (zerado em parada e continuação).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct WaitInfo {
    pub pid: Pid,
    pub uid: u32,
    pub status: WaitStatus,
    pub rusage: Rusage,
}

/// Alvo de `kill`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum KillTarget {
    Pid(Pid),
    /// Grupo de processos (`kill -- -pgid`); `0` = grupo do chamador.
    Group(Pid),
    /// `kill -1`: todos os processos que o chamador pode sinalizar, menos o 1 e ele mesmo.
    All,
}

/// Ajuste de fd no processo novo, aplicado na ordem, antes do programa começar (como
/// `posix_spawn_file_actions`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FdAction {
    /// `dup2(from, to)`; o fd novo não herda `FD_CLOEXEC`.
    Dup2 { from: Fd, to: Fd },
    Close(Fd),
    /// `posix_spawn_file_actions_addclosefrom_np`: fecha todo fd `>= fd` (os que ainda estão abertos).
    CloseFrom(Fd),
    Open { fd: Fd, path: Vec<u8>, flags: OFlags, mode: Mode },
}

/// `POSIX_SPAWN_SETSCHEDULER` e `POSIX_SPAWN_SETSCHEDPARAM`: o filho chama `sched_setscheduler(0, policy,
/// param)` quando há política e `sched_setparam(0, param)` quando não há.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SpawnSched {
    pub policy: Option<i32>,
    pub param: crate::sched::SchedParam,
}

/// Grupo de processos do filho.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ProcessGroup {
    /// Fica no grupo do pai.
    Inherit,
    /// Grupo novo com o próprio pid (`setpgid(0, 0)`).
    New,
    /// Entra num grupo existente.
    Join(Pid),
}

/// Atributos comuns de processo novo, tanto pra `spawn` (programa) quanto pra `spawn_fn` (fork).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcAttrs {
    /// Ambiente completo do filho (`NAME=valor`); `None` herda o do pai.
    pub env: Option<Vec<Vec<u8>>>,
    /// cwd do filho; `None` herda.
    pub cwd: Option<Vec<u8>>,
    pub fd_actions: Vec<FdAction>,
    pub group: ProcessGroup,
    /// `setsid()` no filho.
    pub new_session: bool,
    /// Sinais que voltam à disposição padrão no filho (o bash faz isso com os que ele captura).
    pub reset_signals: Vec<Signal>,
    /// Sinais ignorados no filho (`cmd &` em shell não interativo ignora SIGINT e SIGQUIT).
    pub ignore_signals: Vec<Signal>,
    /// `POSIX_SPAWN_RESETIDS`: o uid e o gid efetivos do filho voltam aos reais.
    pub reset_ids: bool,
    /// `POSIX_SPAWN_SETSCHEDULER`/`SETSCHEDPARAM`; um erro (EPERM, EINVAL) aborta a criação.
    pub scheduler: Option<SpawnSched>,
}

impl Default for ProcAttrs {
    fn default() -> Self {
        ProcAttrs {
            env: None,
            cwd: None,
            fd_actions: Vec::new(),
            group: ProcessGroup::Inherit,
            new_session: false,
            reset_signals: Vec::new(),
            ignore_signals: Vec::new(),
            reset_ids: false,
            scheduler: None,
        }
    }
}

/// Executa um programa num processo novo (`posix_spawn`): `path` já resolvido pelo chamador
/// (o kernel não procura no PATH, como o `execve`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SpawnSpec {
    pub path: Vec<u8>,
    pub argv: Vec<Vec<u8>>,
    pub attrs: ProcAttrs,
}

/// Corpo de um processo criado por `spawn_fn` (o equivalente a `fork` seguido de código no filho).
pub type ProcessFn = Box<dyn FnOnce() -> i32 + Send + 'static>;

/// Conexão de rede aberta pelo kernel ([`crate::Syscalls::net_connect`]).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct NetConn {
    /// fd de socket: lê e escreve bytes, `fstat` dá `S_IFSOCK`.
    pub fd: Fd,
    pub peer: std::net::SocketAddr,
    pub local: std::net::SocketAddr,
}

bitflags! {
    /// Eventos do `poll(2)`.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct PollEvents: u16 {
        const IN = 0x1;
        const PRI = 0x2;
        const OUT = 0x4;
        const ERR = 0x8;
        const HUP = 0x10;
        const NVAL = 0x20;
        /// `POLLRDHUP`: o par encerrou a escrita (o `shutdown` do outro lado ou o `close`).
        const RDHUP = 0x2000;
    }
}

bitflags! {
    /// Flags de `recv(2)` e `send(2)` que um socket de fluxo trata, valores do Linux x86_64.
    #[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
    pub struct MsgFlags: u32 {
        /// `MSG_PEEK`: olha a fila sem consumi-la.
        const PEEK = 0x2;
        /// `MSG_DONTWAIT`: esta chamada não bloqueia, como se o fd fosse `O_NONBLOCK`.
        const DONTWAIT = 0x40;
        /// `MSG_WAITALL`: a leitura espera o tanto pedido (até o fim, um erro ou, sem bloqueio, a fila esvaziar).
        const WAITALL = 0x100;
        /// `MSG_NOSIGNAL`: a escrita que dá EPIPE não gera SIGPIPE.
        const NOSIGNAL = 0x4000;
        /// `MSG_TRUNC` (só na saída do `recvmsg`): a mensagem era maior que o buffer.
        const TRUNC = 0x20;
        /// `MSG_CTRUNC` (só na saída do `recvmsg`): os dados auxiliares não couberam.
        const CTRUNC = 0x8;
        /// `MSG_CMSG_CLOEXEC`: os descritores recebidos por `SCM_RIGHTS` nascem com `FD_CLOEXEC`.
        const CMSG_CLOEXEC = 0x4000_0000;
    }
}

/// O que um `recvmsg` devolve: os dados, o nome de quem enviou (datagrama), o `msg_control` já montado e
/// as flags de saída (`MSG_TRUNC`, `MSG_CTRUNC`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecvMsg {
    pub data: Vec<u8>,
    pub name: Option<Vec<u8>>,
    pub control: Vec<u8>,
    pub flags: MsgFlags,
}

/// Entrada do `poll(2)`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct PollFd {
    pub fd: Fd,
    pub events: PollEvents,
    pub revents: PollEvents,
}

/// Bits de evento do `epoll(7)` (`EPOLL_EVENTS` do glibc). Os oito primeiros coincidem com os do `poll(2)`.
pub mod epoll {
    pub const IN: u32 = 0x1;
    pub const PRI: u32 = 0x2;
    pub const OUT: u32 = 0x4;
    pub const ERR: u32 = 0x8;
    pub const HUP: u32 = 0x10;
    pub const RDNORM: u32 = 0x40;
    pub const RDBAND: u32 = 0x80;
    pub const WRNORM: u32 = 0x100;
    pub const WRBAND: u32 = 0x200;
    pub const MSG: u32 = 0x400;
    pub const RDHUP: u32 = 0x2000;
    pub const EXCLUSIVE: u32 = 1 << 28;
    pub const WAKEUP: u32 = 1 << 29;
    pub const ONESHOT: u32 = 1 << 30;
    pub const ET: u32 = 1 << 31;
    /// `EPOLL_CLOEXEC` (`O_CLOEXEC`).
    pub const CLOEXEC: u32 = 0x80000;
    pub const CTL_ADD: i32 = 1;
    pub const CTL_DEL: i32 = 2;
    pub const CTL_MOD: i32 = 3;
}

/// `struct epoll_event`: máscara de eventos e o dado do usuário devolvido junto com o evento.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub struct EpollEvent {
    pub events: u32,
    pub data: u64,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LockKind {
    Read,
    Write,
    Unlock,
}

/// Trava de faixa de bytes, com dono = open file description (`F_OFD_SETLK`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FileLock {
    pub kind: LockKind,
    pub start: u64,
    /// 0 = até o fim do arquivo (e além).
    pub len: u64,
}

/// Tamanho padrão de um pipe e o valor inicial de `/proc/sys/fs/pipe-max-size` no Debian 13.
pub const PIPE_DEFAULT_SIZE: usize = 65536;
pub const PIPE_MAX_SIZE_DEFAULT: u32 = 1 << 20;
/// A página do pipe: a menor capacidade possível.
pub const PIPE_PAGE_SIZE: usize = 4096;

/// `round_pipe_size`: a capacidade que cobre o pedido, no mínimo uma página e sempre potência de 2. `None`
/// quando o arredondamento estoura 2^31 (o `roundup_pow_of_two` do kernel dá 0 e o chamador responde EINVAL).
pub fn round_pipe_size(size: u32) -> Option<u32> {
    if size < PIPE_PAGE_SIZE as u32 {
        return Some(PIPE_PAGE_SIZE as u32);
    }
    size.checked_next_power_of_two().filter(|n| *n <= 1 << 31)
}

/// `struct flock` do x86-64 (`F_GETLK`, `F_SETLK`, `F_SETLKW` e as `F_OFD_*`), como o programa a preenche.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Flock {
    /// `F_RDLCK`, `F_WRLCK` ou `F_UNLCK` ([`fcntl`]).
    pub l_type: i16,
    /// `SEEK_SET`, `SEEK_CUR` ou `SEEK_END`: de onde `start` conta.
    pub whence: i16,
    pub start: i64,
    /// 0 = até o fim do arquivo (e além); negativo = a faixa termina antes de `start`.
    pub len: i64,
    pub pid: i32,
}

/// O comando do `fcntl(2)` que mexe numa trava de faixa.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LockCmd {
    /// `F_GETLK`: o dono é o processo.
    Get,
    /// `F_SETLK`.
    Set,
    /// `F_SETLKW`.
    SetWait,
    /// `F_OFD_GETLK`: o dono é a descrição de arquivo aberto.
    OfdGet,
    /// `F_OFD_SETLK`.
    OfdSet,
    /// `F_OFD_SETLKW`.
    OfdSetWait,
}

/// Constantes de `fcntl.h` que o `flock(2)` e o `fcntl(2)` de travas usam (valores do x86-64).
pub mod fcntl {
    pub const F_RDLCK: i16 = 0;
    pub const F_WRLCK: i16 = 1;
    pub const F_UNLCK: i16 = 2;
    pub const SEEK_SET: i16 = 0;
    pub const SEEK_CUR: i16 = 1;
    pub const SEEK_END: i16 = 2;
    pub const LOCK_SH: u32 = 1;
    pub const LOCK_EX: u32 = 2;
    pub const LOCK_NB: u32 = 4;
    pub const LOCK_UN: u32 = 8;
    /// `LOCK_MAND`: o Linux 6.12 ignora o pedido e responde 0.
    pub const LOCK_MAND: u32 = 32;
}

impl FileLock {
    /// Faixas `[start, end)` se sobrepõem (len 0 = infinito).
    pub fn overlaps(&self, other: &FileLock) -> bool {
        let end = |l: &FileLock| if l.len == 0 { u64::MAX } else { l.start.saturating_add(l.len) };
        self.start < end(other) && other.start < end(self)
    }
}

/// Informação mínima de processo (pra `ps`, `jobs`, `/proc`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcInfo {
    pub pid: Pid,
    pub ppid: Pid,
    pub pgid: Pid,
    pub sid: Pid,
    /// Estado de uma letra como no `/proc/<pid>/stat`: R, S, D, T, Z.
    pub state: char,
    pub comm: Vec<u8>,
}
