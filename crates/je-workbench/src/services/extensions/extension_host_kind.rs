//! Porte de `src/vs/workbench/services/extensions/common/extensionHostKind.ts`: `ExtensionHostKind`, `ExtensionRunningPreference`,
//! `IExtensionHostKindPicker` e `determineExtensionHostKinds`.
//!
//! Divergências:
//! - `ExtensionHostKind | null` é `Option<ExtensionHostKind>`.
//! - `Map<string, ExtensionHostKind | null>` é um `IndexMap` (o `Map` do JavaScript preserva a ordem de inserção), de chave igual a
//!   `ExtensionIdentifier.toKey`.
//! - O `ExtensionKind` é o de `je_extensions::manifest`, o mesmo do `IExtensionManifestPropertiesService`.

use std::rc::Rc;

use indexmap::IndexMap;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionKind};

use super::extension_description_registry::SharedExtensionDescription;

/// `ExtensionHostKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtensionHostKind {
    LocalProcess = 1,
    LocalWebWorker = 2,
    Remote = 3,
}

/// `extensionHostKindToString(kind)`.
pub fn extension_host_kind_to_string(kind: Option<ExtensionHostKind>) -> &'static str {
    match kind {
        None => "None",
        Some(ExtensionHostKind::LocalProcess) => "LocalProcess",
        Some(ExtensionHostKind::LocalWebWorker) => "LocalWebWorker",
        Some(ExtensionHostKind::Remote) => "Remote",
    }
}

/// `ExtensionRunningPreference`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtensionRunningPreference {
    None,
    Local,
    Remote,
}

/// `extensionRunningPreferenceToString(preference)`.
pub fn extension_running_preference_to_string(preference: ExtensionRunningPreference) -> &'static str {
    match preference {
        ExtensionRunningPreference::None => "None",
        ExtensionRunningPreference::Local => "Local",
        ExtensionRunningPreference::Remote => "Remote",
    }
}

/// `IExtensionHostKindPicker`.
pub trait IExtensionHostKindPicker {
    fn pick_extension_host_kind(
        &self,
        extension_id: &str,
        extension_kinds: &[ExtensionKind],
        is_installed_locally: bool,
        is_installed_remotely: bool,
        preference: ExtensionRunningPreference,
    ) -> Option<ExtensionHostKind>;
}

/// `ExtensionWithKind`.
struct ExtensionWithKind {
    desc: SharedExtensionDescription,
    kind: Vec<ExtensionKind>,
}

impl ExtensionWithKind {
    fn key(&self) -> String {
        ExtensionIdentifier::to_key(&self.desc.identifier.value)
    }

    fn is_under_development(&self) -> bool {
        self.desc.is_under_development
    }
}

/// `ExtensionInfo`.
struct ExtensionInfo<'a> {
    local: Option<&'a ExtensionWithKind>,
    remote: Option<&'a ExtensionWithKind>,
}

impl ExtensionInfo<'_> {
    fn first(&self) -> &ExtensionWithKind {
        self.local.or(self.remote).expect("uma extensão tem a versão local ou a remota")
    }

    fn key(&self) -> String {
        self.first().key()
    }

    fn identifier(&self) -> &ExtensionIdentifier {
        &self.first().desc.identifier
    }

    /// In case of disagreements between extension kinds, it is always better to pick the local extension because it has a much
    /// higher chance of being up-to-date.
    fn kind(&self) -> &[ExtensionKind] {
        &self.first().kind
    }
}

fn to_extension_with_kind(
    extensions: &[SharedExtensionDescription],
    get_extension_kind: &dyn Fn(&SharedExtensionDescription) -> Vec<ExtensionKind>,
) -> IndexMap<String, ExtensionWithKind> {
    let mut result = IndexMap::new();
    for desc in extensions {
        let ext = ExtensionWithKind { desc: Rc::clone(desc), kind: get_extension_kind(desc) };
        result.insert(ext.key(), ext);
    }
    result
}

/// `determineExtensionHostKinds(localExtensions, remoteExtensions, getExtensionKind, pickExtensionHostKind)`.
pub fn determine_extension_host_kinds(
    local_extensions: &[SharedExtensionDescription],
    remote_extensions: &[SharedExtensionDescription],
    get_extension_kind: &dyn Fn(&SharedExtensionDescription) -> Vec<ExtensionKind>,
    pick_extension_host_kind: &dyn Fn(&str, &[ExtensionKind], bool, bool, ExtensionRunningPreference) -> Option<ExtensionHostKind>,
) -> IndexMap<String, Option<ExtensionHostKind>> {
    let local_extensions = to_extension_with_kind(local_extensions, get_extension_kind);
    let remote_extensions = to_extension_with_kind(remote_extensions, get_extension_kind);

    let mut all_extensions: IndexMap<String, ExtensionInfo<'_>> = IndexMap::new();
    for ext in local_extensions.values().chain(remote_extensions.values()) {
        let key = ext.key();
        if all_extensions.contains_key(&key) {
            continue;
        }
        let info = ExtensionInfo { local: local_extensions.get(&key), remote: remote_extensions.get(&key) };
        all_extensions.insert(info.key(), info);
    }

    let mut extension_host_kinds = IndexMap::new();
    for ext in all_extensions.values() {
        let is_installed_locally = ext.local.is_some();
        let is_installed_remotely = ext.remote.is_some();

        let is_locally_under_development = ext.local.is_some_and(ExtensionWithKind::is_under_development);
        let is_remotely_under_development = ext.remote.is_some_and(ExtensionWithKind::is_under_development);

        let mut preference = ExtensionRunningPreference::None;
        if is_locally_under_development && !is_remotely_under_development {
            preference = ExtensionRunningPreference::Local;
        } else if is_remotely_under_development && !is_locally_under_development {
            preference = ExtensionRunningPreference::Remote;
        }

        extension_host_kinds.insert(ext.key(), pick_extension_host_kind(&ext.identifier().value, ext.kind(), is_installed_locally, is_installed_remotely, preference));
    }

    extension_host_kinds
}
