//! Testes próprios de `createRandomIPCHandle` e `createStaticIPCHandle` (`parts/ipc/node_net.rs`), no lugar do
//! `IPC, create handle` do original, que abre servidores de verdade. Não é porte de teste do VS Code. O
//! `sha256` esperado vem de `hashlib` (os 8 primeiros hex do SHA-256 do diretório).

use je_base::parts::ipc::node_net::{IpcHandleEnvironment, create_random_ipc_handle, create_static_ipc_handle};
use je_base::platform::{NativeEnvironment, Platform};

fn env(platform: &str, xdg: Option<&str>, portable: Option<&str>) -> IpcHandleEnvironment {
    let no_env = |_: &str| None;
    IpcHandleEnvironment {
        platform: Platform::from_native(&NativeEnvironment { platform, env: &no_env, is_electron: false }),
        xdg_runtime_dir: xdg.map(str::to_owned),
        tmpdir: "/tmp".to_owned(),
        vscode_portable: portable.map(str::to_owned),
    }
}

#[test]
fn static_handle_uses_xdg_runtime_dir_on_linux() {
    let handle = create_static_ipc_handle(&env("linux", Some("/run/user/1000"), None), "/tmp/x", "test", "1.64.0");
    assert_eq!(handle, format!("/run/user/1000/vscode-{}-1.64-test.sock", SHA_TMP_X));
}

#[test]
fn static_handle_falls_back_to_the_directory_on_mac_and_when_portable() {
    assert_eq!(create_static_ipc_handle(&env("darwin", Some("/run/user/1000"), None), "/tmp/x", "test", "1.64.0"), "/tmp/x/1.64-test.sock");
    assert_eq!(create_static_ipc_handle(&env("linux", Some("/run/user/1000"), Some("/portable")), "/tmp/x", "test", "1.64.0"), "/tmp/x/1.64-test.sock");
    assert_eq!(create_static_ipc_handle(&env("linux", None, None), "/tmp/x", "longertype", "10.20.30"), "/tmp/x/10.2-longer.sock");
}

#[test]
fn static_handle_is_a_named_pipe_on_windows() {
    let handle = create_static_ipc_handle(&env("win32", None, None), "/tmp/x", "test", "1.64.0");
    assert_eq!(handle, format!("\\\\.\\pipe\\{SHA_TMP_X}-1.64.0-test-sock"));
}

#[test]
fn random_handle_fits_the_platform_limit_and_is_unique() {
    let linux = env("linux", Some("/run/user/1000"), None);
    let one = create_random_ipc_handle(&linux);
    let two = create_random_ipc_handle(&linux);
    assert_ne!(one, two);
    assert!(one.starts_with("/run/user/1000/vscode-ipc-") && one.ends_with(".sock"), "{one}");
    assert!(one.len() < 107);

    // Um diretório longo corta o sufixo para o caminho caber no limite de 107 do Linux.
    let long = IpcHandleEnvironment { xdg_runtime_dir: Some(format!("/run/{}", "d".repeat(70))), ..linux.clone() };
    let trimmed = create_random_ipc_handle(&long);
    assert!(trimmed.len() < 107, "{trimmed}");
    assert!(trimmed.ends_with(".sock"));

    assert!(create_random_ipc_handle(&env("win32", None, None)).starts_with("\\\\.\\pipe\\vscode-ipc-"));
    assert!(create_random_ipc_handle(&env("darwin", Some("/run/user/1000"), None)).starts_with("/tmp/vscode-ipc-"));
}

/// Os 8 primeiros hex do SHA-256 de `/tmp/x`.
const SHA_TMP_X: &str = "2e56aa36";
