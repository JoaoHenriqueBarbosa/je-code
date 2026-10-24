//! Porte de `src/vs/workbench/services/workingCopy/test/electron-browser/workingCopyBackupService.test.ts` (suíte
//! `WorkingCopyBackupService` e as aninhadas `hashIdentifier`, `getBackupResource`, `backup`, `discardBackup`,
//! `discardBackups (all)`, `discardBackups (except some)`, `getBackups`, `resolve`, `WorkingCopyBackupsModel` e
//! `typeId migration`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - **Sistema de arquivos.** O teste do original não usa disco: o `NodeTestWorkingCopyBackupService` registra um
//!   `InMemoryFileSystemProvider` no esquema `inmemory` e um `FileUserDataProvider` no esquema `vscode-userdata`
//!   (que embrulha o mesmo provider). O porte faz o mesmo, sem diretório temporário.
//! - **Ambiente.** `TestNativeWorkbenchEnvironmentService` (um `NativeWorkbenchEnvironmentService` com a configuração de
//!   janela de teste) não foi portado: o `backupPath` é o `fsPath` do `workspaceBackupPath` e o `userRoamingDataHome` é
//!   o `URI.file(join(userDataDir, 'User'))` no esquema `vscode-userdata`, como no original. `TestLifecycleService` e o
//!   `FileService` são os do porte.
//! - **Herança.** `NodeTestWorkingCopyBackupService extends NativeWorkingCopyBackupService` é a composição
//!   ([`NodeTestWorkingCopyBackupService`] guarda o serviço nativo) com os métodos que o original sobrescreve
//!   (`backup`, `discardBackup`, `discardBackups`) e os auxiliares (`joinBackupResource`, `joinDiscardBackup`,
//!   `waitForAllBackups`, `getBackupContents`, `testGetFileService`).
//! - **`setup` e `teardown`.** O guarda de vazamentos é criado no começo de cada caso e o `setup` (o [`Env`]) logo depois;
//!   o fim do caso faz o `disposables.clear()`, antes do guarda conferir os vazamentos. Todo caso roda dentro de
//!   `run_with_faked_timers`.
//! - **`createTextModel` e `toBufferOrReadable`.** O primeiro é o de `je-editor/tests/support/test_text_model.rs`, o
//!   segundo o de `textfiles.rs`, com o leitor entregue como `WorkingCopyBackupContent::Readable`.
//! - **`Uint8Array`, `Date.now()` e metadados.** O `meta` é um `Dynamic` objeto, com os números como `f64`; `Date.now()` é o
//!   relógio do sistema em milissegundos.
//! - **`toTypedWorkingCopyId` e `toUntypedWorkingCopyId`** (de `workbenchTestServices.ts`) são as funções locais
//!   [`to_typed_working_copy_id`] e [`to_untyped_working_copy_id`].
//! - **`assert.strictEqual(x, undefined)`** é `is_none()`; `children?.length` é `Option<usize>`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
#[allow(dead_code)]
#[path = "../../je-editor/tests/support/test_text_model.rs"]
mod test_text_model;

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::async_::async_fn;
use je_base::buffer::{VSBuffer, buffer_to_readable, buffer_to_stream, stream_to_buffer};
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::{Dynamic, json_stringify};
use je_base::hash::hash;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::network::schemas;
use je_base::promise::{Promise, Resolver};
use je_base::stream::{Readable, consume_stream};
use je_base::uri::{Field, URI, UriChange, UriComponents};
use je_base::uuid::generate_uuid;
use je_platform::files::file_service::FileService;
use je_platform::files::files::to_radix;
use je_platform::files::i_file_service::IFileService;
use je_platform::files::in_memory::InMemoryFileSystemProvider;
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::resources::{ext_uri, fs_path, host_platform, join_path, uri_file};
use je_platform::uri_identity::uri_identity::IUriIdentityService;
use je_platform::uri_identity::uri_identity_service::UriIdentityService;
use je_platform::user_data::file_user_data_provider::FileUserDataProvider;
use je_platform::user_data_profile::{InMemoryProfilesStore, UserDataProfilesEnvironment, UserDataProfilesService};
use je_instantiation::Injectable;
use je_workbench::services::textfile::textfiles::{EncodedContent, TextOrSnapshot, to_buffer_or_readable};
use je_workbench::services::working_copy::native_working_copy_backup_service::NativeWorkingCopyBackupService;
use je_workbench::services::working_copy::working_copy::{WorkingCopyBackupContent, WorkingCopyIdentifier};
use je_workbench::services::working_copy::working_copy_backup::{DiscardBackupsFilter, IWorkingCopyBackupService, ResolvedWorkingCopyBackup};
use je_workbench::services::working_copy::working_copy_backup_service::{WorkingCopyBackupsModel, hash_identifier};
use je_workbench::test::common::workbench_test_services::TestLifecycleService;
use test_text_model::create_text_model;

/// `isWindows`.
fn is_windows() -> bool {
    je_base::platform::Platform::detect().is_windows
}

/// `hash(value).toString(16)`.
fn hash_hex(text: &JsString) -> String {
    to_radix(i64::from(hash(&Dynamic::String(text.clone()))), 16)
}

/// `uri.fsPath`.
fn fs_path_js(uri: &URI) -> JsString {
    uri.fs_path(host_platform())
}

/// `uri.toString()`.
fn text_of(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `uri.scheme`.
fn scheme_of(uri: &URI) -> String {
    uri.scheme().to_string_lossy()
}

/// `URI.file(...)` na plataforma escolhida pelo caso: `windows` no Windows e `posix` no resto.
fn file_uri(windows: &str, posix: &str) -> URI {
    uri_file(if is_windows() { windows } else { posix })
}

fn workspace_resource() -> URI {
    file_uri("c:\\workspace", "/workspace")
}

fn foo_file() -> URI {
    file_uri("c:\\Foo", "/Foo")
}

fn custom_file() -> URI {
    URI::parse("customScheme://some/path", false)
}

fn custom_file_with_fragment() -> URI {
    URI::parse("customScheme2://some/path#fragment", false)
}

fn bar_file() -> URI {
    file_uri("c:\\Bar", "/Bar")
}

fn foo_bar_file() -> URI {
    file_uri("c:\\Foo Bar", "/Foo Bar")
}

fn untitled_file() -> URI {
    URI::from(&UriComponents { scheme: Some(JsString::from(schemas::UNTITLED)), path: Some(JsString::from("Untitled-1")), ..UriComponents::default() }, false)
}

/// `toTypedWorkingCopyId(resource, typeId = 'testBackupTypeId')`.
fn to_typed_working_copy_id(resource: &URI, type_id: &str) -> WorkingCopyIdentifier {
    WorkingCopyIdentifier { type_id: type_id.to_string(), resource: resource.clone() }
}

/// `toTypedWorkingCopyId(resource)` com o `typeId` padrão.
fn to_default_typed_working_copy_id(resource: &URI) -> WorkingCopyIdentifier {
    to_typed_working_copy_id(resource, "testBackupTypeId")
}

/// `toUntypedWorkingCopyId(resource)`.
fn to_untyped_working_copy_id(resource: &URI) -> WorkingCopyIdentifier {
    to_typed_working_copy_id(resource, "")
}

/// `Date.now()`.
fn now_millis() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0.0, |elapsed| elapsed.as_millis() as f64)
}

/// `{ ...entries }`.
fn meta(entries: Vec<(&str, Dynamic)>) -> Dynamic {
    Dynamic::object(entries)
}

/// `(new Array(30 * 1024)).join('Large String\n')`.
fn large_string() -> String {
    "Large String\n".repeat(30 * 1024 - 1)
}

/// `bufferToReadable(VSBuffer.fromString(text))` como conteúdo de backup.
fn readable_of(buffer: VSBuffer) -> WorkingCopyBackupContent {
    let readable: Box<dyn Readable<VSBuffer>> = Box::new(buffer_to_readable(buffer));

    WorkingCopyBackupContent::Readable(Rc::new(RefCell::new(readable)))
}

/// `bufferToReadable(VSBuffer.fromString(text))`.
fn text_readable(text: &str) -> WorkingCopyBackupContent {
    readable_of(VSBuffer::from_string(text))
}

/// `toBufferOrReadable(model.createSnapshot())` como conteúdo de backup.
fn snapshot_content(model: &je_editor::text_model::TextModel) -> WorkingCopyBackupContent {
    match to_buffer_or_readable(Some(TextOrSnapshot::Snapshot(model.create_snapshot(false)))).expect("o snapshot tem conteúdo") {
        EncodedContent::Readable(readable) => WorkingCopyBackupContent::Readable(readable),
        EncodedContent::Buffer(buffer) => readable_of(buffer),
    }
}

/// `${identifier.resource.toString()} ${JSON.stringify({ ...meta, typeId: identifier.typeId })}\n${content}`.
fn to_expected_preamble(identifier: &WorkingCopyIdentifier, content: &str, meta: Option<&Dynamic>) -> String {
    let mut entries = meta.map(Dynamic::own_entries).unwrap_or_default();
    let type_id = Dynamic::string(&identifier.type_id);
    match entries.iter_mut().find(|(key, _)| *key == "typeId") {
        Some(slot) => slot.1 = type_id,
        None => entries.push((JsString::from("typeId"), type_id)),
    }

    format!("{} {}\n{}", text_of(&identifier.resource), json_stringify(&Dynamic::object(entries)).unwrap_or_default(), content)
}

/// `NodeTestWorkingCopyBackupService`.
#[allow(dead_code)]
struct NodeTestWorkingCopyBackupService {
    native: Rc<NativeWorkingCopyBackupService>,
    backup_resource_joiners: Rc<RefCell<Vec<Resolver<()>>>>,
    discard_backup_joiners: Rc<RefCell<Vec<Resolver<()>>>>,
    discarded_backups: Rc<RefCell<Vec<WorkingCopyIdentifier>>>,
    discarded_all_backups: Cell<bool>,
    pending_backups_arr: Rc<RefCell<Vec<Promise<()>>>>,
    /// `_fileService`.
    file_service: Rc<FileService>,
}

#[allow(dead_code)]
impl NodeTestWorkingCopyBackupService {
    fn new(native: Rc<NativeWorkingCopyBackupService>, file_service: Rc<FileService>) -> Rc<Self> {
        Rc::new(Self {
            native,
            backup_resource_joiners: Rc::new(RefCell::new(Vec::new())),
            discard_backup_joiners: Rc::new(RefCell::new(Vec::new())),
            discarded_backups: Rc::new(RefCell::new(Vec::new())),
            discarded_all_backups: Cell::new(false),
            pending_backups_arr: Rc::new(RefCell::new(Vec::new())),
            file_service,
        })
    }

    fn test_get_file_service(&self) -> &Rc<FileService> {
        self.native.file_service()
    }

    fn wait_for_all_backups(&self) -> Promise<()> {
        let pending = self.pending_backups_arr.borrow().clone();

        async_fn(async move {
            Promise::all(pending).await?;

            Ok(())
        })
    }

    fn join_backup_resource(&self) -> Promise<()> {
        let joiners = self.backup_resource_joiners.clone();

        Promise::with_executor(move |resolver| joiners.borrow_mut().push(resolver))
    }

    fn backup(
        &self,
        identifier: &WorkingCopyIdentifier,
        content: Option<WorkingCopyBackupContent>,
        version_id: Option<u64>,
        meta: Option<Dynamic>,
        token: Option<CancellationToken>,
    ) -> Promise<()> {
        let p = self.native.backup(identifier, content, version_id, meta, token);
        let tracked = p.then_result(|result| result);
        self.pending_backups_arr.borrow_mut().push(tracked.clone());

        let pending = self.pending_backups_arr.clone();
        let joiners = self.backup_resource_joiners.clone();

        async_fn(async move {
            let result = p.await;
            pending.borrow_mut().retain(|candidate| !candidate.ptr_eq(&tracked));
            result?;

            loop {
                let resolver = joiners.borrow_mut().pop();
                let Some(resolver) = resolver else { break };
                resolver.resolve(());
            }

            Ok(())
        })
    }

    fn join_discard_backup(&self) -> Promise<()> {
        let joiners = self.discard_backup_joiners.clone();

        Promise::with_executor(move |resolver| joiners.borrow_mut().push(resolver))
    }

    fn discard_backup(&self, identifier: &WorkingCopyIdentifier) -> Promise<()> {
        let discarded = self.native.discard_backup(identifier, None);
        let identifier = identifier.clone();
        let discarded_backups = self.discarded_backups.clone();
        let joiners = self.discard_backup_joiners.clone();

        async_fn(async move {
            discarded.await?;
            discarded_backups.borrow_mut().push(identifier);

            loop {
                let resolver = joiners.borrow_mut().pop();
                let Some(resolver) = resolver else { break };
                resolver.resolve(());
            }

            Ok(())
        })
    }

    fn discard_backups(&self, filter: Option<DiscardBackupsFilter>) -> Promise<()> {
        self.discarded_all_backups.set(true);

        self.native.discard_backups(filter)
    }

    fn get_backup_contents(&self, identifier: &WorkingCopyIdentifier) -> Promise<String> {
        let backup_resource = self.native.to_backup_resource(identifier);
        let file_service = self.native.file_service().clone();

        async_fn(async move {
            let file_contents = file_service.read_file(&backup_resource, None, None).await?;

            Ok(file_contents.value.to_string())
        })
    }

    fn to_backup_resource(&self, identifier: &WorkingCopyIdentifier) -> URI {
        self.native.to_backup_resource(identifier)
    }

    fn has_backup_sync(&self, identifier: &WorkingCopyIdentifier, version_id: Option<u64>, meta: Option<&Dynamic>) -> bool {
        self.native.has_backup_sync(identifier, version_id, meta)
    }

    fn join_backups(&self) -> Promise<()> {
        self.native.join_backups()
    }

    fn reinitialize(&self, backup_workspace_home: Option<&URI>) {
        self.native.reinitialize(backup_workspace_home);
    }

    fn get_backups(&self) -> Promise<Vec<WorkingCopyIdentifier>> {
        self.native.get_backups()
    }

    fn resolve(&self, identifier: &WorkingCopyIdentifier) -> Promise<Option<ResolvedWorkingCopyBackup>> {
        self.native.resolve(identifier)
    }
}

impl IDisposable for NodeTestWorkingCopyBackupService {
    fn dispose(&self) {
        self.native.dispose();
    }
}

/// O `setup` da suíte `WorkingCopyBackupService`.
struct Env {
    disposables: Rc<DisposableStore>,
    backup_home: URI,
    workspace_backup_path: URI,
    service: Rc<NodeTestWorkingCopyBackupService>,
    file_service: Rc<FileService>,
}

impl Env {
    async fn new() -> Env {
        let disposables = DisposableStore::new();

        let test_dir = uri_file(&format!("{}/vsctests/workingcopybackupservice", generate_uuid()))
            .with(&UriChange { scheme: Field::from(schemas::IN_MEMORY), ..UriChange::default() });
        let backup_home = join_path(&test_dir, &["Backups"]);
        let workspaces_json_path = join_path(&backup_home, &["workspaces.json"]);
        let workspace_backup_path = join_path(&backup_home, &[&hash_hex(&fs_path_js(&workspace_resource()))]);

        // `new NodeTestWorkingCopyBackupService(testDir, workspaceBackupPath)`
        let log: LogServiceRef = NullLogService::shared();
        let file_service = disposables.add_rc(FileService::new(log.clone()));
        let lifecycle_service = disposables.add_rc(TestLifecycleService::new());

        let app_settings_home = uri_file(&format!("{}/User", fs_path(&test_dir)));
        let user_roaming_data_home = app_settings_home.with(&UriChange { scheme: Field::from(schemas::VSCODE_USER_DATA), ..UriChange::default() });
        let native = NativeWorkingCopyBackupService::new(
            Some(&fs_path(&workspace_backup_path)),
            &user_roaming_data_home,
            file_service.clone(),
            log.clone(),
            lifecycle_service.clone(),
        );

        let file_system_provider = disposables.add_rc(InMemoryFileSystemProvider::new());
        disposables.add(file_service.register_provider(schemas::IN_MEMORY, file_system_provider.clone()).unwrap());
        let file_service_contract: Rc<dyn IFileService> = Rc::new(file_service.clone());
        let uri_identity_service = disposables.add_rc(UriIdentityService::create((), (file_service_contract,)));
        let user_data_profiles_service = disposables.add_rc(UserDataProfilesService::new(
            UserDataProfilesEnvironment {
                user_roaming_data_home: user_roaming_data_home.clone(),
                cache_home: uri_file(&fs_path(&test_dir)),
                agent_sessions_workspace: Some(join_path(&app_settings_home, &["agent-sessions.code-workspace"])),
            },
            file_service.clone(),
            uri_identity_service.ext_uri(),
            log.clone(),
            Rc::new(InMemoryProfilesStore::default()),
        ));
        let user_data_provider = disposables.add_rc(FileUserDataProvider::new(
            schemas::FILE,
            file_system_provider.clone(),
            schemas::VSCODE_USER_DATA,
            user_data_profiles_service.clone(),
            uri_identity_service.clone(),
            log,
        ));
        disposables.add(file_service.register_provider(schemas::VSCODE_USER_DATA, user_data_provider).unwrap());

        let service = disposables.add_rc(NodeTestWorkingCopyBackupService::new(native, file_service.clone()));

        file_service.create_folder(&backup_home).await.unwrap();
        file_service.write_file(&workspaces_json_path, VSBuffer::from_string("").into(), None).await.unwrap();

        Env { disposables, backup_home, workspace_backup_path, service, file_service }
    }

    /// O `teardown`: `disposables.clear()`.
    fn teardown(&self) {
        self.disposables.dispose();
    }

    /// `joinPath(workspaceBackupPath, identifier.resource.scheme, hashIdentifier(identifier))`.
    fn backup_path_of(&self, identifier: &WorkingCopyIdentifier) -> URI {
        join_path(&self.workspace_backup_path, &[&scheme_of(&identifier.resource), &hash_identifier(identifier)])
    }

    /// `joinPath(workspaceBackupPath, ...fragments)`.
    fn under_workspace_backup_path(&self, fragments: &[&str]) -> URI {
        join_path(&self.workspace_backup_path, fragments)
    }

    /// `(await fileService.resolve(joinPath(workspaceBackupPath, folder))).children?.length`.
    async fn children_len(&self, folder: &str) -> Option<usize> {
        let stat = self.file_service.resolve(&self.under_workspace_backup_path(&[folder]), None).await.unwrap();

        stat.children.map(|children| children.len())
    }

    /// `await fileService.exists(resource)`.
    async fn exists(&self, resource: &URI) -> bool {
        self.file_service.exists(resource).await.unwrap()
    }

    /// `(await fileService.readFile(resource)).value.toString()`.
    async fn read_text(&self, resource: &URI) -> String {
        self.file_service.read_file(resource, None, None).await.unwrap().value.to_string()
    }

    /// `fileService.writeFile(resource, VSBuffer.fromString(text))`.
    async fn write_text(&self, resource: &URI, text: &str) {
        self.file_service.write_file(resource, VSBuffer::from_string(text).into(), None).await.unwrap();
    }

    /// `fileService.createFolder(resource)`.
    async fn create_folder(&self, resource: &URI) {
        self.file_service.create_folder(resource).await.unwrap();
    }
}

/// Um caso: o guarda de vazamentos, o `setup`, o corpo e o `teardown`.
fn run<F, Fut>(body: F)
where
    F: FnOnce(Rc<Env>) -> Fut + 'static,
    Fut: Future<Output = ()> + 'static,
{
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();

    run_with_faked_timers(async move {
        let env = Rc::new(Env::new().await);
        body(env.clone()).await;
        env.teardown();
    });
}

mod hash_identifier_suite {
    use super::*;

    #[test]
    fn should_correctly_hash_the_identifier_for_untitled_scheme_uris() {
        run(|_env| async move {
            let uri = untitled_file();

            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // If these hashes change people will lose their backed up files
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

            let untyped_backup_hash = hash_identifier(&to_untyped_working_copy_id(&uri));
            assert_eq!(untyped_backup_hash, "-7f9c1a2e");
            assert_eq!(untyped_backup_hash, hash_hex(&fs_path_js(&uri)));

            let typed_backup_hash = hash_identifier(&WorkingCopyIdentifier { type_id: "hashTest".to_string(), resource: uri.clone() });
            if is_windows() {
                assert_eq!(typed_backup_hash, "-17c47cdc");
            } else {
                assert_eq!(typed_backup_hash, "-8ad5f4f");
            }

            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // If these hashes collide people will lose their backed up files
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

            assert_ne!(untyped_backup_hash, typed_backup_hash);
        });
    }

    #[test]
    fn should_correctly_hash_the_identifier_for_file_scheme_uris() {
        run(|_env| async move {
            let uri = uri_file("/foo");

            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // If these hashes change people will lose their backed up files
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

            let untyped_backup_hash = hash_identifier(&to_untyped_working_copy_id(&uri));
            if is_windows() {
                assert_eq!(untyped_backup_hash, "20ffaa13");
            } else {
                assert_eq!(untyped_backup_hash, "20eb3560");
            }
            assert_eq!(untyped_backup_hash, hash_hex(&fs_path_js(&uri)));

            let typed_backup_hash = hash_identifier(&WorkingCopyIdentifier { type_id: "hashTest".to_string(), resource: uri.clone() });
            if is_windows() {
                assert_eq!(typed_backup_hash, "-55fc55db");
            } else {
                assert_eq!(typed_backup_hash, "51e56bf");
            }

            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // If these hashes collide people will lose their backed up files
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

            assert_ne!(untyped_backup_hash, typed_backup_hash);
        });
    }

    #[test]
    fn should_correctly_hash_the_identifier_for_custom_scheme_uris() {
        run(|_env| async move {
            let uri = URI::from(&UriComponents { scheme: Some(JsString::from("vscode-custom")), path: Some(JsString::from("somePath")), ..UriComponents::default() }, false);

            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // If these hashes change people will lose their backed up files
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

            let untyped_backup_hash = hash_identifier(&to_untyped_working_copy_id(&uri));
            assert_eq!(untyped_backup_hash, "-44972d98");
            assert_eq!(untyped_backup_hash, hash_hex(&uri.to_string(false)));

            let typed_backup_hash = hash_identifier(&WorkingCopyIdentifier { type_id: "hashTest".to_string(), resource: uri.clone() });
            assert_eq!(typed_backup_hash, "502149c7");

            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // If these hashes collide people will lose their backed up files
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

            assert_ne!(untyped_backup_hash, typed_backup_hash);
        });
    }

    #[test]
    fn should_not_fail_for_uris_without_path() {
        run(|_env| async move {
            let uri = URI::from(&UriComponents { scheme: Some(JsString::from("vscode-fragment")), fragment: Some(JsString::from("frag")), ..UriComponents::default() }, false);

            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // If these hashes change people will lose their backed up files
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

            let untyped_backup_hash = hash_identifier(&to_untyped_working_copy_id(&uri));
            assert_eq!(untyped_backup_hash, "-2f6b2f1b");
            assert_eq!(untyped_backup_hash, hash_hex(&uri.to_string(false)));

            let typed_backup_hash = hash_identifier(&WorkingCopyIdentifier { type_id: "hashTest".to_string(), resource: uri.clone() });
            assert_eq!(typed_backup_hash, "6e82ca57");

            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
            // If these hashes collide people will lose their backed up files
            // !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!

            assert_ne!(untyped_backup_hash, typed_backup_hash);
        });
    }
}

mod get_backup_resource {
    use super::*;

    /// `joinPath(backupHome, workspaceHash, scheme, filePathHash).with({ scheme: Schemas.vscodeUserData }).toString()`.
    fn expected_path(env: &Env, workspace_hash: &str, scheme: &str, file_path_hash: &str) -> String {
        text_of(&join_path(&env.backup_home, &[workspace_hash, scheme, file_path_hash]).with(&UriChange { scheme: Field::from(schemas::VSCODE_USER_DATA), ..UriChange::default() }))
    }

    #[test]
    fn should_get_the_correct_backup_path_for_text_files() {
        run(|env| async move {
            // Format should be: <backupHome>/<workspaceHash>/<scheme>/<filePathHash>
            let backup_resource = foo_file();
            let workspace_hash = hash_hex(&fs_path_js(&workspace_resource()));

            // No Type ID
            let mut backup_id = to_untyped_working_copy_id(&backup_resource);
            let mut file_path_hash = hash_identifier(&backup_id);
            let mut expected = expected_path(&env, &workspace_hash, schemas::FILE, &file_path_hash);
            assert_eq!(text_of(&env.service.to_backup_resource(&backup_id)), expected);

            // With Type ID
            backup_id = to_default_typed_working_copy_id(&backup_resource);
            file_path_hash = hash_identifier(&backup_id);
            expected = expected_path(&env, &workspace_hash, schemas::FILE, &file_path_hash);
            assert_eq!(text_of(&env.service.to_backup_resource(&backup_id)), expected);
        });
    }

    #[test]
    fn should_get_the_correct_backup_path_for_untitled_files() {
        run(|env| async move {
            // Format should be: <backupHome>/<workspaceHash>/<scheme>/<filePathHash>
            let backup_resource = untitled_file();
            let workspace_hash = hash_hex(&fs_path_js(&workspace_resource()));

            // No Type ID
            let mut backup_id = to_untyped_working_copy_id(&backup_resource);
            let mut file_path_hash = hash_identifier(&backup_id);
            let mut expected = expected_path(&env, &workspace_hash, schemas::UNTITLED, &file_path_hash);
            assert_eq!(text_of(&env.service.to_backup_resource(&backup_id)), expected);

            // With Type ID
            backup_id = to_default_typed_working_copy_id(&backup_resource);
            file_path_hash = hash_identifier(&backup_id);
            expected = expected_path(&env, &workspace_hash, schemas::UNTITLED, &file_path_hash);
            assert_eq!(text_of(&env.service.to_backup_resource(&backup_id)), expected);
        });
    }

    #[test]
    fn should_get_the_correct_backup_path_for_custom_files() {
        run(|env| async move {
            // Format should be: <backupHome>/<workspaceHash>/<scheme>/<filePathHash>
            let backup_resource = URI::from(&UriComponents { scheme: Some(JsString::from("custom")), path: Some(JsString::from("custom/file.txt")), ..UriComponents::default() }, false);
            let workspace_hash = hash_hex(&fs_path_js(&workspace_resource()));

            // No Type ID
            let mut backup_id = to_untyped_working_copy_id(&backup_resource);
            let mut file_path_hash = hash_identifier(&backup_id);
            let mut expected = expected_path(&env, &workspace_hash, "custom", &file_path_hash);
            assert_eq!(text_of(&env.service.to_backup_resource(&backup_id)), expected);

            // With Type ID
            backup_id = to_default_typed_working_copy_id(&backup_resource);
            file_path_hash = hash_identifier(&backup_id);
            expected = expected_path(&env, &workspace_hash, "custom", &file_path_hash);
            assert_eq!(text_of(&env.service.to_backup_resource(&backup_id)), expected);
        });
    }
}

mod backup {
    use super::*;

    #[test]
    fn joining() {
        run(|env| async move {
            let backup_joined = Rc::new(Cell::new(false));
            let join_backups_promise = env.service.join_backups();
            {
                let backup_joined = backup_joined.clone();
                join_backups_promise.then(move |_| backup_joined.set(true));
            }
            join_backups_promise.clone().await.unwrap();
            assert!(backup_joined.get());

            backup_joined.set(false);
            {
                let backup_joined = backup_joined.clone();
                env.service.join_backups().then(move |_| backup_joined.set(true));
            }

            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);

            let backup_promise = env.service.backup(&identifier, None, None, None, None);
            assert!(!backup_joined.get());
            backup_promise.await.unwrap();
            assert!(backup_joined.get());

            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "", None));
            assert!(env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn no_text() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);

            env.service.backup(&identifier, None, None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "", None));
            assert!(env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn text_file() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);

            env.service.backup(&identifier, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "test", None));
            assert!(env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn text_file_with_version() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);

            env.service.backup(&identifier, Some(text_readable("test")), Some(666), None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "test", None));
            assert!(!env.service.has_backup_sync(&identifier, Some(555), None));
            assert!(env.service.has_backup_sync(&identifier, Some(666), None));
        });
    }

    #[test]
    fn text_file_with_meta() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);
            let meta = meta(vec![("etag", Dynamic::string("678")), ("orphaned", Dynamic::Bool(true))]);

            env.service.backup(&identifier, Some(text_readable("test")), None, Some(meta.clone()), None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "test", Some(&meta)));
            assert!(env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn text_file_with_whitespace_in_name_and_type_with_meta() {
        run(|env| async move {
            let file_with_space = file_uri("c:\\Foo \n Bar", "/Foo \n Bar");
            let identifier = to_typed_working_copy_id(&file_with_space, " test id \n");
            let backup_path = env.backup_path_of(&identifier);
            let meta = meta(vec![("etag", Dynamic::string("678 \n k")), ("orphaned", Dynamic::Bool(true))]);

            env.service.backup(&identifier, Some(text_readable("test")), None, Some(meta.clone()), None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "test", Some(&meta)));
            assert!(env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn text_file_with_unicode_character_in_name_and_type_with_meta() {
        run(|env| async move {
            let file_with_unicode = file_uri("c:\\so𒀅meࠄ", "/so𒀅meࠄ");
            let identifier = to_typed_working_copy_id(&file_with_unicode, " test so𒀅meࠄ id \n");
            let backup_path = env.backup_path_of(&identifier);
            let meta = meta(vec![("etag", Dynamic::string("678so𒀅meࠄ")), ("orphaned", Dynamic::Bool(true))]);

            env.service.backup(&identifier, Some(text_readable("test")), None, Some(meta.clone()), None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "test", Some(&meta)));
            assert!(env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn untitled_file() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&super::untitled_file());
            let backup_path = env.backup_path_of(&identifier);

            env.service.backup(&identifier, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("untitled").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "test", None));
            assert!(env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn text_file_readable() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);
            let model = create_text_model("test");

            env.service.backup(&identifier, Some(snapshot_content(&model)), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "test", None));
            assert!(env.service.has_backup_sync(&identifier, None, None));

            model.dispose();
        });
    }

    #[test]
    fn untitled_file_readable() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&super::untitled_file());
            let backup_path = env.backup_path_of(&identifier);
            let model = create_text_model("test");

            env.service.backup(&identifier, Some(snapshot_content(&model)), None, None, None).await.unwrap();
            assert_eq!(env.children_len("untitled").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "test", None));

            model.dispose();
        });
    }

    #[test]
    fn text_file_large_file_stream() {
        run(|env| async move {
            let large_string = large_string();

            let stream = buffer_to_stream(VSBuffer::from_string(&large_string));
            test_large_text_file(&env, &large_string, WorkingCopyBackupContent::Stream(stream)).await;
        });
    }

    #[test]
    fn text_file_large_file_readable() {
        run(|env| async move {
            let large_string = large_string();
            let model = create_text_model(&large_string);

            test_large_text_file(&env, &large_string, snapshot_content(&model)).await;

            model.dispose();
        });
    }

    async fn test_large_text_file(env: &Env, large_string: &str, buffer: WorkingCopyBackupContent) {
        let identifier = to_untyped_working_copy_id(&foo_file());
        let backup_path = env.backup_path_of(&identifier);
        let meta = meta(vec![("largeTest", Dynamic::Bool(true))]);

        env.service.backup(&identifier, Some(buffer), None, Some(meta.clone()), None).await.unwrap();
        assert_eq!(env.children_len("file").await, Some(1));
        assert!(env.exists(&backup_path).await);
        assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, large_string, Some(&meta)));
        assert!(env.service.has_backup_sync(&identifier, None, None));
    }

    #[test]
    fn untitled_file_large_file_readable() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&super::untitled_file());
            let backup_path = env.backup_path_of(&identifier);
            let large_string = large_string();
            let model = create_text_model(&large_string);

            env.service.backup(&identifier, Some(snapshot_content(&model)), None, None, None).await.unwrap();
            assert_eq!(env.children_len("untitled").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, &large_string, None));
            assert!(env.service.has_backup_sync(&identifier, None, None));

            model.dispose();
        });
    }

    #[test]
    fn cancellation() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);

            let cts = CancellationTokenSource::new(None);
            let promise = env.service.backup(&identifier, None, None, None, Some(cts.token()));
            cts.cancel();
            promise.await.unwrap();

            assert!(!env.exists(&backup_path).await);
            assert!(!env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn multiple() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);

            Promise::all(vec![
                env.service.backup(&identifier, None, None, None, None),
                env.service.backup(&identifier, None, None, None, None),
                env.service.backup(&identifier, None, None, None, None),
                env.service.backup(&identifier, None, None, None, None),
            ])
            .await
            .unwrap();

            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.read_text(&backup_path).await, to_expected_preamble(&identifier, "", None));
            assert!(env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn multiple_same_resource_different_type_id() {
        run(|env| async move {
            let backup_id1 = to_untyped_working_copy_id(&foo_file());
            let backup_id2 = to_typed_working_copy_id(&foo_file(), "type1");
            let backup_id3 = to_typed_working_copy_id(&foo_file(), "type2");

            Promise::all(vec![
                env.service.backup(&backup_id1, None, None, None, None),
                env.service.backup(&backup_id2, None, None, None, None),
                env.service.backup(&backup_id3, None, None, None, None),
            ])
            .await
            .unwrap();

            assert_eq!(env.children_len("file").await, Some(3));

            for backup_id in [&backup_id1, &backup_id2, &backup_id3] {
                let foo_backup_path = env.backup_path_of(backup_id);
                assert!(env.exists(&foo_backup_path).await);
                assert_eq!(env.read_text(&foo_backup_path).await, to_expected_preamble(backup_id, "", None));
                assert!(env.service.has_backup_sync(backup_id, None, None));
            }
        });
    }
}

mod discard_backup {
    use super::*;

    #[test]
    fn joining() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);

            env.service.backup(&identifier, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.service.has_backup_sync(&identifier, None, None));

            let backup_joined = Rc::new(Cell::new(false));
            {
                let backup_joined = backup_joined.clone();
                env.service.join_backups().then(move |_| backup_joined.set(true));
            }

            let discard_backup_promise = env.service.discard_backup(&identifier);
            assert!(!backup_joined.get());
            discard_backup_promise.await.unwrap();
            assert!(backup_joined.get());

            assert!(!env.exists(&backup_path).await);
            assert_eq!(env.children_len("file").await, Some(0));
            assert!(!env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn text_file() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());
            let backup_path = env.backup_path_of(&identifier);

            env.service.backup(&identifier, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));
            assert!(env.service.has_backup_sync(&identifier, None, None));

            env.service.discard_backup(&identifier).await.unwrap();
            assert!(!env.exists(&backup_path).await);
            assert_eq!(env.children_len("file").await, Some(0));
            assert!(!env.service.has_backup_sync(&identifier, None, None));
        });
    }

    #[test]
    fn untitled_file() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&super::untitled_file());
            let backup_path = env.backup_path_of(&identifier);

            env.service.backup(&identifier, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("untitled").await, Some(1));

            env.service.discard_backup(&identifier).await.unwrap();
            assert!(!env.exists(&backup_path).await);
            assert_eq!(env.children_len("untitled").await, Some(0));
        });
    }

    #[test]
    fn multiple_same_resource_different_type_id() {
        run(|env| async move {
            let backup_id1 = to_untyped_working_copy_id(&foo_file());
            let backup_id2 = to_typed_working_copy_id(&foo_file(), "type1");
            let backup_id3 = to_typed_working_copy_id(&foo_file(), "type2");

            Promise::all(vec![
                env.service.backup(&backup_id1, None, None, None, None),
                env.service.backup(&backup_id2, None, None, None, None),
                env.service.backup(&backup_id3, None, None, None, None),
            ])
            .await
            .unwrap();

            assert_eq!(env.children_len("file").await, Some(3));

            for backup_id in [&backup_id1, &backup_id2, &backup_id3] {
                let backup_path = env.backup_path_of(backup_id);
                env.service.discard_backup(backup_id).await.unwrap();
                assert!(!env.exists(&backup_path).await);
            }
            assert_eq!(env.children_len("file").await, Some(0));
        });
    }
}

mod discard_backups_all {
    use super::*;

    #[test]
    fn text_file() {
        run(|env| async move {
            let backup_id1 = to_untyped_working_copy_id(&foo_file());
            let backup_id2 = to_untyped_working_copy_id(&bar_file());
            let backup_id3 = to_default_typed_working_copy_id(&bar_file());

            env.service.backup(&backup_id1, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));

            env.service.backup(&backup_id2, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(2));

            env.service.backup(&backup_id3, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(3));

            env.service.discard_backups(None).await.unwrap();
            for backup_id in [&backup_id1, &backup_id2, &backup_id3] {
                let backup_path = env.backup_path_of(backup_id);
                assert!(!env.exists(&backup_path).await);
            }

            assert!(!env.exists(&env.under_workspace_backup_path(&["file"])).await);
        });
    }

    #[test]
    fn untitled_file() {
        run(|env| async move {
            let backup_id = to_untyped_working_copy_id(&super::untitled_file());
            let backup_path = env.backup_path_of(&backup_id);

            env.service.backup(&backup_id, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("untitled").await, Some(1));

            env.service.discard_backups(None).await.unwrap();
            assert!(!env.exists(&backup_path).await);
            assert!(!env.exists(&env.under_workspace_backup_path(&["untitled"])).await);
        });
    }

    #[test]
    fn can_backup_after_discarding_all() {
        run(|env| async move {
            env.service.discard_backups(None).await.unwrap();
            env.service.backup(&to_untyped_working_copy_id(&super::untitled_file()), Some(text_readable("test")), None, None, None).await.unwrap();
            assert!(env.exists(&env.workspace_backup_path).await);
        });
    }
}

mod discard_backups_except_some {
    use super::*;

    #[test]
    fn text_file() {
        run(|env| async move {
            let backup_id1 = to_untyped_working_copy_id(&foo_file());
            let backup_id2 = to_untyped_working_copy_id(&bar_file());
            let backup_id3 = to_default_typed_working_copy_id(&bar_file());

            env.service.backup(&backup_id1, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(1));

            env.service.backup(&backup_id2, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(2));

            env.service.backup(&backup_id3, Some(text_readable("test")), None, None, None).await.unwrap();
            assert_eq!(env.children_len("file").await, Some(3));

            env.service.discard_backups(Some(DiscardBackupsFilter { except: vec![backup_id2.clone(), backup_id3.clone()] })).await.unwrap();

            let mut backup_path = env.backup_path_of(&backup_id1);
            assert!(!env.exists(&backup_path).await);

            backup_path = env.backup_path_of(&backup_id2);
            assert!(env.exists(&backup_path).await);

            backup_path = env.backup_path_of(&backup_id3);
            assert!(env.exists(&backup_path).await);

            env.service.discard_backups(Some(DiscardBackupsFilter { except: vec![backup_id1.clone()] })).await.unwrap();

            for backup_id in [&backup_id1, &backup_id2, &backup_id3] {
                let backup_path = env.backup_path_of(backup_id);
                assert!(!env.exists(&backup_path).await);
            }
        });
    }

    #[test]
    fn untitled_file() {
        run(|env| async move {
            let backup_id = to_untyped_working_copy_id(&super::untitled_file());
            let backup_path = env.backup_path_of(&backup_id);

            env.service.backup(&backup_id, Some(text_readable("test")), None, None, None).await.unwrap();
            assert!(env.exists(&backup_path).await);
            assert_eq!(env.children_len("untitled").await, Some(1));

            env.service.discard_backups(Some(DiscardBackupsFilter { except: vec![backup_id.clone()] })).await.unwrap();
            assert!(env.exists(&backup_path).await);
        });
    }
}

mod get_backups {
    use super::*;

    #[test]
    fn text_file() {
        run(|env| async move {
            Promise::all(vec![
                env.service.backup(&to_untyped_working_copy_id(&foo_file()), Some(text_readable("test")), None, None, None),
                env.service.backup(&to_typed_working_copy_id(&foo_file(), "type1"), Some(text_readable("test")), None, None, None),
                env.service.backup(&to_typed_working_copy_id(&foo_file(), "type2"), Some(text_readable("test")), None, None, None),
            ])
            .await
            .unwrap();

            let mut backups = env.service.get_backups().await.unwrap();
            assert_eq!(backups.len(), 3);

            for backup in &backups {
                if backup.type_id.is_empty() {
                    assert_eq!(text_of(&backup.resource), text_of(&foo_file()));
                } else if backup.type_id == "type1" {
                    assert_eq!(text_of(&backup.resource), text_of(&foo_file()));
                } else if backup.type_id == "type2" {
                    assert_eq!(text_of(&backup.resource), text_of(&foo_file()));
                } else {
                    panic!("Unexpected backup");
                }
            }

            env.service.backup(&to_untyped_working_copy_id(&bar_file()), Some(text_readable("test")), None, None, None).await.unwrap();

            backups = env.service.get_backups().await.unwrap();
            assert_eq!(backups.len(), 4);
        });
    }

    #[test]
    fn untitled_file() {
        run(|env| async move {
            Promise::all(vec![
                env.service.backup(&to_untyped_working_copy_id(&super::untitled_file()), Some(text_readable("test")), None, None, None),
                env.service.backup(&to_typed_working_copy_id(&super::untitled_file(), "type1"), Some(text_readable("test")), None, None, None),
                env.service.backup(&to_typed_working_copy_id(&super::untitled_file(), "type2"), Some(text_readable("test")), None, None, None),
            ])
            .await
            .unwrap();

            let backups = env.service.get_backups().await.unwrap();
            assert_eq!(backups.len(), 3);

            for backup in &backups {
                if backup.type_id.is_empty() {
                    assert_eq!(text_of(&backup.resource), text_of(&super::untitled_file()));
                } else if backup.type_id == "type1" {
                    assert_eq!(text_of(&backup.resource), text_of(&super::untitled_file()));
                } else if backup.type_id == "type2" {
                    assert_eq!(text_of(&backup.resource), text_of(&super::untitled_file()));
                } else {
                    panic!("Unexpected backup");
                }
            }
        });
    }
}

mod resolve {
    use super::*;

    /// `{ etag, size, mtime, orphaned }` (`IBackupTestMetaData`).
    fn test_meta(etag: &str, size: f64, mtime: f64, orphaned: bool) -> Dynamic {
        meta(vec![("etag", Dynamic::string(etag)), ("size", Dynamic::Number(size)), ("mtime", Dynamic::Number(mtime)), ("orphaned", Dynamic::Bool(orphaned))])
    }

    /// `['Lorem ipsum ', 'dolor öäü sit amet ', 'consectetur ', 'adipiscing ßß elit'].join('')`.
    fn lorem_contents() -> String {
        ["Lorem ipsum ", "dolor öäü sit amet ", "consectetur ", "adipiscing ßß elit"].join("")
    }

    /// `['Lorem ipsum ', 'dolor öäü sit amet ', 'adipiscing ßß elit', 'consectetur '].join('')`.
    fn lorem_contents_reordered() -> String {
        ["Lorem ipsum ", "dolor öäü sit amet ", "adipiscing ßß elit", "consectetur "].join("")
    }

    #[test]
    fn should_restore_the_original_contents_untitled_file() {
        run(|env| async move {
            let contents = "test\nand more stuff";

            test_resolve_backup(&env, &super::untitled_file(), contents, None, false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_untitled_file_with_metadata() {
        run(|env| async move {
            let contents = "test\nand more stuff";

            let meta = test_meta("the Etag", 666.0, now_millis(), true);

            test_resolve_backup(&env, &super::untitled_file(), contents, Some(&meta), false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_untitled_file_empty_with_metadata() {
        run(|env| async move {
            let contents = "";

            let meta = test_meta("the Etag", 666.0, now_millis(), true);

            test_resolve_backup(&env, &super::untitled_file(), contents, Some(&meta), false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_untitled_large_file_with_metadata() {
        run(|env| async move {
            let contents = large_string();

            let meta = test_meta("the Etag", 666.0, now_millis(), true);

            test_resolve_backup(&env, &super::untitled_file(), &contents, Some(&meta), false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_text_file() {
        run(|env| async move {
            let contents = lorem_contents();

            test_resolve_backup(&env, &foo_file(), &contents, None, false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_text_file_custom_scheme() {
        run(|env| async move {
            let contents = lorem_contents();

            test_resolve_backup(&env, &custom_file(), &contents, None, false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_text_file_with_metadata() {
        run(|env| async move {
            let contents = lorem_contents_reordered();

            let meta = test_meta("theEtag", 888.0, now_millis(), false);

            test_resolve_backup(&env, &foo_file(), &contents, Some(&meta), false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_empty_text_file_with_metadata() {
        run(|env| async move {
            let contents = "";

            let meta = test_meta("theEtag", 888.0, now_millis(), false);

            test_resolve_backup(&env, &foo_file(), contents, Some(&meta), false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_large_text_file_with_metadata() {
        run(|env| async move {
            let contents = large_string();

            let meta = test_meta("theEtag", 888.0, now_millis(), false);

            test_resolve_backup(&env, &foo_file(), &contents, Some(&meta), false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_text_file_with_metadata_changed_once() {
        run(|env| async move {
            let contents = lorem_contents_reordered();

            let meta = test_meta("theEtag", 888.0, now_millis(), false);

            test_resolve_backup(&env, &foo_file(), &contents, Some(&meta), false).await;

            // Change meta and test again
            if let Dynamic::Object(object) = &meta {
                object.set(&JsString::from("size"), Dynamic::Number(999.0));
            }
            test_resolve_backup(&env, &foo_file(), &contents, Some(&meta), false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_text_file_with_metadata_and_fragment_uri() {
        run(|env| async move {
            let contents = lorem_contents_reordered();

            let meta = test_meta("theEtag", 888.0, now_millis(), false);

            test_resolve_backup(&env, &custom_file_with_fragment(), &contents, Some(&meta), false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_text_file_with_space_in_name_with_metadata() {
        run(|env| async move {
            let contents = lorem_contents_reordered();

            let meta = test_meta("theEtag", 888.0, now_millis(), false);

            test_resolve_backup(&env, &foo_bar_file(), &contents, Some(&meta), false).await;
        });
    }

    #[test]
    fn should_restore_the_original_contents_text_file_with_too_large_metadata_to_persist() {
        run(|env| async move {
            let contents = lorem_contents_reordered();

            let meta = test_meta(&"Large String".repeat(100 * 1024 - 1), 888.0, now_millis(), false);

            test_resolve_backup(&env, &foo_file(), &contents, Some(&meta), true).await;
        });
    }

    async fn test_resolve_backup(env: &Env, resource: &URI, contents: &str, meta: Option<&Dynamic>, expect_no_meta: bool) {
        do_test_resolve_backup(env, &to_untyped_working_copy_id(resource), contents, meta, expect_no_meta).await;
        do_test_resolve_backup(env, &to_default_typed_working_copy_id(resource), contents, meta, expect_no_meta).await;
    }

    async fn do_test_resolve_backup(env: &Env, identifier: &WorkingCopyIdentifier, contents: &str, meta: Option<&Dynamic>, expect_no_meta: bool) {
        env.service.backup(identifier, Some(text_readable(contents)), Some(1), meta.cloned(), None).await.unwrap();

        let backup = env.service.resolve(identifier).await.unwrap();
        let backup = backup.expect("o backup deve existir");
        assert_eq!(contents, stream_to_buffer(&*backup.value).await.unwrap().to_string());

        match (expect_no_meta, meta) {
            (false, Some(meta)) => {
                let resolved_meta = backup.meta.expect("o backup deve ter metadados");
                for key in ["etag", "size", "mtime", "orphaned"] {
                    assert!(resolved_meta.get_str(key).strict_equals(&meta.get_str(key)), "a chave {key} difere");
                }

                assert_eq!(meta.own_keys().len(), resolved_meta.own_keys().len());
            }
            _ => assert!(backup.meta.is_none()),
        }
    }

    #[test]
    fn should_restore_the_original_contents_text_file_with_broken_metadata() {
        run(|env| async move {
            test_should_restore_original_contents_with_broken_backup(&env, &to_untyped_working_copy_id(&foo_file())).await;
            test_should_restore_original_contents_with_broken_backup(&env, &to_default_typed_working_copy_id(&foo_file())).await;
        });
    }

    async fn test_should_restore_original_contents_with_broken_backup(env: &Env, identifier: &WorkingCopyIdentifier) {
        let contents = lorem_contents_reordered();

        let meta = test_meta("theEtag", 888.0, now_millis(), false);

        env.service.backup(identifier, Some(text_readable(&contents)), Some(1), Some(meta), None).await.unwrap();

        let backup_path = env.backup_path_of(identifier);

        let file_contents = env.read_text(&backup_path).await;
        assert_eq!(file_contents.find(&text_of(&identifier.resource)), Some(0));

        let meta_index = file_contents.find('{').expect("o preâmbulo tem metadados");
        let new_file_contents = format!("{}{{{{{}", &file_contents[..meta_index], &file_contents[meta_index..]);
        env.write_text(&backup_path, &new_file_contents).await;

        let backup = env.service.resolve(identifier).await.unwrap();
        let backup = backup.expect("o backup deve existir");
        assert_eq!(contents, stream_to_buffer(&*backup.value).await.unwrap().to_string());
        assert!(backup.meta.is_none());
    }

    #[test]
    fn should_update_metadata_from_file_into_model_when_resolving() {
        run(|env| async move {
            test_should_update_meta_from_file_when_resolving(&env, &to_untyped_working_copy_id(&foo_file())).await;
            test_should_update_meta_from_file_when_resolving(&env, &to_default_typed_working_copy_id(&foo_file())).await;
        });
    }

    async fn test_should_update_meta_from_file_when_resolving(env: &Env, identifier: &WorkingCopyIdentifier) {
        let contents = "Foo Bar";

        let etag = "theEtagForThisMetadataTest";
        let mtime = now_millis();
        let meta = test_meta(etag, 888.0, mtime, false);

        let updated_etag = format!("{etag}{etag}");
        let updated_meta = test_meta(&updated_etag, 888.0, mtime, false);

        env.service.backup(identifier, Some(text_readable(contents)), Some(1), Some(meta.clone()), None).await.unwrap();

        let backup_path = env.backup_path_of(identifier);

        // Simulate the condition of the backups model loading initially without
        // meta data information and then getting the meta data updated on the
        // first call to resolve the backup. We simulate this by explicitly changing
        // the meta data in the file and then verifying that the updated meta data
        // is persisted back into the model (verified via `hasBackupSync`).
        // This is not really something that would happen in real life because any
        // backup that is made via backup service will update the model accordingly.

        let original_file_contents = env.read_text(&backup_path).await;
        env.write_text(&backup_path, &original_file_contents.replacen(etag, &updated_etag, 1)).await;

        env.service.resolve(identifier).await.unwrap();

        assert!(!env.service.has_backup_sync(identifier, None, Some(&meta)));
        assert!(env.service.has_backup_sync(identifier, None, Some(&updated_meta)));

        env.write_text(&backup_path, &original_file_contents).await;

        env.service.get_backups().await.unwrap();

        assert!(env.service.has_backup_sync(identifier, None, Some(&meta)));
        assert!(!env.service.has_backup_sync(identifier, None, Some(&updated_meta)));
    }

    #[test]
    fn should_ignore_invalid_backups_empty_file() {
        run(|env| async move {
            let contents = "test\nand more stuff";

            env.service.backup(&to_untyped_working_copy_id(&foo_file()), Some(text_readable(contents)), Some(1), None, None).await.unwrap();

            let mut backup = env.service.resolve(&to_untyped_working_copy_id(&foo_file())).await.unwrap();
            assert!(backup.is_some());

            env.service
                .test_get_file_service()
                .write_file(&env.service.to_backup_resource(&to_untyped_working_copy_id(&foo_file())), VSBuffer::from_string("").into(), None)
                .await
                .unwrap();

            backup = env.service.resolve(&to_untyped_working_copy_id(&foo_file())).await.unwrap();
            assert!(backup.is_none());
        });
    }

    #[test]
    fn should_ignore_invalid_backups_no_preamble() {
        run(|env| async move {
            let contents = "testand more stuff";

            env.service.backup(&to_untyped_working_copy_id(&foo_file()), Some(text_readable(contents)), Some(1), None, None).await.unwrap();

            let mut backup = env.service.resolve(&to_untyped_working_copy_id(&foo_file())).await.unwrap();
            assert!(backup.is_some());

            env.service
                .test_get_file_service()
                .write_file(&env.service.to_backup_resource(&to_untyped_working_copy_id(&foo_file())), VSBuffer::from_string(contents).into(), None)
                .await
                .unwrap();

            backup = env.service.resolve(&to_untyped_working_copy_id(&foo_file())).await.unwrap();
            assert!(backup.is_none());
        });
    }

    #[test]
    fn file_with_binary_data() {
        run(|env| async move {
            let identifier = to_untyped_working_copy_id(&foo_file());

            let buffer: Vec<u8> = vec![
                137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 73, 0, 0, 0, 67, 8, 2, 0, 0, 0, 95, 138, 191, 237, 0, 0, 0, 1, 115, 82, 71, 66, 0, 174, 206, 28, 233, 0, 0, 0, 4, 103, 65, 77, 65, 0, 0, 177, 143, 11, 252, 97, 5, 0, 0, 0, 9, 112, 72, 89, 115, 0, 0, 14, 195, 0, 0, 14, 195, 1, 199, 111, 168, 100, 0, 0, 0, 71, 116, 69, 88, 116, 83, 111, 117, 114, 99, 101, 0, 83, 104, 111, 116, 116, 121, 32, 118, 50, 46, 48, 46, 50, 46, 50, 49, 54, 32, 40, 67, 41, 32, 84, 104, 111, 109, 97, 115, 32, 66, 97, 117, 109, 97, 110, 110, 32, 45, 32, 104, 116, 116, 112, 58, 47, 47, 115, 104, 111, 116, 116, 121, 46, 100, 101, 118, 115, 45, 111, 110, 46, 110, 101, 116, 44, 132, 21, 213, 0, 0, 0, 84, 73, 68, 65, 84, 120, 218, 237, 207, 65, 17, 0, 0, 12, 2, 32, 211, 217, 63, 146, 37, 246, 218, 65, 3, 210, 191, 226, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 230, 118, 100, 169, 4, 173, 8, 44, 248, 184, 40, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
            ];
            let byte_length = buffer.len();

            env.service
                .backup(&identifier, Some(readable_of(VSBuffer::wrap(buffer))), None, Some(meta(vec![("binaryTest", Dynamic::string("true"))])), None)
                .await
                .unwrap();

            let backup = env.service.resolve(&to_untyped_working_copy_id(&foo_file())).await.unwrap();
            let backup = backup.expect("o backup deve existir");

            let backup_buffer = consume_stream(&*backup.value, Rc::new(|chunks: Vec<VSBuffer>| VSBuffer::concat(&chunks, None))).await.unwrap();
            assert_eq!(backup_buffer.byte_length(), byte_length);
        });
    }
}

mod working_copy_backups_model {
    use super::*;

    #[test]
    fn simple() {
        run(|env| async move {
            let model = WorkingCopyBackupsModel::create(&env.workspace_backup_path, env.service.test_get_file_service()).await.unwrap();

            let resource1 = uri_file("test.html");

            assert!(!model.has(&resource1, None, None));

            model.add(&resource1, None, None);

            assert!(model.has(&resource1, None, None));
            assert!(model.has(&resource1, Some(0), None));
            assert!(!model.has(&resource1, Some(1), None));
            assert!(!model.has(&resource1, Some(1), Some(&meta(vec![("foo", Dynamic::string("bar"))]))));

            model.remove(&resource1);

            assert!(!model.has(&resource1, None, None));

            model.add(&resource1, None, None);

            assert!(model.has(&resource1, None, None));
            assert!(model.has(&resource1, Some(0), None));
            assert!(!model.has(&resource1, Some(1), None));

            model.clear();

            assert!(!model.has(&resource1, None, None));

            model.add(&resource1, Some(1), None);

            assert!(model.has(&resource1, None, None));
            assert!(!model.has(&resource1, Some(0), None));
            assert!(model.has(&resource1, Some(1), None));

            let resource2 = uri_file("test1.html");
            let resource3 = uri_file("test2.html");
            let resource4 = uri_file("test3.html");

            model.add(&resource2, None, None);
            model.add(&resource3, None, None);
            model.add(&resource4, None, Some(&meta(vec![("foo", Dynamic::string("bar"))])));

            assert!(model.has(&resource1, None, None));
            assert!(model.has(&resource2, None, None));
            assert!(model.has(&resource3, None, None));

            assert!(model.has(&resource4, None, None));
            assert!(model.has(&resource4, None, Some(&meta(vec![("foo", Dynamic::string("bar"))]))));
            assert!(!model.has(&resource4, None, Some(&meta(vec![("bar", Dynamic::string("foo"))]))));

            model.update(&resource4, Some(&meta(vec![("foo", Dynamic::string("nothing"))])));
            assert!(model.has(&resource4, None, Some(&meta(vec![("foo", Dynamic::string("nothing"))]))));
            assert!(!model.has(&resource4, None, Some(&meta(vec![("foo", Dynamic::string("bar"))]))));

            model.update(&resource4, None);
            assert!(model.has(&resource4, None, None));
            assert!(!model.has(&resource4, None, Some(&meta(vec![("foo", Dynamic::string("nothing"))]))));
        });
    }

    #[test]
    fn create() {
        run(|env| async move {
            let foo_backup_path = env.backup_path_of(&to_untyped_working_copy_id(&foo_file()));
            env.create_folder(&ext_uri().dirname(&foo_backup_path)).await;
            env.write_text(&foo_backup_path, "foo").await;
            let model = WorkingCopyBackupsModel::create(&env.workspace_backup_path, env.service.test_get_file_service()).await.unwrap();

            assert!(model.has(&foo_backup_path, None, None));
        });
    }

    #[test]
    fn get() {
        run(|env| async move {
            let model = WorkingCopyBackupsModel::create(&env.workspace_backup_path, env.service.test_get_file_service()).await.unwrap();

            assert!(model.get().is_empty());

            let file1 = uri_file("/root/file/foo.html");
            let file2 = uri_file("/root/file/bar.html");
            let untitled = uri_file("/root/untitled/bar.html");

            model.add(&file1, None, None);
            model.add(&file2, None, None);
            model.add(&untitled, None, None);

            assert_eq!(model.get().iter().map(fs_path).collect::<Vec<_>>(), vec![fs_path(&file1), fs_path(&file2), fs_path(&untitled)]);
        });
    }
}

mod type_id_migration {
    use super::*;

    #[test]
    fn works_when_meta_is_missing() {
        run(|env| async move {
            let foo_backup_id = to_untyped_working_copy_id(&foo_file());
            let untitled_backup_id = to_untyped_working_copy_id(&untitled_file());
            let custom_backup_id = to_untyped_working_copy_id(&custom_file());

            let foo_backup_path = env.backup_path_of(&foo_backup_id);
            let untitled_backup_path = env.backup_path_of(&untitled_backup_id);
            let custom_file_backup_path = env.backup_path_of(&custom_backup_id);

            // Prepare backups of the old format without meta
            env.create_folder(&env.under_workspace_backup_path(&[&scheme_of(&foo_file())])).await;
            env.create_folder(&env.under_workspace_backup_path(&[&scheme_of(&untitled_file())])).await;
            env.create_folder(&env.under_workspace_backup_path(&[&scheme_of(&custom_file())])).await;
            env.write_text(&foo_backup_path, &format!("{}\ntest file", text_of(&foo_file()))).await;
            env.write_text(&untitled_backup_path, &format!("{}\ntest untitled", text_of(&untitled_file()))).await;
            env.write_text(&custom_file_backup_path, &format!("{}\ntest custom", text_of(&custom_file()))).await;

            env.service.reinitialize(Some(&env.workspace_backup_path));

            let backups = env.service.get_backups().await.unwrap();
            assert_eq!(backups.len(), 3);
            assert!(backups.iter().any(|backup| ext_uri().is_equal(Some(&backup.resource), Some(&foo_file()), false)));
            assert!(backups.iter().any(|backup| ext_uri().is_equal(Some(&backup.resource), Some(&untitled_file()), false)));
            assert!(backups.iter().any(|backup| ext_uri().is_equal(Some(&backup.resource), Some(&custom_file()), false)));
            assert!(backups.iter().all(|backup| backup.type_id.is_empty()));
        });
    }

    #[test]
    fn works_when_type_id_in_meta_is_missing() {
        run(|env| async move {
            let foo_backup_id = to_untyped_working_copy_id(&foo_file());
            let untitled_backup_id = to_untyped_working_copy_id(&untitled_file());
            let custom_backup_id = to_untyped_working_copy_id(&custom_file());

            let foo_backup_path = env.backup_path_of(&foo_backup_id);
            let untitled_backup_path = env.backup_path_of(&untitled_backup_id);
            let custom_file_backup_path = env.backup_path_of(&custom_backup_id);

            // Prepare backups of the old format without meta
            env.create_folder(&env.under_workspace_backup_path(&[&scheme_of(&foo_file())])).await;
            env.create_folder(&env.under_workspace_backup_path(&[&scheme_of(&untitled_file())])).await;
            env.create_folder(&env.under_workspace_backup_path(&[&scheme_of(&custom_file())])).await;
            let foo_meta = json_stringify(&meta(vec![("foo", Dynamic::string("bar"))])).unwrap();
            env.write_text(&foo_backup_path, &format!("{} {}\ntest file", text_of(&foo_file()), foo_meta)).await;
            env.write_text(&untitled_backup_path, &format!("{} {}\ntest untitled", text_of(&untitled_file()), foo_meta)).await;
            env.write_text(&custom_file_backup_path, &format!("{} {}\ntest custom", text_of(&custom_file()), foo_meta)).await;

            env.service.reinitialize(Some(&env.workspace_backup_path));

            let backups = env.service.get_backups().await.unwrap();
            assert_eq!(backups.len(), 3);
            assert!(backups.iter().any(|backup| ext_uri().is_equal(Some(&backup.resource), Some(&foo_file()), false)));
            assert!(backups.iter().any(|backup| ext_uri().is_equal(Some(&backup.resource), Some(&untitled_file()), false)));
            assert!(backups.iter().any(|backup| ext_uri().is_equal(Some(&backup.resource), Some(&custom_file()), false)));
            assert!(backups.iter().all(|backup| backup.type_id.is_empty()));
        });
    }
}
