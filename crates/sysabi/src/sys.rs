//! As syscalls do pseudo-linus.
//!
//! O kernel implementa [`Syscalls`] pra cada pseudo-processo e instala o objeto na thread do processo
//! (modelo de execução A: uma thread do SO por pseudo-processo). Programas chamam as funções livres
//! deste módulo, que encontram o processo corrente pelo thread-local, exatamente como um programa C
//! chama a libc sem saber qual processo ele é.
//!
//! Regras do contrato:
//!
//! - Caminhos são bytes (`&[u8]`), como no Linux; nada de `Path` do host.
//! - Toda syscall é ponto de preempção e de entrega de sinal. Um sinal fatal pendente faz a syscall não
//!   voltar: o kernel desenrola a pilha do processo (ver [`KillUnwind`]).
//! - Erros são sempre [`Errno`] com o número do Linux.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use crate::itimer::{Itimer, Itimerval};
use crate::linux::{Errno, Signal};
use crate::sched::{SchedAttr, SchedParam};
use crate::types::*;

pub type SysResult<T> = Result<T, Errno>;

/// A interface de um processo com o kernel. Todos os métodos agem sobre o processo dono do objeto.
pub trait Syscalls: Send + Sync {
    // ---- arquivos ----
    fn openat(&self, dirfd: Fd, path: &[u8], flags: OFlags, mode: Mode) -> SysResult<Fd>;
    fn close(&self, fd: Fd) -> SysResult<()>;
    fn read(&self, fd: Fd, buf: &mut [u8]) -> SysResult<usize>;
    fn write(&self, fd: Fd, buf: &[u8]) -> SysResult<usize>;
    fn pread(&self, fd: Fd, buf: &mut [u8], offset: u64) -> SysResult<usize>;
    fn pwrite(&self, fd: Fd, buf: &[u8], offset: u64) -> SysResult<usize>;
    fn lseek(&self, fd: Fd, offset: i64, whence: Whence) -> SysResult<u64>;
    fn fstat(&self, fd: Fd) -> SysResult<Stat>;
    fn fstatat(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<Stat>;
    fn faccessat(&self, dirfd: Fd, path: &[u8], mode: AccessMode, flags: AtFlags) -> SysResult<()>;
    fn mkdirat(&self, dirfd: Fd, path: &[u8], mode: Mode) -> SysResult<()>;
    /// `AtFlags::REMOVEDIR` remove diretório (rmdir).
    fn unlinkat(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<()>;
    fn renameat2(&self, olddir: Fd, old: &[u8], newdir: Fd, new: &[u8], flags: RenameFlags) -> SysResult<()>;
    fn linkat(&self, olddir: Fd, old: &[u8], newdir: Fd, new: &[u8], flags: AtFlags) -> SysResult<()>;
    fn symlinkat(&self, target: &[u8], dirfd: Fd, path: &[u8]) -> SysResult<()>;
    fn readlinkat(&self, dirfd: Fd, path: &[u8]) -> SysResult<Vec<u8>>;
    /// FIFO (`mkfifo`) e, pro root, dispositivos.
    fn mknodat(&self, dirfd: Fd, path: &[u8], mode: Mode, dev: u64) -> SysResult<()>;
    fn fchmodat(&self, dirfd: Fd, path: &[u8], mode: Mode, flags: AtFlags) -> SysResult<()>;
    fn fchmod(&self, fd: Fd, mode: Mode) -> SysResult<()>;
    /// `None` não muda o dono/grupo (o `-1` do Linux).
    fn fchownat(&self, dirfd: Fd, path: &[u8], uid: Option<Uid>, gid: Option<Gid>, flags: AtFlags) -> SysResult<()>;
    fn utimensat(&self, dirfd: Fd, path: &[u8], atime: SetTime, mtime: SetTime, flags: AtFlags) -> SysResult<()>;
    fn futimens(&self, fd: Fd, atime: SetTime, mtime: SetTime) -> SysResult<()>;
    fn ftruncate(&self, fd: Fd, len: u64) -> SysResult<()>;
    /// `fallocate(2)`: reserva (ou, com PUNCH_HOLE, libera) o intervalo `[offset, offset + len)`.
    /// `offset` e `len` são `loff_t` com sinal, como no Linux, pra que os EINVAL de valor negativo saiam
    /// do kernel. Os errnos e a ordem seguem o `vfs_fallocate` e o `shmem_fallocate` do Linux 6.12.
    fn fallocate(&self, fd: Fd, mode: FallocFlags, offset: i64, len: i64) -> SysResult<()>;
    fn fsync(&self, fd: Fd) -> SysResult<()>;
    /// Próximo lote de entradas de um diretório aberto (como `getdents64`); vazio no fim. Inclui `.` e
    /// `..`, na ordem do sistema de arquivos.
    fn getdents(&self, fd: Fd) -> SysResult<Vec<DirEntry>>;
    fn statfs(&self, path: &[u8]) -> SysResult<StatFs>;
    fn fstatfs(&self, fd: Fd) -> SysResult<StatFs>;

    // ---- descritores ----
    fn dup(&self, fd: Fd) -> SysResult<Fd>;
    /// `dup3`; `cloexec` liga `FD_CLOEXEC` no novo. `old == new` é EINVAL (como `dup3`).
    fn dup3(&self, old: Fd, new: Fd, cloexec: bool) -> SysResult<Fd>;
    /// Menor fd livre `>= min` (`F_DUPFD`, `F_DUPFD_CLOEXEC`).
    fn dup_min(&self, fd: Fd, min: Fd, cloexec: bool) -> SysResult<Fd>;
    fn get_cloexec(&self, fd: Fd) -> SysResult<bool>;
    fn set_cloexec(&self, fd: Fd, on: bool) -> SysResult<()>;
    /// `F_GETFL`.
    fn get_status_flags(&self, fd: Fd) -> SysResult<OFlags>;
    /// `F_SETFL` (só APPEND e NONBLOCK mudam, como no Linux).
    fn set_status_flags(&self, fd: Fd, flags: OFlags) -> SysResult<()>;
    fn pipe2(&self, flags: OFlags) -> SysResult<(Fd, Fd)>;
    fn isatty(&self, fd: Fd) -> bool;
    fn tcgetwinsize(&self, fd: Fd) -> SysResult<Winsize>;
    /// `TCGETS2`: atributos do terminal. EBADF pra fd ruim, ENOTTY pra fd que não é terminal.
    fn tcgetattr(&self, fd: Fd) -> SysResult<Termios>;
    /// `TCSETS2`/`TCSETSW2`/`TCSETSF2` conforme `when`. Mesmos erros do [`Syscalls::tcgetattr`].
    fn tcsetattr(&self, fd: Fd, when: SetAttrWhen, termios: &Termios) -> SysResult<()>;
    /// `TIOCSWINSZ`. Mudar o tamanho manda SIGWINCH pro grupo em primeiro plano.
    fn tcsetwinsize(&self, fd: Fd, ws: Winsize) -> SysResult<()>;
    /// `TCFLSH`: descarta a entrada (`TCIFLUSH`), a saída (`TCOFLUSH`) ou as duas (`TCIOFLUSH`) do
    /// terminal. EINVAL pra fila desconhecida; mesmos erros de fd do [`Syscalls::tcgetattr`].
    fn tcflush(&self, fd: Fd, queue: i32) -> SysResult<()>;
    /// `TCXONC`: suspende (`TCOOFF`) ou retoma (`TCOON`) a saída, ou manda o caractere STOP (`TCIOFF`)
    /// ou START (`TCION`) pelo terminal. EINVAL pra ação desconhecida.
    fn tcflow(&self, fd: Fd, action: i32) -> SysResult<()>;
    /// `TIOCGPTN`: número do pty de um mestre (`/dev/pts/N`). ENOTTY pra fd que não é mestre.
    fn pty_number(&self, fd: Fd) -> SysResult<u32> {
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }

    // ---- TCP de loopback (`127.0.0.1`) entre os processos do sandbox ----
    /// `socket` + `bind` + `listen`: porta 0 escolhe uma efêmera. Devolve o fd e a porta.
    fn tcp_listen(&self, port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = (port, backlog, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `accept4`: a conexão e a porta de quem conectou.
    fn tcp_accept(&self, fd: Fd, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = (fd, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `socket` + `connect`: a conexão e a porta efêmera local.
    fn tcp_connect(&self, port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = (port, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `tcp_listen` com o endereço do `bind` (`0.0.0.0`, `127.0.0.1`, `::`), que o `/proc/net/tcp`
    /// mostra e que decide a família do socket.
    fn tcp_listen_at(&self, ip: std::net::IpAddr, port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = ip;
        self.tcp_listen(port, backlog, nonblock, cloexec)
    }
    /// `socket` + `bind` sem `listen`: reserva `ip:port` (porta 0 sorteia uma efêmera ímpar) e devolve o
    /// fd e a porta. EADDRINUSE se outro socket, ligado ou em escuta, já tem a porta num endereço que se
    /// encontra com `ip`. A reserva some no `close`; não aparece no `/proc/net/tcp` e não atende `connect`.
    /// `reuse_addr` é o `SO_REUSEADDR` do socket: com ele, uma conexão em TIME_WAIT ou aberta na porta não
    /// impede o `bind` (só um socket em escuta ou ligado sem `SO_REUSEADDR` impede); sem ele, impede.
    fn tcp_bind(&self, ip: std::net::IpAddr, port: u16, reuse_addr: bool, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = (ip, port, reuse_addr, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `connect` num fd que veio do `tcp_bind` e ainda não escuta: o mesmo número de fd passa a ser a
    /// conexão, e a ponta local dela é a reserva do `bind` (a porta, o endereço se não era o curinga, o
    /// inode do socket); devolve a porta local. EISCONN se o socket já escuta ou já conectou, ENOTSOCK se
    /// não for socket TCP, ECONNREFUSED sem ninguém escutando (o fd segue ligado, como no Linux).
    fn tcp_connect_bound(&self, fd: Fd, ip: std::net::IpAddr, port: u16) -> SysResult<u16> {
        let _ = (fd, ip, port);
        Err(Errno::ENOSYS)
    }
    /// `listen` num fd que veio do `tcp_bind`: o socket passa a escutar na mesma reserva (chamar de novo
    /// só muda o backlog). EINVAL num socket já conectado, ENOTSOCK se não for socket TCP.
    fn tcp_listen_bound(&self, fd: Fd, backlog: u32) -> SysResult<()> {
        let _ = (fd, backlog);
        Err(Errno::ENOSYS)
    }
    /// `tcp_connect` a um endereço de loopback (`127.0.0.1`, `::1`): a família do socket é a dele.
    fn tcp_connect_at(&self, ip: std::net::IpAddr, port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)> {
        let _ = ip;
        self.tcp_connect(port, nonblock, cloexec)
    }
    /// `socket(AF_INET ou AF_INET6, SOCK_STREAM)`: sem endereço nem porta até o `bind` ou o `connect`.
    fn tcp_socket(&self, v6: bool, nonblock: bool, cloexec: bool) -> SysResult<Fd> {
        let _ = (v6, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `bind` num fd do `tcp_socket`: devolve a porta (0 sorteia uma efêmera ímpar). EINVAL se o socket já
    /// tem endereço, EADDRINUSE como o `tcp_bind`.
    fn tcp_bind_fd(&self, fd: Fd, ip: std::net::IpAddr, port: u16, reuse_addr: bool) -> SysResult<u16> {
        let _ = (fd, ip, port, reuse_addr);
        Err(Errno::ENOSYS)
    }
    /// `connect` num fd do `tcp_socket` (ligado ou não; sem `bind` a porta local é uma efêmera par). Com o
    /// fd não bloqueante o loopback devolve EINPROGRESS (a conexão já vale; a recusa fica em `sock_error`, no
    /// poll e no `connect` seguinte); o `connect` seguinte conclui com 0 e o terceiro dá EISCONN. Com a fila de
    /// aceite do ouvinte cheia (mais de `backlog` conexões completas) o SYN é descartado: o bloqueante espera as
    /// retransmissões (1, 3, 7... s, `ETIMEDOUT` em 127 s) e o não bloqueante dá EINPROGRESS, depois EALREADY
    /// até a conexão sair (`POLLOUT`) ou falhar.
    fn tcp_connect_fd(&self, fd: Fd, ip: std::net::IpAddr, port: u16) -> SysResult<()> {
        let _ = (fd, ip, port);
        Err(Errno::ENOSYS)
    }
    /// `getsockname` e `getpeername` de um socket TCP; o par é `None` sem conexão.
    #[allow(clippy::type_complexity)]
    fn tcp_names(&self, fd: Fd) -> SysResult<((std::net::IpAddr, u16), Option<(std::net::IpAddr, u16)>)> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }
    /// `(domínio, tipo, protocolo, em escuta)` de um socket: `SO_DOMAIN`, `SO_TYPE`, `SO_PROTOCOL` e
    /// `SO_ACCEPTCONN`. ENOTSOCK se o fd não é socket.
    fn sock_info(&self, fd: Fd) -> SysResult<(i32, i32, i32, bool)> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }
    /// `SO_ERROR`: o erro pendente do socket, que a leitura zera.
    fn sock_error(&self, fd: Fd) -> SysResult<i32> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }
    /// Guarda o valor de uma opção de socket (`setsockopt`); o Python valida e normaliza antes.
    fn sock_setopt(&self, fd: Fd, level: i32, name: i32, value: &[u8]) -> SysResult<()> {
        let _ = (fd, level, name, value);
        Err(Errno::ENOSYS)
    }
    /// O valor guardado de uma opção de socket, ou `None` se ninguém a definiu.
    fn sock_getopt(&self, fd: Fd, level: i32, name: i32) -> SysResult<Option<Vec<u8>>> {
        let _ = (fd, level, name);
        Err(Errno::ENOSYS)
    }
    /// `recv(2)` num socket de fluxo (TCP ou Unix): até `max` bytes, com `MSG_PEEK`, `MSG_DONTWAIT` e
    /// `MSG_WAITALL`. Fim de fluxo é o vetor vazio; os dados que já chegaram saem antes de qualquer erro.
    /// Sem conexão: ENOTCONN no TCP (depois de um `connect` recusado, o erro pendente uma vez e então 0).
    fn sock_recv(&self, fd: Fd, max: usize, flags: MsgFlags) -> SysResult<Vec<u8>> {
        let _ = (fd, max, flags);
        Err(Errno::ENOSYS)
    }
    /// `send(2)` num socket de fluxo (TCP ou Unix): `MSG_DONTWAIT` e `MSG_NOSIGNAL`. EPIPE (com SIGPIPE, salvo
    /// `MSG_NOSIGNAL`) sem conexão no TCP, depois de `SHUT_WR` e depois do RST.
    fn sock_send(&self, fd: Fd, buf: &[u8], flags: MsgFlags) -> SysResult<usize> {
        let _ = (fd, buf, flags);
        Err(Errno::ENOSYS)
    }
    /// `shutdown(2)`: fecha a leitura, a escrita ou as duas.
    fn tcp_shutdown(&self, fd: Fd, read: bool, write: bool) -> SysResult<()> {
        let _ = (fd, read, write);
        Err(Errno::ENOSYS)
    }
    /// Portas local e remota (`getsockname`, `getpeername`); a remota é `None` num socket em escuta.
    fn tcp_ports(&self, fd: Fd) -> SysResult<(u16, Option<u16>)> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }

    // ---- UDP de loopback ----
    /// `socket(AF_INET ou AF_INET6, SOCK_DGRAM)`.
    fn udp_socket(&self, v6: bool, nonblock: bool, cloexec: bool) -> SysResult<Fd> {
        let _ = (v6, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `bind`: porta 0 sorteia uma. `reuse` é o `SO_REUSEADDR`. Devolve a porta.
    fn udp_bind(&self, fd: Fd, ip: std::net::IpAddr, port: u16, reuse: bool) -> SysResult<u16> {
        let _ = (fd, ip, port, reuse);
        Err(Errno::ENOSYS)
    }
    fn udp_connect(&self, fd: Fd, ip: std::net::IpAddr, port: u16) -> SysResult<()> {
        let _ = (fd, ip, port);
        Err(Errno::ENOSYS)
    }
    /// `sendto`; sem destino, para o par do `connect`.
    fn udp_sendto(&self, fd: Fd, data: &[u8], dst: Option<(std::net::IpAddr, u16)>) -> SysResult<usize> {
        let _ = (fd, data, dst);
        Err(Errno::ENOSYS)
    }
    /// `recvfrom`: até `max` bytes do próximo datagrama e quem enviou.
    fn udp_recvfrom(&self, fd: Fd, max: usize, peek: bool) -> SysResult<(Vec<u8>, std::net::IpAddr, u16)> {
        let _ = (fd, max, peek);
        Err(Errno::ENOSYS)
    }
    /// `getsockname` e `getpeername` juntos.
    #[allow(clippy::type_complexity)]
    fn udp_names(&self, fd: Fd) -> SysResult<((std::net::IpAddr, u16), Option<(std::net::IpAddr, u16)>)> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }

    // ---- sockets do domínio Unix (`AF_UNIX`) ----
    // Os tipos são os do Linux: 1 (`SOCK_STREAM`), 2 (`SOCK_DGRAM`) e 5 (`SOCK_SEQPACKET`). Nomes vêm
    // como o `sun_path` (no espaço abstrato, com o byte nulo na frente).
    /// `socket(AF_UNIX, ty)`.
    fn unix_socket(&self, ty: u8, nonblock: bool, cloexec: bool) -> SysResult<Fd> {
        let _ = (ty, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `socketpair(AF_UNIX, ty)`.
    fn unix_socketpair(&self, ty: u8, nonblock: bool, cloexec: bool) -> SysResult<(Fd, Fd)> {
        let _ = (ty, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `bind`: num caminho, cria o arquivo do socket.
    fn unix_bind(&self, fd: Fd, name: &[u8]) -> SysResult<()> {
        let _ = (fd, name);
        Err(Errno::ENOSYS)
    }
    fn unix_listen(&self, fd: Fd, backlog: u32) -> SysResult<()> {
        let _ = (fd, backlog);
        Err(Errno::ENOSYS)
    }
    /// `accept4`.
    fn unix_accept(&self, fd: Fd, nonblock: bool, cloexec: bool) -> SysResult<Fd> {
        let _ = (fd, nonblock, cloexec);
        Err(Errno::ENOSYS)
    }
    /// `connect`: num socket de datagrama, só o destino padrão.
    fn unix_connect(&self, fd: Fd, name: &[u8]) -> SysResult<()> {
        let _ = (fd, name);
        Err(Errno::ENOSYS)
    }
    /// `getsockname` e `getpeername` juntos: o nome, o do par e se está conectado.
    fn unix_names(&self, fd: Fd) -> SysResult<(Option<Vec<u8>>, Option<Vec<u8>>, bool)> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }
    /// `sendto` de datagrama; sem `name`, para o par do `connect`.
    fn unix_sendto(&self, fd: Fd, data: &[u8], name: Option<&[u8]>) -> SysResult<usize> {
        let _ = (fd, data, name);
        Err(Errno::ENOSYS)
    }
    /// `recvfrom` de datagrama: até `max` bytes da próxima mensagem e o nome de quem enviou.
    fn unix_recvfrom(&self, fd: Fd, max: usize, peek: bool) -> SysResult<(Vec<u8>, Option<Vec<u8>>)> {
        let _ = (fd, max, peek);
        Err(Errno::ENOSYS)
    }
    /// `sendmsg(2)` num socket `AF_UNIX`: os dados, o nome do destino (datagrama) e o `msg_control` cru
    /// (`SCM_RIGHTS` e `SCM_CREDENTIALS`, ver [`crate::cmsg`]). Os erros e a ordem são os do `__scm_send` e do
    /// `unix_*_sendmsg`: o controle malformado é EINVAL, descritor ruim EBADF, controle grande ENOBUFS.
    fn unix_sendmsg(&self, fd: Fd, data: &[u8], name: Option<&[u8]>, control: &[u8], flags: MsgFlags) -> SysResult<usize> {
        let _ = (fd, data, name, control, flags);
        Err(Errno::ENOSYS)
    }
    /// `recvmsg(2)` num socket `AF_UNIX`: até `max` bytes e um `msg_control` de até `control_len` bytes. Os
    /// descritores recebidos entram na tabela de quem chama; o que não coube vira `MSG_CTRUNC`.
    fn unix_recvmsg(&self, fd: Fd, max: usize, control_len: usize, flags: MsgFlags) -> SysResult<RecvMsg> {
        let _ = (fd, max, control_len, flags);
        Err(Errno::ENOSYS)
    }
    /// `TIOCSPTLCK`: trava (`true`) ou destrava o escravo de um mestre (`unlockpt` destrava).
    fn pty_set_lock(&self, fd: Fd, locked: bool) -> SysResult<()> {
        let _ = locked;
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCGPGRP`: grupo em primeiro plano. Pelo escravo, só no terminal de controle (ENOTTY).
    fn tcgetpgrp(&self, fd: Fd) -> SysResult<Pid> {
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCSPGRP`: só no terminal de controle (ENOTTY); EINVAL pra grupo negativo, ESRCH pra grupo
    /// que não existe, EPERM pra grupo de outra sessão.
    fn tcsetpgrp(&self, fd: Fd, pgrp: Pid) -> SysResult<()> {
        let _ = pgrp;
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCGSID`: sessão de que o terminal é terminal de controle.
    fn tcgetsid(&self, fd: Fd) -> SysResult<Pid> {
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCSCTTY`: o terminal vira o terminal de controle da sessão de quem chama (que tem de ser
    /// líder e não ter outro). `force` é o argumento 1, que deixa o root roubar de outra sessão.
    fn tiocsctty(&self, fd: Fd, force: bool) -> SysResult<()> {
        let _ = force;
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// `TIOCNOTTY`: solta o terminal de controle.
    fn tiocnotty(&self, fd: Fd) -> SysResult<()> {
        self.fstat(fd)?;
        Err(Errno::ENOTTY)
    }
    /// Fds abertos (pra `/proc/self/fd` e pra ferramentas que fecham tudo).
    fn open_fds(&self) -> Vec<Fd>;

    // ---- diretório corrente e máscara ----
    fn chdir(&self, path: &[u8]) -> SysResult<()>;
    fn fchdir(&self, fd: Fd) -> SysResult<()>;
    fn getcwd(&self) -> SysResult<Vec<u8>>;
    fn umask(&self, mask: Mode) -> Mode;

    // ---- processos ----
    fn getpid(&self) -> Pid;
    fn getppid(&self) -> Pid;
    fn getpgid(&self, pid: Pid) -> SysResult<Pid>;
    fn setpgid(&self, pid: Pid, pgid: Pid) -> SysResult<()>;
    fn getsid(&self, pid: Pid) -> SysResult<Pid>;
    fn setsid(&self) -> SysResult<Pid>;
    /// Executa um programa num processo novo (`posix_spawn`). ENOENT, EACCES, ENOEXEC como o `execve`.
    fn spawn(&self, spec: SpawnSpec) -> SysResult<Pid>;
    /// Cria um processo novo que herda uma cópia do estado deste (cwd, ambiente, umask, fds com as
    /// ações aplicadas, disposições de sinal, rlimits) e roda `body` nele. É o `fork` do pseudo-linus:
    /// o shell usa pra subshell, pipeline e substituição de comando.
    fn spawn_fn(&self, attrs: ProcAttrs, name: Vec<u8>, body: ProcessFn) -> SysResult<Pid>;
    /// Substitui o programa do processo corrente (mesmo pid). Só volta em caso de erro.
    fn execve(&self, path: &[u8], argv: &[Vec<u8>], env: Option<&[Vec<u8>]>) -> Errno;
    /// `None` com `NOHANG` e nenhum filho mudou; ECHILD sem filhos.
    fn wait4(&self, target: WaitTarget, options: WaitOptions) -> SysResult<Option<(Pid, WaitStatus)>> {
        Ok(self.wait4_info(target, options)?.map(|i| (i.pid, i.status)))
    }
    /// O `wait4(2)` completo: além do pid e do estado, o `rusage` do filho colhido.
    fn wait4_info(&self, target: WaitTarget, options: WaitOptions) -> SysResult<Option<WaitInfo>>;
    /// `waitid(2)`: `options` leva `EXITED`, `UNTRACED` (`WSTOPPED`) e `CONTINUED` (ao menos um, senão
    /// EINVAL), `NOHANG` e `NOWAIT`. `None` com `NOHANG` e nenhum evento; ECHILD sem filhos que casem.
    fn waitid(&self, target: WaitIdTarget, options: WaitOptions) -> SysResult<Option<WaitInfo>> {
        let _ = (target, options);
        Err(Errno::ENOSYS)
    }
    fn kill(&self, target: KillTarget, sig: Signal) -> SysResult<()>;
    fn sigaction(&self, sig: Signal, disposition: SigDisposition) -> SysResult<SigDisposition>;
    /// Sinais com disposição `Catch` que chegaram desde a última chamada, na ordem de chegada.
    fn take_caught_signals(&self) -> Vec<Signal>;
    /// Ponto de preempção e entrega de sinal pra laços de CPU que não fazem syscall.
    fn checkpoint(&self);
    fn sched_yield(&self);
    fn getpriority(&self, pid: Pid) -> SysResult<i32>;
    fn setpriority(&self, pid: Pid, nice: i32) -> SysResult<()>;
    fn getrlimit(&self, res: Resource) -> SysResult<Rlimit>;
    fn setrlimit(&self, res: Resource, lim: Rlimit) -> SysResult<()>;
    fn getrusage(&self, who: RusageWho) -> SysResult<Rusage>;
    /// Processos visíveis neste sandbox.
    fn list_processes(&self) -> Vec<ProcInfo>;
    /// Thread nova no mesmo processo (`clone` com `CLONE_THREAD`): divide memória, fds e cwd, é
    /// escalonada como qualquer thread e morre junto com o processo. `exit` em qualquer thread termina o
    /// processo inteiro (como `exit_group`); a thread acaba normalmente quando `body` volta.
    fn spawn_thread(&self, body: ThreadFn) -> SysResult<Tid>;
    /// Espera a thread `tid` terminar (`pthread_join`). ESRCH se não existe, EINVAL se já foi juntada,
    /// EDEADLK se for a própria.
    fn join_thread(&self, tid: Tid) -> SysResult<()>;
    /// Reserva uma vaga de tarefa para uma thread que o programa mantém por conta própria, sem thread do SO (as
    /// threads do `_thread` do interpretador Python são pilhas de quadros da VM): conta no `RLIMIT_NPROC` do usuário
    /// como qualquer thread de `clone`. EAGAIN quando o limite vale (não é root) e o usuário já o atingiu.
    fn reserve_task(&self) -> SysResult<()>;
    /// Devolve uma vaga de [`Syscalls::reserve_task`] (a thread acabou). Sem vaga reservada, não faz nada.
    fn release_task(&self);
    fn gettid(&self) -> Tid;
    /// CPUs em que o processo pode rodar (`sched_getaffinity`): as CPUs virtuais do sandbox.
    fn sched_getaffinity(&self) -> Vec<usize>;
    /// `sched_getaffinity(pid, ...)`: a máscara do processo `pid` (0 é o corrente), só com CPUs online.
    /// ESRCH se o processo não existe.
    fn sched_getaffinity_of(&self, pid: Pid) -> SysResult<Vec<usize>> {
        let _ = pid;
        Err(Errno::ENOSYS)
    }
    /// `sched_setaffinity(pid, mask)`. A máscara é a lista de CPUs; as que não estão online são
    /// ignoradas e, se não sobra nenhuma, EINVAL. ESRCH se o processo não existe, EPERM se é de outro
    /// dono (o contêiner padrão não tem `CAP_SYS_NICE`). O filho herda a máscara no fork.
    fn sched_setaffinity(&self, pid: Pid, cpus: &[usize]) -> SysResult<()> {
        let _ = (pid, cpus);
        Err(Errno::ENOSYS)
    }
    /// `personality(2)`: devolve a personality de antes; `0xffffffff` só lê. É por processo, herdada no
    /// fork e no exec (o exec setuid apaga `PER_CLEAR_ON_SETID`). Segue o seccomp padrão do docker: só
    /// 0x0, 0x8, 0x20000, 0x20008 e a leitura passam, o resto é EPERM. `uname` reflete `PER_LINUX32`
    /// (`machine` vira `i686`) e `UNAME26` (`release` vira `2.6.N`).
    fn personality(&self, persona: u32) -> SysResult<u32> {
        let _ = persona;
        Err(Errno::ENOSYS)
    }
    /// `sched_getscheduler`: a política, com `SCHED_RESET_ON_FORK` se ligado. EINVAL pra pid negativo.
    fn sched_getscheduler(&self, pid: Pid) -> SysResult<i32> {
        let _ = pid;
        Err(Errno::ENOSYS)
    }
    /// `sched_setscheduler`: política (0 OTHER, 1 FIFO, 2 RR, 3 BATCH, 5 IDLE, com `SCHED_RESET_ON_FORK`
    /// opcional) e prioridade. EINVAL de forma, ESRCH, e EPERM sem `CAP_SYS_NICE` pra tempo real e outras
    /// escaladas (ver [`crate::sched`]).
    fn sched_setscheduler(&self, pid: Pid, policy: i32, param: SchedParam) -> SysResult<()> {
        let _ = (pid, policy, param);
        Err(Errno::ENOSYS)
    }
    /// `sched_getparam`.
    fn sched_getparam(&self, pid: Pid) -> SysResult<SchedParam> {
        let _ = pid;
        Err(Errno::ENOSYS)
    }
    /// `sched_setparam`: muda só a prioridade, a política fica.
    fn sched_setparam(&self, pid: Pid, param: SchedParam) -> SysResult<()> {
        let _ = (pid, param);
        Err(Errno::ENOSYS)
    }
    /// `sched_rr_get_interval`: a fatia (100 ms em RR, 0 em FIFO).
    fn sched_rr_get_interval(&self, pid: Pid) -> SysResult<Duration> {
        let _ = pid;
        Err(Errno::ENOSYS)
    }
    /// `sched_getattr(pid, attr, size, flags)`: `size` entre 48 e 4096 e `flags` 0, senão EINVAL.
    fn sched_getattr(&self, pid: Pid, size: u32, flags: u32) -> SysResult<SchedAttr> {
        let _ = (pid, size, flags);
        Err(Errno::ENOSYS)
    }
    /// `sched_setattr(pid, attr, flags)`: `attr.size` abaixo de 48 é E2BIG (0 vale 48), `flags` não zero
    /// é EINVAL.
    fn sched_setattr(&self, pid: Pid, attr: &SchedAttr, flags: u32) -> SysResult<()> {
        let _ = (pid, attr, flags);
        Err(Errno::ENOSYS)
    }
    /// `ioprio_get(which, who)`: `which` é `IOPRIO_WHO_PROCESS` (1), `PGRP` (2) ou `USER` (3), `who` 0
    /// é o corrente. Devolve `(classe << 13) | dados`; sem valor gravado, a classe BE com nível
    /// `(nice + 20) / 5` (IDLE ou RT se a política do processo for essa). ESRCH sem alvo.
    fn ioprio_get(&self, which: i32, who: i32) -> SysResult<i32> {
        let _ = (which, who);
        Err(Errno::ENOSYS)
    }
    /// `ioprio_set(which, who, ioprio)`. EINVAL pra classe ou nível inválido, EPERM pra RT (sem
    /// `CAP_SYS_ADMIN`) e pra processo de outro dono.
    fn ioprio_set(&self, which: i32, who: i32, ioprio: i32) -> SysResult<()> {
        let _ = (which, who, ioprio);
        Err(Errno::ENOSYS)
    }

    // ---- identidade e ambiente ----
    fn getuid(&self) -> Uid;
    fn geteuid(&self) -> Uid;
    fn getgid(&self) -> Gid;
    fn getegid(&self) -> Gid;
    fn getgroups(&self) -> Vec<Gid>;
    /// `getresuid`: (real, efetivo, salvo).
    fn getresuid(&self) -> (Uid, Uid, Uid) {
        let (r, e) = (self.getuid(), self.geteuid());
        (r, e, e)
    }
    /// `getresgid`: (real, efetivo, salvo).
    fn getresgid(&self) -> (Gid, Gid, Gid) {
        let (r, e) = (self.getgid(), self.getegid());
        (r, e, e)
    }
    /// `setresuid(r, e, s)`; [`ID_UNCHANGED`] (o `-1` do C) mantém o valor.
    fn setresuid(&self, r: Uid, e: Uid, s: Uid) -> SysResult<()> {
        let _ = (r, e, s);
        Err(Errno::EPERM)
    }
    fn setresgid(&self, r: Gid, e: Gid, s: Gid) -> SysResult<()> {
        let _ = (r, e, s);
        Err(Errno::EPERM)
    }
    /// `setreuid(r, e)` com a regra do salvo do Linux.
    fn setreuid(&self, r: Uid, e: Uid) -> SysResult<()> {
        let _ = (r, e);
        Err(Errno::EPERM)
    }
    fn setregid(&self, r: Gid, e: Gid) -> SysResult<()> {
        let _ = (r, e);
        Err(Errno::EPERM)
    }
    /// `setuid(uid)`: com `CAP_SETUID` troca real, efetivo e salvo; sem, só o efetivo (para o real ou
    /// o salvo).
    fn setuid(&self, uid: Uid) -> SysResult<()> {
        let _ = uid;
        Err(Errno::EPERM)
    }
    fn setgid(&self, gid: Gid) -> SysResult<()> {
        let _ = gid;
        Err(Errno::EPERM)
    }
    /// `setgroups`: exige `CAP_SETGID`; mais de `NGROUPS_MAX` (65536) dá EINVAL.
    fn setgroups(&self, groups: &[Gid]) -> SysResult<()> {
        let _ = groups;
        Err(Errno::EPERM)
    }
    /// argv do processo (o mesmo passado ao `main`).
    fn argv(&self) -> Vec<Vec<u8>>;
    /// Ambiente do processo, `NAME=valor`.
    fn environ(&self) -> Vec<Vec<u8>>;
    fn getenv(&self, name: &[u8]) -> Option<Vec<u8>>;
    fn setenv(&self, name: &[u8], value: &[u8]) -> SysResult<()>;
    fn unsetenv(&self, name: &[u8]) -> SysResult<()>;
    fn uname(&self) -> Utsname;
    fn sethostname(&self, name: &[u8]) -> SysResult<()>;
    /// `setdomainname(2)`: o domínio NIS do `uname`.
    fn setdomainname(&self, name: &[u8]) -> SysResult<()>;

    // ---- tempo e aleatoriedade ----
    fn clock_gettime(&self, clock: Clock) -> SysResult<TimeSpec>;
    /// EINTR se um sinal capturado chegar antes do fim.
    fn nanosleep(&self, d: Duration) -> SysResult<()>;
    /// `setitimer(2)`: rearma o relógio `which` (`ITIMER_REAL` 0, `ITIMER_VIRTUAL` 1, `ITIMER_PROF` 2) e devolve o
    /// que ele tinha. O timer é do processo: o filho do `fork` nasce sem eles e o `execve` os preserva. Vence com
    /// `SIGALRM`, `SIGVTALRM` ou `SIGPROF`. EINVAL com `which` desconhecido ou `timeval` inválido (`usec` fora de
    /// `0..1_000_000`, ou algum campo negativo).
    fn setitimer(&self, which: i32, new: Itimerval) -> SysResult<Itimerval> {
        let _ = (which, new);
        Err(Errno::ENOSYS)
    }
    /// `getitimer(2)`: o que falta para o relógio `which` vencer e o intervalo dele.
    fn getitimer(&self, which: i32) -> SysResult<Itimerval> {
        let _ = which;
        Err(Errno::ENOSYS)
    }
    /// `alarm(2)`: um `SIGALRM` daqui a `seconds` (0 cancela; o excedente de `INT_MAX` é cortado) e os segundos que
    /// faltavam do alarme anterior (ver [`Itimerval::alarm_remaining`]). É o `setitimer(ITIMER_REAL)` sem intervalo.
    fn alarm(&self, seconds: u32) -> SysResult<u32> {
        let seconds = i64::from(seconds.min(i32::MAX as u32));
        Ok(self.setitimer(Itimer::Real as i32, Itimerval::oneshot(seconds))?.alarm_remaining())
    }
    fn getrandom(&self, buf: &mut [u8]) -> SysResult<usize>;
    /// Fuso local do sandbox (conteúdo de `/etc/localtime` ou `TZ`), pra ferramentas de data.
    fn local_timezone(&self) -> Vec<u8>;

    // ---- espera e travas ----
    /// `poll(2)` em qualquer fd (socket, pipe, arquivo). `None` espera sem limite. EINTR com sinal
    /// capturado. Devolve quantas entradas têm `revents` não vazio.
    fn poll(&self, fds: &mut [PollFd], timeout: Option<Duration>) -> SysResult<usize>;
    /// `epoll_create1(2)`: um fd de epoll (`anon_inode:[eventpoll]`).
    fn epoll_create1(&self, cloexec: bool) -> SysResult<Fd> {
        let _ = cloexec;
        Err(Errno::ENOSYS)
    }
    /// `pidfd_open(2)`: um fd (`anon_inode:[pidfd]`) que fica legível quando o processo termina. `flags` aceita
    /// `O_NONBLOCK` (`PIDFD_NONBLOCK`) e `0o200` (`PIDFD_THREAD`). ESRCH se o pid não existe, ENOENT se é
    /// de uma thread que não é líder e `PIDFD_THREAD` não veio.
    fn pidfd_open(&self, pid: Pid, flags: u32) -> SysResult<Fd> {
        let _ = (pid, flags);
        Err(Errno::ENOSYS)
    }
    /// `chroot(2)`: a raiz do processo passa a ser o diretório `path`. EPERM para quem não é root (o contêiner
    /// padrão do docker dá `CAP_SYS_CHROOT` ao root).
    fn chroot(&self, path: &[u8]) -> SysResult<()> {
        let _ = path;
        Err(Errno::ENOSYS)
    }
    /// `eventfd2(2)`: um contador de 64 bits atrás de um fd (`anon_inode:[eventfd]`). `flags` aceita
    /// `EFD_SEMAPHORE` (1), `EFD_NONBLOCK` (`O_NONBLOCK`) e `EFD_CLOEXEC` (`O_CLOEXEC`); outro bit é EINVAL.
    /// `read` e `write` movem 8 bytes (EINVAL com menos).
    fn eventfd(&self, initval: u32, flags: u32) -> SysResult<Fd> {
        let _ = (initval, flags);
        Err(Errno::ENOSYS)
    }
    /// `timerfd_create(2)`: `clock` é `CLOCK_REALTIME` (0), `CLOCK_MONOTONIC` (1), `CLOCK_BOOTTIME` (7),
    /// `CLOCK_REALTIME_ALARM` (8) ou `CLOCK_BOOTTIME_ALARM` (9); `flags` aceita `TFD_NONBLOCK` e `TFD_CLOEXEC`.
    fn timerfd_create(&self, clock: i32, flags: u32) -> SysResult<Fd> {
        let _ = (clock, flags);
        Err(Errno::ENOSYS)
    }
    /// `timerfd_settime(2)` em nanossegundos: `value_ns` zero desarma; `flags` leva `TFD_TIMER_ABSTIME` (1) e
    /// `TFD_TIMER_CANCEL_ON_SET` (2). Devolve o que faltava e o intervalo de antes.
    fn timerfd_settime(&self, fd: Fd, flags: u32, value_ns: u64, interval_ns: u64) -> SysResult<(u64, u64)> {
        let _ = (fd, flags, value_ns, interval_ns);
        Err(Errno::ENOSYS)
    }
    /// `timerfd_gettime(2)`: o que falta para vencer e o intervalo, em nanossegundos.
    fn timerfd_gettime(&self, fd: Fd) -> SysResult<(u64, u64)> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }
    /// `memfd_create(2)`: um arquivo anônimo na memória, visível em `/proc/<pid>/fd` como `/memfd:NOME (deleted)`.
    /// `flags` aceita `MFD_CLOEXEC` (1) e `MFD_ALLOW_SEALING` (2); `MFD_HUGETLB` é EINVAL nesta máquina.
    fn memfd_create(&self, name: &[u8], flags: u32) -> SysResult<Fd> {
        let _ = (name, flags);
        Err(Errno::ENOSYS)
    }
    /// `getxattr`/`lgetxattr`/`fgetxattr`: o valor do atributo `name`. `AtFlags::EMPTY_PATH` com `path` vazio é
    /// o `fgetxattr` de `dirfd`; `SYMLINK_NOFOLLOW` é o `lgetxattr`. ENODATA sem o atributo.
    fn getxattr(&self, dirfd: Fd, path: &[u8], flags: AtFlags, name: &[u8]) -> SysResult<Vec<u8>> {
        let _ = (dirfd, path, flags, name);
        Err(Errno::ENOSYS)
    }
    /// `setxattr` e variantes; `xflags` é `XATTR_CREATE` (1) ou `XATTR_REPLACE` (2).
    fn setxattr(&self, dirfd: Fd, path: &[u8], flags: AtFlags, name: &[u8], value: &[u8], xflags: u32) -> SysResult<()> {
        let _ = (dirfd, path, flags, name, value, xflags);
        Err(Errno::ENOSYS)
    }
    /// `listxattr` e variantes: os nomes, na ordem em que o sistema de arquivos os guarda.
    fn listxattr(&self, dirfd: Fd, path: &[u8], flags: AtFlags) -> SysResult<Vec<Vec<u8>>> {
        let _ = (dirfd, path, flags);
        Err(Errno::ENOSYS)
    }
    /// `removexattr` e variantes. ENODATA sem o atributo.
    fn removexattr(&self, dirfd: Fd, path: &[u8], flags: AtFlags, name: &[u8]) -> SysResult<()> {
        let _ = (dirfd, path, flags, name);
        Err(Errno::ENOSYS)
    }
    /// `epoll_ctl(2)`: `op` é `epoll::CTL_ADD`, `CTL_DEL` ou `CTL_MOD`; `event` é ignorado em `CTL_DEL`.
    fn epoll_ctl(&self, epfd: Fd, op: i32, fd: Fd, event: EpollEvent) -> SysResult<()> {
        let _ = (epfd, op, fd, event);
        Err(Errno::ENOSYS)
    }
    /// `epoll_wait(2)`: até `max` eventos prontos; `None` espera sem limite, e o prazo vencido dá lista vazia.
    /// EINTR com sinal capturado.
    fn epoll_wait(&self, epfd: Fd, max: usize, timeout: Option<Duration>) -> SysResult<Vec<EpollEvent>> {
        let _ = (epfd, max, timeout);
        Err(Errno::ENOSYS)
    }
    /// `F_OFD_SETLK`/`F_OFD_SETLKW`: o dono da trava é a open file description, e ela solta quando o
    /// último fd que a referencia fecha. `wait = false` dá EAGAIN em conflito; `wait = true` bloqueia
    /// (EINTR com sinal capturado, EDEADLK em impasse).
    fn ofd_setlk(&self, fd: Fd, lock: FileLock, wait: bool) -> SysResult<()>;
    /// `F_OFD_GETLK`: a primeira trava de outro dono que conflita com `lock`, ou `None`.
    fn ofd_getlk(&self, fd: Fd, lock: FileLock) -> SysResult<Option<FileLock>>;
    /// `fcntl(F_GETLK | F_SETLK | F_SETLKW | F_OFD_*)` com o `struct flock` do programa: valida e converte
    /// `whence` e `start`/`len` como o `flock_to_posix_lock` do Linux, e devolve o `struct flock` de volta (o
    /// que `F_GETLK` preenche com a trava que conflita, ou `F_UNLCK` quando nenhuma).
    fn fcntl_lock(&self, fd: Fd, cmd: LockCmd, flock: Flock) -> SysResult<Flock> {
        let _ = (fd, cmd, flock);
        Err(Errno::ENOSYS)
    }
    /// `flock(2)`: `op` é `LOCK_SH`, `LOCK_EX` ou `LOCK_UN`, com `LOCK_NB` somado ([`crate::fcntl`]).
    fn flock(&self, fd: Fd, op: u32) -> SysResult<()> {
        let _ = (fd, op);
        Err(Errno::ENOSYS)
    }
    /// `fcntl(F_GETPIPE_SZ)`: a capacidade do pipe em bytes. EBADF se o fd não é um pipe.
    fn pipe_size(&self, fd: Fd) -> SysResult<usize> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }
    /// `fcntl(F_SETPIPE_SZ)`: muda a capacidade (arredondada à potência de 2 que cobre o pedido, no mínimo
    /// uma página) e devolve a nova. EPERM acima de `pipe-max-size` sem privilégio, EBUSY se o pipe tem mais
    /// dados do que cabe, EINVAL se o tamanho não cabe em 2^31.
    fn set_pipe_size(&self, fd: Fd, size: u32) -> SysResult<usize> {
        let _ = (fd, size);
        Err(Errno::ENOSYS)
    }
    /// `ioctl(FIONREAD)`: os bytes que um `read` leria agora sem esperar.
    fn fionread(&self, fd: Fd) -> SysResult<i64> {
        let _ = fd;
        Err(Errno::ENOSYS)
    }

    // ---- rede (resolução, política de allowlist e conexão ficam no kernel) ----
    /// Conexão TCP. `timeout` cobre resolução e conexão. Erros: ENOENT = nome não resolve; EACCES = a
    /// política negou (nome fora da allowlist ou endereço interno); ECONNREFUSED, ETIMEDOUT,
    /// ENETUNREACH, EHOSTUNREACH como no `connect(2)`.
    fn net_connect(&self, host: &[u8], port: u16, timeout: Option<Duration>) -> SysResult<NetConn>;
}

thread_local! {
    static CURRENT: RefCell<Option<Arc<dyn Syscalls>>> = const { RefCell::new(None) };
}

/// Instala o processo corrente da thread. Só o kernel (e o testkit) chamam isto.
pub fn install(sys: Arc<dyn Syscalls>) -> Option<Arc<dyn Syscalls>> {
    CURRENT.with(|c| c.borrow_mut().replace(sys))
}

/// Remove o processo corrente da thread.
pub fn uninstall() -> Option<Arc<dyn Syscalls>> {
    CURRENT.with(|c| c.borrow_mut().take())
}

/// O processo corrente. Panic se a thread não pertence a um pseudo-processo (bug do chamador).
pub fn current() -> Arc<dyn Syscalls> {
    CURRENT.with(|c| c.borrow().clone()).expect("thread sem pseudo-processo instalado")
}

pub fn try_current() -> Option<Arc<dyn Syscalls>> {
    CURRENT.with(|c| c.borrow().clone())
}

/// Payload de unwind de `exit`: o kernel captura na entrada do processo e termina com esse código.
#[derive(Debug)]
pub struct ExitUnwind(pub i32);

/// Payload de unwind de morte por sinal (SIGKILL, ou ação padrão de terminar).
#[derive(Debug)]
pub struct KillUnwind(pub Signal);

/// Payload de unwind de `execve` bem-sucedido: o kernel troca o programa no mesmo processo.
pub struct ExecUnwind {
    pub path: Vec<u8>,
    pub argv: Vec<Vec<u8>>,
    pub env: Option<Vec<Vec<u8>>>,
}

/// Termina o processo corrente com `code` (`_exit`). Desenrola a pilha, então os `Drop` rodam.
pub fn exit(code: i32) -> ! {
    std::panic::resume_unwind(Box::new(ExitUnwind(code)))
}

// ---- atalhos de uso comum, sobre o processo corrente ----

pub fn open(path: &[u8], flags: OFlags, mode: Mode) -> SysResult<Fd> {
    current().openat(Fd::CWD, path, flags, mode)
}

pub fn close(fd: Fd) -> SysResult<()> {
    current().close(fd)
}

pub fn read(fd: Fd, buf: &mut [u8]) -> SysResult<usize> {
    current().read(fd, buf)
}

pub fn write(fd: Fd, buf: &[u8]) -> SysResult<usize> {
    current().write(fd, buf)
}

/// Escreve tudo, repetindo em escrita parcial; EINTR é repetido.
pub fn write_all(fd: Fd, mut buf: &[u8]) -> SysResult<()> {
    let sys = current();
    while !buf.is_empty() {
        match sys.write(fd, buf) {
            Ok(0) => return Err(Errno::EIO),
            Ok(n) => buf = &buf[n..],
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Lê até o fim.
pub fn read_to_end(fd: Fd) -> SysResult<Vec<u8>> {
    let sys = current();
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        match sys.read(fd, &mut buf) {
            Ok(0) => return Ok(out),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
}

/// `fallocate(2)` sobre o processo corrente.
pub fn fallocate(fd: Fd, mode: FallocFlags, offset: i64, len: i64) -> SysResult<()> {
    current().fallocate(fd, mode, offset, len)
}

/// `personality(2)` sobre o processo corrente; `0xffffffff` só lê.
pub fn personality(persona: u32) -> SysResult<u32> {
    current().personality(persona)
}

pub fn sched_getscheduler(pid: Pid) -> SysResult<i32> {
    current().sched_getscheduler(pid)
}

pub fn sched_setscheduler(pid: Pid, policy: i32, param: SchedParam) -> SysResult<()> {
    current().sched_setscheduler(pid, policy, param)
}

pub fn sched_getparam(pid: Pid) -> SysResult<SchedParam> {
    current().sched_getparam(pid)
}

pub fn sched_setparam(pid: Pid, param: SchedParam) -> SysResult<()> {
    current().sched_setparam(pid, param)
}

pub use crate::sched::{priority_max as sched_get_priority_max, priority_min as sched_get_priority_min};

pub fn sched_rr_get_interval(pid: Pid) -> SysResult<Duration> {
    current().sched_rr_get_interval(pid)
}

pub fn sched_getattr(pid: Pid, size: u32, flags: u32) -> SysResult<SchedAttr> {
    current().sched_getattr(pid, size, flags)
}

pub fn sched_setattr(pid: Pid, attr: &SchedAttr, flags: u32) -> SysResult<()> {
    current().sched_setattr(pid, attr, flags)
}

/// `sched_getaffinity(pid)`: a máscara do processo `pid` (0 é o corrente).
pub fn sched_getaffinity_of(pid: Pid) -> SysResult<Vec<usize>> {
    current().sched_getaffinity_of(pid)
}

pub fn sched_setaffinity(pid: Pid, cpus: &[usize]) -> SysResult<()> {
    current().sched_setaffinity(pid, cpus)
}

pub fn ioprio_get(which: i32, who: i32) -> SysResult<i32> {
    current().ioprio_get(which, who)
}

pub fn ioprio_set(which: i32, who: i32, ioprio: i32) -> SysResult<()> {
    current().ioprio_set(which, who, ioprio)
}

pub fn tcgetattr(fd: Fd) -> SysResult<Termios> {
    current().tcgetattr(fd)
}

pub fn tcsetattr(fd: Fd, when: SetAttrWhen, termios: &Termios) -> SysResult<()> {
    current().tcsetattr(fd, when, termios)
}

pub fn tcgetwinsize(fd: Fd) -> SysResult<Winsize> {
    current().tcgetwinsize(fd)
}

pub fn tcsetwinsize(fd: Fd, ws: Winsize) -> SysResult<()> {
    current().tcsetwinsize(fd, ws)
}

pub fn tcgetpgrp(fd: Fd) -> SysResult<Pid> {
    current().tcgetpgrp(fd)
}

/// Gera as funções livres que repassam à syscall homônima do processo corrente: o contrato do socket
/// é o mesmo para TCP, UDP e Unix, e a única coisa que muda entre elas é a assinatura.
macro_rules! current_syscalls {
    ($($(#[$meta:meta])* fn $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        $($(#[$meta])* pub fn $name($($arg: $ty),*) -> $ret {
            current().$name($($arg),*)
        })*
    };
}

current_syscalls! {
    fn tcp_listen(port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)>;
    fn tcp_accept(fd: Fd, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)>;
    fn tcp_connect(port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)>;
    fn tcp_listen_at(ip: std::net::IpAddr, port: u16, backlog: u32, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)>;
    fn tcp_bind(ip: std::net::IpAddr, port: u16, reuse_addr: bool, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)>;
    fn tcp_connect_bound(fd: Fd, ip: std::net::IpAddr, port: u16) -> SysResult<u16>;
    fn tcp_listen_bound(fd: Fd, backlog: u32) -> SysResult<()>;
    fn tcp_connect_at(ip: std::net::IpAddr, port: u16, nonblock: bool, cloexec: bool) -> SysResult<(Fd, u16)>;
    fn tcp_shutdown(fd: Fd, read: bool, write: bool) -> SysResult<()>;
    fn tcp_ports(fd: Fd) -> SysResult<(u16, Option<u16>)>;
    fn tcp_socket(v6: bool, nonblock: bool, cloexec: bool) -> SysResult<Fd>;
    fn tcp_bind_fd(fd: Fd, ip: std::net::IpAddr, port: u16, reuse_addr: bool) -> SysResult<u16>;
    fn tcp_connect_fd(fd: Fd, ip: std::net::IpAddr, port: u16) -> SysResult<()>;
    #[allow(clippy::type_complexity)]
    fn tcp_names(fd: Fd) -> SysResult<((std::net::IpAddr, u16), Option<(std::net::IpAddr, u16)>)>;
    fn sock_info(fd: Fd) -> SysResult<(i32, i32, i32, bool)>;
    fn sock_error(fd: Fd) -> SysResult<i32>;
    fn sock_setopt(fd: Fd, level: i32, name: i32, value: &[u8]) -> SysResult<()>;
    fn sock_getopt(fd: Fd, level: i32, name: i32) -> SysResult<Option<Vec<u8>>>;
    fn sock_recv(fd: Fd, max: usize, flags: MsgFlags) -> SysResult<Vec<u8>>;
    fn sock_send(fd: Fd, buf: &[u8], flags: MsgFlags) -> SysResult<usize>;
    fn unix_socket(ty: u8, nonblock: bool, cloexec: bool) -> SysResult<Fd>;
    fn udp_socket(v6: bool, nonblock: bool, cloexec: bool) -> SysResult<Fd>;
    fn udp_bind(fd: Fd, ip: std::net::IpAddr, port: u16, reuse: bool) -> SysResult<u16>;
    fn udp_connect(fd: Fd, ip: std::net::IpAddr, port: u16) -> SysResult<()>;
    fn udp_sendto(fd: Fd, data: &[u8], dst: Option<(std::net::IpAddr, u16)>) -> SysResult<usize>;
    fn udp_recvfrom(fd: Fd, max: usize, peek: bool) -> SysResult<(Vec<u8>, std::net::IpAddr, u16)>;
    #[allow(clippy::type_complexity)]
    fn udp_names(fd: Fd) -> SysResult<((std::net::IpAddr, u16), Option<(std::net::IpAddr, u16)>)>;
    fn unix_socketpair(ty: u8, nonblock: bool, cloexec: bool) -> SysResult<(Fd, Fd)>;
    fn unix_bind(fd: Fd, name: &[u8]) -> SysResult<()>;
    fn unix_listen(fd: Fd, backlog: u32) -> SysResult<()>;
    fn unix_accept(fd: Fd, nonblock: bool, cloexec: bool) -> SysResult<Fd>;
    fn unix_connect(fd: Fd, name: &[u8]) -> SysResult<()>;
    fn unix_names(fd: Fd) -> SysResult<(Option<Vec<u8>>, Option<Vec<u8>>, bool)>;
    fn unix_sendto(fd: Fd, data: &[u8], name: Option<&[u8]>) -> SysResult<usize>;
    fn unix_recvfrom(fd: Fd, max: usize, peek: bool) -> SysResult<(Vec<u8>, Option<Vec<u8>>)>;
    fn unix_sendmsg(fd: Fd, data: &[u8], name: Option<&[u8]>, control: &[u8], flags: MsgFlags) -> SysResult<usize>;
    fn unix_recvmsg(fd: Fd, max: usize, control_len: usize, flags: MsgFlags) -> SysResult<RecvMsg>;
}

pub fn tcsetpgrp(fd: Fd, pgrp: Pid) -> SysResult<()> {
    current().tcsetpgrp(fd, pgrp)
}

pub fn tcgetsid(fd: Fd) -> SysResult<Pid> {
    current().tcgetsid(fd)
}

/// `posix_openpt` da glibc: `open("/dev/ptmx", flags)`.
pub fn posix_openpt(flags: OFlags) -> SysResult<Fd> {
    open(b"/dev/ptmx", flags, 0)
}

/// `grantpt` da glibc no Linux: o devpts já cria o nó com o dono, o grupo `tty` e o modo certos, então
/// só confere que o fd é um mestre (EINVAL se não é).
pub fn grantpt(fd: Fd) -> SysResult<()> {
    match current().pty_number(fd) {
        Ok(_) => Ok(()),
        Err(Errno::ENOTTY) => Err(Errno::EINVAL),
        Err(e) => Err(e),
    }
}

/// `unlockpt`: `TIOCSPTLCK` com 0 (EINVAL se o fd não é mestre).
pub fn unlockpt(fd: Fd) -> SysResult<()> {
    match current().pty_set_lock(fd, false) {
        Err(Errno::ENOTTY) => Err(Errno::EINVAL),
        r => r,
    }
}

/// `ptsname`: `/dev/pts/N` pelo `TIOCGPTN` (ENOTTY se o fd não é mestre).
pub fn ptsname(fd: Fd) -> SysResult<Vec<u8>> {
    let n = current().pty_number(fd)?;
    Ok(format!("/dev/pts/{n}").into_bytes())
}

pub fn stat(path: &[u8]) -> SysResult<Stat> {
    current().fstatat(Fd::CWD, path, AtFlags::empty())
}

pub fn lstat(path: &[u8]) -> SysResult<Stat> {
    current().fstatat(Fd::CWD, path, AtFlags::SYMLINK_NOFOLLOW)
}

pub fn getenv(name: &str) -> Option<Vec<u8>> {
    current().getenv(name.as_bytes())
}

pub fn checkpoint() {
    if let Some(sys) = try_current() {
        sys.checkpoint();
    }
}

/// Lê um arquivo inteiro.
pub fn read_file(path: &[u8]) -> SysResult<Vec<u8>> {
    let fd = open(path, OFlags::RDONLY | OFlags::CLOEXEC, 0)?;
    let data = read_to_end(fd);
    let _ = close(fd);
    data
}

/// Lista um diretório inteiro (sem `.` e `..`), na ordem do sistema de arquivos.
pub fn read_dir(path: &[u8]) -> SysResult<Vec<DirEntry>> {
    let sys = current();
    let fd = sys.openat(Fd::CWD, path, OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC, 0)?;
    let mut out = Vec::new();
    loop {
        match sys.getdents(fd) {
            Ok(batch) if batch.is_empty() => break,
            Ok(batch) => out.extend(batch.into_iter().filter(|e| e.name != b"." && e.name != b"..")),
            Err(e) => {
                let _ = sys.close(fd);
                return Err(e);
            }
        }
    }
    let _ = sys.close(fd);
    Ok(out)
}
