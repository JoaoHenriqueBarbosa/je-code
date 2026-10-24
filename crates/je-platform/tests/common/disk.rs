//! Auxiliares dos testes do provider de disco: o diretório de teste aleatório (`getRandomTestPath`), as
//! fixtures de `src/vs/platform/files/test/node/fixtures`, o laço de eventos real para a E/S de verdade e o
//! `TestDiskFileSystemProvider` do começo do `diskFileService.integrationTest.ts`.
//!
//! O laço de eventos é o real, não o virtual do `runWithFakedTimers`: a E/S de disco leva tempo de verdade
//! (as esperas de `write` e `rm` são timers reais).
#![allow(dead_code)]

use std::cell::Cell;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_base::extpath::random_path;
use je_base::lifecycle::IDisposable;
use je_base::path::{Environment, PathModule, ProcessEnvironment};
use je_base::runtime;
use je_base::uri::URI;
use je_platform::files::disk::DiskFileSystemProvider;
use je_platform::files::files::*;
use je_platform::files::pfs;
use je_platform::log::NullLogService;

use super::base;
use super::files::{Overrides, OverridableProvider};

/// Roda a tarefa `async` até o fim no laço de eventos real do thread, esperando os timers de verdade, e
/// reprova se ela não terminou (ficou esperando algo que nunca chega) ou se algum erro inesperado foi
/// reportado. É o `async () => { ... }` de um `test(...)` do mocha, para o que faz E/S real.
pub fn run_on_real_loop(body: impl Future<Output = ()> + 'static) {
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    runtime::spawn_local(async move {
        body.await;
        finished.set(true);
    });
    runtime::run_until_idle();
    base::fail_on_unexpected_errors();
    assert!(done.get(), "o teste assíncrono não terminou");
}

/// `getRandomTestPath(tmpdir(), ...segments)`: o caminho (sem criar nada) de um diretório novo sob a pasta
/// temporária do sistema.
pub fn get_random_test_path(segments: &[&str]) -> PathBuf {
    let env = ProcessEnvironment;
    let tmpdir = std::env::temp_dir().to_string_lossy().into_owned();
    let mut parts = vec![tmpdir.as_str()];
    parts.extend_from_slice(segments);
    PathBuf::from(random_path(Some(&PathModule::native(&env).join(&parts)), None, 8, &env))
}

/// O `testDir` de um teste: apagado ao sair do escopo (o `teardown` do original).
pub struct TestDir {
    path: PathBuf,
}

impl TestDir {
    /// `testDir = getRandomTestPath(...)` e `promises.mkdir(testDir, { recursive: true })`.
    pub fn new(segments: &[&str]) -> Self {
        let dir = Self::uncreated(segments);
        std::fs::create_dir_all(&dir.path).expect("criar o diretório de teste");
        dir
    }

    /// Só o caminho, sem criar: para o teste que copia as fixtures para ele (a cópia cria).
    pub fn uncreated(segments: &[&str]) -> Self {
        Self { path: get_random_test_path(segments) }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// `join(testDir, ...segments)`.
    pub fn join(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// O mesmo como `URI.file(join(testDir, name))`.
    pub fn uri(&self, name: &str) -> URI {
        file_uri(self.join(name))
    }

    /// `URI.file(testDir)`.
    pub fn root_uri(&self) -> URI {
        file_uri(&self.path)
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// `URI.file(path)`.
pub fn file_uri(path: impl AsRef<Path>) -> URI {
    URI::file(path.as_ref().to_string_lossy().into_owned(), ProcessEnvironment.platform())
}

/// `FileAccess.asFileUri(relative).fsPath` para um caminho sob `src` do VS Code (`ref/` fica fora do git, o
/// caminho é relativo à raiz do repositório). O caminho sai normalizado (sem `..`), como o do original.
pub fn vscode_source_path(relative: &str) -> PathBuf {
    let mut dir = PathBuf::new();
    for component in Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ref/vscode/src").join(relative).components() {
        match component {
            std::path::Component::ParentDir => {
                dir.pop();
            }
            other => dir.push(other),
        }
    }
    dir
}

/// A pasta das fixtures do VS Code. O serviço de arquivos compara os URIs dos filhos (que o `joinPath`
/// normaliza) com o da raiz, por isso o caminho sai sem `..`.
pub fn fixtures_dir() -> PathBuf {
    vscode_source_path("vs/platform/files/test/node/fixtures")
}

/// `FileAccess.asFileUri('vs/platform/files/test/node/fixtures/<relative>').fsPath`.
pub fn fixture_path(relative: &str) -> PathBuf {
    fixtures_dir().join(relative)
}

/// `Promises.copy(fixtureDir, target, { preserveSymlinks: false })`: copia uma pasta de fixtures (por
/// exemplo `service`) para `target`, que é criado.
pub fn copy_fixtures(relative: &str, target: &Path) {
    pfs::copy(&fixture_path(relative).to_string_lossy(), &target.to_string_lossy(), false).expect("copiar as fixtures");
}

/// O estado do `TestDiskFileSystemProvider`: o que os setters do original ligam e o contador de bytes lidos.
pub struct TestDiskState {
    /// `totalBytesRead`.
    pub total_bytes_read: Cell<usize>,
    invalid_stat_size: Cell<bool>,
    small_stat_size: Cell<bool>,
    readonly: Cell<bool>,
    capabilities: Cell<Option<FileSystemProviderCapabilities>>,
}

/// As capacidades padrão do `TestDiskFileSystemProvider`: as do disco mais `Trash`.
fn default_test_capabilities() -> FileSystemProviderCapabilities {
    let mut capabilities = FileSystemProviderCapabilities::FILE_READ_WRITE
        | FileSystemProviderCapabilities::FILE_OPEN_READ_WRITE_CLOSE
        | FileSystemProviderCapabilities::FILE_READ_STREAM
        | FileSystemProviderCapabilities::TRASH
        | FileSystemProviderCapabilities::FILE_FOLDER_COPY
        | FileSystemProviderCapabilities::FILE_WRITE_UNLOCK
        | FileSystemProviderCapabilities::FILE_ATOMIC_READ
        | FileSystemProviderCapabilities::FILE_ATOMIC_WRITE
        | FileSystemProviderCapabilities::FILE_ATOMIC_DELETE
        | FileSystemProviderCapabilities::FILE_CLONE
        | FileSystemProviderCapabilities::FILE_APPEND
        | FileSystemProviderCapabilities::FILE_REALPATH;
    if cfg!(target_os = "linux") {
        capabilities |= FileSystemProviderCapabilities::PATH_CASE_SENSITIVE;
    }
    capabilities
}

/// `TestDiskFileSystemProvider` do `diskFileService.integrationTest.ts`: um `DiskFileSystemProvider` que
/// conta os bytes lidos (`read` e `readFile`), tem capacidades que o teste troca e sabe falsear o `stat`.
///
/// A "subclasse" é um [`OverridableProvider`] em volta do provider de disco; registre
/// [`as_provider`](Self::as_provider) no `FileService` e descarte o conjunto com `store.add_rc(provider)`.
/// O provider de disco enxerga a camada de cima (`set_outer`), então `readFileStream` também conta os bytes.
pub struct TestDiskFileSystemProvider {
    disk: Rc<DiskFileSystemProvider>,
    provider: Rc<OverridableProvider>,
    state: Rc<TestDiskState>,
}

impl TestDiskFileSystemProvider {
    /// `new TestDiskFileSystemProvider(new NullLogService())`.
    pub fn new() -> Rc<Self> {
        let disk = DiskFileSystemProvider::new(NullLogService::shared(), None);
        let state = Rc::new(TestDiskState {
            total_bytes_read: Cell::new(0),
            invalid_stat_size: Cell::new(false),
            small_stat_size: Cell::new(false),
            readonly: Cell::new(false),
            capabilities: Cell::new(None),
        });

        let overrides = Overrides {
            capabilities: Some({
                let state = state.clone();
                // `if (!this._testCapabilities)` do original: zero conta como "ainda não definido" e volta ao padrão.
                Rc::new(move |_| state.capabilities.get().filter(|capabilities| capabilities.bits() != 0).unwrap_or_else(default_test_capabilities))
            }),
            stat: Some({
                let state = state.clone();
                Rc::new(move |sup, resource| {
                    let state = state.clone();
                    sup.stat(resource).then(move |mut stat| {
                        if state.invalid_stat_size.get() {
                            // `res.size = String(res.size)`: o `Stat` de Rust não tem tamanho que não seja número,
                            // então o caso do #72909 só mantém os outros dois desligados, como o `else if` do original.
                        } else if state.small_stat_size.get() {
                            stat.size = 1;
                        } else if state.readonly.get() {
                            stat.permissions = Some(FilePermission::READONLY);
                        }
                        stat
                    })
                })
            }),
            read: Some({
                let state = state.clone();
                Rc::new(move |sup, fd, pos, data, offset, length| {
                    let state = state.clone();
                    sup.read(fd, pos, data, offset, length).then(move |bytes_read| {
                        state.total_bytes_read.set(state.total_bytes_read.get() + bytes_read);
                        bytes_read
                    })
                })
            }),
            read_file: Some({
                let state = state.clone();
                Rc::new(move |sup, resource, atomic| {
                    let state = state.clone();
                    sup.read_file(resource, atomic).then(move |content| {
                        state.total_bytes_read.set(state.total_bytes_read.get() + content.byte_length());
                        content
                    })
                })
            }),
            ..Overrides::default()
        };
        let provider = OverridableProvider::new(disk.clone(), overrides);
        disk.set_outer(&(provider.clone() as Rc<dyn FileSystemProvider>));
        Rc::new(Self { disk, provider, state })
    }

    /// O provider para registrar no `FileService` (`registerProvider(scheme, provider)`).
    pub fn as_provider(&self) -> Rc<dyn FileSystemProvider> {
        self.provider.clone()
    }

    /// O `DiskFileSystemProvider` de baixo, sem as sobrescritas.
    pub fn disk(&self) -> &Rc<DiskFileSystemProvider> {
        &self.disk
    }

    /// O estado (`totalBytesRead`).
    pub fn state(&self) -> &TestDiskState {
        &self.state
    }

    /// `provider.totalBytesRead`.
    pub fn total_bytes_read(&self) -> usize {
        self.state.total_bytes_read.get()
    }

    /// `provider.totalBytesRead = value`.
    pub fn set_total_bytes_read(&self, value: usize) {
        self.state.total_bytes_read.set(value);
    }

    /// `provider.capabilities = capabilities`.
    pub fn set_capabilities(&self, capabilities: FileSystemProviderCapabilities) {
        self.state.capabilities.set(Some(capabilities));
    }

    /// `setCapabilities(provider, capabilities)` do teste: troca e, no Linux, mantém `PathCaseSensitive`. Zero volta
    /// às capacidades padrão (o `|=` do original lê o getter, que reinicializa quando o valor é falsy).
    pub fn set_capabilities_keeping_case_sensitivity(&self, capabilities: FileSystemProviderCapabilities) {
        let keep_case_sensitivity = cfg!(target_os = "linux") && capabilities.bits() != 0;
        self.set_capabilities(if keep_case_sensitivity { capabilities | FileSystemProviderCapabilities::PATH_CASE_SENSITIVE } else { capabilities });
    }

    pub fn set_invalid_stat_size(&self, enabled: bool) {
        self.state.invalid_stat_size.set(enabled);
    }

    pub fn set_small_stat_size(&self, enabled: bool) {
        self.state.small_stat_size.set(enabled);
    }

    pub fn set_readonly(&self, readonly: bool) {
        self.state.readonly.set(readonly);
    }
}

impl IDisposable for TestDiskFileSystemProvider {
    fn dispose(&self) {
        self.disk.dispose();
    }
}
