//! O que o `sandboxHelper.ts` importa de `src/vs/base/node` e que o `je-base` ainda não porta:
//! `findExecutable` (`processes.ts`), `getOSReleaseInfo` (`osReleaseInfo.ts`) e o `execFile` do Node
//! (`child_process`), mais a leitura do uid do processo.
//!
//! Divergências: tudo é síncrono (o original usa `Promise`); em `find_executable` os candidatos de
//! `PATHEXT` são testados em sequência, parando no primeiro achado (o original dispara todos e lê na
//! ordem, com o mesmo resultado).

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use je_base::path::{Environment, PathModule, Platform as PathPlatform, ProcessEnvironment};
use je_base::platform::Platform;

/// `process.env`, como pares na ordem do sistema.
pub fn process_env() -> Vec<(String, String)> {
    std::env::vars_os().map(|(key, value)| (key.to_string_lossy().into_owned(), value.to_string_lossy().into_owned())).collect()
}

/// `getCaseInsensitive` (`objects.ts`) sobre o ambiente.
fn get_case_insensitive<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
    let lowercase_key = key.to_lowercase();
    env.iter().find(|(candidate, _)| candidate.to_lowercase() == lowercase_key).map(|(_, value)| value.as_str())
}

/// `fileExistsDefault`: existe e não é diretório (um `EACCES` no `stat` tenta o `lstat`, por causa de symlink).
fn file_exists_default(path: &str) -> bool {
    match std::fs::metadata(path) {
        Ok(metadata) => !metadata.is_dir(),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => std::fs::symlink_metadata(path).is_ok_and(|metadata| !metadata.is_dir()),
        Err(_) => false,
    }
}

/// `findExecutable`. Os argumentos opcionais têm os mesmos padrões do original: o diretório atual do
/// processo, o `PATH` do ambiente do processo e a existência de arquivo no disco.
pub fn find_executable(
    command: &str,
    cwd: Option<&str>,
    paths: Option<Vec<String>>,
    env: Option<&[(String, String)]>,
    file_exists: Option<&dyn Fn(&str) -> bool>,
) -> Option<String> {
    let process_environment = ProcessEnvironment;
    let path = PathModule::native(&process_environment);
    let default_env;
    let env = match env {
        Some(env) => env,
        None => {
            default_env = process_env();
            &default_env
        }
    };
    let file_exists = file_exists.unwrap_or(&file_exists_default);
    let exists_or_none = |candidate: String| file_exists(&candidate).then_some(candidate);

    // If we have an absolute path then we take it.
    if path.is_absolute(command) {
        return exists_or_none(command.to_owned());
    }
    let cwd = cwd.map_or_else(|| process_environment.cwd(), str::to_owned);
    let dir = path.dirname(command);
    if dir != "." {
        // We have a directory and the directory is relative (see above). Make the path absolute
        // to the current working directory.
        return exists_or_none(path.join(&[&cwd, command]));
    }
    let paths = paths.or_else(|| get_case_insensitive(env, "PATH").map(|env_path| env_path.split(path.delimiter()).map(str::to_owned).collect()));
    // No PATH environment. Make path absolute to the cwd.
    let Some(paths) = paths.filter(|paths| !paths.is_empty()) else {
        return exists_or_none(path.join(&[&cwd, command]));
    };

    // We have a simple file name. We get the path variable from the env
    // and try to find the executable on the path.
    for path_entry in &paths {
        // The path entry is absolute.
        let full_path = if path.is_absolute(path_entry) { path.join(&[path_entry, command]) } else { path.join(&[&cwd, path_entry, command]) };
        if process_environment.platform() == PathPlatform::Win32 {
            let path_ext = get_case_insensitive(env, "PATHEXT").filter(|ext| !ext.is_empty()).unwrap_or(".COM;.EXE;.BAT;.CMD");
            if let Some(found) = path_ext.split(';').find_map(|ext| exists_or_none(format!("{full_path}{ext}"))) {
                return Some(found);
            }
        }
        if file_exists(&full_path) {
            return Some(full_path);
        }
    }
    exists_or_none(path.join(&[&cwd, command]))
}

/// `ReleaseInfo` de `osReleaseInfo.ts`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OsReleaseInfo {
    pub id: String,
    pub id_like: Option<String>,
    pub version_id: Option<String>,
}

/// As linhas como o `readline` do Node as separa: `\n`, `\r\n`, `\r`, `U+2028` e `U+2029`.
fn split_lines(text: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((index, c)) = chars.next() {
        let end = match c {
            '\n' | '\u{2028}' | '\u{2029}' => index + c.len_utf8(),
            '\r' => {
                if chars.peek().is_some_and(|(_, next)| *next == '\n') {
                    chars.next();
                    index + 2
                } else {
                    index + 1
                }
            }
            _ => continue,
        };
        lines.push(&text[start..index]);
        start = end;
    }
    if start < text.len() {
        lines.push(&text[start..]);
    }
    lines
}

/// `getOSReleaseInfo`: no Linux, o `ID`, o `ID_LIKE` e a versão do primeiro arquivo de release legível.
pub fn get_os_release_info(error_logger: &dyn Fn(&str)) -> Option<OsReleaseInfo> {
    let host = Platform::detect();
    if host.is_macintosh || host.is_windows {
        return None;
    }

    // Extract release information on linux based systems
    // using the identifiers specified in
    // https://www.freedesktop.org/software/systemd/man/os-release.html
    let handle = ["/etc/os-release", "/usr/lib/os-release", "/etc/lsb-release"].iter().find_map(|file_path| std::fs::File::open(file_path).ok());
    let Some(mut handle) = handle else {
        error_logger("Unable to retrieve release information from known identifier paths.");
        return None;
    };

    let mut bytes = Vec::new();
    if let Err(error) = handle.read_to_end(&mut bytes) {
        error_logger(&error.to_string());
        return None;
    }
    let content = String::from_utf8_lossy(&bytes);

    let mut release_info = OsReleaseInfo { id: "unknown".to_owned(), id_like: None, version_id: None };
    for line in split_lines(&content) {
        if !line.contains('=') {
            continue;
        }
        let mut parts = line.split('=');
        let key = parts.next().unwrap_or_default().to_uppercase().trim().to_owned();
        if !matches!(key.as_str(), "ID" | "DISTRIB_ID" | "ID_LIKE" | "VERSION_ID" | "DISTRIB_RELEASE") {
            continue;
        }
        let value = parts.next().unwrap_or_default().replace('"', "").to_lowercase().trim().to_owned();
        match key.as_str() {
            "ID" | "DISTRIB_ID" => release_info.id = value,
            "ID_LIKE" => release_info.id_like = Some(value),
            _ => release_info.version_id = Some(value),
        }
    }
    Some(release_info)
}

/// O que o callback de `execFile` recebe: o erro (se houve), a saída padrão e a de erro.
#[derive(Clone, Debug, Default)]
pub struct ExecFileResult {
    pub error: Option<String>,
    pub stdout: String,
    pub stderr: String,
}

fn read_to_string_in_thread(mut pipe: impl Read + Send + 'static) -> thread::JoinHandle<String> {
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = pipe.read_to_end(&mut bytes);
        String::from_utf8_lossy(&bytes).into_owned()
    })
}

/// `execFile(command, args, { encoding: 'utf8', timeout })`: roda o programa sem shell, mata-o ao fim
/// do prazo e junta as duas saídas.
pub fn exec_file(command: &str, args: &[&str], timeout: Duration) -> ExecFileResult {
    let spawned = Command::new(command).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) => return ExecFileResult { error: Some(error.to_string()), ..Default::default() },
    };
    let stdout = child.stdout.take().map(read_to_string_in_thread);
    let stderr = child.stderr.take().map(read_to_string_in_thread);

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break Err("killed".to_owned());
            }
            Ok(None) => thread::sleep(Duration::from_millis(5)),
            Err(error) => break Err(error.to_string()),
        }
    };
    let stdout = stdout.and_then(|handle| handle.join().ok()).unwrap_or_default();
    let stderr = stderr.and_then(|handle| handle.join().ok()).unwrap_or_default();
    let failure = match status {
        Ok(status) if status.success() => None,
        Ok(_) | Err(_) => Some(format!("Command failed: {command} {}\n{stderr}", args.join(" "))),
    };
    ExecFileResult { error: failure, stdout, stderr }
}

/// `process.getuid?.() === 0`: o uid real lido de `/proc/self/status` (sem `unsafe`); onde não há `/proc`
/// (Windows, web) o original também não tem `getuid`.
pub fn process_is_root() -> bool {
    std::fs::read_to_string("/proc/self/status")
        .ok()
        .and_then(|status| status.lines().find_map(|line| line.strip_prefix("Uid:")).and_then(|uids| uids.split_whitespace().next()).map(|uid| uid == "0"))
        .unwrap_or(false)
}
