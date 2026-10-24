//! Porte parcial de `src/vs/workbench/services/extensions/common/extensionHostProtocol.ts`: só o `IExtensionDescriptionDelta`,
//! que o `IExtensionHostManager.deltaExtensions` recebe do `AbstractExtensionService`.
//!
//! Fora do porte (entram com o `ExtensionHostManager` e o `ExtHostExtensionService`, que os leem):
//! `IExtensionDescriptionSnapshot`, `IExtensionHostInitData`, `UIKind`, `RemoteTrustOption`, `ExtensionHostExitCode` e as
//! mensagens do protocolo.
//!
//! Divergências: `{ [extensionId: string]: string[] }` é `HashMap<String, Vec<String>>` (a chave é o `ExtensionIdentifier.toKey`).

use std::collections::HashMap;

use je_extensions::manifest::ExtensionIdentifier;

use super::extension_description_registry::SharedExtensionDescription;

/// `IExtensionDescriptionDelta`.
#[derive(Clone, Debug)]
pub struct IExtensionDescriptionDelta {
    pub version_id: i64,
    pub to_remove: Vec<ExtensionIdentifier>,
    pub to_add: Vec<SharedExtensionDescription>,
    pub add_activation_events: HashMap<String, Vec<String>>,
    pub my_to_remove: Vec<ExtensionIdentifier>,
    pub my_to_add: Vec<ExtensionIdentifier>,
}
