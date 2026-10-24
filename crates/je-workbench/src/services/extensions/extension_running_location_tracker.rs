//! Porte de `src/vs/workbench/services/extensions/common/extensionRunningLocationTracker.ts`: onde cada extensão roda (processo local,
//! web worker local ou remoto) e com qual afinidade (extensões da mesma afinidade dividem o mesmo extension host).
//!
//! Divergências:
//! - `ExtensionIdentifierMap<ExtensionRunningLocation | null>` é `ExtensionIdentifierMap<Option<ExtensionRunningLocation>>`.
//! - As extensões são [`SharedExtensionDescription`] (o `ExtensionDescription` do `je-extensions` compartilhado) e o `readExtensionKinds`
//!   pede ao `IExtensionManifestPropertiesService` o tipo pelo manifesto. O `extensionDevelopmentKind` do ambiente é o `ExtensionKind` da
//!   plataforma, convertido para o de `je_extensions::manifest` (os dois têm as mesmas três variantes).
//! - `IExtensionHostManager` (`extensionHostManagers.ts`) ainda não está portado; `filterByExtensionHostManager` recebe o que ele oferece
//!   para o rastreador, o [`IRepresentsRunningLocation`].
//! - `extensionLocation.scheme === Schemas.vscodeRemote` olha o esquema do texto da localização.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::network::schemas;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionIdentifierMap, ExtensionKind};
use je_platform::configuration::service::IConfigurationService;
use je_platform::environment::environment::ExtensionKind as PlatformExtensionKind;
use je_platform::log::{LogServiceRef, Logger};

use super::extension_description_registry::{has_main_or_browser, IReadOnlyExtensionDescriptionRegistry, SharedExtensionDescription};
use super::extension_host_kind::{determine_extension_host_kinds, ExtensionHostKind, ExtensionRunningPreference, IExtensionHostKindPicker};
use super::extension_manifest_properties_service::IExtensionManifestPropertiesService;
use super::extension_running_location::ExtensionRunningLocation;
use super::extensions::is_proposed_api_enabled;
use crate::services::environment::environment_service::IWorkbenchEnvironmentService;

/// O que `filterByExtensionHostManager` pede do `IExtensionHostManager`: `representsRunningLocation(runningLocation)`.
pub trait IRepresentsRunningLocation {
    fn represents_running_location(&self, running_location: &ExtensionRunningLocation) -> bool;
}

type RunningLocations = ExtensionIdentifierMap<Option<ExtensionRunningLocation>>;

/// O resultado de `_doComputeRunningLocation`.
struct ComputedRunningLocation {
    running_location: RunningLocations,
    max_local_process_affinity: u32,
    max_local_web_worker_affinity: u32,
}

/// O resultado de `_computeAffinity`.
struct ComputedAffinity {
    affinities: ExtensionIdentifierMap<u32>,
    max_affinity: u32,
}

fn to_manifest_kind(kind: PlatformExtensionKind) -> ExtensionKind {
    match kind {
        PlatformExtensionKind::Ui => ExtensionKind::Ui,
        PlatformExtensionKind::Workspace => ExtensionKind::Workspace,
        PlatformExtensionKind::Web => ExtensionKind::Web,
    }
}

/// `ExtensionRunningLocationTracker`.
pub struct ExtensionRunningLocationTracker {
    running_location: RefCell<RunningLocations>,
    max_local_process_affinity: Cell<u32>,
    max_local_web_worker_affinity: Cell<u32>,
    registry: Rc<dyn IReadOnlyExtensionDescriptionRegistry>,
    extension_host_kind_picker: Rc<dyn IExtensionHostKindPicker>,
    environment_service: Rc<dyn IWorkbenchEnvironmentService>,
    configuration_service: Rc<dyn IConfigurationService>,
    log_service: LogServiceRef,
    extension_manifest_properties_service: Rc<dyn IExtensionManifestPropertiesService>,
}

impl ExtensionRunningLocationTracker {
    pub fn new(
        registry: Rc<dyn IReadOnlyExtensionDescriptionRegistry>,
        extension_host_kind_picker: Rc<dyn IExtensionHostKindPicker>,
        environment_service: Rc<dyn IWorkbenchEnvironmentService>,
        configuration_service: Rc<dyn IConfigurationService>,
        log_service: LogServiceRef,
        extension_manifest_properties_service: Rc<dyn IExtensionManifestPropertiesService>,
    ) -> Self {
        Self {
            running_location: RefCell::new(ExtensionIdentifierMap::new()),
            max_local_process_affinity: Cell::new(0),
            max_local_web_worker_affinity: Cell::new(0),
            registry,
            extension_host_kind_picker,
            environment_service,
            configuration_service,
            log_service,
            extension_manifest_properties_service,
        }
    }

    pub fn max_local_process_affinity(&self) -> u32 {
        self.max_local_process_affinity.get()
    }

    pub fn max_local_web_worker_affinity(&self) -> u32 {
        self.max_local_web_worker_affinity.get()
    }

    pub fn set(&self, extension_id: &str, running_location: ExtensionRunningLocation) {
        self.running_location.borrow_mut().set(extension_id, Some(running_location));
    }

    pub fn read_extension_kinds(&self, extension_description: &SharedExtensionDescription) -> Vec<ExtensionKind> {
        if extension_description.is_under_development
            && let Some(kinds) = self.environment_service.extension_development_kind()
        {
            return kinds.into_iter().map(to_manifest_kind).collect();
        }

        self.extension_manifest_properties_service.get_extension_kind(&extension_description.manifest)
    }

    pub fn get_running_location(&self, extension_id: &str) -> Option<ExtensionRunningLocation> {
        self.running_location.borrow().get(extension_id).copied().flatten()
    }

    pub fn filter_by_running_location(&self, extensions: &[SharedExtensionDescription], desired_running_location: &ExtensionRunningLocation) -> Vec<SharedExtensionDescription> {
        filter_extension_descriptions(extensions, &self.running_location.borrow(), |ext_running_location| desired_running_location.equals(ext_running_location))
    }

    pub fn filter_by_extension_host_kind(&self, extensions: &[SharedExtensionDescription], desired_extension_host_kind: ExtensionHostKind) -> Vec<SharedExtensionDescription> {
        filter_extension_descriptions(extensions, &self.running_location.borrow(), |ext_running_location| ext_running_location.kind() == desired_extension_host_kind)
    }

    pub fn filter_by_extension_host_manager(&self, extensions: &[SharedExtensionDescription], extension_host_manager: &dyn IRepresentsRunningLocation) -> Vec<SharedExtensionDescription> {
        filter_extension_descriptions(extensions, &self.running_location.borrow(), |ext_running_location| extension_host_manager.represents_running_location(ext_running_location))
    }

    fn compute_affinity(&self, input_extensions: &[SharedExtensionDescription], extension_host_kind: ExtensionHostKind, is_initial_allocation: bool) -> ComputedAffinity {
        // Only analyze extensions that can execute
        let mut extensions: ExtensionIdentifierMap<SharedExtensionDescription> = ExtensionIdentifierMap::new();
        for extension in input_extensions {
            if has_main_or_browser(extension) {
                extensions.set(&extension.identifier.value, Rc::clone(extension));
            }
        }
        // Also add existing extensions of the same kind that can execute
        for extension in self.registry.get_all_extension_descriptions() {
            if has_main_or_browser(&extension) {
                let running_location = self.running_location.borrow().get(&extension.identifier.value).copied().flatten();
                if running_location.is_some_and(|running_location| running_location.kind() == extension_host_kind) {
                    let identifier = extension.identifier.value.clone();
                    extensions.set(&identifier, extension);
                }
            }
        }

        // Initially, each extension belongs to its own group
        let mut groups: ExtensionIdentifierMap<u32> = ExtensionIdentifierMap::new();
        let mut group_number = 0;
        for extension in extensions.values() {
            group_number += 1;
            groups.set(&extension.identifier.value, group_number);
        }

        fn change_group(groups: &mut ExtensionIdentifierMap<u32>, from: u32, to: u32) {
            let keys: Vec<String> = groups.iter().filter(|(_, group)| **group == from).map(|(key, _)| key.clone()).collect();
            for key in keys {
                groups.set(&key, to);
            }
        }

        // We will group things together when there are dependencies
        for extension in extensions.values() {
            let Some(extension_dependencies) = &extension.manifest.extension_dependencies else {
                continue;
            };
            let my_group = *groups.get(&extension.identifier.value).expect("toda extensão analisada tem grupo");
            for dep_id in extension_dependencies {
                let Some(dep_group) = groups.get(dep_id).copied() else {
                    // probably can't execute, so it has no impact
                    continue;
                };

                if dep_group == my_group {
                    // already in the same group
                    continue;
                }

                change_group(&mut groups, dep_group, my_group);
            }
        }

        // We will also group things together when there are extensionAffinity declarations
        for extension in extensions.values() {
            let Some(extension_affinity) = &extension.manifest.extension_affinity else {
                continue;
            };
            if !is_proposed_api_enabled(extension, "extensionAffinity") {
                self.log_service.warn(&format!("Extension '{}' declares 'extensionAffinity' in its package.json but does not enable the 'extensionAffinity' API proposal. Add '\"enabledApiProposals\": [\"extensionAffinity\"]' to the extension's package.json to use this feature.", extension.identifier.value));
                continue;
            }
            let my_group = *groups.get(&extension.identifier.value).expect("toda extensão analisada tem grupo");
            for colocate_id in extension_affinity {
                let Some(colocate_group) = groups.get(colocate_id).copied() else {
                    // the extension is not installed or can't execute, so it has no impact
                    continue;
                };

                if colocate_group == my_group {
                    // already in the same group
                    continue;
                }

                change_group(&mut groups, colocate_group, my_group);
            }
        }

        // Initialize with existing affinities
        let mut resulting_affinities: HashMap<u32, u32> = HashMap::new();
        let mut last_affinity = 0;
        for extension in extensions.values() {
            let running_location = self.running_location.borrow().get(&extension.identifier.value).copied().flatten();
            if let Some(running_location) = running_location {
                let group = *groups.get(&extension.identifier.value).expect("toda extensão analisada tem grupo");
                resulting_affinities.insert(group, running_location.affinity());
                last_affinity = last_affinity.max(running_location.affinity());
            }
        }

        // When doing extension host debugging, we will ignore the configured affinity
        // because we can currently debug a single extension host
        if !self.environment_service.is_extension_development() {
            // Go through each configured affinity and try to accomodate it
            let configured = self.configuration_service.get_value(Some("extensions.experimental.affinity"), None);
            let configured_affinities = if je_base::dynamic::Truthy::is_truthy(&configured) { configured.own_entries() } else { Vec::new() };
            let mut configured_affinity_to_resulting_affinity: HashMap<u32, u32> = HashMap::new();
            for (extension_id, configured_affinity) in configured_affinities {
                let extension_id = extension_id.to_string_lossy();
                let configured_affinity = match configured_affinity {
                    je_base::dynamic::Dynamic::Number(n) if n > 0.0 && n.floor() == n && n <= f64::from(u32::MAX) => n as u32,
                    _ => {
                        self.log_service.info(&format!("Ignoring configured affinity for '{extension_id}' because the value is not a positive integer."));
                        continue;
                    }
                };
                let Some(group) = groups.get(&extension_id).copied() else {
                    // The extension is not known or cannot execute for this extension host kind
                    continue;
                };

                if let Some(affinity1) = resulting_affinities.get(&group).copied().filter(|affinity| *affinity != 0) {
                    // Affinity for this group is already established
                    configured_affinity_to_resulting_affinity.insert(configured_affinity, affinity1);
                    continue;
                }

                if let Some(affinity2) = configured_affinity_to_resulting_affinity.get(&configured_affinity).copied().filter(|affinity| *affinity != 0) {
                    // Affinity for this configuration is already established
                    resulting_affinities.insert(group, affinity2);
                    continue;
                }

                if !is_initial_allocation {
                    self.log_service.info(&format!("Ignoring configured affinity for '{extension_id}' because extension host(s) are already running. Reload window."));
                    continue;
                }

                last_affinity += 1;
                let affinity3 = last_affinity;
                configured_affinity_to_resulting_affinity.insert(configured_affinity, affinity3);
                resulting_affinities.insert(group, affinity3);
            }
        }

        let mut result: ExtensionIdentifierMap<u32> = ExtensionIdentifierMap::new();
        for extension in input_extensions {
            let group = groups.get(&extension.identifier.value).copied().unwrap_or(0);
            let affinity = resulting_affinities.get(&group).copied().unwrap_or(0);
            result.set(&extension.identifier.value, affinity);
        }

        if last_affinity > 0 && is_initial_allocation {
            for affinity in 1..=last_affinity {
                let extension_ids: Vec<&str> = input_extensions
                    .iter()
                    .filter(|extension| result.get(&extension.identifier.value).copied() == Some(affinity))
                    .map(|extension| extension.identifier.value.as_str())
                    .collect();
                self.log_service.info(&format!("Placing extension(s) {} on a separate extension host.", extension_ids.join(", ")));
            }
        }

        ComputedAffinity { affinities: result, max_affinity: last_affinity }
    }

    pub fn compute_running_location(
        &self,
        local_extensions: &[SharedExtensionDescription],
        remote_extensions: &[SharedExtensionDescription],
        is_initial_allocation: bool,
    ) -> RunningLocations {
        self.do_compute_running_location(&self.running_location.borrow(), local_extensions, remote_extensions, is_initial_allocation).running_location
    }

    fn do_compute_running_location(
        &self,
        existing_running_location: &RunningLocations,
        local_extensions: &[SharedExtensionDescription],
        remote_extensions: &[SharedExtensionDescription],
        is_initial_allocation: bool,
    ) -> ComputedRunningLocation {
        // Skip extensions that have an existing running location
        let local_extensions: Vec<SharedExtensionDescription> = local_extensions.iter().filter(|extension| !existing_running_location.has(&extension.identifier.value)).cloned().collect();
        let remote_extensions: Vec<SharedExtensionDescription> = remote_extensions.iter().filter(|extension| !existing_running_location.has(&extension.identifier.value)).cloned().collect();

        let extension_host_kinds = determine_extension_host_kinds(
            &local_extensions,
            &remote_extensions,
            &|extension| self.read_extension_kinds(extension),
            &|extension_id, extension_kinds, is_installed_locally, is_installed_remotely, preference| {
                self.extension_host_kind_picker.pick_extension_host_kind(extension_id, extension_kinds, is_installed_locally, is_installed_remotely, preference)
            },
        );

        let mut extensions: ExtensionIdentifierMap<SharedExtensionDescription> = ExtensionIdentifierMap::new();
        for extension in local_extensions.iter().chain(remote_extensions.iter()) {
            extensions.set(&extension.identifier.value, Rc::clone(extension));
        }

        let mut result: RunningLocations = ExtensionIdentifierMap::new();
        let mut local_process_extensions: Vec<SharedExtensionDescription> = Vec::new();
        let mut local_web_worker_extensions: Vec<SharedExtensionDescription> = Vec::new();
        for (extension_id_key, extension_host_kind) in &extension_host_kinds {
            let mut running_location = None;
            match extension_host_kind {
                Some(ExtensionHostKind::LocalProcess) => {
                    if let Some(extension_description) = extensions.get(extension_id_key) {
                        local_process_extensions.push(Rc::clone(extension_description));
                    }
                }
                Some(ExtensionHostKind::LocalWebWorker) => {
                    if let Some(extension_description) = extensions.get(extension_id_key) {
                        local_web_worker_extensions.push(Rc::clone(extension_description));
                    }
                }
                Some(ExtensionHostKind::Remote) => running_location = Some(ExtensionRunningLocation::Remote),
                None => {}
            }
            result.set(extension_id_key, running_location);
        }

        let ComputedAffinity { affinities, max_affinity } = self.compute_affinity(&local_process_extensions, ExtensionHostKind::LocalProcess, is_initial_allocation);
        for extension in &local_process_extensions {
            let affinity = affinities.get(&extension.identifier.value).copied().unwrap_or(0);
            result.set(&extension.identifier.value, Some(ExtensionRunningLocation::LocalProcess { affinity }));
        }
        let ComputedAffinity { affinities: local_web_worker_affinities, max_affinity: max_local_web_worker_affinity } =
            self.compute_affinity(&local_web_worker_extensions, ExtensionHostKind::LocalWebWorker, is_initial_allocation);
        for extension in &local_web_worker_extensions {
            let affinity = local_web_worker_affinities.get(&extension.identifier.value).copied().unwrap_or(0);
            result.set(&extension.identifier.value, Some(ExtensionRunningLocation::LocalWebWorker { affinity }));
        }

        // Add extensions that already have an existing running location
        for (extension_id_key, running_location) in existing_running_location.iter() {
            if let Some(running_location) = running_location {
                result.set(extension_id_key, Some(*running_location));
            }
        }

        ComputedRunningLocation { running_location: result, max_local_process_affinity: max_affinity, max_local_web_worker_affinity }
    }

    pub fn initialize_running_location(&self, local_extensions: &[SharedExtensionDescription], remote_extensions: &[SharedExtensionDescription]) {
        let ComputedRunningLocation { running_location, max_local_process_affinity, max_local_web_worker_affinity } =
            self.do_compute_running_location(&self.running_location.borrow(), local_extensions, remote_extensions, true);
        *self.running_location.borrow_mut() = running_location;
        self.max_local_process_affinity.set(max_local_process_affinity);
        self.max_local_web_worker_affinity.set(max_local_web_worker_affinity);
    }

    /// Returns the running locations for the removed extensions.
    pub fn delta_extensions(&self, to_add: &[SharedExtensionDescription], to_remove: &[ExtensionIdentifier]) -> RunningLocations {
        // Remove old running location
        let mut removed_running_location: RunningLocations = ExtensionIdentifierMap::new();
        for extension_id in to_remove {
            let mut running_location = self.running_location.borrow_mut();
            removed_running_location.set(&extension_id.value, running_location.get(&extension_id.value).copied().flatten());
            running_location.delete(&extension_id.value);
        }

        // Determine new running location
        self.update_running_location_for_added_extensions(to_add);

        removed_running_location
    }

    /// Update `self.running_location` with running locations for newly enabled/installed extensions.
    fn update_running_location_for_added_extensions(&self, to_add: &[SharedExtensionDescription]) {
        // Determine new running location
        let mut local_process_extensions: Vec<SharedExtensionDescription> = Vec::new();
        let mut local_web_worker_extensions: Vec<SharedExtensionDescription> = Vec::new();
        for extension in to_add {
            let extension_kind = self.read_extension_kinds(extension);
            let is_remote = extension.extension_location.starts_with(&format!("{}:", schemas::VSCODE_REMOTE));
            let extension_host_kind =
                self.extension_host_kind_picker.pick_extension_host_kind(&extension.identifier.value, &extension_kind, !is_remote, is_remote, ExtensionRunningPreference::None);
            let mut running_location = None;
            match extension_host_kind {
                Some(ExtensionHostKind::LocalProcess) => local_process_extensions.push(Rc::clone(extension)),
                Some(ExtensionHostKind::LocalWebWorker) => local_web_worker_extensions.push(Rc::clone(extension)),
                Some(ExtensionHostKind::Remote) => running_location = Some(ExtensionRunningLocation::Remote),
                None => {}
            }
            self.running_location.borrow_mut().set(&extension.identifier.value, running_location);
        }

        let ComputedAffinity { affinities, .. } = self.compute_affinity(&local_process_extensions, ExtensionHostKind::LocalProcess, false);
        for extension in &local_process_extensions {
            let affinity = affinities.get(&extension.identifier.value).copied().unwrap_or(0);
            self.running_location.borrow_mut().set(&extension.identifier.value, Some(ExtensionRunningLocation::LocalProcess { affinity }));
        }

        let ComputedAffinity { affinities: web_worker_extensions_affinities, .. } = self.compute_affinity(&local_web_worker_extensions, ExtensionHostKind::LocalWebWorker, false);
        for extension in &local_web_worker_extensions {
            let affinity = web_worker_extensions_affinities.get(&extension.identifier.value).copied().unwrap_or(0);
            self.running_location.borrow_mut().set(&extension.identifier.value, Some(ExtensionRunningLocation::LocalWebWorker { affinity }));
        }
    }
}

/// `filterExtensionDescriptions(extensions, runningLocation, predicate)`.
pub fn filter_extension_descriptions(
    extensions: &[SharedExtensionDescription],
    running_location: &RunningLocations,
    predicate: impl Fn(&ExtensionRunningLocation) -> bool,
) -> Vec<SharedExtensionDescription> {
    extensions
        .iter()
        .filter(|ext| running_location.get(&ext.identifier.value).copied().flatten().is_some_and(|ext_running_location| predicate(&ext_running_location)))
        .cloned()
        .collect()
}

/// `filterExtensionIdentifiers(extensions, runningLocation, predicate)`.
pub fn filter_extension_identifiers(
    extensions: &[ExtensionIdentifier],
    running_location: &RunningLocations,
    predicate: impl Fn(&ExtensionRunningLocation) -> bool,
) -> Vec<ExtensionIdentifier> {
    extensions
        .iter()
        .filter(|ext| running_location.get(&ext.value).copied().flatten().is_some_and(|ext_running_location| predicate(&ext_running_location)))
        .cloned()
        .collect()
}
