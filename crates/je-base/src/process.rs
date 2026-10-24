//! Porte de `src/vs/base/common/process.ts`: `cwd`, `env`, `platform` e `arch` do processo, sem tocar o
//! `process` do Node.
//!
//! O original escolhe entre três ambientes ao carregar o módulo (sandbox do Electron, Node e web) e exporta
//! constantes. Aqui o dado é um [`SafeProcess`] montado por quem hospeda: [`SafeProcess::native`] lê o
//! processo real (o ramo Node: `VSCODE_CWD` antes do diretório atual), [`SafeProcess::web`] é o ramo web
//! (`env` vazio, `cwd` igual a `/`, `arch` indefinido) e [`SafeProcess::new`] monta um valor qualquer, que é o
//! caminho do sandbox. O ramo do `vscode.process` do Electron não tem equivalente em Rust: o renderer nativo
//! monta o seu com [`SafeProcess::new`].

use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::platform::Platform;

/// `IProcessEnvironment`: `{ [key: string]: string | undefined }`. Uma chave ausente é `undefined`.
pub type ProcessEnvironment = BTreeMap<String, String>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SafeProcess {
    /// `process.platform`: `win32`, `darwin` ou `linux` (no Node, também `freebsd` e afins).
    pub platform: String,
    /// `process.arch`: `x64`, `arm64`... ; `None` na web.
    pub arch: Option<String>,
    pub env: ProcessEnvironment,
    cwd: String,
}

impl SafeProcess {
    pub fn new(platform: impl Into<String>, arch: Option<String>, env: ProcessEnvironment, cwd: impl Into<String>) -> Self {
        Self { platform: platform.into(), arch, env, cwd: cwd.into() }
    }

    /// O ramo Node: `process.env['VSCODE_CWD'] || process.cwd()`.
    pub fn native() -> Self {
        let env: ProcessEnvironment = std::env::vars_os().filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?))).collect();
        let cwd = match env.get("VSCODE_CWD") {
            Some(cwd) if !cwd.is_empty() => cwd.clone(),
            _ => std::env::current_dir().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
        };
        let platform = match std::env::consts::OS {
            "windows" => "win32",
            "macos" => "darwin",
            other => other,
        };
        Self { platform: platform.to_owned(), arch: Some(node_arch(std::env::consts::ARCH).to_owned()), env, cwd }
    }

    /// O ramo web: o sistema vem do `userAgent`, `arch` é indefinido, `env` é vazio e `cwd` é `/`.
    pub fn web(platform: &Platform) -> Self {
        let name = if platform.is_windows {
            "win32"
        } else if platform.is_macintosh {
            "darwin"
        } else {
            "linux"
        };
        Self { platform: name.to_owned(), arch: None, env: ProcessEnvironment::new(), cwd: "/".to_owned() }
    }

    /// O ramo que o alvo da compilação escolhe: web em `wasm32-unknown-unknown`, Node nos demais.
    pub fn detect() -> Self {
        if cfg!(all(target_arch = "wasm32", target_os = "unknown")) { Self::web(&Platform::detect()) } else { Self::native() }
    }

    /// `cwd()`.
    pub fn cwd(&self) -> &str {
        &self.cwd
    }
}

/// `process.arch` do Node para o `ARCH` do Rust.
fn node_arch(arch: &str) -> &str {
    match arch {
        "x86_64" => "x64",
        "aarch64" => "arm64",
        "x86" => "ia32",
        "powerpc64" => "ppc64",
        "s390x" => "s390x",
        other => other,
    }
}

thread_local! {
    /// O `env` exportado pelo módulo: a cópia do ambiente do processo que o código do workbench lê e, como no Node, pode
    /// alterar (`env['HOME'] = ...`). Alterar o ambiente real do processo é `unsafe` em Rust (e os testes rodam em
    /// paralelo), então a cópia é por thread, montada de [`SafeProcess::detect`] na primeira leitura.
    static ENV: RefCell<ProcessEnvironment> = RefCell::new(SafeProcess::detect().env);
}

/// `env[name]`.
pub fn env_var(name: &str) -> Option<String> {
    ENV.with(|env| env.borrow().get(name).cloned())
}

/// `env[name] = value`, ou `delete env[name]` com `None`.
pub fn set_env_var(name: &str, value: Option<&str>) {
    ENV.with(|env| match value {
        Some(value) => {
            env.borrow_mut().insert(name.to_owned(), value.to_owned());
        }
        None => {
            env.borrow_mut().remove(name);
        }
    });
}
