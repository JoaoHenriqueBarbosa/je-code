//! Porte do gerenciamento de extensões do VS Code: `src/vs/platform/extensions` (manifesto, identificadores,
//! validação) e `src/vs/platform/extensionManagement` (nomes e tipos, scanner, perfil, instalação de VSIX e
//! cliente do marketplace). Lê e escreve o MESMO diretório de extensões do VS Code (`~/.vscode/extensions`, com o
//! `extensions.json`, o `.obsolete` e as pastas `publisher.name-version[-platform]`).
//!
//! Por que um crate próprio, e não um módulo do `je-platform`: o gerenciamento puxa cliente HTTPS (ureq com rustls)
//! e leitor de zip, dependências pesadas que não cabem no `je-platform`, e o crate evita conflito de edição com os
//! outros trabalhos em curso no `je-platform`.

// Tipos e regras puras (compilam em todos os alvos).
pub mod allowed;
pub mod config_remotes;
pub mod js;
pub mod json_util;
pub mod manifest;
pub mod management;
pub mod nls;
pub mod paths;
pub mod util;
pub mod validator;

// Disco, perfil, scanner, cliente do marketplace.
pub mod fs;
pub mod gallery;
pub mod http;
pub mod lifecycle;
pub mod marketplace;
pub mod profile;
pub mod profile_scanner;
pub mod scanner;

// Instalação de VSIX: precisa de zip e de sistema de arquivos, fora do alvo web (a instalação na web é do servidor remoto).
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod downloader;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod management_service;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod vsix;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub mod watcher;
