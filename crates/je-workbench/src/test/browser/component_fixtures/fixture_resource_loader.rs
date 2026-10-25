//! Porte de `src/vs/workbench/test/browser/componentFixtures/fixtureResourceLoader.ts`: onde os fixtures leem os arquivos
//! do próprio VS Code (gramáticas TextMate e temas das extensões embutidas).
//!
//! Notas de tradução:
//! - `fixtureResourceUri(path)` do original é o caminho dentro do repositório servido (nativo: o pai de `out/`; web: a raiz
//!   do servidor de desenvolvimento). Aqui é o arquivo dentro do oráculo `ref/vscode` (fora do git), achado subindo a
//!   partir de `CARGO_MANIFEST_DIR`, como `tests/encoding.rs`.
//! - O `XMLHttpRequest` é a leitura do arquivo; `readFixtureBinaryResource` não existe porque o único uso é o `onig.wasm`
//!   do `vscode-oniguruma`, e o Oniguruma do porte é o `ferroni` (Rust), compilado na crate.
//! - [`FixtureExtensionResourceLoader`] é o `IExtensionResourceLoaderService` anônimo que os testes montam com
//!   `readExtensionResource: readFixtureTextResource`.

use std::path::{Path, PathBuf};

use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::resources::{fs_path, uri_file};

use crate::contrib::snippets::snippets_file::IExtensionResourceLoaderService;

/// A raiz do VS Code dos fixtures: `ref/vscode`.
pub fn fixture_root() -> PathBuf {
    let relative = "ref/vscode";
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .map(|dir| dir.join(relative))
        .find(|root| root.join("extensions").is_dir())
        .unwrap_or_else(|| panic!("o oráculo não foi achado ({relative}) acima de CARGO_MANIFEST_DIR"))
}

/// `fixtureResourceUri(path)`.
pub fn fixture_resource_uri(path: &str) -> URI {
    uri_file(&fixture_root().join(path).to_string_lossy())
}

/// `readFixtureTextResource(resource)`.
pub fn read_fixture_text_resource(resource: &URI) -> Promise<String> {
    match std::fs::read_to_string(fs_path(resource)) {
        Ok(content) => Promise::resolved(content),
        Err(error) => Promise::rejected(Error::new(format!("Failed to load fixture resource {}: {error}", fs_path(resource)))),
    }
}

/// O `IExtensionResourceLoaderService` dos fixtures: lê o recurso como texto do repositório.
pub struct FixtureExtensionResourceLoader;

impl IExtensionResourceLoaderService for FixtureExtensionResourceLoader {
    fn read_extension_resource(&self, uri: &URI) -> Promise<String> {
        read_fixture_text_resource(uri)
    }
}
