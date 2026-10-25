//! Porte de `src/vs/platform/environment`: os argumentos da linha de comando, o serviço de ambiente nativo
//! e os caminhos de dados do usuário.
//!
//! Para reusar o perfil do VS Code do usuário (retrocompatibilidade), o je-code lê os MESMOS diretórios:
//! `~/.config/Code` (Linux, ou `$XDG_CONFIG_HOME/Code`), `%APPDATA%\Code` (Windows) e
//! `~/Library/Application Support/Code` (macOS), mais `~/.vscode` para extensões e `argv.json`. O
//! `--user-data-dir` e o modo portátil (`VSCODE_PORTABLE`) funcionam como no original.
//!
//! Não portados: `stdin.ts` e `wait.ts` (leitura do stdin e arquivo-marcador do `--wait`, nenhum teste os
//! exige), `copilotHome*.ts` e `argvHelper.ts#parseMainProcessArgv` ligado ao Electron (o `app.getPath`).

pub mod argv;
pub mod argv_helper;
#[allow(clippy::module_inception)] // `environment.rs` é o porte de `environment.ts`, como no original
pub mod environment;
pub mod environment_main_service;
pub mod environment_service;
pub mod minimist;
pub mod native;
pub mod native_parsed_args;
pub mod user_data_path;

pub use native_parsed_args::NativeParsedArgs;
