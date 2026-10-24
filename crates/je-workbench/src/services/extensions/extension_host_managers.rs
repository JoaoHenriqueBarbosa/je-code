//! Porte de `src/vs/workbench/services/extensions/common/extensionHostManagers.ts`: o `IExtensionHostManager`, o que o
//! `AbstractExtensionService` pede de cada gerenciador de host de extensões (o `ExtensionHostManager` real, o
//! `LazyCreateExtensionHostManager` ou um dublê de teste).
//!
//! Divergências:
//! - `representsRunningLocation` vem do supertrait [`IRepresentsRunningLocation`] (o contrato que o
//!   `ExtensionRunningLocationTracker.filterByExtensionHostManager` já lê), e `dispose()` do [`IDisposable`].
//! - `onDidExit: Event<[number, string | null]>` é `Event<(i32, Option<String>)>`; `pid: number | null` é `Option<u32>`;
//!   `friendyName` (o erro de digitação do original) é `friendly_name`.
//! - `start(extensionRegistryVersionId, allExtensions, myExtensions)` recebe as listas por valor.
//! - `setRemoteEnvironment(env)`: `{ [key: string]: string | null }` é a lista de pares, com `None` no `null`.

use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_extensions::manifest::ExtensionIdentifier;

use super::extension_activation::ExtensionActivationReason;
use super::extension_description_registry::SharedExtensionDescription;
use super::extension_host_kind::ExtensionHostKind;
use super::extension_host_protocol::IExtensionDescriptionDelta;
use super::extension_host_proxy::IResolveAuthorityResult;
use super::extension_running_location_tracker::IRepresentsRunningLocation;
use super::extensions::{ActivationKind, ExtensionHostStartup, IExtensionInspectInfo};
use super::rpc_protocol::ResponsiveState;

/// `IExtensionHostManager`.
pub trait IExtensionHostManager: IRepresentsRunningLocation + IDisposable {
    fn pid(&self) -> Option<u32>;
    fn kind(&self) -> ExtensionHostKind;
    fn startup(&self) -> ExtensionHostStartup;
    fn friendly_name(&self) -> String;
    fn on_did_exit(&self) -> Event<(i32, Option<String>)>;
    fn on_did_change_responsive_state(&self) -> Event<ResponsiveState>;
    fn disconnect(&self) -> Promise<()>;
    fn is_ready(&self) -> bool;
    fn ready(&self) -> Promise<()>;
    fn delta_extensions(&self, extensions_delta: IExtensionDescriptionDelta) -> Promise<()>;
    fn contains_extension(&self, extension_id: &ExtensionIdentifier) -> bool;
    fn activate(&self, extension: &ExtensionIdentifier, reason: ExtensionActivationReason) -> Promise<bool>;
    fn activate_by_event(&self, activation_event: &str, activation_kind: ActivationKind) -> Promise<()>;
    fn activation_event_is_done(&self, activation_event: &str) -> bool;
    fn get_inspect_port(&self, try_enable_inspector: bool) -> Promise<Option<IExtensionInspectInfo>>;
    fn resolve_authority(&self, remote_authority: &str, resolve_attempt: u32) -> Promise<IResolveAuthorityResult>;
    /// Returns `None` if no resolver for `remoteAuthority` is found.
    fn get_canonical_uri(&self, remote_authority: &str, uri: &URI) -> Promise<Option<URI>>;
    fn start(&self, extension_registry_version_id: i64, all_extensions: Vec<SharedExtensionDescription>, my_extensions: Vec<ExtensionIdentifier>) -> Promise<()>;
    fn extension_tests_execute(&self) -> Promise<i32>;
    fn set_remote_environment(&self, env: Vec<(String, Option<String>)>) -> Promise<()>;
}
