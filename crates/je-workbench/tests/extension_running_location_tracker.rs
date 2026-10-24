//! Porte de `src/vs/workbench/services/extensions/test/common/extensionRunningLocationTracker.test.ts` (suíte `ExtensionRunningLocationTracker - extensionAffinity`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda do começo de cada caso.
//! - O registro, o seletor de tipo de host e o serviço de propriedades do manifesto são objetos literais no original (só o que o
//!   rastreador lê); aqui são os dublês [`TestRegistry`], [`LocalProcessPicker`] e [`WorkspaceKindProperties`]. Os métodos do contrato que o
//!   original nem define (e que lançariam `TypeError` se chamados) falham com `panic!`.
//! - O ambiente é o `TestEnvironmentService` (`isExtensionDevelopment: false`, `extensionDevelopmentKind: undefined`).
//! - `configurationService.setUserConfiguration('extensions.experimental.affinity', configuredAffinities)` é o mesmo método do
//!   `TestConfigurationService`.
//! - `assert.ok(loc, msg)` é `expect`; `assert.strictEqual(locA.affinity, locB.affinity)` é `assert_eq!` sobre `affinity()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_base::product::BoolOrLimited;
use je_extensions::manifest::{Engines, ExtensionIdentifier, ExtensionIdentifierInfo, ExtensionKind, ExtensionManifest, TargetPlatform};
use je_extensions::scanner::ExtensionDescription;
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::log::NullLogService;
use je_workbench::services::extensions::extension_description_registry::{IReadOnlyExtensionDescriptionRegistry, SharedExtensionDescription};
use je_workbench::services::extensions::extension_host_kind::{ExtensionHostKind, ExtensionRunningPreference, IExtensionHostKindPicker};
use je_workbench::services::extensions::extension_manifest_properties_service::{ConfiguredExtensionKind, IExtensionManifestPropertiesService};
use je_workbench::services::extensions::extension_running_location_tracker::ExtensionRunningLocationTracker;
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;

fn create_extension(id: &str, deps: Option<Vec<&str>>, extension_affinity: Option<Vec<&str>>) -> SharedExtensionDescription {
    let texts = |values: Vec<&str>| values.into_iter().map(str::to_owned).collect::<Vec<_>>();
    let has_affinity = extension_affinity.is_some();
    Rc::new(ExtensionDescription {
        id: id.to_owned(),
        identifier: ExtensionIdentifier::new(id),
        is_builtin: false,
        is_user_builtin: false,
        is_under_development: false,
        extension_location: format!("file:///test/{id}"),
        uuid: None,
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        pre_release: false,
        manifest: ExtensionManifest {
            name: id.to_owned(),
            publisher: "test".to_owned(),
            version: "1.0.0".to_owned(),
            engines: Engines { vscode: Some("*".to_owned()), ..Engines::default() },
            main: Some("main.js".to_owned()),
            extension_dependencies: deps.map(texts),
            extension_affinity: extension_affinity.map(texts),
            enabled_api_proposals: has_affinity.then(|| vec!["extensionAffinity".to_owned()]),
            ..ExtensionManifest::default()
        },
    })
}

/// `IReadOnlyExtensionDescriptionRegistry` literal do `createTracker`.
struct TestRegistry {
    extensions: Vec<SharedExtensionDescription>,
}

impl IReadOnlyExtensionDescriptionRegistry for TestRegistry {
    fn contains_activation_event(&self, _activation_event: &str) -> bool {
        false
    }

    fn contains_extension(&self, _extension_id: &str) -> bool {
        false
    }

    fn get_extension_descriptions_for_activation_event(&self, _activation_event: &str) -> Vec<SharedExtensionDescription> {
        Vec::new()
    }

    fn get_all_extension_descriptions(&self) -> Vec<SharedExtensionDescription> {
        self.extensions.clone()
    }

    fn get_extension_description(&self, extension_id: &str) -> Option<SharedExtensionDescription> {
        self.extensions.iter().find(|extension| extension.identifier.value == extension_id).cloned()
    }

    fn get_extension_description_by_uuid(&self, _uuid: &str) -> Option<SharedExtensionDescription> {
        None
    }
}

/// `{ pickExtensionHostKind: () => ExtensionHostKind.LocalProcess }`.
struct LocalProcessPicker;

impl IExtensionHostKindPicker for LocalProcessPicker {
    fn pick_extension_host_kind(
        &self,
        _extension_id: &str,
        _extension_kinds: &[ExtensionKind],
        _is_installed_locally: bool,
        _is_installed_remotely: bool,
        _preference: ExtensionRunningPreference,
    ) -> Option<ExtensionHostKind> {
        Some(ExtensionHostKind::LocalProcess)
    }
}

/// `{ getExtensionKind: () => ['workspace'] }`.
struct WorkspaceKindProperties;

impl IExtensionManifestPropertiesService for WorkspaceKindProperties {
    fn prefers_execute_on_ui(&self, _manifest: &ExtensionManifest) -> bool {
        panic!("preferExecuteOnUI is not a function")
    }

    fn prefers_execute_on_workspace(&self, _manifest: &ExtensionManifest) -> bool {
        panic!("preferExecuteOnWorkspace is not a function")
    }

    fn prefers_execute_on_web(&self, _manifest: &ExtensionManifest) -> bool {
        panic!("preferExecuteOnWeb is not a function")
    }

    fn can_execute_on_ui(&self, _manifest: &ExtensionManifest) -> bool {
        panic!("canExecuteOnUI is not a function")
    }

    fn can_execute_on_workspace(&self, _manifest: &ExtensionManifest) -> bool {
        panic!("canExecuteOnWorkspace is not a function")
    }

    fn can_execute_on_web(&self, _manifest: &ExtensionManifest) -> bool {
        panic!("canExecuteOnWeb is not a function")
    }

    fn can_execute_on_sessions_window(&self, _manifest: &ExtensionManifest, _is_builtin: bool) -> bool {
        panic!("canExecuteOnSessionsWindow is not a function")
    }

    fn get_extension_kind(&self, _manifest: &ExtensionManifest) -> Vec<ExtensionKind> {
        vec![ExtensionKind::Workspace]
    }

    fn get_user_configured_extension_kind(&self, _extension_identifier: &ExtensionIdentifierInfo) -> Option<Vec<ConfiguredExtensionKind>> {
        panic!("getUserConfiguredExtensionKind is not a function")
    }

    fn get_extension_untrusted_workspace_support_type(&self, _manifest: &ExtensionManifest) -> BoolOrLimited {
        panic!("getExtensionUntrustedWorkspaceSupportType is not a function")
    }

    fn get_extension_virtual_workspace_support_type(&self, _manifest: &ExtensionManifest) -> BoolOrLimited {
        panic!("getExtensionVirtualWorkspaceSupportType is not a function")
    }
}

fn create_tracker(extensions: &[SharedExtensionDescription], configured_affinities: &[(&str, f64)]) -> ExtensionRunningLocationTracker {
    let registry = Rc::new(TestRegistry { extensions: extensions.to_vec() });
    let configuration_service = TestConfigurationService::new(None);
    let _ = configuration_service.set_user_configuration(
        "extensions.experimental.affinity",
        Dynamic::object(configured_affinities.iter().map(|(id, affinity)| (*id, Dynamic::Number(*affinity))).collect::<Vec<_>>()),
        None,
    );

    ExtensionRunningLocationTracker::new(
        registry,
        Rc::new(LocalProcessPicker),
        TestEnvironmentService::new(),
        configuration_service,
        NullLogService::shared(),
        Rc::new(WorkspaceKindProperties),
    )
}

/// Os `affinity` das extensões, `assert.ok(loc)` incluído.
fn affinities(tracker: &ExtensionRunningLocationTracker, extensions: &[SharedExtensionDescription]) -> Vec<u32> {
    let running_locations = tracker.compute_running_location(extensions, &[], true);
    extensions
        .iter()
        .map(|extension| {
            running_locations
                .get(&extension.identifier.value)
                .copied()
                .flatten()
                .unwrap_or_else(|| panic!("Extension {} should have a running location", extension.identifier.value))
                .affinity()
        })
        .collect()
}

#[test]
fn extensions_with_extension_affinity_should_have_the_same_affinity() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_a = create_extension("publisher.extA", None, None);
    let ext_b = create_extension("publisher.extB", None, Some(vec!["publisher.extA"]));

    let tracker = create_tracker(&[ext_a.clone(), ext_b.clone()], &[]);
    let loc = affinities(&tracker, &[ext_a, ext_b]);

    assert_eq!(loc[0], loc[1], "Extensions with extensionAffinity should have the same affinity");
}

#[test]
fn transitive_extension_affinity_should_group_all_extensions_together() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_a = create_extension("publisher.extA", None, None);
    let ext_b = create_extension("publisher.extB", None, Some(vec!["publisher.extA"]));
    let ext_c = create_extension("publisher.extC", None, Some(vec!["publisher.extB"]));

    let tracker = create_tracker(&[ext_a.clone(), ext_b.clone(), ext_c.clone()], &[]);
    let loc = affinities(&tracker, &[ext_a, ext_b, ext_c]);

    assert_eq!(loc[0], loc[1], "A and B should have the same affinity");
    assert_eq!(loc[1], loc[2], "B and C should have the same affinity");
}

#[test]
fn extension_affinity_with_non_installed_extension_should_be_ignored() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_a = create_extension("publisher.extA", None, Some(vec!["publisher.notInstalled"]));
    let ext_b = create_extension("publisher.extB", None, None);

    let tracker = create_tracker(&[ext_a.clone(), ext_b.clone()], &[]);
    // Both extensions should have running locations (`affinities` confere). They should not be grouped together since the
    // extensionAffinity target doesn't exist (unless they would naturally have affinity 0, which they both do by default).
    let _loc = affinities(&tracker, &[ext_a, ext_b]);
}

#[test]
fn extension_affinity_combined_with_extension_dependencies() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_a = create_extension("publisher.extA", None, None);
    let ext_b = create_extension("publisher.extB", Some(vec!["publisher.extA"]), None);
    let ext_c = create_extension("publisher.extC", None, Some(vec!["publisher.extA"]));

    let tracker = create_tracker(&[ext_a.clone(), ext_b.clone(), ext_c.clone()], &[]);
    let loc = affinities(&tracker, &[ext_a, ext_b, ext_c]);

    // B depends on A, C has extensionAffinity to A - all should be in the same group
    assert_eq!(loc[0], loc[1], "A and B (dependency) should have the same affinity");
    assert_eq!(loc[0], loc[2], "A and C (extensionAffinity) should have the same affinity");
}

#[test]
fn user_configured_affinity_should_override_extension_affinity() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let ext_a = create_extension("publisher.extA", None, None);
    let ext_b = create_extension("publisher.extB", None, Some(vec!["publisher.extA"]));

    let tracker = create_tracker(&[ext_a.clone(), ext_b.clone()], &[("publisher.extA", 1.0), ("publisher.extB", 2.0)]);
    // Both extensions should have running locations (`affinities` confere). With user-configured affinities, they should be in
    // different groups. Note: the actual behavior depends on the order of operations in _computeAffinity; the user config creates
    // separate affinities, but grouping happens first.
    let _loc = affinities(&tracker, &[ext_a, ext_b]);
}

#[test]
fn one_way_extension_affinity_is_sufficient() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    // Only extB declares extensionAffinity, extA doesn't need to know about extB
    let ext_a = create_extension("publisher.extA", None, None);
    let ext_b = create_extension("publisher.extB", None, Some(vec!["publisher.extA"]));

    let tracker = create_tracker(&[ext_a.clone(), ext_b.clone()], &[]);
    let loc = affinities(&tracker, &[ext_a, ext_b]);

    assert_eq!(loc[0], loc[1], "One-way extensionAffinity should be sufficient to group extensions");
}
