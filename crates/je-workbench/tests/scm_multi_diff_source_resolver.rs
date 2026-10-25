//! Porte de `src/vs/workbench/contrib/multiDiffEditor/test/browser/scmMultiDiffSourceResolver.test.ts` (suítes
//! `ScmMultiDiffSourceResolver` e `ScmHistoryItemResolver`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard`, criado no início de cada caso (que roda no relógio real, como
//!   os casos `async` do mocha); os `Emitter` do original entram no guarda com `add_rc`.
//! - `new class extends mock<ISCMProvider>() { ... }` e os demais `mock<...>` são os dublês [`StubResource`], [`StubGroup`],
//!   [`StubProvider`], [`StubRepository`], [`StubScmService`], [`StubActivityService`] e [`StubHistoryProvider`], que
//!   implementam os contratos de `je_workbench::contrib::scm::common::scm` (recorte do que o resolvedor lê). Os membros que o
//!   `mock` deixaria sem valor (e que o caso não toca) têm o valor vazio do contrato.
//! - `getRepository(idOrResource)` aceita só o id (ver `scm.rs`); `Event.None` é `Event::none()`; `observableValue(this, x)` é
//!   `observable_value`.
//! - `URI.file(path)` é `uri_file` (a plataforma do hospedeiro); `assert.deepStrictEqual` é `assert_eq!` sobre valores
//!   comparáveis.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::RefCell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_real_timers};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableRef, IDisposable};
use je_base::observable::base::{IObservable, Observable};
use je_base::observable::{const_observable, observable_value};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::resources::uri_file;
use je_workbench::contrib::multi_diff_editor::browser::multi_diff_source_resolver_service::IMultiDiffSourceResolver;
use je_workbench::contrib::multi_diff_editor::browser::scm_multi_diff_source_resolver::{ScmHistoryItemResolver, ScmMultiDiffSourceResolver};
use je_workbench::contrib::scm::common::history::ISCMHistoryItemChange;
use je_workbench::contrib::scm::common::scm::{ISCMHistoryProvider, ISCMInput, ISCMProvider, ISCMRepository, ISCMResource, ISCMResourceGroup, ISCMService};
use je_workbench::services::activity::activity::{ActivityChangeTarget, IActivity, IActivityService};

/// `new class extends mock<ISCMResource>() { sourceUri; multiDiffEditorModifiedUri = this.sourceUri }`.
struct StubResource {
    source_uri: URI,
}

impl ISCMResource for StubResource {
    fn source_uri(&self) -> URI {
        self.source_uri.clone()
    }

    fn multi_diff_editor_modified_uri(&self) -> Option<URI> {
        Some(self.source_uri.clone())
    }
}

/// O `ISCMResourceGroup` com `id = 'changes'`, `label = 'Changes'` e a lista de recursos que o caso preenche depois.
struct StubGroup {
    resources: Rc<RefCell<Vec<Rc<dyn ISCMResource>>>>,
    on_did_change_resources: Rc<Emitter<()>>,
}

impl ISCMResourceGroup for StubGroup {
    fn id(&self) -> String {
        "changes".to_string()
    }

    fn label(&self) -> String {
        "Changes".to_string()
    }

    fn resources(&self) -> Vec<Rc<dyn ISCMResource>> {
        self.resources.borrow().clone()
    }

    fn on_did_change_resources(&self) -> Event<()> {
        self.on_did_change_resources.event()
    }
}

/// O `ISCMProvider` de `id = 'scm0'`.
struct StubProvider {
    root_uri: URI,
    provider_id: String,
    groups: Vec<Rc<dyn ISCMResourceGroup>>,
    on_did_change_resource_groups: Rc<Emitter<()>>,
    history_provider: IObservable<Option<Rc<dyn ISCMHistoryProvider>>>,
}

impl IDisposable for StubProvider {
    fn dispose(&self) {}
}

impl ISCMProvider for StubProvider {
    fn id(&self) -> String {
        "scm0".to_string()
    }

    fn provider_id(&self) -> String {
        self.provider_id.clone()
    }

    fn label(&self) -> String {
        panic!("o caso não lê o rótulo do provedor");
    }

    fn name(&self) -> String {
        panic!("o caso não lê o nome do provedor");
    }

    fn root_uri(&self) -> Option<URI> {
        Some(self.root_uri.clone())
    }

    fn groups(&self) -> Vec<Rc<dyn ISCMResourceGroup>> {
        self.groups.clone()
    }

    fn on_did_change_resource_groups(&self) -> Event<()> {
        self.on_did_change_resource_groups.event()
    }

    fn history_provider(&self) -> IObservable<Option<Rc<dyn ISCMHistoryProvider>>> {
        self.history_provider.clone()
    }
}

/// `new class extends mock<ISCMRepository>() { id = provider.id; provider }`.
struct StubRepository {
    provider: Rc<dyn ISCMProvider>,
}

impl IDisposable for StubRepository {
    fn dispose(&self) {}
}

impl ISCMRepository for StubRepository {
    fn id(&self) -> String {
        self.provider.id()
    }

    fn provider(&self) -> Rc<dyn ISCMProvider> {
        self.provider.clone()
    }

    fn input(&self) -> Rc<dyn ISCMInput> {
        panic!("o caso não lê a caixa de entrada do repositório");
    }
}

/// O `ISCMService` com a lista mutável de repositórios e o `onDidAddRepository`.
struct StubScmService {
    repositories: Rc<RefCell<Vec<Rc<dyn ISCMRepository>>>>,
    on_did_add_repository: Rc<Emitter<Rc<dyn ISCMRepository>>>,
}

impl ISCMService for StubScmService {
    fn on_did_add_repository(&self) -> Event<Rc<dyn ISCMRepository>> {
        self.on_did_add_repository.event()
    }

    fn repositories(&self) -> Vec<Rc<dyn ISCMRepository>> {
        self.repositories.borrow().clone()
    }

    fn on_did_remove_repository(&self) -> Event<Rc<dyn ISCMRepository>> {
        Event::none()
    }

    fn register_scm_provider(&self, _provider: Rc<dyn ISCMProvider>) -> Rc<dyn ISCMRepository> {
        panic!("o caso não registra provedor");
    }

    fn get_repository(&self, id: &str) -> Option<Rc<dyn ISCMRepository>> {
        self.repositories.borrow().iter().find(|r| r.id() == id).cloned()
    }

    fn get_repository_by_resource(&self, _resource: &URI) -> Option<Rc<dyn ISCMRepository>> {
        panic!("o caso não procura o repositório pelo recurso");
    }
}

/// `onDidChangeActivity = Event.None; getViewContainerActivities() { return []; }`: os outros membros do `mock<IActivityService>()`
/// lançam se chamados.
struct StubActivityService;

impl IActivityService for StubActivityService {
    fn on_did_change_activity(&self) -> Event<ActivityChangeTarget> {
        Event::none()
    }

    fn show_view_container_activity(&self, _view_container_id: &str, _badge: IActivity) -> DisposableRef {
        panic!("showViewContainerActivity is not a function");
    }

    fn get_view_container_activities(&self, _view_container_id: &str) -> Vec<IActivity> {
        Vec::new()
    }

    fn show_view_activity(&self, _view_id: &str, _badge: IActivity) -> DisposableRef {
        panic!("o caso não mostra atividade");
    }

    fn show_accounts_activity(&self, _activity: IActivity) -> DisposableRef {
        panic!("showAccountsActivity is not a function");
    }

    fn show_global_activity(&self, _activity: IActivity) -> DisposableRef {
        panic!("showGlobalActivity is not a function");
    }

    fn get_activity(&self, _id: &str) -> Vec<IActivity> {
        panic!("getActivity is not a function");
    }
}

/// O `ISCMHistoryProvider` que anota as chamadas de `provideHistoryItemChanges` e devolve uma mudança.
struct StubHistoryProvider {
    calls: Rc<RefCell<Vec<(String, Option<String>)>>>,
    change: ISCMHistoryItemChange,
}

impl ISCMHistoryProvider for StubHistoryProvider {
    fn provide_history_item_changes(&self, history_item_id: &str, history_item_parent_id: Option<&str>) -> Promise<Option<Vec<ISCMHistoryItemChange>>> {
        self.calls.borrow_mut().push((history_item_id.to_string(), history_item_parent_id.map(str::to_string)));
        Promise::resolved(Some(vec![self.change.clone()]))
    }
}

#[test]
fn scm_multi_diff_source_resolver_sorts_resources_by_path() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();

        let resources: Rc<RefCell<Vec<Rc<dyn ISCMResource>>>> = Rc::new(RefCell::new(Vec::new()));
        let on_did_change_resources = guard.add_rc(Emitter::<()>::new());
        let group: Rc<dyn ISCMResourceGroup> = Rc::new(StubGroup { resources: resources.clone(), on_did_change_resources: on_did_change_resources.clone() });
        let on_did_change_resource_groups = guard.add_rc(Emitter::<()>::new());
        let provider: Rc<dyn ISCMProvider> = Rc::new(StubProvider {
            root_uri: uri_file("/repository"),
            provider_id: "scm0".to_string(),
            groups: vec![group.clone()],
            on_did_change_resource_groups,
            history_provider: const_observable(None),
        });
        let repository: Rc<dyn ISCMRepository> = Rc::new(StubRepository { provider: provider.clone() });
        let repositories = Rc::new(RefCell::new(vec![repository]));
        let on_did_add_repository = guard.add_rc(Emitter::<Rc<dyn ISCMRepository>>::new());
        let scm_service = Rc::new(StubScmService { repositories, on_did_add_repository });

        let resolver = ScmMultiDiffSourceResolver::new(scm_service, Rc::new(StubActivityService));
        let source_uri = ScmMultiDiffSourceResolver::get_multi_diff_source_uri(&provider.root_uri().unwrap().to_string(false).to_string_lossy(), &group.id());
        let source_promise = resolver.resolve_diff_source(&source_uri);

        let create_resource = |path: &str| -> Rc<dyn ISCMResource> { Rc::new(StubResource { source_uri: uri_file(path) }) };

        resources.borrow_mut().extend([
            create_resource("/repository/githubServer.ts"),
            create_resource("/repository/env.ts"),
            create_resource("/repository/githubUri.ts"),
        ]);
        on_did_change_resources.fire(());

        let source = source_promise.await.unwrap();
        let paths: Vec<Option<String>> = source.resources().value().iter().map(|resource| resource.go_to_file_uri.as_ref().map(|uri| uri.path().to_string_lossy())).collect();
        assert_eq!(
            paths,
            vec![
                Some("/repository/env.ts".to_string()),
                Some("/repository/githubServer.ts".to_string()),
                Some("/repository/githubUri.ts".to_string()),
            ]
        );
    });
}

#[test]
fn scm_history_item_resolver_waits_for_the_repository_to_be_added() {
    run_with_real_timers(async {
        let guard = ensure_no_disposables_are_leaked_in_test_suite();

        let calls: Rc<RefCell<Vec<(String, Option<String>)>>> = Rc::new(RefCell::new(Vec::new()));
        let original_uri = uri_file("original.ts");
        let modified_uri = uri_file("modified.ts");
        let history_provider: Rc<dyn ISCMHistoryProvider> = Rc::new(StubHistoryProvider {
            calls: calls.clone(),
            change: ISCMHistoryItemChange { uri: modified_uri.clone(), original_uri: Some(original_uri.clone()), modified_uri: Some(modified_uri.clone()) },
        });
        let history_provider_value = observable_value::<Option<Rc<dyn ISCMHistoryProvider>>, ()>("historyProvider", Some(history_provider));
        let provider: Rc<dyn ISCMProvider> = Rc::new(StubProvider {
            root_uri: uri_file("repository"),
            provider_id: "scm0".to_string(),
            groups: Vec::new(),
            on_did_change_resource_groups: guard.add_rc(Emitter::<()>::new()),
            history_provider: history_provider_value.to_observable(),
        });
        let repository: Rc<dyn ISCMRepository> = Rc::new(StubRepository { provider: provider.clone() });

        let repositories: Rc<RefCell<Vec<Rc<dyn ISCMRepository>>>> = Rc::new(RefCell::new(Vec::new()));
        let on_did_add_repository = guard.add_rc(Emitter::<Rc<dyn ISCMRepository>>::new());
        let scm_service = Rc::new(StubScmService { repositories: repositories.clone(), on_did_add_repository: on_did_add_repository.clone() });

        let resolver = ScmHistoryItemResolver::new(scm_service);
        let source_uri = ScmHistoryItemResolver::get_multi_diff_source_uri(provider.as_ref(), "commit", Some("parent"), Some("display"));
        let source_promise = resolver.resolve_diff_source(&source_uri);

        repositories.borrow_mut().push(repository.clone());
        on_did_add_repository.fire(repository);

        let source = source_promise.await.unwrap();
        let resources: Vec<(Option<String>, Option<String>, Option<String>, Option<String>)> = source
            .resources()
            .value()
            .iter()
            .map(|resource| {
                (
                    resource.original_uri.as_ref().map(|uri| uri.to_string(false).to_string_lossy()),
                    resource.modified_uri.as_ref().map(|uri| uri.to_string(false).to_string_lossy()),
                    resource.go_to_file_uri.as_ref().map(|uri| uri.to_string(false).to_string_lossy()),
                    resource.go_to_file_editor_title.clone(),
                )
            })
            .collect();
        assert_eq!(*calls.borrow(), vec![("commit".to_string(), Some("parent".to_string()))]);
        assert_eq!(
            resources,
            vec![(
                Some(original_uri.to_string(false).to_string_lossy()),
                Some(modified_uri.to_string(false).to_string_lossy()),
                Some(modified_uri.to_string(false).to_string_lossy()),
                Some("modified.ts (display)".to_string()),
            )]
        );
    });
}
