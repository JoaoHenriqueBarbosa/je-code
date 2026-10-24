//! O `OsHost` contra o sistema operacional real: arquivos, rede de loopback e processos filhos.

use std::time::Duration;

use je_exthost::os_host::OsHost;
use zjsc::runtime::host::{FsTime, Host, NetWatch, ProcExit, SpawnRequest, SpawnStdio};

const EAGAIN: i32 = 11;
const ENOENT: i32 = 2;
const EBADF: i32 = 9;
const ECONNREFUSED: i32 = 111;
const O_RDONLY: u32 = 0;
const O_RDWR: u32 = 2;
const O_CREAT: u32 = 0o100;
const O_EXCL: u32 = 0o200;
const S_IFDIR: u32 = 0o040000;

fn path(dir: &tempfile::TempDir, name: &str) -> String {
    dir.path().join(name).to_string_lossy().into_owned()
}

fn wait_readable(host: &OsHost, fd: i32) {
    let ready = host.net_poll(&[NetWatch { fd, read: true, write: false }], Some(Duration::from_secs(10))).unwrap();
    assert!(ready[0].readable, "o descritor {fd} devia ficar legível em até 10 s");
}

/// Lê até o fim do fluxo.
fn drain(host: &OsHost, fd: i32) -> Vec<u8> {
    let mut all = Vec::new();
    loop {
        wait_readable(host, fd);
        let chunk = host.net_recv(fd, 4096).unwrap();
        if chunk.is_empty() {
            return all;
        }
        all.extend(chunk);
    }
}

#[test]
fn file_roundtrip_with_descriptors() {
    let dir = tempfile::tempdir().unwrap();
    let file = path(&dir, "a.txt");
    let host = OsHost::new();
    let fd = host.fs_open(&file, O_RDWR | O_CREAT, 0o644).unwrap();
    assert!(fd >= 3, "descritor virtual a partir de 3");
    assert_eq!(host.fs_write(fd, b"hello world", None).unwrap(), 11);
    assert_eq!(host.fs_read(fd, 5, Some(6)).unwrap(), b"world");
    assert_eq!(host.fs_fstat(fd).unwrap().size, 11);
    host.fs_ftruncate(fd, 5).unwrap();
    host.fs_close(fd).unwrap();
    assert_eq!(host.read_file(&file).unwrap(), b"hello");
    assert_eq!(host.fs_close(fd), Err(EBADF), "fechar de novo é EBADF");
}

#[test]
fn open_exclusive_and_missing_file_report_errno() {
    let dir = tempfile::tempdir().unwrap();
    let file = path(&dir, "x");
    let host = OsHost::new();
    let fd = host.fs_open(&file, O_RDWR | O_CREAT | O_EXCL, 0o600).unwrap();
    host.fs_close(fd).unwrap();
    assert_eq!(host.fs_open(&file, O_RDWR | O_CREAT | O_EXCL, 0o600), Err(17), "EEXIST");
    assert_eq!(host.fs_open(&path(&dir, "nao-existe"), O_RDONLY, 0), Err(ENOENT));
    assert_eq!(host.read_file(&path(&dir, "nao-existe")), Err(ENOENT));
}

#[test]
fn directory_operations() {
    let dir = tempfile::tempdir().unwrap();
    let host = OsHost::new();
    let sub = path(&dir, "sub");
    host.fs_mkdir(&sub, 0o755).unwrap();
    assert!(host.stat(&sub).unwrap().is_dir);
    assert_eq!(host.fs_stat(&sub, true).unwrap().mode & 0o170000, S_IFDIR);
    let a = format!("{sub}/a");
    let b = format!("{sub}/b");
    let created = host.fs_open(&a, O_RDWR | O_CREAT, 0o644).unwrap();
    host.fs_close(created).unwrap();
    host.fs_rename(&a, &b).unwrap();
    let names: Vec<_> = host.fs_readdir(&sub).unwrap().into_iter().map(|entry| String::from_utf8(entry.name).unwrap()).collect();
    assert_eq!(names, ["b"]);
    host.fs_access(&b, 4).unwrap();
    assert_eq!(host.fs_access(&a, 0), Err(ENOENT));
    assert_eq!(host.fs_realpath(&b).unwrap(), std::fs::canonicalize(&b).unwrap().to_string_lossy());
    assert_eq!(host.fs_rmdir(&sub), Err(39), "ENOTEMPTY");
    host.fs_unlink(&b).unwrap();
    host.fs_rmdir(&sub).unwrap();
}

#[cfg(unix)]
#[test]
fn links_permissions_and_times() {
    let dir = tempfile::tempdir().unwrap();
    let host = OsHost::new();
    let file = path(&dir, "f");
    let link = path(&dir, "l");
    std::fs::write(&file, b"x").unwrap();
    host.fs_symlink(&file, &link).unwrap();
    assert_eq!(host.fs_readlink(&link).unwrap(), file);
    assert_eq!(host.fs_stat(&link, false).unwrap().mode & 0o170000, 0o120000, "lstat vê a ligação");
    host.fs_chmod(&file, 0o600).unwrap();
    assert_eq!(host.fs_stat(&file, true).unwrap().mode & 0o777, 0o600);
    host.fs_utimes(&file, FsTime::At(1_000_000_000, 5), FsTime::At(1_500_000_000, 7), true).unwrap();
    let stat = host.fs_stat(&file, true).unwrap();
    assert_eq!((stat.atime, stat.mtime), ((1_000_000_000, 5), (1_500_000_000, 7)));
    host.fs_utimes(&file, FsTime::Omit, FsTime::At(1_600_000_000, 0), true).unwrap();
    let stat = host.fs_stat(&file, true).unwrap();
    assert_eq!((stat.atime.0, stat.mtime.0), (1_000_000_000, 1_600_000_000), "Omit deixa o atime como está");
    let hard = path(&dir, "h");
    host.fs_link(&file, &hard).unwrap();
    assert_eq!(host.fs_stat(&file, true).unwrap().nlink, 2);
    // Trocar o dono para o próprio dono é permitido sem privilégio; `u32::MAX` mantém o valor.
    let me = host.fs_stat(&file, true).unwrap();
    host.fs_chown(&file, me.uid, u32::MAX, true).unwrap();
}

#[test]
fn environment_and_process_facts() {
    let host = OsHost::new();
    assert_eq!(host.pid(), std::process::id());
    assert!(host.cpus() >= 1);
    assert!(host.environ().iter().any(|(key, _)| key == "PATH"));
    assert_eq!(host.getenv("PATH"), std::env::var("PATH").ok());
    let mut bytes = [0u8; 32];
    host.random_fill(&mut bytes);
    assert!(bytes.iter().any(|byte| *byte != 0), "32 bytes de entropia não saem todos zero");
}

#[test]
fn loopback_echo_between_listener_and_client() {
    let host = OsHost::new();
    let (listener, port) = host.net_listen(0, 16).unwrap();
    assert_ne!(port, 0, "a porta 0 sorteia uma efêmera");
    assert_eq!(host.net_accept(listener), Err(EAGAIN), "sem conexão na fila");
    let (client, client_port) = host.net_connect(port).unwrap();
    wait_readable(&host, listener);
    let (server, peer_port) = host.net_accept(listener).unwrap();
    assert_eq!(peer_port, client_port);
    assert_eq!(host.net_recv(server, 16), Err(EAGAIN), "nada a ler ainda");
    assert_eq!(host.net_send(client, b"ping").unwrap(), 4);
    wait_readable(&host, server);
    assert_eq!(host.net_recv(server, 16).unwrap(), b"ping");
    host.net_send(server, b"pong").unwrap();
    wait_readable(&host, client);
    assert_eq!(host.net_recv(client, 16).unwrap(), b"pong");
    // O fim da conexão: vazio, e o poll marca o hangup.
    host.net_shutdown(client, false, true).unwrap();
    let ready = host.net_poll(&[NetWatch { fd: server, read: true, write: false }], Some(Duration::from_secs(10))).unwrap();
    assert!(ready[0].readable && ready[0].hangup);
    assert_eq!(host.net_recv(server, 16).unwrap(), b"");
    host.fs_close(client).unwrap();
    host.fs_close(server).unwrap();
    host.fs_close(listener).unwrap();
}

#[test]
fn connecting_to_a_closed_port_is_refused() {
    let host = OsHost::new();
    let (listener, port) = host.net_listen(0, 1).unwrap();
    host.fs_close(listener).unwrap();
    assert_eq!(host.net_connect(port).map(|_| ()), Err(ECONNREFUSED));
}

#[test]
fn poll_times_out_without_events() {
    let host = OsHost::new();
    let (listener, _) = host.net_listen(0, 1).unwrap();
    let started = std::time::Instant::now();
    let ready = host.net_poll(&[NetWatch { fd: listener, read: true, write: false }], Some(Duration::from_millis(30))).unwrap();
    assert!(!ready[0].any());
    assert!(started.elapsed() >= Duration::from_millis(30));
    assert!(host.net_poll(&[NetWatch { fd: 999, read: true, write: false }], Some(Duration::ZERO)).unwrap()[0].error, "descritor desconhecido é erro");
}

fn request(file: &str, args: &[&str], stdio: Vec<SpawnStdio>) -> SpawnRequest {
    let mut argv = vec![file.to_owned()];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    SpawnRequest { file: file.to_owned(), argv, stdio, ..SpawnRequest::default() }
}

#[cfg(unix)]
#[test]
fn spawn_collects_stdout_and_exit_code() {
    let host = OsHost::new();
    let spawned = host.proc_spawn(&request("sh", &["-c", "echo hi; echo err 1>&2; exit 3"], vec![SpawnStdio::Ignore, SpawnStdio::Pipe, SpawnStdio::Pipe])).unwrap();
    let stdout = spawned.parent_fds[1].expect("pipe do stdout");
    let stderr = spawned.parent_fds[2].expect("pipe do stderr");
    assert_eq!(drain(&host, stdout), b"hi\n");
    assert_eq!(drain(&host, stderr), b"err\n");
    assert_eq!(host.proc_wait(spawned.pid, false).unwrap(), Some(ProcExit::Exited(3)));
    assert_eq!(host.proc_wait(spawned.pid, false), Err(10), "ECHILD depois de colhido");
}

#[cfg(unix)]
#[test]
fn spawn_feeds_stdin_and_sees_end_of_file_on_close() {
    let host = OsHost::new();
    let spawned = host.proc_spawn(&request("cat", &[], vec![SpawnStdio::Pipe, SpawnStdio::Pipe, SpawnStdio::Inherit])).unwrap();
    let (stdin, stdout) = (spawned.parent_fds[0].unwrap(), spawned.parent_fds[1].unwrap());
    host.fs_write(stdin, b"abc", None).unwrap();
    wait_readable(&host, stdout);
    assert_eq!(host.net_recv(stdout, 16).unwrap(), b"abc");
    host.fs_close(stdin).unwrap();
    assert_eq!(drain(&host, stdout), b"");
    assert_eq!(host.proc_wait(spawned.pid, false).unwrap(), Some(ProcExit::Exited(0)));
}

#[cfg(unix)]
#[test]
fn spawn_socketpair_is_duplex() {
    let host = OsHost::new();
    let spawned = host.proc_spawn(&request("cat", &[], vec![SpawnStdio::SocketPair, SpawnStdio::SocketPair, SpawnStdio::Ignore])).unwrap();
    let (to_child, from_child) = (spawned.parent_fds[0].unwrap(), spawned.parent_fds[1].unwrap());
    host.net_send(to_child, b"duplex").unwrap();
    wait_readable(&host, from_child);
    assert_eq!(host.net_recv(from_child, 16).unwrap(), b"duplex");
    host.kill(spawned.pid, 9).unwrap();
    assert_eq!(host.proc_wait(spawned.pid, false).unwrap(), Some(ProcExit::Signaled(9)));
}

#[cfg(unix)]
#[test]
fn spawn_applies_env_cwd_and_reports_missing_program() {
    let dir = tempfile::tempdir().unwrap();
    let host = OsHost::new();
    let mut spawn = request("sh", &["-c", "echo $FOO; pwd"], vec![SpawnStdio::Ignore, SpawnStdio::Pipe, SpawnStdio::Ignore]);
    spawn.env = Some(vec![("FOO".into(), "bar".into()), ("PATH".into(), std::env::var("PATH").unwrap())]);
    spawn.cwd = Some(dir.path().to_string_lossy().into_owned());
    let spawned = host.proc_spawn(&spawn).unwrap();
    let out = String::from_utf8(drain(&host, spawned.parent_fds[1].unwrap())).unwrap();
    let real = std::fs::canonicalize(dir.path()).unwrap();
    assert_eq!(out, format!("bar\n{}\n", real.display()));
    host.proc_wait(spawned.pid, false).unwrap();
    assert_eq!(host.proc_spawn(&request("programa-que-nao-existe", &[], vec![])).map(|_| ()), Err(ENOENT));
}

#[cfg(unix)]
#[test]
fn wait_nohang_reports_running_child() {
    let host = OsHost::new();
    let spawned = host.proc_spawn(&request("sleep", &["30"], vec![SpawnStdio::Ignore, SpawnStdio::Ignore, SpawnStdio::Ignore])).unwrap();
    assert_eq!(host.proc_wait(spawned.pid, true).unwrap(), None);
    host.kill(spawned.pid, 9).unwrap();
    assert_eq!(host.proc_wait(spawned.pid, false).unwrap(), Some(ProcExit::Signaled(9)));
}
