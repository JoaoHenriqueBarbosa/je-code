//! Porte de `src/vs/platform/environment/node/userDataPath.ts`: onde ficam os dados do usuário.
//!
//! A ordem de decisão é a do original: `VSCODE_DEV` fixa o nome do produto, depois o modo portátil
//! (`VSCODE_PORTABLE`), `VSCODE_APPDATA`, o `--user-data-dir` e, por fim, o padrão da plataforma
//! (`%APPDATA%\Code`, `~/Library/Application Support/Code`, `$XDG_CONFIG_HOME/Code` ou `~/.config/Code`).
//!
//! Decisões de tradução: o original lê `process.env`, `process.platform` e `os.homedir()`. Aqui tudo isso
//! entra por parâmetro ([`Environment`], [`ProcessPlatform`] e o diretório do usuário), porque o ambiente do
//! processo não pode ser alterado sem `unsafe` e os testes precisam simular outras situações; as funções sem
//! sufixo usam o processo real.

use je_base::errors::Error;
use je_base::path::{Environment, PathModule, ProcessEnvironment};

use super::native_parsed_args::NativeParsedArgs;

/// `process.platform`, nos três valores que `getDefaultUserDataPath` distingue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessPlatform {
    Win32,
    Darwin,
    Linux,
}

impl ProcessPlatform {
    /// A plataforma do processo.
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Win32
        } else if cfg!(target_os = "macos") {
            Self::Darwin
        } else {
            Self::Linux
        }
    }
}

/// `os.homedir()`.
pub fn home_dir() -> String {
    std::env::home_dir().map(|dir| dir.to_string_lossy().into_owned()).unwrap_or_default()
}

/// `os.tmpdir()`.
pub fn tmp_dir() -> String {
    std::env::temp_dir().to_string_lossy().into_owned()
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.filter(|v| !v.is_empty())
}

/// `getUserDataPath(cliArgs, productName)` no processo real.
pub fn get_user_data_path(cli_args: &NativeParsedArgs, product_name: &str) -> Result<String, Error> {
    get_user_data_path_in(&ProcessEnvironment, ProcessPlatform::current(), &home_dir(), cli_args, product_name)
}

/// `getUserDataPath(cliArgs, productName)` sobre um ambiente dado.
pub fn get_user_data_path_in(
    env: &dyn Environment,
    platform: ProcessPlatform,
    home_dir: &str,
    cli_args: &NativeParsedArgs,
    product_name: &str,
) -> Result<String, Error> {
    let user_data_path = do_get_user_data_path(env, platform, home_dir, cli_args, product_name)?;
    let path = PathModule::native(env);
    let mut paths_to_resolve = vec![user_data_path.clone()];

    // If the user-data-path is not absolute, make sure to resolve it against the passed in current working
    // directory. We cannot use the node.js `path.resolve()` logic because it will not pick up our
    // `VSCODE_CWD` environment variable (https://github.com/microsoft/vscode/issues/120269)
    if !path.is_absolute(&user_data_path) {
        paths_to_resolve.insert(0, env.cwd());
    }

    let segments: Vec<&str> = paths_to_resolve.iter().map(String::as_str).collect();
    Ok(path.resolve(&segments))
}

fn do_get_user_data_path(
    env: &dyn Environment,
    platform: ProcessPlatform,
    home_dir: &str,
    cli_args: &NativeParsedArgs,
    product_name: &str,
) -> Result<String, Error> {
    let path = PathModule::native(env);
    let mut product_name = product_name.to_owned();

    // 0. Running out of sources has a fixed productName
    if non_empty(env.env_var("VSCODE_DEV")).is_some() {
        product_name = "code-oss-dev".to_owned();
    }

    // 1. Support portable mode
    if let Some(portable_path) = non_empty(env.env_var("VSCODE_PORTABLE")) {
        return Ok(path.join(&[&portable_path, "user-data"]));
    }

    // 2. Support global VSCODE_APPDATA environment variable
    if let Some(app_data_path) = non_empty(env.env_var("VSCODE_APPDATA")) {
        return Ok(path.join(&[&app_data_path, &product_name]));
    }

    // With Electron>=13 --user-data-dir switch will be propagated to all processes
    // https://github.com/electron/electron/blob/1897b14af36a02e9aa7e4d814159303441548251/shell/browser/electron_browser_client.cc#L546-L553
    // Check VSCODE_PORTABLE and VSCODE_APPDATA before this case to get correct values.
    // 3. Support explicit --user-data-dir
    if let Some(cli_path) = non_empty(cli_args.get_string("user-data-dir")) {
        return Ok(cli_path);
    }

    get_default_user_data_path_in(env, platform, home_dir, &product_name)
}

/// `getDefaultUserDataPath(productName)` no processo real.
pub fn get_default_user_data_path(product_name: &str) -> Result<String, Error> {
    get_default_user_data_path_in(&ProcessEnvironment, ProcessPlatform::current(), &home_dir(), product_name)
}

/// `getDefaultUserDataPath(productName)` sobre um ambiente dado.
pub fn get_default_user_data_path_in(env: &dyn Environment, platform: ProcessPlatform, home_dir: &str, product_name: &str) -> Result<String, Error> {
    let path = PathModule::native(env);

    // 4. Otherwise check per platform
    let app_data_path = match platform {
        ProcessPlatform::Win32 => match non_empty(env.env_var("APPDATA")) {
            Some(app_data) => app_data,
            None => {
                let Some(user_profile) = env.env_var("USERPROFILE") else {
                    return Err(Error::new("Windows: Unexpected undefined %USERPROFILE% environment variable"));
                };
                path.join(&[&user_profile, "AppData", "Roaming"])
            }
        },
        ProcessPlatform::Darwin => path.join(&[home_dir, "Library", "Application Support"]),
        ProcessPlatform::Linux => non_empty(env.env_var("XDG_CONFIG_HOME")).unwrap_or_else(|| path.join(&[home_dir, ".config"])),
    };

    Ok(path.join(&[&app_data_path, product_name]))
}
