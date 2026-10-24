//! Porte de `src/vs/workbench/contrib/url/test/browser/trustedDomainsValidator.test.ts` (suíte
//! `OpenerValidatorContributions`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - Os serviços de confiança do original são literais com um ou dois métodos convertidos por `as`: aqui são structs que
//!   implementam o trait inteiro, e os métodos que o literal não tem falham com mensagem (o `TypeError` do JavaScript).
//! - `NullOpenerService` (`platform/opener/test/common/nullOpenerService.ts`) é a struct de mesmo nome neste arquivo.
//! - Os `undefined!` do construtor são `None`, e `workspaceTrustManagementService`/`workspaceTrustRequestService` são os
//!   fakes acima. O construtor não recebe os serviços de armazenamento, quick input, editores, telemetria e
//!   instanciação (ver as divergências do módulo de produção).
//! - Os links são strings, como no original (`OpenResource::Text`), e `allowCommands` é `AllowCommands`.
//! - `assert.deepStrictEqual` sobre o objeto vira `assert_eq!` sobre uma struct com os mesmos campos.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers};
use je_base::JsString;
use je_base::event::Event;
use je_base::lifecycle::{self, DisposableRef};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_platform::opener::{
    AllowCommands, IExternalOpener, IExternalUriResolver, IOpener, IOpenerService, IValidator, OpenOptions,
    OpenResource, ResolveExternalUriOptions, ResolvedExternalUri,
};
use je_platform::workspace_trust::{
    IWorkspaceTrustManagementService, IWorkspaceTrustRequestService, IWorkspaceTrustTransitionParticipant, ResourceTrustRequestOptions,
    WorkspaceTrustRequestOptions, WorkspaceTrustUriInfo, WorkspaceTrustUriResponse,
};
use je_workbench::contrib::url::trusted_domains_validator::OpenerValidatorContributions;

/// `NullOpenerService`.
struct NullOpenerService;

impl IOpenerService for NullOpenerService {
    fn register_opener(&self, _opener: Rc<dyn IOpener>) -> DisposableRef {
        lifecycle::none()
    }
    fn register_validator(&self, _validator: Rc<dyn IValidator>) -> DisposableRef {
        lifecycle::none()
    }
    fn register_external_uri_resolver(&self, _resolver: Rc<dyn IExternalUriResolver>) -> DisposableRef {
        lifecycle::none()
    }
    fn set_default_external_opener(&self, _opener: Rc<dyn IExternalOpener>) {}
    fn register_external_opener(&self, _opener: Rc<dyn IExternalOpener>) -> DisposableRef {
        lifecycle::none()
    }
    fn open(&self, _resource: &OpenResource, _options: Option<&OpenOptions>) -> Promise<bool> {
        Promise::resolved(false)
    }
    fn resolve_external_uri(&self, resource: &URI, _options: Option<&ResolveExternalUriOptions>) -> Promise<ResolvedExternalUri> {
        Promise::resolved(ResolvedExternalUri { resolved: resource.clone(), disposable: None })
    }
}

fn missing(member: &str) -> ! {
    panic!("TypeError: {member} is not a function")
}

/// `{ isWorkspaceTrusted: () => isWorkspaceTrusted } as IWorkspaceTrustManagementService`.
struct FakeWorkspaceTrustManagementService {
    is_workspace_trusted: bool,
}

impl IWorkspaceTrustManagementService for FakeWorkspaceTrustManagementService {
    fn on_did_change_trust(&self) -> Event<bool> {
        missing("onDidChangeTrust")
    }
    fn on_did_change_trusted_folders(&self) -> Event<()> {
        missing("onDidChangeTrustedFolders")
    }
    fn workspace_resolved(&self) -> Promise<()> {
        missing("workspaceResolved")
    }
    fn workspace_trust_initialized(&self) -> Promise<()> {
        missing("workspaceTrustInitialized")
    }
    fn accepts_out_of_workspace_files(&self) -> bool {
        missing("acceptsOutOfWorkspaceFiles")
    }
    fn set_accepts_out_of_workspace_files(&self, _value: bool) {
        missing("acceptsOutOfWorkspaceFiles")
    }
    fn is_workspace_trusted(&self) -> bool {
        self.is_workspace_trusted
    }
    fn is_workspace_trust_forced(&self) -> bool {
        missing("isWorkspaceTrustForced")
    }
    fn can_set_parent_folder_trust(&self) -> bool {
        missing("canSetParentFolderTrust")
    }
    fn set_parent_folder_trust(&self, _trusted: bool) -> Promise<()> {
        missing("setParentFolderTrust")
    }
    fn can_set_workspace_trust(&self) -> bool {
        missing("canSetWorkspaceTrust")
    }
    fn set_workspace_trust(&self, _trusted: bool) -> Promise<()> {
        missing("setWorkspaceTrust")
    }
    fn get_uri_trust_info(&self, _uri: &URI) -> Promise<WorkspaceTrustUriInfo> {
        missing("getUriTrustInfo")
    }
    fn set_uris_trust(&self, _uris: &[URI], _trusted: bool) -> Promise<()> {
        missing("setUrisTrust")
    }
    fn register_trusted_authority(&self, _scheme: &str, _authority: &str) -> DisposableRef {
        missing("registerTrustedAuthority")
    }
    fn get_trusted_uris(&self) -> Vec<URI> {
        missing("getTrustedUris")
    }
    fn set_trusted_uris(&self, _uris: &[URI]) -> Promise<()> {
        missing("setTrustedUris")
    }
    fn add_workspace_trust_transition_participant(&self, _participant: Rc<dyn IWorkspaceTrustTransitionParticipant>) -> DisposableRef {
        missing("addWorkspaceTrustTransitionParticipant")
    }
}

/// `{ requestWorkspaceTrust: async () => { trustRequestCount++; return trustRequestResult; } } as IWorkspaceTrustRequestService`.
struct FakeWorkspaceTrustRequestService {
    trust_request_count: Rc<Cell<u32>>,
    trust_request_result: Option<bool>,
}

impl IWorkspaceTrustRequestService for FakeWorkspaceTrustRequestService {
    fn on_did_initiate_open_files_trust_request(&self) -> Event<()> {
        missing("onDidInitiateOpenFilesTrustRequest")
    }
    fn on_did_initiate_workspace_trust_request(&self) -> Event<Option<WorkspaceTrustRequestOptions>> {
        missing("onDidInitiateWorkspaceTrustRequest")
    }
    fn on_did_initiate_workspace_trust_request_on_startup(&self) -> Event<()> {
        missing("onDidInitiateWorkspaceTrustRequestOnStartup")
    }
    fn on_did_initiate_resources_trust_request(&self) -> Event<ResourceTrustRequestOptions> {
        missing("onDidInitiateResourcesTrustRequest")
    }
    fn complete_open_files_trust_request(&self, _result: WorkspaceTrustUriResponse, _save_response: Option<bool>) -> Promise<()> {
        missing("completeOpenFilesTrustRequest")
    }
    fn request_open_files_trust(&self, _open_files: &[URI]) -> Promise<WorkspaceTrustUriResponse> {
        missing("requestOpenFilesTrust")
    }
    fn complete_resources_trust_request(&self, _uri: &URI, _result: WorkspaceTrustUriResponse) -> Promise<()> {
        missing("completeResourcesTrustRequest")
    }
    fn request_resources_trust(&self, _options: ResourceTrustRequestOptions) -> Promise<Option<bool>> {
        missing("requestResourcesTrust")
    }
    fn cancel_workspace_trust_request(&self) {
        missing("cancelWorkspaceTrustRequest")
    }
    fn complete_workspace_trust_request(&self, _trusted: Option<bool>) -> Promise<()> {
        missing("completeWorkspaceTrustRequest")
    }
    fn request_workspace_trust(&self, _options: Option<WorkspaceTrustRequestOptions>) -> Promise<Option<bool>> {
        self.trust_request_count.set(self.trust_request_count.get() + 1);
        Promise::resolved(self.trust_request_result)
    }
    fn request_workspace_trust_on_startup(&self) {
        missing("requestWorkspaceTrustOnStartup")
    }
}

struct Validator {
    validator: Rc<OpenerValidatorContributions>,
    trust_request_count: Rc<Cell<u32>>,
}

impl Validator {
    fn get_trust_request_count(&self) -> u32 {
        self.trust_request_count.get()
    }
}

fn create_validator(is_workspace_trusted: bool, trust_request_result: Option<bool>) -> Validator {
    let trust_request_count = Rc::new(Cell::new(0));
    let workspace_trust_management_service = Rc::new(FakeWorkspaceTrustManagementService { is_workspace_trusted });
    let workspace_trust_request_service =
        Rc::new(FakeWorkspaceTrustRequestService { trust_request_count: trust_request_count.clone(), trust_request_result });

    let opener_service: Rc<dyn IOpenerService> = Rc::new(NullOpenerService);
    let validator = OpenerValidatorContributions::new(
        &opener_service,
        None,
        None,
        None,
        None,
        workspace_trust_management_service,
        workspace_trust_request_service,
        None,
        None,
    );

    Validator { validator, trust_request_count }
}

fn command_link() -> OpenResource {
    OpenResource::from("command:workbench.action.openSettings?%22security.workspace.trust.enabled%22")
}

fn options(from_workspace: bool, allow_commands: AllowCommands) -> OpenOptions {
    OpenOptions { from_workspace: Some(from_workspace), allow_commands: Some(allow_commands), ..OpenOptions::default() }
}

#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    trusted: bool,
    trusted_request_count: u32,
    accepted: bool,
    accepted_request_count: u32,
    cancelled: bool,
    cancelled_request_count: u32,
    not_from_workspace: bool,
    not_from_workspace_request_count: u32,
    commands_disabled: bool,
    commands_disabled_request_count: u32,
    command_not_allowlisted: bool,
    command_not_allowlisted_request_count: u32,
}

#[test]
fn requires_workspace_trust_for_command_uris_from_the_workspace() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let trusted = create_validator(true, None);
    let accepted = create_validator(false, Some(true));
    let cancelled = create_validator(false, None);
    let not_from_workspace = create_validator(false, None);
    let commands_disabled = create_validator(false, None);
    let command_not_allowlisted = create_validator(false, None);

    run_with_faked_timers(async move {
        let actual = Outcome {
            trusted: trusted.validator.validate_link(command_link(), Some(&options(true, AllowCommands::Bool(true)))).await.expect("trusted"),
            trusted_request_count: trusted.get_trust_request_count(),
            accepted: accepted.validator.validate_link(command_link(), Some(&options(true, AllowCommands::Bool(true)))).await.expect("accepted"),
            accepted_request_count: accepted.get_trust_request_count(),
            cancelled: cancelled.validator.validate_link(command_link(), Some(&options(true, AllowCommands::Bool(true)))).await.expect("cancelled"),
            cancelled_request_count: cancelled.get_trust_request_count(),
            not_from_workspace: not_from_workspace.validator.validate_link(command_link(), None).await.expect("notFromWorkspace"),
            not_from_workspace_request_count: not_from_workspace.get_trust_request_count(),
            commands_disabled: commands_disabled
                .validator
                .validate_link(command_link(), Some(&options(true, AllowCommands::Bool(false))))
                .await
                .expect("commandsDisabled"),
            commands_disabled_request_count: commands_disabled.get_trust_request_count(),
            command_not_allowlisted: command_not_allowlisted
                .validator
                .validate_link(command_link(), Some(&options(true, AllowCommands::Only(vec![JsString::from("workbench.action.closeWindow")]))))
                .await
                .expect("commandNotAllowlisted"),
            command_not_allowlisted_request_count: command_not_allowlisted.get_trust_request_count(),
        };

        assert_eq!(
            actual,
            Outcome {
                trusted: true,
                trusted_request_count: 0,
                accepted: true,
                accepted_request_count: 1,
                cancelled: false,
                cancelled_request_count: 1,
                not_from_workspace: true,
                not_from_workspace_request_count: 0,
                commands_disabled: true,
                commands_disabled_request_count: 0,
                command_not_allowlisted: true,
                command_not_allowlisted_request_count: 0,
            }
        );
    });
}
