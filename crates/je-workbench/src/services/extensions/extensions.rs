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

use je_base::event::Event;
use je_base::errors::Error;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_instantiation::service_id;

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
