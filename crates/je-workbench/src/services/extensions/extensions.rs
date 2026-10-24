//! Porte parcial de `src/vs/workbench/services/extensions/common/extensions.ts`: o contrato `IExtensionService` reduzido
//! ao que os serviços do workbench portados leem dele.
//!
//! Divergências:
//! - **Contrato reduzido** ao que os serviços portados leem: `getExtension(id)` (o `configurationResolverService`, para
//!   `${extensionInstallFolder:id}`), `onDidRegisterExtensions` e `whenInstalledExtensionsRegistered` (o
//!   `EditorResolverService` e o `TextFileEditorModel`). O restante (o ciclo de vida do host de extensões,
//!   `activateById`...) entra com o porte do `ExtensionService`.
//! - **`IExtensionDescription`** devolve só o `extensionLocation`, que é o campo que esses serviços leem; o descritor
//!   inteiro (manifesto, identificador, `isBuiltin`...) é o `ExtensionDescription` do `je-extensions`, que guarda o local
//!   como texto e exige o manifesto.
//! - **`extensions`** (o `readonly extensions: IExtensionDescription[]` do original, lido pelo `KeybindingsEditorModel`)
//!   devolve o `ExtensionDescription` do `je-extensions`, o descritor inteiro, e não o `IExtensionDescription` reduzido
//!   acima. Quem implementa [`IExtensionService`] precisa implementá-lo (o dublê `TestExtensionService` devolve a lista
//!   vazia, como o `extensions = []` do original).
//! - `activateByEvent` e `activationEventIsDone` entram com a ponte `MainThreadCommands`; `ActivationKind` é o enum
//!   [`ActivationKind`] e o `activationKind?` é `Option<ActivationKind>`.
//! - `onDidChangeExtensionsStatus` e `canRemoveExtension` (lidos pelo `ContributedExternalUriOpenersStore`) têm corpo
//!   padrão, o do `NullExtensionService` (`Event.None` e `false`).
//! - `nullExtensionDescription`, `isProposedApiEnabled` e `checkProposedApiEnabled` trabalham sobre o
//!   `ExtensionDescription` do `je-extensions` ([`null_extension_description`], [`is_proposed_api_enabled`],
//!   [`check_proposed_api_enabled`]); a proposta é o nome como texto (`ApiProposalName` do original), e o erro que o
//!   original lança é o `Err`.
//! - Entram com o `AbstractExtensionService`: `toExtension`, `toExtensionDescription`, `IMessage`, `ActivationTimes`,
//!   `IExtensionsStatus`, `ExtensionPointContribution`, `ExtensionHostStartup`, `IExtensionInspectInfo`, `IExtensionHost`,
//!   `IWillActivateEvent`, `IResponsiveStateChangeEvent` e `IInternalExtensionService`. `ExtensionPointContribution.value` é o
//!   JSON da contribuição. `ExtensionHostExtensions` ([`ExtensionHostExtensions`], com `toSnapshot`) foi portado com o
//!   `ExtensionHostManager`. `IExtensionHostProfile`, `ProfileSession` e `NullExtensionService` não foram portados (os dois
//!   primeiros dependem do perfilador, o último não tem leitor).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use je_base::event::Event;
use je_base::errors::Error;
use je_base::parts::ipc::IMessagePassingProtocol;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::management::{InstallSource, LocalExtension};
use je_extensions::manifest::{
    ExtensionIdentifier, ExtensionIdentifierInfo, ExtensionIdentifierMap, ExtensionIdentifierSet, ExtensionManifest, ExtensionType, TargetPlatform,
};
use je_extensions::scanner::ExtensionDescription;
use je_extensions::util::{get_extension_id, get_gallery_extension_id};
use je_instantiation::service_id;
use je_platform::notification::notification::Severity;

use super::extension_activation::ExtensionActivationReason;
use super::extension_description_registry::SharedExtensionDescription;
use super::extension_host_kind::ExtensionHostKind;
use super::extension_host_protocol::{IExtensionDescriptionDelta, IExtensionDescriptionSnapshot};
use super::extension_running_location::ExtensionRunningLocation;
use super::implicit_activation_events::implicit_activation_events;

/// `nullExtensionDescription`: a descrição de extensão que os testes e o código sem extensão de origem usam.
pub fn null_extension_description() -> ExtensionDescription {
    ExtensionDescription {
        id: "nullExtensionDescription".to_owned(),
        identifier: ExtensionIdentifier::new("nullExtensionDescription"),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: "void:location".to_owned(),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: "Null Extension Description".to_owned(),
            version: "0.0.0".to_owned(),
            publisher: "vscode".to_owned(),
            ..ExtensionManifest::default()
        },
    }
}

/// `isProposedApiEnabled(extension, proposal)`. O `ApiProposalName` do original é o nome da proposta como texto.
pub fn is_proposed_api_enabled(extension: &ExtensionDescription, proposal: &str) -> bool {
    match &extension.manifest.enabled_api_proposals {
        Some(proposals) => proposals.iter().any(|enabled| enabled == proposal),
        None => false,
    }
}

/// `checkProposedApiEnabled(extension, proposal)`: o erro do original quando a extensão não declara a proposta.
pub fn check_proposed_api_enabled(extension: &ExtensionDescription, proposal: &str) -> Result<(), Error> {
    if is_proposed_api_enabled(extension, proposal) {
        return Ok(());
    }
    let declared = extension.manifest.enabled_api_proposals.as_ref().map_or_else(|| "[]".to_owned(), |proposals| proposals.join(", "));
    Err(Error::new(format!(
        "Extension '{id}' CANNOT use API proposal: {proposal}.\nIts package.json#enabledApiProposals-property declares: {declared} but NOT {proposal}.\n The missing proposal MUST be added and you must start in extension development mode or use the following command line switch: --enable-proposed-api {id}",
        id = extension.identifier.value,
    )))
}

/// `IExtensionDescription`, reduzido ao `extensionLocation`.
#[derive(Clone, Debug)]
pub struct IExtensionDescription {
    pub extension_location: URI,
    /// `displayName`, quando o manifesto declara.
    pub display_name: Option<String>,
}

/// `ActivationKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivationKind {
    Normal = 0,
    Immediate = 1,
}

/// `WillStopExtensionHostsEvent`: o evento que os ouvintes de `onWillStop` recebem e usam para vetar a parada dos hosts de
/// extensões (`veto(value, reason)` guarda o voto, que quem disparou o evento lê em [`Self::vetoes`]).
#[derive(Clone)]
pub struct WillStopExtensionHostsEvent {
    /// A human readable reason for stopping the extension hosts that e.g. can be shown in a confirmation dialog to the user.
    pub reason: String,
    /// A flag to indicate if the operation was triggered automatically.
    pub auto: bool,
    vetoes: std::rc::Rc<std::cell::RefCell<Vec<(Promise<bool>, String)>>>,
}

impl WillStopExtensionHostsEvent {
    pub fn new(reason: &str, auto: bool) -> Self {
        Self { reason: reason.to_owned(), auto, vetoes: Default::default() }
    }

    /// `veto(value, reason)`: allows to veto the stopping of extension hosts. The veto can be a long running operation
    /// (`value` é a promessa; o `boolean` é a promessa já resolvida). `reason` is a human readable reason for vetoing the
    /// extension host stop in case where the resolved `value: true`.
    pub fn veto(&self, value: Promise<bool>, reason: &str) {
        self.vetoes.borrow_mut().push((value, reason.to_owned()));
    }

    /// Os votos registrados por `veto`, para quem disparou o evento esperar.
    pub fn vetoes(&self) -> Vec<(Promise<bool>, String)> {
        self.vetoes.borrow().clone()
    }
}

/// `IExtensionService`.
pub trait IExtensionService {
    /// `onWillStop`: an event that is fired when the extension host is about to stop; listeners can veto the stop.
    /// A implementação padrão é o `Event.None` do `NullExtensionService`.
    fn on_will_stop(&self) -> Event<WillStopExtensionHostsEvent> {
        Event::none()
    }

    /// Send an activation event and activate interested extensions (`activateByEvent(activationEvent, activationKind?)`).
    ///
    /// This will wait for the normal startup of the extension host(s).
    ///
    /// In extraordinary circumstances, if the activation event needs to activate
    /// one or more extensions before the normal startup is finished, then you can use
    /// `ActivationKind.Immediate`. Please do not use this flag unless really necessary
    /// and you understand all consequences.
    fn activate_by_event(&self, activation_event: &str, activation_kind: Option<ActivationKind>) -> Promise<()>;

    /// Determine if `activateByEvent(activationEvent)` has resolved already.
    ///
    /// i.e. the activation event is finished and all interested extensions are already active.
    fn activation_event_is_done(&self, activation_event: &str) -> bool;

    /// Returns the description of the extension with the given id, or `None` when it is not installed.
    fn get_extension(&self, id: &str) -> Promise<Option<IExtensionDescription>>;

    /// `onDidRegisterExtensions`: fired when extensions have been registered in the extension host.
    fn on_did_register_extensions(&self) -> Event<()>;

    /// `whenInstalledExtensionsRegistered()`: a promise that resolves when all installed extensions are registered.
    fn when_installed_extensions_registered(&self) -> Promise<bool>;

    /// `extensions`: the descriptions of the extensions registered so far.
    fn extensions(&self) -> Vec<ExtensionDescription>;

    /// `onDidChangeExtensions`: fired when extensions are added or removed. A implementação padrão é o `Event.None` do
    /// `NullExtensionService` (que o `TestExtensionService` herda).
    fn on_did_change_extensions(&self) -> Event<IExtensionsChangeEvent> {
        Event::none()
    }

    /// `onDidChangeExtensionsStatus`: fired when the status of extensions changes (for example, when one is activated).
    /// A implementação padrão é o `Event.None` do `NullExtensionService`.
    fn on_did_change_extensions_status(&self) -> Event<Vec<ExtensionIdentifier>> {
        Event::none()
    }

    /// `canRemoveExtension(extension)`: whether the extension can be removed without restarting the extension host (it
    /// is not running). A implementação padrão é a do `NullExtensionService`, que devolve `false`.
    fn can_remove_extension(&self, _extension: &ExtensionDescription) -> bool {
        false
    }

    /// `canAddExtension(extension)`: whether the extension can be added without restarting the extension host. A
    /// implementação padrão é a do `NullExtensionService`, que devolve `false`.
    fn can_add_extension(&self, _extension: &ExtensionDescription) -> bool {
        false
    }

    /// `stopExtensionHosts(reason, auto)`: resolves to `true` when the hosts were stopped. A implementação padrão é a do
    /// `NullExtensionService`, que devolve `true`.
    fn stop_extension_hosts(&self, _reason: &str, _auto: bool) -> Promise<bool> {
        Promise::resolved(true)
    }

    /// `startExtensionHosts(updates)`: starts the hosts, with the extensions to add and the ids to remove. A
    /// implementação padrão é a do `NullExtensionService`, que não faz nada.
    fn start_extension_hosts(&self, _updates: Option<ExtensionHostUpdates>) -> Promise<()> {
        Promise::resolved(())
    }
}

/// `{ toAdd: ILocalExtension[]; toRemove: string[] }` de `startExtensionHosts`.
#[derive(Clone, Debug, Default)]
pub struct ExtensionHostUpdates {
    pub to_add: Vec<je_extensions::management::LocalExtension>,
    pub to_remove: Vec<String>,
}

/// `IExtensionsChangeEvent`: `{ added, removed }`, as descrições completas das extensões.
#[derive(Clone, Debug, Default)]
pub struct IExtensionsChangeEvent {
    pub added: Vec<ExtensionDescription>,
    pub removed: Vec<ExtensionDescription>,
}

service_id!(pub I_EXTENSION_SERVICE: dyn IExtensionService = "extensionService");

/// `toExtension(extensionDescription)`: a extensão que a descrição em execução representa.
pub fn to_extension(extension_description: &ExtensionDescription) -> LocalExtension {
    LocalExtension {
        extension_type: if extension_description.is_builtin { ExtensionType::System } else { ExtensionType::User },
        is_builtin: extension_description.is_builtin || extension_description.is_user_builtin,
        identifier: ExtensionIdentifierInfo {
            id: get_gallery_extension_id(Some(extension_description.manifest.publisher.as_str()).filter(|publisher| !publisher.is_empty()), &extension_description.manifest.name),
            uuid: extension_description.uuid.clone(),
        },
        manifest: extension_description.manifest.clone(),
        location: extension_description.extension_location.clone(),
        target_platform: extension_description.target_platform,
        publisher_display_name: extension_description.publisher_display_name.clone(),
        readme_url: None,
        changelog_url: None,
        is_valid: true,
        validations: Vec::new(),
        pre_release: extension_description.pre_release,
        is_workspace_scoped: false,
        is_machine_scoped: false,
        is_application_scoped: false,
        publisher_id: None,
        installed_timestamp: None,
        is_pre_release_version: extension_description.pre_release,
        has_pre_release_version: false,
        private: false,
        updated: false,
        pinned: false,
        force_auto_update: false,
        source: InstallSource::Vsix,
        size: 0.0,
    }
}

/// `toExtensionDescription(extension, isUnderDevelopment?)`.
pub fn to_extension_description(extension: &LocalExtension, is_under_development: bool) -> ExtensionDescription {
    let id = get_extension_id(&extension.manifest.publisher, &extension.manifest.name);
    ExtensionDescription {
        identifier: ExtensionIdentifier::new(id.clone()),
        id,
        is_builtin: extension.extension_type == ExtensionType::System,
        is_user_builtin: extension.extension_type == ExtensionType::User && extension.is_builtin,
        is_under_development,
        extension_location: extension.location.clone(),
        uuid: extension.identifier.uuid.clone(),
        target_platform: extension.target_platform,
        publisher_display_name: extension.publisher_display_name.clone(),
        pre_release: extension.pre_release,
        manifest: extension.manifest.clone(),
    }
}

/// `IMessage`: uma mensagem de validação de um ponto de extensão. O campo `type` do original é `severity`.
#[derive(Clone, Debug)]
pub struct IMessage {
    pub severity: Severity,
    pub message: String,
    pub extension_id: ExtensionIdentifier,
    pub extension_point_id: String,
}

/// `ActivationTimes`.
#[derive(Clone, Debug, PartialEq)]
pub struct ActivationTimes {
    pub code_loading_time: f64,
    pub activate_call_time: f64,
    pub activate_resolved_time: f64,
    pub activation_reason: ExtensionActivationReason,
}

/// `IExtensionsStatus`. `activationTimes: ActivationTimes | undefined` é `Option`.
#[derive(Clone, Debug)]
pub struct IExtensionsStatus {
    pub id: ExtensionIdentifier,
    pub messages: Vec<IMessage>,
    pub activation_started: bool,
    pub activation_times: Option<ActivationTimes>,
    pub runtime_errors: Vec<Error>,
    pub running_location: Option<ExtensionRunningLocation>,
}

/// `ExtensionPointContribution<T>`: o `T` é o JSON da contribuição (`desc.contributes[extPoint.name]`).
#[derive(Clone, Debug)]
pub struct ExtensionPointContribution {
    pub description: SharedExtensionDescription,
    pub value: serde_json::Value,
}

/// `desc.contributes` como JSON, uma chave por ponto de extensão (vazio quando a extensão não contribui nada). O
/// `contributes` do porte é uma struct tipada; o `AbstractExtensionService` e os geradores de eventos de ativação implícitos
/// leem as contribuições por nome de ponto, como o `for (const extPointName in desc.contributes)` do original. A ordem é a dos
/// campos da struct e, depois, a das chaves que ela não conhece.
pub fn extension_contributions(extension_description: &ExtensionDescription) -> serde_json::Map<String, serde_json::Value> {
    match extension_description.manifest.contributes.as_ref().map(serde_json::to_value) {
        Some(Ok(serde_json::Value::Object(contributions))) => contributions,
        _ => serde_json::Map::new(),
    }
}

/// `ExtensionHostStartup`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionHostStartup {
    /// The extension host should be launched immediately and doesn't require a `$startExtensionHost` call.
    EagerAutoStart = 1,
    /// The extension host should be launched immediately and needs a `$startExtensionHost` call.
    EagerManualStart = 2,
    /// The extension host should be launched lazily and only when it has extensions it needs to host. It doesn't
    /// require a `$startExtensionHost` call.
    LazyAutoStart = 3,
}

/// `IExtensionInspectInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IExtensionInspectInfo {
    pub port: u16,
    pub host: String,
    pub devtools_url: Option<String>,
    pub devtools_label: Option<String>,
}

/// `IExtensionHost`: um host de extensões (processo local, web worker local ou remoto) que o `AbstractExtensionService`
/// gerencia por trás de um `IExtensionHostManager`.
///
/// Divergências:
/// - `extensions: ExtensionHostExtensions | null` tem corpo padrão `None` (o `undefined` de um `mock<IExtensionHost>()`); quem
///   executa extensões de verdade o implementa, e o `ExtensionHostManager` o lê.
/// - `disconnect?()` é opcional no original: o padrão devolve `None`, o `undefined` de quem não o declara.
pub trait IExtensionHost {
    fn pid(&self) -> Option<u32>;
    fn running_location(&self) -> ExtensionRunningLocation;
    fn remote_authority(&self) -> Option<String>;
    fn startup(&self) -> ExtensionHostStartup;
    /// `extensions: ExtensionHostExtensions | null`: a coleção de extensões que o host executa (ou vai executar). O corpo
    /// padrão é o `null` do `mock<IExtensionHost>()` do original. Reflete as extensões corretamente só depois de `start()`.
    fn extensions(&self) -> Option<Rc<ExtensionHostExtensions>> {
        None
    }
    fn on_exit(&self) -> Event<(i32, Option<String>)>;

    fn start(&self) -> Promise<Rc<dyn IMessagePassingProtocol>>;
    fn get_inspect_port(&self) -> Option<IExtensionInspectInfo>;
    fn enable_inspect_port(&self) -> Promise<bool>;
    fn disconnect(&self) -> Option<Promise<()>> {
        None
    }
    fn dispose(&self);
}

struct ExtensionHostExtensionsState {
    version_id: i64,
    all_extensions: Vec<SharedExtensionDescription>,
    my_extensions: Vec<ExtensionIdentifier>,
    my_activation_events: Option<HashSet<String>>,
}

/// `ExtensionHostExtensions`: as extensões que um host recebeu (todas as do registro, e as que ele mesmo executa). Os métodos são
/// `&self` porque o host e o gerenciador compartilham a coleção (`Rc`), como a referência do JavaScript.
pub struct ExtensionHostExtensions {
    state: RefCell<ExtensionHostExtensionsState>,
}

/// `extensionDescriptionArrayToMap`.
fn extension_description_array_to_map(extensions: &[SharedExtensionDescription]) -> ExtensionIdentifierMap<SharedExtensionDescription> {
    let mut result = ExtensionIdentifierMap::new();
    for extension in extensions {
        result.set(&extension.identifier.value, extension.clone());
    }
    result
}

impl ExtensionHostExtensions {
    pub fn new(version_id: i64, all_extensions: &[SharedExtensionDescription], my_extensions: &[ExtensionIdentifier]) -> Rc<Self> {
        Rc::new(Self {
            state: RefCell::new(ExtensionHostExtensionsState {
                version_id,
                all_extensions: all_extensions.to_vec(),
                my_extensions: my_extensions.to_vec(),
                my_activation_events: None,
            }),
        })
    }

    pub fn version_id(&self) -> i64 {
        self.state.borrow().version_id
    }

    pub fn all_extensions(&self) -> Vec<SharedExtensionDescription> {
        self.state.borrow().all_extensions.clone()
    }

    pub fn my_extensions(&self) -> Vec<ExtensionIdentifier> {
        self.state.borrow().my_extensions.clone()
    }

    /// `toSnapshot()`.
    pub fn to_snapshot(&self) -> IExtensionDescriptionSnapshot {
        let state = self.state.borrow();
        IExtensionDescriptionSnapshot {
            version_id: state.version_id,
            all_extensions: state.all_extensions.clone(),
            my_extensions: state.my_extensions.clone(),
            activation_events: implicit_activation_events().create_activation_events_map(&state.all_extensions),
        }
    }

    /// `set(versionId, allExtensions, myExtensions)`: o delta que leva a coleção ao novo estado (já aplicado nela). O `throw` do
    /// original é o `Err`.
    pub fn set(&self, version_id: i64, all_extensions: &[SharedExtensionDescription], my_extensions: &[ExtensionIdentifier]) -> Result<IExtensionDescriptionDelta, Error> {
        let (old_all_extensions, old_my_extensions) = {
            let state = self.state.borrow();
            if state.version_id > version_id {
                return Err(Error::new(format!("ExtensionHostExtensions: invalid versionId {version_id} (current: {})", state.version_id)));
            }
            (state.all_extensions.clone(), state.my_extensions.clone())
        };
        let mut to_remove: Vec<ExtensionIdentifier> = Vec::new();
        let mut to_add: Vec<SharedExtensionDescription> = Vec::new();
        let mut my_to_remove: Vec<ExtensionIdentifier> = Vec::new();
        let mut my_to_add: Vec<ExtensionIdentifier> = Vec::new();

        let mut old_extensions_map = extension_description_array_to_map(&old_all_extensions);
        let new_extensions_map = extension_description_array_to_map(all_extensions);
        // O `||` do original (e não `&&`): uma coincidência em qualquer campo basta para considerar as duas iguais.
        let extensions_are_the_same = |a: &ExtensionDescription, b: &ExtensionDescription| {
            a.extension_location == b.extension_location
                || a.is_builtin == b.is_builtin
                || a.is_user_builtin == b.is_user_builtin
                || a.is_under_development == b.is_under_development
        };

        for old_extension in &old_all_extensions {
            let Some(new_extension) = new_extensions_map.get(&old_extension.identifier.value) else {
                to_remove.push(old_extension.identifier.clone());
                old_extensions_map.delete(&old_extension.identifier.value);
                continue;
            };
            if !extensions_are_the_same(old_extension, new_extension) {
                // The new extension is different than the old one (e.g. maybe it executes in a different location)
                to_remove.push(old_extension.identifier.clone());
                old_extensions_map.delete(&old_extension.identifier.value);
                continue;
            }
        }
        for new_extension in all_extensions {
            let Some(old_extension) = old_extensions_map.get(&new_extension.identifier.value).cloned() else {
                to_add.push(new_extension.clone());
                continue;
            };
            if !extensions_are_the_same(&old_extension, new_extension) {
                // The new extension is different than the old one (e.g. maybe it executes in a different location)
                to_remove.push(old_extension.identifier.clone());
                old_extensions_map.delete(&old_extension.identifier.value);
                continue;
            }
        }

        let my_old_extensions_set = ExtensionIdentifierSet::new(old_my_extensions.iter().map(|id| id.value.as_str()));
        let my_new_extensions_set = ExtensionIdentifierSet::new(my_extensions.iter().map(|id| id.value.as_str()));
        for old_extension_id in &old_my_extensions {
            if !my_new_extensions_set.has(&old_extension_id.value) {
                my_to_remove.push(old_extension_id.clone());
            }
        }
        for new_extension_id in my_extensions {
            if !my_old_extensions_set.has(&new_extension_id.value) {
                my_to_add.push(new_extension_id.clone());
            }
        }

        let add_activation_events = implicit_activation_events().create_activation_events_map(&to_add);
        let delta = IExtensionDescriptionDelta { version_id, to_remove, to_add, add_activation_events, my_to_remove, my_to_add };
        self.delta(&delta);
        Ok(delta)
    }

    /// `delta(extensionsDelta)`: `None` quando o delta é antigo demais (a coleção já o tem).
    pub fn delta(&self, extensions_delta: &IExtensionDescriptionDelta) -> Option<IExtensionDescriptionDelta> {
        let mut state = self.state.borrow_mut();
        if state.version_id >= extensions_delta.version_id {
            // ignore older deltas
            return None;
        }

        // First handle removals
        let to_remove_set = ExtensionIdentifierSet::new(extensions_delta.to_remove.iter().map(|id| id.value.as_str()));
        let my_to_remove_set = ExtensionIdentifierSet::new(extensions_delta.my_to_remove.iter().map(|id| id.value.as_str()));
        state.all_extensions.retain(|extension| !to_remove_set.has(&extension.identifier.value));
        state.my_extensions.retain(|extension_id| !my_to_remove_set.has(&extension_id.value));
        // Then handle additions
        for extension in &extensions_delta.to_add {
            state.all_extensions.push(extension.clone());
        }
        for extension_id in &extensions_delta.my_to_add {
            state.my_extensions.push(extension_id.clone());
        }

        // clear cached activation events
        state.my_activation_events = None;
        Some(extensions_delta.clone())
    }

    pub fn contains_extension(&self, extension_id: &ExtensionIdentifier) -> bool {
        self.state.borrow().my_extensions.iter().any(|my_extension_id| my_extension_id == extension_id)
    }

    pub fn contains_activation_event(&self, activation_event: &str) -> bool {
        if self.state.borrow().my_activation_events.is_none() {
            let events = self.read_my_activation_events();
            self.state.borrow_mut().my_activation_events = Some(events);
        }
        self.state.borrow().my_activation_events.as_ref().is_some_and(|events| events.contains(activation_event))
    }

    fn read_my_activation_events(&self) -> HashSet<String> {
        let mut result = HashSet::new();
        for extension_description in self.all_extensions() {
            if !self.contains_extension(&extension_description.identifier) {
                continue;
            }
            for activation_event in implicit_activation_events().read_activation_events(&extension_description) {
                result.insert(activation_event);
            }
        }
        result
    }
}

/// `IWillActivateEvent`.
#[derive(Clone)]
pub struct IWillActivateEvent {
    pub event: String,
    pub activation: Promise<()>,
    pub activation_kind: ActivationKind,
}

/// `IResponsiveStateChangeEvent`. `getInspectListener(tryEnableInspector)` devolve a porta de inspeção, ou `None`
/// (`0` no original: inspecionar não é possível).
#[derive(Clone)]
pub struct IResponsiveStateChangeEvent {
    pub extension_host_kind: ExtensionHostKind,
    pub is_responsive: bool,
    pub get_inspect_listener: Rc<dyn Fn(bool) -> Promise<Option<IExtensionInspectInfo>>>,
}

/// `IInternalExtensionService`: o que o `AbstractExtensionService` entrega a cada host de extensões.
pub trait IInternalExtensionService {
    fn activate_by_id(&self, extension_id: &ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<()>;
    fn on_will_activate_extension(&self, extension_id: &ExtensionIdentifier);
    fn on_did_activate_extension(
        &self,
        extension_id: &ExtensionIdentifier,
        code_loading_time: f64,
        activate_call_time: f64,
        activate_resolved_time: f64,
        activation_reason: ExtensionActivationReason,
    );
    fn on_did_activate_extension_error(&self, extension_id: &ExtensionIdentifier, error: &Error);
    fn on_extension_runtime_error(&self, extension_id: &ExtensionIdentifier, err: &Error);
}
