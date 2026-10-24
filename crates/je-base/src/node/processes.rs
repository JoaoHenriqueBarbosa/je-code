//! Porte parcial de `src/vs/base/node/processes.ts`: `findExecutable` (com o `fileExistsDefault` que ele
//! usa). O resto do original (`getWindowsShell`, `createQueuedSender`, `killTree`) não tem consumidor ainda.
//!
//! Divergências:
//! - O `fileExists` opcional é uma função que devolve um futuro local ([`FileExists`]); o `await` do original é
//!   o `.await` do futuro. As promessas do `pathExtsFound` (disparadas juntas e esperadas em ordem) são
//!   esperadas em sequência, o que só difere se o `fileExists` tivesse efeito colateral (o do Node não tem).
//! - `env = processCommon.env` ausente é a cópia do ambiente do processo (`process::env_var`), e o `cwd`
//!   ausente é o `cwd()` do processo (`SafeProcess::detect`). O `PATH` do Windows é achado sem diferenciar
//!   maiúsculas, como o `getCaseInsensitive` do original faz sobre um `env` explícito.

use crate::node::pfs::{self, LocalFuture};
use crate::path::{PathModule, ProcessEnvironment as HostEnvironment};
use crate::platform::Platform;
use crate::process::{ProcessEnvironment, SafeProcess, env_var};

/// O `fileExists: (path: string) => Promise<boolean>` de `findExecutable`.
pub type FileExists<'a> = &'a dyn for<'p> Fn(&'p str) -> LocalFuture<'p, bool>;

async fn file_exists_default(path: &str) -> bool {
    if pfs::exists(path).await {
        let stat = match std::fs::metadata(path) {
            Ok(stat) => Some(stat),
            // `e.message.startsWith('EACCES')`: it might be symlink
            Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => std::fs::symlink_metadata(path).ok(),
            Err(_) => None,
        };
        return stat.is_some_and(|stat| !stat.is_dir());
    }
    false
}

/// O `fileExistsDefault` como o valor de [`FileExists`].
pub fn file_exists_default_boxed(path: &str) -> LocalFuture<'_, bool> {
    Box::pin(file_exists_default(path))
}

/// `getCaseInsensitive(target, key)` de `objects.ts`.
fn get_case_insensitive(target: &ProcessEnvironment, key: &str) -> Option<String> {
    let lowercase_key = key.to_lowercase();
    match target.keys().find(|candidate| candidate.to_lowercase() == lowercase_key) {
        Some(equivalent_key) => target.get(equivalent_key).cloned(),
        None => target.get(key).cloned(),
    }
}

/// `env['NAME']` do `processCommon.env`, quando nenhum `env` é dado.
fn process_env_var(name: &str) -> Option<String> {
    env_var(name).or_else(|| if Platform::detect().is_windows { env_var(&format!("{}{}", &name[..1], name[1..].to_lowercase())) } else { None })
}

pub async fn find_executable(
    command: &str,
    cwd: Option<&str>,
    paths: Option<&[String]>,
    env: Option<&ProcessEnvironment>,
    file_exists: Option<FileExists<'_>>,
) -> Option<String> {
    let file_exists: FileExists<'_> = file_exists.unwrap_or(&file_exists_default_boxed);
    let host = HostEnvironment;
    let path = PathModule::native(&host);

    // If we have an absolute path then we take it.
    if path.is_absolute(command) {
        return if file_exists(command).await { Some(command.to_owned()) } else { None };
    }
    let cwd = cwd.map_or_else(|| SafeProcess::detect().cwd().to_owned(), str::to_owned);
    let dir = path.dirname(command);
    if dir != "." {
        // We have a directory and the directory is relative (see above). Make the path absolute
        // to the current working directory.
        let full_path = path.join(&[&cwd, command]);
        return if file_exists(&full_path).await { Some(full_path) } else { None };
    }
    let env_path = match env {
        Some(env) => get_case_insensitive(env, "PATH"),
        None => process_env_var("PATH"),
    };
    let mut paths: Option<Vec<String>> = paths.map(<[String]>::to_vec);
    if paths.is_none() {
        if let Some(env_path) = env_path {
            paths = Some(env_path.split(path.delimiter()).map(str::to_owned).collect());
        }
    }
    // No PATH environment. Make path absolute to the cwd.
    let paths = match paths {
        Some(paths) if !paths.is_empty() => paths,
        _ => {
            let full_path = path.join(&[&cwd, command]);
            return if file_exists(&full_path).await { Some(full_path) } else { None };
        }
    };

    // We have a simple file name. We get the path variable from the env
    // and try to find the executable on the path.
    for path_entry in &paths {
        // The path entry is absolute.
        let full_path = if path.is_absolute(path_entry) { path.join(&[path_entry, command]) } else { path.join(&[&cwd, path_entry, command]) };
        if Platform::detect().is_windows {
            let path_ext = match env {
                Some(env) => get_case_insensitive(env, "PATHEXT"),
                None => process_env_var("PATHEXT"),
            }
            .filter(|path_ext| !path_ext.is_empty())
            .unwrap_or_else(|| ".COM;.EXE;.BAT;.CMD".to_owned());
            for ext in path_ext.split(';') {
                let with_extension = format!("{full_path}{ext}");
                if file_exists(&with_extension).await {
                    return Some(with_extension);
                }
            }
        }

        if file_exists(&full_path).await {
            return Some(full_path);
        }
    }
    let full_path = path.join(&[&cwd, command]);
    if file_exists(&full_path).await { Some(full_path) } else { None }
}
