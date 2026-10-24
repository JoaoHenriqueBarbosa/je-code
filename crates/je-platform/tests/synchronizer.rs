//! Porte de `src/vs/platform/userDataSync/test/common/synchronizer.test.ts` (suítes `TestSynchronizer - Auto
//! Sync`, `Manual Sync` e `Last Sync Data`), caso a caso e na mesma ordem.
//!
//! Divergências:
//! - O `setup` (o servidor, o cliente e o `TestSynchroniser`) roda no começo do corpo de cada teste, dentro do
//!   `runWithFakedTimers`, e o `teardown` (`clear` do servidor) no fim; o servidor é um por teste (o do original é
//!   um por suíte, mas o `clear` do `teardown` o devolve ao estado inicial). O `client.setUp()` do original usa o
//!   `empty` padrão (falso), então o cliente nasce com os arquivos padrão.
//! - A subclasse `TestSynchroniser extends AbstractSynchroniser` é o [`TestSynchroniser`], que implementa o trait
//!   `SynchroniserImpl` e dá `Deref` para a base. Os métodos `protected` que ela sobrescreve (`doSync`,
//!   `getLatestRemoteUserData`, `doTriggerLocalChange`) são ganchos do trait, e o `super.x(...)` é o `super_x` da
//!   base. `stop` é um método próprio do porte (o `stop` do trait `IUserDataSynchroniser` é da base). O trait
//!   `SynchroniserImpl` não é importado de propósito: os métodos dele têm `self: Rc<Self>` e escondiam, na
//!   resolução de métodos, os de mesmo nome do `IUserDataSynchroniser` (`resolve_content`).
//! - `getMachineId`, `getLastSyncResource` e `testTriggerLocalChange` só existem no original para abrir membros
//!   `protected`: aqui são o campo `current_machine_id_promise`, o campo `last_sync_resource` e o método
//!   `trigger_local_change` da própria base, todos públicos.
//! - `assertConflicts` e `assertPreviews` são o mesmo corpo e viram [`assert_resources`].
//! - `testObject.sync(...)` usa os argumentos padrão do original (configuração e cabeçalhos vazios). Sem `await`, a
//!   promessa roda até o primeiro `await`, como no original, e fica guardada numa variável.
//! - `discard` do original devolve o `ISyncResourcePreview`; aqui devolve o preview completo da base e
//!   [`discard`] o converte para a visão do `userDataSync.ts` (`to_base`).
//! - No original, `testObject.sync(..., true)` devolve objetos que o `discard` e o `accept` seguintes mutam no
//!   lugar, então o `preview` antigo enxerga o `mergeState` novo (`conflicts: preview -> discard`). Os valores do
//!   porte são cópias: esse teste usa o preview que o `discard` devolve, e fica idêntico ao
//!   `conflicts: preivew -> discard`, com o mesmo corpo ([`preview_discard`]). Os demais pares de testes de corpo
//!   idêntico (`conflicts: preview -> accept` e `accept 2`; `preivew -> accept -> discard`; `preivew -> discard ->
//!   accept -> apply` e `conflicts: preivew -> accept -> discard -> accept -> apply`) também compartilham o corpo.
//! - Em `status is set correctly when sync has errors` e `status is reset when getting latest remote data fails`
//!   o `try { ...; assert.fail(...) } catch` do original engole o próprio `assert.fail`; aqui a falha esperada é
//!   conferida de verdade (o `Err` de `sync`).

mod common;

use std::cell::{Cell, RefCell};
use std::ops::Deref;
use std::rc::Rc;

use common::files::async_test;
use common::user_data_sync_client::{RecordedRequest, UserDataSyncClient, UserDataSyncTestServer};
use common::{if_match_headers, read_text, write_text};
use je_base::JsString;
use je_base::async_::{Barrier, async_fn};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Emitter;
use je_base::json::Value;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::parts::request::IHeaders;
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_platform::files::file_service::FileContentSource;
use je_platform::log::NullLogService;
use je_platform::resources::host_environment;
use je_platform::storage::{StorageScope, StorageService};
use je_platform::telemetry::null_telemetry_service;
use je_platform::user_data_profile::IUserDataProfilesService;
use je_platform::user_data_sync::abstract_synchronizer::{
    self, AbstractSynchroniser, IAcceptResult, IMergeResult, IResourcePreview, SyncStrategy, SynchroniserServices, parse_json, stringify_json, with_sync_authority,
};
use je_platform::user_data_sync::user_data_sync::{
    Change, IRemoteUserData, IResourcePreview as IBaseResourcePreview, ISyncData, IUserDataSyncConfiguration, IUserDataSyncResource, IUserDataSyncResourcePreview, IUserDataSyncStoreService,
    IUserDataSynchroniser, MergeState, RefOrUserData, ServerResource, SyncResource, SyncStatus,
};

/// A chave do estado da última sincronização de `SyncResource.Settings` no armazenamento.
const LAST_SYNC_STATE_KEY: &str = "settings.lastSyncUserData";

// region: TestSynchroniser

/// `syncResult`: `{ hasConflicts, hasError }`.
#[derive(Clone, Copy)]
struct SyncResult {
    has_conflicts: bool,
    has_error: bool,
}

impl SyncResult {
    const NONE: Self = Self { has_conflicts: false, has_error: false };
    const CONFLICTS: Self = Self { has_conflicts: true, has_error: false };
    const ERROR: Self = Self { has_conflicts: false, has_error: true };
}

/// `content === null ? (existed ? Change.Deleted : Change.None) : Change.Modified`.
fn change_of(content: &Option<String>, existed: bool) -> Change {
    match content {
        Some(_) => Change::Modified,
        None if existed => Change::Deleted,
        None => Change::None,
    }
}

/// Os serviços que `createInstance(TestSynchroniser, ...)` injeta: os do cliente.
fn synchroniser_services(client: &UserDataSyncClient) -> SynchroniserServices {
    SynchroniserServices {
        file_service: client.file_service.clone(),
        environment_service: client.environment_service.clone(),
        storage_service: client.storage_service.clone(),
        user_data_sync_store_service: client.user_data_sync_store_service.clone(),
        user_data_sync_local_store_service: client.user_data_sync_local_store_service.clone(),
        user_data_sync_enablement_service: client.user_data_sync_enablement_service.clone(),
        telemetry_service: null_telemetry_service(),
        log_service: NullLogService::shared(),
        configuration_service: client.configuration_service.clone(),
        ext_uri: ExtUri::unbiased(host_environment()),
    }
}

/// `TestSynchroniser`. O `ITestResourcePreview` é o [`IResourcePreview`] com o `ref` (a `String`) em `extra`.
struct TestSynchroniser {
    base: Rc<AbstractSynchroniser<TestSynchroniser>>,
    sync_barrier: RefCell<Barrier>,
    sync_result: Cell<SyncResult>,
    on_do_sync_call: Rc<Emitter<()>>,
    fail_when_getting_latest_remote_user_data: Cell<bool>,
    cancelled: Cell<bool>,
    local_resource: URI,
    on_did_trigger_local_change_call: Rc<Emitter<()>>,
}

impl Deref for TestSynchroniser {
    type Target = AbstractSynchroniser<TestSynchroniser>;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl IDisposable for TestSynchroniser {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl TestSynchroniser {
    /// `createInstance(TestSynchroniser, { syncResource: SyncResource.Settings, profile: defaultProfile }, undefined)`.
    fn new(client: &UserDataSyncClient) -> Rc<Self> {
        let base = AbstractSynchroniser::<Self>::new(
            IUserDataSyncResource { sync_resource: SyncResource::Settings, profile: client.user_data_profiles_service.default_profile() },
            None,
            synchroniser_services(client),
        );
        let local_resource = base.services.ext_uri.join_path(&client.environment_service.user_roaming_data_home, &["testResource.json"]);
        let (on_do_sync_call, on_did_trigger_local_change_call) = (Emitter::new(), Emitter::new());
        base.register(on_do_sync_call.clone());
        base.register(on_did_trigger_local_change_call.clone());
        let this = Rc::new(Self {
            base: base.clone(),
            sync_barrier: RefCell::new(Barrier::new()),
            sync_result: Cell::new(SyncResult::NONE),
            on_do_sync_call,
            fail_when_getting_latest_remote_user_data: Cell::new(false),
            cancelled: Cell::new(false),
            local_resource,
            on_did_trigger_local_change_call,
        });
        base.bind(Rc::downgrade(&this));
        this
    }

    /// `applyRef`.
    fn apply_ref(&self, content: Option<String>, r#ref: &str) -> Promise<()> {
        let (base, r#ref) = (self.base.clone(), r#ref.to_string());
        async_fn(async move {
            let remote_user_data = base.update_remote_user_data(content.unwrap_or_default(), Some(r#ref)).await?;
            base.update_last_sync_user_data(remote_user_data, Vec::new()).await
        })
    }

    /// `override async stop()`: o `super.stop()` não é esperado.
    fn stop(&self) -> Promise<()> {
        self.cancelled.set(true);
        self.sync_barrier.borrow().open();
        let _stopping = self.base.stop();
        Promise::resolved(())
    }
}

impl abstract_synchronizer::SynchroniserImpl for TestSynchroniser {
    type Preview = String;
    type Accept = ();

    fn version(&self) -> f64 {
        1.0
    }

    fn generate_sync_preview(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        _last_sync_user_data: Option<IRemoteUserData>,
        _is_remote_data_from_current_machine: bool,
        _user_data_sync_configuration: IUserDataSyncConfiguration,
        _token: CancellationToken,
    ) -> Promise<Vec<IResourcePreview<String>>> {
        async_fn(async move {
            if self.sync_result.get().has_error {
                return Err(Error::new("failed"));
            }

            let file_content = self.services.file_service.read_file(&self.local_resource, None, None).await.ok();
            let virtual_resource = |authority: &str| with_sync_authority(&self.local_resource, authority);
            Ok(vec![IResourcePreview {
                base_resource: virtual_resource("base"),
                base_content: None,
                local_resource: self.local_resource.clone(),
                local_content: file_content.map(|file_content| file_content.value.to_string()),
                remote_resource: virtual_resource("remote"),
                remote_content: remote_user_data.sync_data.map(|sync_data| sync_data.content),
                preview_resource: virtual_resource("preview"),
                local_change: Change::Modified,
                remote_change: Change::Modified,
                accepted_resource: virtual_resource("accepted"),
                extra: remote_user_data.r#ref,
            }])
        })
    }

    fn has_remote_changed(self: Rc<Self>, _last_sync_user_data: IRemoteUserData) -> Promise<bool> {
        Promise::resolved(true)
    }

    fn get_merge_result(self: Rc<Self>, resource_preview: IResourcePreview<String>, _token: CancellationToken) -> Promise<IMergeResult<()>> {
        Promise::resolved(IMergeResult {
            result: IAcceptResult { content: Some(resource_preview.extra), local_change: Change::Modified, remote_change: Change::Modified, extra: () },
            has_conflicts: self.sync_result.get().has_conflicts,
        })
    }

    fn get_accept_result(self: Rc<Self>, resource_preview: IResourcePreview<String>, resource: URI, content: Option<Option<String>>, _token: CancellationToken) -> Promise<IAcceptResult<()>> {
        async_fn(async move {
            let is_resource = |other: &URI| self.is_same_resource(&resource, other);

            if is_resource(&resource_preview.local_resource) {
                return Ok(IAcceptResult {
                    content: resource_preview.local_content.clone(),
                    local_change: Change::None,
                    remote_change: change_of(&resource_preview.local_content, true),
                    extra: (),
                });
            }

            if is_resource(&resource_preview.remote_resource) {
                return Ok(IAcceptResult {
                    content: resource_preview.remote_content.clone(),
                    local_change: change_of(&resource_preview.remote_content, true),
                    remote_change: Change::None,
                    extra: (),
                });
            }

            if is_resource(&resource_preview.preview_resource) {
                return Ok(match content {
                    // `content === undefined`
                    None => IAcceptResult { content: Some(resource_preview.extra.clone()), local_change: Change::Modified, remote_change: Change::Modified, extra: () },
                    Some(content) => IAcceptResult {
                        local_change: change_of(&content, resource_preview.local_content.is_some()),
                        remote_change: change_of(&content, resource_preview.remote_content.is_some()),
                        content,
                        extra: (),
                    },
                });
            }

            Err(Error::new(format!("Invalid Resource: {}", resource.to_string(false).to_string_lossy())))
        })
    }

    fn apply_result(
        self: Rc<Self>,
        remote_user_data: IRemoteUserData,
        _last_sync_user_data: Option<IRemoteUserData>,
        resource_previews: Vec<(IResourcePreview<String>, IAcceptResult<()>)>,
        _force: bool,
    ) -> Promise<()> {
        async_fn(async move {
            let accept_result = &resource_previews[0].1;
            let file_service = &self.services.file_service;
            if accept_result.local_change == Change::Deleted {
                file_service.del(&self.local_resource, None).await?;
            }

            if matches!(accept_result.local_change, Change::Added | Change::Modified) {
                let content = accept_result.content.as_deref().expect("o conteúdo existe quando o arquivo local muda");
                file_service.write_file(&self.local_resource, FileContentSource::from(content), None).await?;
            }

            if accept_result.remote_change == Change::Deleted {
                self.apply_ref(None, &remote_user_data.r#ref).await?;
            }

            if matches!(accept_result.remote_change, Change::Added | Change::Modified) {
                self.apply_ref(accept_result.content.clone(), &remote_user_data.r#ref).await?;
            }
            Ok(())
        })
    }

    fn has_local_data(self: Rc<Self>) -> Promise<bool> {
        Promise::rejected(Error::new("not implemented"))
    }

    fn resolve_content(self: Rc<Self>, _uri: URI) -> Promise<Option<String>> {
        Promise::resolved(None)
    }

    fn do_sync(
        self: Rc<Self>,
        base: Rc<AbstractSynchroniser<Self>>,
        remote_user_data: IRemoteUserData,
        last_sync_user_data: Option<IRemoteUserData>,
        strategy: SyncStrategy,
        user_data_sync_configuration: IUserDataSyncConfiguration,
    ) -> Promise<SyncStatus> {
        async_fn(async move {
            self.cancelled.set(false);
            self.on_do_sync_call.fire(());
            let barrier = self.sync_barrier.borrow().clone();
            barrier.wait().await?;

            if self.cancelled.get() {
                return Ok(SyncStatus::Idle);
            }

            base.super_do_sync(remote_user_data, last_sync_user_data, strategy, user_data_sync_configuration).await
        })
    }

    fn get_latest_remote_user_data(self: Rc<Self>, base: Rc<AbstractSynchroniser<Self>>, ref_or_latest_data: Option<RefOrUserData>, last_sync_user_data: Option<IRemoteUserData>) -> Promise<IRemoteUserData> {
        if self.fail_when_getting_latest_remote_user_data.get() {
            return Promise::rejected(Error::new(""));
        }
        base.super_get_latest_remote_user_data(ref_or_latest_data, last_sync_user_data)
    }

    fn do_trigger_local_change(self: Rc<Self>, base: Rc<AbstractSynchroniser<Self>>) -> Promise<()> {
        async_fn(async move {
            base.super_do_trigger_local_change().await?;
            self.on_did_trigger_local_change_call.fire(());
            Ok(())
        })
    }
}

// endregion

// region: auxiliares dos testes

struct Fixture {
    server: Rc<UserDataSyncTestServer>,
    client: Rc<UserDataSyncClient>,
    test_object: Rc<TestSynchroniser>,
}

/// O `setup` da suíte e o `createInstance(TestSynchroniser, ...)` do começo de cada teste.
async fn setup(disposables: &Rc<DisposableStore>) -> Fixture {
    let server = Rc::new(UserDataSyncTestServer::new());
    let client = disposables.add_rc(UserDataSyncClient::set_up(server.clone(), false).await.unwrap());
    let test_object = disposables.add_rc(TestSynchroniser::new(&client));
    Fixture { server, client, test_object }
}

/// O `teardown` da suíte.
async fn teardown(fixture: &Fixture) {
    fixture.client.user_data_sync_store_service.clear().await.unwrap();
}

/// `client.getLatestRef(testObject.resource)`.
async fn latest_ref(client: &UserDataSyncClient, test_object: &TestSynchroniser) -> Option<String> {
    client.get_latest_ref(test_object.resource).await.unwrap()
}

/// `testObject.sync(manifest, preview)`.
fn sync(test_object: &TestSynchroniser, manifest: Option<String>, preview: bool) -> Promise<Option<IUserDataSyncResourcePreview>> {
    test_object.sync(manifest.map(RefOrUserData::Ref), preview, IUserDataSyncConfiguration::default(), IHeaders::new())
}

/// `testObject.sync(await client.getLatestRef(testObject.resource), preview)`.
async fn sync_latest(client: &UserDataSyncClient, test_object: &TestSynchroniser, preview: bool) -> Result<Option<IUserDataSyncResourcePreview>, Error> {
    sync(test_object, latest_ref(client, test_object).await, preview).await
}

/// `testObject.apply(false)`.
fn apply(test_object: &TestSynchroniser) -> Promise<Option<IUserDataSyncResourcePreview>> {
    test_object.apply(false, IHeaders::new())
}

/// `testObject.discard(resource)`, na visão do `userDataSync.ts`.
fn discard(test_object: &TestSynchroniser, resource: &URI) -> Promise<Option<IUserDataSyncResourcePreview>> {
    let discarded = test_object.discard(resource);
    async_fn(async move { Ok(discarded.await?.map(|preview| preview.to_base())) })
}

/// `preview!.resourcePreviews`.
fn resource_previews(preview: &Option<IUserDataSyncResourcePreview>) -> &[IBaseResourcePreview] {
    &preview.as_ref().expect("há um preview").resource_previews
}

/// `preview!.resourcePreviews[0]`.
fn first_preview(preview: &Option<IUserDataSyncResourcePreview>) -> &IBaseResourcePreview {
    &resource_previews(preview)[0]
}

/// `testObject.conflicts.conflicts[0]`.
fn first_conflict(test_object: &TestSynchroniser) -> IBaseResourcePreview {
    test_object.conflicts().conflicts[0].clone()
}

/// `assertConflicts` e `assertPreviews`: os `localResource` dos previews são os esperados.
#[track_caller]
fn assert_resources(actual: &[IBaseResourcePreview], expected: &[&URI]) {
    let actual: Vec<String> = actual.iter().map(|preview| preview.local_resource.to_string(false).to_string_lossy()).collect();
    let expected: Vec<String> = expected.iter().map(|uri| uri.to_string(false).to_string_lossy()).collect();
    assert_eq!(actual, expected);
}

/// Os estados que `testObject.onDidChangeStatus` anunciou.
fn record_statuses(disposables: &Rc<DisposableStore>, test_object: &TestSynchroniser) -> Rc<RefCell<Vec<SyncStatus>>> {
    let actual = Rc::new(RefCell::new(Vec::new()));
    let sink = actual.clone();
    disposables.add(test_object.on_did_change_status().listen(move |status| sink.borrow_mut().push(*status)));
    actual
}

/// `(await testObject.getRemoteUserData(null)).syncData?.content`.
async fn remote_content(test_object: &TestSynchroniser) -> Option<String> {
    test_object.get_remote_user_data(None).await.unwrap().sync_data.map(|sync_data| sync_data.content)
}

/// `(await fileService.readFile(testObject.localResource)).value.toString()`.
async fn local_text(client: &UserDataSyncClient, test_object: &TestSynchroniser) -> String {
    read_text(&client.file_service, &test_object.local_resource).await
}

/// `await testObject.getMachineId()`.
async fn machine_id(test_object: &TestSynchroniser) -> String {
    test_object.current_machine_id_promise.clone().await.unwrap()
}

/// `storageService.get('settings.lastSyncUserData', StorageScope.APPLICATION)`.
fn stored_last_sync_state(client: &UserDataSyncClient) -> Option<String> {
    client.storage_service.get(LAST_SYNC_STATE_KEY, StorageScope::Application, None)
}

/// `{ type, url, headers }` de `server.requests`.
fn recorded(r#type: &str, url: String, headers: IHeaders) -> RecordedRequest {
    RecordedRequest { r#type: r#type.to_string(), url, headers }
}

/// `{ ref: '1', syncData: { version: 1, machineId, content: '0' } }`.
fn expected_last_sync_data(machine_id: &str) -> IRemoteUserData {
    IRemoteUserData::new("1", Some(ISyncData { version: 1.0, machine_id: Some(machine_id.to_string()), content: "0".to_string() }))
}

/// `JSON.stringify({ ref: '1', version: 1, content: JSON.stringify({ content: '0', machineId, version: 1 }),
/// additionalData: { foo: 'bar' } })`: um estado que não é um `IRemoteUserData`.
fn invalid_last_sync_content(machine_id: &str) -> String {
    let inner = format!(r#"{{"content":"0","machineId":"{machine_id}","version":1}}"#);
    let inner = stringify_json(&Value::String(JsString::from(inner.as_str())));
    format!(r#"{{"ref":"1","version":1,"content":{inner},"additionalData":{{"foo":"bar"}}}}"#)
}

// endregion

// region: TestSynchronizer - Auto Sync

#[test]
fn auto_sync_status_is_syncing() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);

        let actual = record_statuses(&d, test_object);

        let promise = test_object.on_do_sync_call.event().to_promise(None);

        let _syncing = sync(test_object, latest_ref(client, test_object).await, false);
        promise.await.unwrap();

        assert_eq!(*actual.borrow(), vec![SyncStatus::Syncing]);
        assert_eq!(test_object.status(), SyncStatus::Syncing);

        let _stopping = test_object.stop();
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_status_is_set_correctly_when_sync_is_finished() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        let actual = record_statuses(&d, test_object);
        sync_latest(client, test_object, false).await.unwrap();

        assert_eq!(*actual.borrow(), vec![SyncStatus::Syncing, SyncStatus::Idle]);
        assert_eq!(test_object.status(), SyncStatus::Idle);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_status_is_set_correctly_when_sync_has_errors() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::ERROR);
        test_object.sync_barrier.borrow().open();

        let actual = record_statuses(&d, test_object);

        assert!(sync_latest(client, test_object, false).await.is_err(), "Should fail");
        assert_eq!(*actual.borrow(), vec![SyncStatus::Syncing, SyncStatus::Idle]);
        assert_eq!(test_object.status(), SyncStatus::Idle);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_status_is_set_to_has_conflicts_when_asked_to_sync_if_there_are_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::CONFLICTS);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        assert_resources(&test_object.conflicts().conflicts, &[&test_object.local_resource]);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_sync_should_not_run_if_syncing_already() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        let promise = test_object.on_do_sync_call.event().to_promise(None);

        let _syncing = sync(test_object, latest_ref(client, test_object).await, false);
        promise.await.unwrap();

        let actual = record_statuses(&d, test_object);
        sync_latest(client, test_object, false).await.unwrap();

        assert_eq!(*actual.borrow(), Vec::<SyncStatus>::new());
        assert_eq!(test_object.status(), SyncStatus::Syncing);

        test_object.stop().await.unwrap();
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_sync_should_not_run_if_there_are_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::CONFLICTS);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();

        let actual = record_statuses(&d, test_object);
        sync_latest(client, test_object, false).await.unwrap();

        assert_eq!(*actual.borrow(), Vec::<SyncStatus>::new());
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_accept_preview_during_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::CONFLICTS);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        test_object.accept(&first_conflict(test_object).preview_resource, None).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_resources(&test_object.conflicts().conflicts, &[]);

        apply(test_object).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_eq!(remote_content(test_object).await, Some(local_text(client, test_object).await));
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_accept_remote_during_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();
        let current_remote_content = remote_content(test_object).await;
        let new_local_content = "conflict";
        write_text(&client.file_service, &test_object.local_resource, new_local_content).await.unwrap();

        test_object.sync_result.set(SyncResult::CONFLICTS);
        sync_latest(client, test_object, false).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        test_object.accept(&first_conflict(test_object).remote_resource, None).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_resources(&test_object.conflicts().conflicts, &[]);

        apply(test_object).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_eq!(remote_content(test_object).await, current_remote_content);
        assert_eq!(Some(local_text(client, test_object).await), current_remote_content);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_accept_local_during_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();
        let new_local_content = "conflict";
        write_text(&client.file_service, &test_object.local_resource, new_local_content).await.unwrap();

        test_object.sync_result.set(SyncResult::CONFLICTS);
        sync_latest(client, test_object, false).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        test_object.accept(&first_conflict(test_object).local_resource, None).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_resources(&test_object.conflicts().conflicts, &[]);

        apply(test_object).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_eq!(remote_content(test_object).await.as_deref(), Some(new_local_content));
        assert_eq!(local_text(client, test_object).await, new_local_content);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_accept_new_content_during_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();
        let new_local_content = "conflict";
        write_text(&client.file_service, &test_object.local_resource, new_local_content).await.unwrap();

        test_object.sync_result.set(SyncResult::CONFLICTS);
        sync_latest(client, test_object, false).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        let merge_content = "newContent";
        test_object.accept(&first_conflict(test_object).preview_resource, Some(Some(merge_content.to_string()))).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_resources(&test_object.conflicts().conflicts, &[]);

        apply(test_object).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_eq!(remote_content(test_object).await.as_deref(), Some(merge_content));
        assert_eq!(local_text(client, test_object).await, merge_content);
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_accept_delete_during_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();
        let new_local_content = "conflict";
        write_text(&client.file_service, &test_object.local_resource, new_local_content).await.unwrap();

        test_object.sync_result.set(SyncResult::CONFLICTS);
        sync_latest(client, test_object, false).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        test_object.accept(&first_conflict(test_object).preview_resource, Some(None)).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_resources(&test_object.conflicts().conflicts, &[]);

        apply(test_object).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_eq!(remote_content(test_object).await.as_deref(), Some(""));
        assert!(!client.file_service.exists(&test_object.local_resource).await.unwrap());
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_accept_deleted_local_during_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();
        client.file_service.del(&test_object.local_resource, None).await.unwrap();

        test_object.sync_result.set(SyncResult::CONFLICTS);
        sync_latest(client, test_object, false).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        test_object.accept(&first_conflict(test_object).local_resource, None).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_resources(&test_object.conflicts().conflicts, &[]);

        apply(test_object).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_eq!(remote_content(test_object).await.as_deref(), Some(""));
        assert!(!client.file_service.exists(&test_object.local_resource).await.unwrap());
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_accept_deleted_remote_during_conflicts() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();
        write_text(&client.file_service, &test_object.local_resource, "some content").await.unwrap();
        test_object.sync_result.set(SyncResult::CONFLICTS);

        sync_latest(client, test_object, false).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::HasConflicts);

        test_object.accept(&first_conflict(test_object).remote_resource, None).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_resources(&test_object.conflicts().conflicts, &[]);

        apply(test_object).await.unwrap();
        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_eq!(test_object.get_remote_user_data(None).await.unwrap().sync_data, None);
        assert!(!client.file_service.exists(&test_object.local_resource).await.unwrap());
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_request_latest_data_on_precondition_failure() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        // Sync once
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();
        *test_object.sync_barrier.borrow_mut() = Barrier::new();

        // update remote data before syncing so that 412 is thrown by server
        let reference = latest_ref(client, test_object).await.expect("o recurso foi sincronizado");
        let (listener_object, listener_server, listener_reference) = (test_object.clone(), server.clone(), reference.clone());
        d.add(test_object.on_do_sync_call.event().once().listen(move |_| {
            let (test_object, server, reference) = (listener_object.clone(), listener_server.clone(), listener_reference.clone());
            let _applying = async_fn(async move {
                test_object.apply_ref(Some(reference.clone()), &reference).await?;
                server.reset();
                test_object.sync_barrier.borrow().open();
                Ok(())
            });
        }));

        // Start sycing
        sync_latest(client, test_object, false).await.unwrap();

        let resource_url = format!("{}/v1/resource/{}", server.url, test_object.resource.as_str());
        let next_reference = (reference.parse::<i64>().unwrap() + 1).to_string();
        assert_eq!(
            server.requests(),
            vec![
                recorded("POST", resource_url.clone(), if_match_headers(&reference)),
                recorded("GET", format!("{resource_url}/latest"), IHeaders::new()),
                recorded("POST", resource_url, if_match_headers(&next_reference)),
            ]
        );
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_no_requests_are_made_to_server_when_local_change_is_triggered() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();

        server.reset();
        let promise = test_object.on_did_trigger_local_change_call.event().to_promise(None);
        test_object.trigger_local_change();

        promise.await.unwrap();
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn auto_sync_status_is_reset_when_getting_latest_remote_data_fails() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.fail_when_getting_latest_remote_user_data.set(true);

        assert!(sync_latest(client, test_object, false).await.is_err(), "Should throw an error");

        assert_eq!(test_object.status(), SyncStatus::Idle);
        teardown(&fixture).await;
    });
}

// endregion

// region: TestSynchronizer - Manual Sync

/// `preivew -> discard` e `conflicts: preivew -> discard`.
async fn preview_discard(d: Rc<DisposableStore>, sync_result: SyncResult) {
    let fixture = setup(&d).await;
    let (client, test_object) = (&fixture.client, &fixture.test_object);
    test_object.sync_result.set(sync_result);
    test_object.sync_barrier.borrow().open();

    let preview = sync_latest(client, test_object, true).await.unwrap();
    let preview = discard(test_object, &first_preview(&preview).preview_resource).await.unwrap();

    assert_eq!(test_object.status(), SyncStatus::Syncing);
    assert_resources(resource_previews(&preview), &[&test_object.local_resource]);
    assert_eq!(first_preview(&preview).merge_state, MergeState::Preview);
    assert_resources(&test_object.conflicts().conflicts, &[]);
    teardown(&fixture).await;
}

/// `preivew -> discard -> accept` e `conflicts: preivew -> discard -> accept`.
async fn preview_discard_accept(d: Rc<DisposableStore>, sync_result: SyncResult) {
    let fixture = setup(&d).await;
    let (client, test_object) = (&fixture.client, &fixture.test_object);
    test_object.sync_result.set(sync_result);
    test_object.sync_barrier.borrow().open();

    let preview = sync_latest(client, test_object, true).await.unwrap();
    let preview = discard(test_object, &first_preview(&preview).preview_resource).await.unwrap();
    let preview = test_object.accept(&first_preview(&preview).remote_resource, None).await.unwrap();

    assert_eq!(test_object.status(), SyncStatus::Syncing);
    assert_resources(resource_previews(&preview), &[&test_object.local_resource]);
    assert_eq!(first_preview(&preview).merge_state, MergeState::Accepted);
    assert_resources(&test_object.conflicts().conflicts, &[]);
    teardown(&fixture).await;
}

/// `preivew -> accept -> discard -> accept` e `conflicts: preivew -> accept -> discard -> accept`.
async fn preview_accept_discard_accept(d: Rc<DisposableStore>, sync_result: SyncResult) {
    let fixture = setup(&d).await;
    let (client, test_object) = (&fixture.client, &fixture.test_object);
    test_object.sync_result.set(sync_result);
    test_object.sync_barrier.borrow().open();

    let preview = sync_latest(client, test_object, true).await.unwrap();
    let preview = test_object.accept(&first_preview(&preview).preview_resource, None).await.unwrap();
    let preview = discard(test_object, &first_preview(&preview).preview_resource).await.unwrap();
    let preview = test_object.accept(&first_preview(&preview).remote_resource, None).await.unwrap();

    assert_eq!(test_object.status(), SyncStatus::Syncing);
    assert_resources(resource_previews(&preview), &[&test_object.local_resource]);
    assert_eq!(first_preview(&preview).merge_state, MergeState::Accepted);
    assert_resources(&test_object.conflicts().conflicts, &[]);
    teardown(&fixture).await;
}

/// `preivew -> accept -> discard` e `conflicts: preivew -> accept -> discard` (os dois sem conflitos).
async fn preview_accept_discard(d: Rc<DisposableStore>) {
    let fixture = setup(&d).await;
    let (client, test_object) = (&fixture.client, &fixture.test_object);
    test_object.sync_result.set(SyncResult::NONE);
    test_object.sync_barrier.borrow().open();

    let preview = sync_latest(client, test_object, true).await.unwrap();
    let preview = test_object.accept(&first_preview(&preview).remote_resource, None).await.unwrap();
    let preview = discard(test_object, &first_preview(&preview).preview_resource).await.unwrap();

    assert_eq!(test_object.status(), SyncStatus::Syncing);
    assert_resources(resource_previews(&preview), &[&test_object.local_resource]);
    assert_eq!(first_preview(&preview).merge_state, MergeState::Preview);
    assert_resources(&test_object.conflicts().conflicts, &[]);
    teardown(&fixture).await;
}

/// `preivew -> discard -> accept -> apply` e `conflicts: preivew -> accept -> discard -> accept -> apply`: o corpo
/// dos dois é o mesmo (aceita o remoto, descarta e aceita o local).
async fn preview_accept_remote_discard_accept_local_apply(d: Rc<DisposableStore>) {
    let fixture = setup(&d).await;
    let (client, test_object) = (&fixture.client, &fixture.test_object);
    test_object.sync_result.set(SyncResult::NONE);
    test_object.sync_barrier.borrow().open();
    sync_latest(client, test_object, false).await.unwrap();

    let expected_content = local_text(client, test_object).await;
    let preview = sync_latest(client, test_object, true).await.unwrap();
    let preview = test_object.accept(&first_preview(&preview).remote_resource, None).await.unwrap();
    let preview = discard(test_object, &first_preview(&preview).preview_resource).await.unwrap();
    let preview = test_object.accept(&first_preview(&preview).local_resource, None).await.unwrap();
    let preview = apply(test_object).await.unwrap();

    assert_eq!(test_object.status(), SyncStatus::Idle);
    assert!(preview.is_none());
    assert_resources(&test_object.conflicts().conflicts, &[]);
    assert_eq!(remote_content(test_object).await.as_deref(), Some(expected_content.as_str()));
    assert_eq!(local_text(client, test_object).await, expected_content);
    teardown(&fixture).await;
}

/// `conflicts: preview -> accept` e `conflicts: preview -> accept 2`: o corpo dos dois é o mesmo.
async fn conflicts_preview_accept(d: Rc<DisposableStore>) {
    let fixture = setup(&d).await;
    let (client, test_object) = (&fixture.client, &fixture.test_object);
    test_object.sync_result.set(SyncResult::CONFLICTS);
    test_object.sync_barrier.borrow().open();

    let preview = sync_latest(client, test_object, true).await.unwrap();
    let content = test_object.resolve_content(&first_preview(&preview).preview_resource).await.unwrap();
    let preview = test_object.accept(&first_preview(&preview).preview_resource, Some(content)).await.unwrap();

    assert_eq!(test_object.status(), SyncStatus::Syncing);
    assert_resources(resource_previews(&preview), &[&test_object.local_resource]);
    assert_resources(&test_object.conflicts().conflicts, &[]);
    teardown(&fixture).await;
}

#[test]
fn manual_sync_preview() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::NONE);
        test_object.sync_barrier.borrow().open();

        let preview = sync_latest(client, test_object, true).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_resources(resource_previews(&preview), &[&test_object.local_resource]);
        assert_eq!(first_preview(&preview).merge_state, MergeState::Accepted);
        assert_resources(&test_object.conflicts().conflicts, &[]);
        teardown(&fixture).await;
    });
}

#[test]
fn manual_sync_preview_accept() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::NONE);
        test_object.sync_barrier.borrow().open();

        let preview = sync_latest(client, test_object, true).await.unwrap();
        let preview = test_object.accept(&first_preview(&preview).local_resource, None).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Syncing);
        assert_resources(resource_previews(&preview), &[&test_object.local_resource]);
        assert_eq!(first_preview(&preview).merge_state, MergeState::Accepted);
        assert_resources(&test_object.conflicts().conflicts, &[]);
        teardown(&fixture).await;
    });
}

#[test]
fn manual_sync_preview_merge_apply() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::NONE);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();

        let reference = latest_ref(client, test_object).await;
        sync(test_object, reference.clone(), true).await.unwrap();
        let preview = apply(test_object).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(preview.is_none());
        assert_resources(&test_object.conflicts().conflicts, &[]);

        assert_eq!(remote_content(test_object).await, reference);
        assert_eq!(Some(local_text(client, test_object).await), reference);
        teardown(&fixture).await;
    });
}

#[test]
fn manual_sync_preview_accept_apply() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::NONE);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();

        let reference = latest_ref(client, test_object).await;
        let preview = sync(test_object, reference.clone(), true).await.unwrap();
        test_object.accept(&first_preview(&preview).preview_resource, None).await.unwrap();
        let preview = apply(test_object).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(preview.is_none());
        assert_resources(&test_object.conflicts().conflicts, &[]);

        assert_eq!(remote_content(test_object).await, reference);
        assert_eq!(Some(local_text(client, test_object).await), reference);
        teardown(&fixture).await;
    });
}

#[test]
fn manual_sync_preivew_discard() {
    async_test(|d| preview_discard(d, SyncResult::NONE));
}

#[test]
fn manual_sync_preivew_discard_accept() {
    async_test(|d| preview_discard_accept(d, SyncResult::NONE));
}

#[test]
fn manual_sync_preivew_accept_discard_accept() {
    async_test(|d| preview_accept_discard_accept(d, SyncResult::NONE));
}

#[test]
fn manual_sync_preivew_accept_discard() {
    async_test(preview_accept_discard);
}

#[test]
fn manual_sync_preivew_discard_accept_apply() {
    async_test(preview_accept_remote_discard_accept_local_apply);
}

#[test]
fn manual_sync_conflicts_preview() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::CONFLICTS);
        test_object.sync_barrier.borrow().open();

        let preview = sync_latest(client, test_object, true).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::HasConflicts);
        assert_resources(resource_previews(&preview), &[&test_object.local_resource]);
        assert_eq!(first_preview(&preview).merge_state, MergeState::Conflict);
        assert_resources(&test_object.conflicts().conflicts, &[&first_preview(&preview).local_resource]);
        teardown(&fixture).await;
    });
}

#[test]
fn manual_sync_conflicts_preview_discard() {
    async_test(|d| preview_discard(d, SyncResult::CONFLICTS));
}

#[test]
fn manual_sync_conflicts_preview_accept() {
    async_test(conflicts_preview_accept);
}

#[test]
fn manual_sync_conflicts_preview_accept_2() {
    async_test(conflicts_preview_accept);
}

#[test]
fn manual_sync_conflicts_preview_accept_apply() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::NONE);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();

        test_object.sync_result.set(SyncResult::CONFLICTS);
        let reference = latest_ref(client, test_object).await;
        let preview = sync(test_object, reference.clone(), true).await.unwrap();

        test_object.accept(&first_preview(&preview).preview_resource, None).await.unwrap();
        let preview = apply(test_object).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(preview.is_none());
        assert_resources(&test_object.conflicts().conflicts, &[]);

        assert_eq!(remote_content(test_object).await, reference);
        assert_eq!(Some(local_text(client, test_object).await), reference);
        teardown(&fixture).await;
    });
}

#[test]
fn manual_sync_conflicts_preivew_discard() {
    async_test(|d| preview_discard(d, SyncResult::CONFLICTS));
}

#[test]
fn manual_sync_conflicts_preivew_discard_accept() {
    async_test(|d| preview_discard_accept(d, SyncResult::CONFLICTS));
}

#[test]
fn manual_sync_conflicts_preivew_accept_discard_accept() {
    async_test(|d| preview_accept_discard_accept(d, SyncResult::CONFLICTS));
}

#[test]
fn manual_sync_conflicts_preivew_accept_discard() {
    async_test(preview_accept_discard);
}

#[test]
fn manual_sync_conflicts_preivew_discard_accept_apply() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_result.set(SyncResult::NONE);
        test_object.sync_barrier.borrow().open();
        sync_latest(client, test_object, false).await.unwrap();

        let expected_content = local_text(client, test_object).await;
        let preview = sync_latest(client, test_object, true).await.unwrap();
        let preview = discard(test_object, &first_preview(&preview).preview_resource).await.unwrap();
        test_object.accept(&first_preview(&preview).local_resource, None).await.unwrap();
        let preview = apply(test_object).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert!(preview.is_none());
        assert_resources(&test_object.conflicts().conflicts, &[]);
        assert_eq!(remote_content(test_object).await.as_deref(), Some(expected_content.as_str()));
        assert_eq!(local_text(client, test_object).await, expected_content);
        teardown(&fixture).await;
    });
}

#[test]
fn manual_sync_conflicts_preivew_accept_discard_accept_apply() {
    async_test(preview_accept_remote_discard_accept_local_apply);
}

#[test]
fn manual_sync_remote_is_accepted_if_last_sync_state_does_not_exists_in_server() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();

        let client2 = d.add_rc(UserDataSyncClient::set_up(server.clone(), false).await.unwrap());
        let synchronizer2 = d.add_rc(TestSynchroniser::new(&client2));
        synchronizer2.sync_barrier.borrow().open();
        let reference = latest_ref(&client2, test_object).await;
        sync(&synchronizer2, reference.clone(), false).await.unwrap();

        client.file_service.del(&test_object.last_sync_resource, None).await.unwrap();
        sync_latest(client, test_object, false).await.unwrap();

        assert_eq!(test_object.status(), SyncStatus::Idle);
        assert_eq!(Some(local_text(client, test_object).await), reference);
        teardown(&fixture).await;
    });
}

// endregion

// region: TestSynchronizer - Last Sync Data

#[test]
fn last_sync_data_last_sync_data_is_null_when_not_synced_before() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let test_object = &fixture.test_object;

        let actual = test_object.get_last_sync_user_data().await.unwrap();

        assert_eq!(actual, None);
        teardown(&fixture).await;
    });
}

#[test]
fn last_sync_data_last_sync_data_is_set_after_sync() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();
        let machine_id = machine_id(test_object).await;
        let actual = test_object.get_last_sync_user_data().await.unwrap();

        assert_eq!(stored_last_sync_state(client).as_deref(), Some(r#"{"ref":"1"}"#));
        let stored = IRemoteUserData::from_json(&parse_json(&read_text(&client.file_service, &test_object.last_sync_resource).await).unwrap());
        assert_eq!(stored, Some(expected_last_sync_data(&machine_id)));
        assert_eq!(actual, Some(expected_last_sync_data(&machine_id)));
        teardown(&fixture).await;
    });
}

#[test]
fn last_sync_data_last_sync_data_is_read_from_server_after_sync_if_last_sync_resource_is_deleted() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();
        let machine_id = machine_id(test_object).await;
        client.file_service.del(&test_object.last_sync_resource, None).await.unwrap();
        let actual = test_object.get_last_sync_user_data().await.unwrap();

        assert_eq!(stored_last_sync_state(client).as_deref(), Some(r#"{"ref":"1"}"#));
        assert_eq!(actual, Some(expected_last_sync_data(&machine_id)));
        teardown(&fixture).await;
    });
}

#[test]
fn last_sync_data_last_sync_data_is_read_from_server_after_sync_and_sync_data_is_invalid() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();
        let machine_id = machine_id(test_object).await;
        write_text(&client.file_service, &test_object.last_sync_resource, &invalid_last_sync_content(&machine_id)).await.unwrap();
        server.reset();
        let actual = test_object.get_last_sync_user_data().await.unwrap();

        assert_eq!(stored_last_sync_state(client).as_deref(), Some(r#"{"ref":"1"}"#));
        assert_eq!(actual, Some(expected_last_sync_data(&machine_id)));
        assert_eq!(server.requests(), vec![recorded("GET", "http://host:3000/v1/resource/settings/1".to_string(), IHeaders::new())]);
        teardown(&fixture).await;
    });
}

#[test]
fn last_sync_data_last_sync_data_is_read_from_server_after_sync_and_stored_sync_data_is_tampered() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();
        let machine_id = machine_id(test_object).await;
        let tampered = format!(r#"{{"ref":"2","syncData":{{"content":"0","machineId":"{machine_id}","version":1}}}}"#);
        write_text(&client.file_service, &test_object.last_sync_resource, &tampered).await.unwrap();
        server.reset();
        let actual = test_object.get_last_sync_user_data().await.unwrap();

        assert_eq!(stored_last_sync_state(client).as_deref(), Some(r#"{"ref":"1"}"#));
        assert_eq!(actual, Some(expected_last_sync_data(&machine_id)));
        assert_eq!(server.requests(), vec![recorded("GET", "http://host:3000/v1/resource/settings/1".to_string(), IHeaders::new())]);
        teardown(&fixture).await;
    });
}

#[test]
fn last_sync_data_reading_last_sync_data_no_requests_are_made_to_server_when_sync_data_is_invalid() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();
        let machine_id = machine_id(test_object).await;
        write_text(&client.file_service, &test_object.last_sync_resource, &invalid_last_sync_content(&machine_id)).await.unwrap();
        test_object.get_last_sync_user_data().await.unwrap();
        server.reset();

        test_object.get_last_sync_user_data().await.unwrap();
        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn last_sync_data_reading_last_sync_data_no_requests_are_made_to_server_when_sync_data_is_null() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (server, client, test_object) = (&fixture.server, &fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();
        server.reset();
        write_text(&client.file_service, &test_object.last_sync_resource, r#"{"ref":"1","syncData":null}"#).await.unwrap();
        test_object.get_last_sync_user_data().await.unwrap();

        assert_eq!(server.requests(), Vec::<RecordedRequest>::new());
        teardown(&fixture).await;
    });
}

#[test]
fn last_sync_data_last_sync_data_is_null_after_sync_if_last_sync_state_is_deleted() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();
        client.storage_service.remove(LAST_SYNC_STATE_KEY, StorageScope::Application);
        let actual = test_object.get_last_sync_user_data().await.unwrap();

        assert_eq!(actual, None);
        teardown(&fixture).await;
    });
}

#[test]
fn last_sync_data_last_sync_data_is_null_after_sync_if_last_sync_content_is_deleted_everywhere() {
    async_test(|d| async move {
        let fixture = setup(&d).await;
        let (client, test_object) = (&fixture.client, &fixture.test_object);
        test_object.sync_barrier.borrow().open();

        sync_latest(client, test_object, false).await.unwrap();
        client.file_service.del(&test_object.last_sync_resource, None).await.unwrap();
        client.user_data_sync_store_service.delete_resource(ServerResource::from(test_object.sync_resource.sync_resource), None, None).await.unwrap();
        let actual = test_object.get_last_sync_user_data().await.unwrap();

        assert_eq!(stored_last_sync_state(client).as_deref(), Some(r#"{"ref":"1"}"#));
        assert_eq!(actual, None);
        teardown(&fixture).await;
    });
}

// endregion
