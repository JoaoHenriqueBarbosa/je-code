//! Porte de `src/vs/platform/extensionManagement/test/common/allowedExtensionsService.test.ts` (suíte `AllowedExtensionsService`), caso a caso e na mesma ordem.
//!
//! Divergência: o `TestConfigurationService` vira um valor compartilhado que o leitor do serviço consulta; o disparo
//! de `onDidChangeConfiguration` do último caso vira a chamada de `on_configuration_changed`.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use je_extensions::allowed::{AllowedCandidate, AllowedExtensionsService};
use je_extensions::management::{
    GalleryExtension, GalleryExtensionAsset, GalleryExtensionAssets, GalleryExtensionProperties, InstallSource, LocalExtension,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest, ExtensionType, TargetPlatform};
use serde_json::{Value, json};

fn service(config: Value, publisher_orgs: &[&str]) -> AllowedExtensionsService {
    let config = Rc::new(RefCell::new(config));
    let orgs: Vec<String> = publisher_orgs.iter().map(|s| (*s).to_owned()).collect();
    AllowedExtensionsService::new(&orgs, move || Some(config.borrow().clone()))
}

fn fields<'a>(id: &'a str, version: Option<&'a str>, prerelease: Option<bool>, target_platform: Option<TargetPlatform>, display: Option<&'a str>) -> AllowedCandidate<'a> {
    AllowedCandidate::Fields { id, publisher_display_name: display, version, prerelease, target_platform }
}

fn a_gallery_extension(name: &str) -> GalleryExtension {
    let asset = || GalleryExtensionAsset { uri: String::new(), fallback_uri: String::new() };
    GalleryExtension {
        name: name.to_owned(),
        identifier: ExtensionIdentifierInfo::with_uuid(format!("pub.{name}"), "uuid"),
        version: "1.0.0".to_owned(),
        display_name: String::new(),
        publisher_id: String::new(),
        publisher: "pub".to_owned(),
        publisher_display_name: "Pub".to_owned(),
        publisher_domain: None,
        publisher_link: None,
        publisher_sponsor_link: None,
        description: String::new(),
        install_count: 0.0,
        rating: 0.0,
        rating_count: 0.0,
        categories: vec![],
        tags: vec![],
        release_date: 0.0,
        last_updated: 0.0,
        preview: false,
        private: false,
        blocking_info: None,
        has_pre_release_version: false,
        has_release_version: true,
        is_signed: false,
        all_target_platforms: vec![TargetPlatform::Universal],
        assets: GalleryExtensionAssets {
            manifest: None,
            readme: None,
            changelog: None,
            license: None,
            repository: None,
            download: asset(),
            icon: None,
            signature: None,
            core_translations: vec![],
        },
        properties: GalleryExtensionProperties {
            dependencies: Some(vec![]),
            extension_pack: None,
            engine: None,
            enabled_api_proposals: None,
            localized_languages: None,
            target_platform: TargetPlatform::Universal,
            is_pre_release_version: false,
            executes_code: None,
        },
        details_link: None,
        rating_link: None,
        support_link: None,
    }
}

fn a_local_extension(id: &str) -> LocalExtension {
    let (publisher, name) = id.split_once('.').expect("id com publisher");
    LocalExtension {
        extension_type: ExtensionType::User,
        is_builtin: false,
        identifier: ExtensionIdentifierInfo::new(id),
        manifest: ExtensionManifest { name: name.to_owned(), publisher: publisher.to_owned(), ..ExtensionManifest::default() },
        location: format!("/pub.{name}"),
        target_platform: TargetPlatform::Undefined,
        publisher_display_name: None,
        readme_url: None,
        changelog_url: None,
        is_valid: true,
        validations: vec![],
        pre_release: false,
        is_workspace_scoped: false,
        is_machine_scoped: false,
        is_application_scoped: false,
        publisher_id: None,
        installed_timestamp: None,
        is_pre_release_version: false,
        has_pre_release_version: false,
        private: false,
        updated: false,
        pinned: false,
        force_auto_update: false,
        source: InstallSource::Vsix,
        size: 0.0,
    }
}

#[test]
fn should_allow_all_extensions_if_no_allowed_extensions_are_configured() {
    let test_object = service(json!("*"), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_not_allow_specific_extension_if_not_in_allowed_list() {
    let test_object = service(json!({ "test.extension": false }), &[]);
    assert!(!test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_allow_specific_extension_if_in_allowed_list() {
    let test_object = service(json!({ "test.extension": true }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_not_allow_pre_release_extension_if_only_stable_is_allowed() {
    let test_object = service(json!({ "test.extension": "stable" }), &[]);
    assert!(!test_object.is_allowed(&fields("test.extension", None, Some(true), None, None)).is_ok());
}

#[test]
fn should_allow_pre_release_extension_if_pre_release_is_allowed() {
    let test_object = service(json!({ "test.extension": true }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", None, Some(true), None, None)).is_ok());
}

#[test]
fn should_allow_specific_version_of_an_extension_when_configured_to_that_version() {
    let test_object = service(json!({ "test.extension": ["1.2.3"] }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", Some("1.2.3"), None, None, None)).is_ok());
}

#[test]
fn should_allow_any_version_of_an_extension_when_a_specific_version_is_configured() {
    let test_object = service(json!({ "test.extension": ["1.2.3"] }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_allow_any_version_of_an_extension_when_stable_is_configured() {
    let test_object = service(json!({ "test.extension": "stable" }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_allow_a_version_of_an_extension_when_stable_is_configured() {
    let test_object = service(json!({ "test.extension": "stable" }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", Some("1.2.3"), None, None, None)).is_ok());
}

#[test]
fn should_allow_a_pre_release_version_of_an_extension_when_stable_is_configured() {
    let test_object = service(json!({ "test.extension": "stable" }), &[]);
    assert!(!test_object.is_allowed(&fields("test.extension", Some("1.2.3"), Some(true), None, None)).is_ok());
}

#[test]
fn should_allow_specific_version_of_an_extension_when_configured_to_multiple_versions() {
    let test_object = service(json!({ "test.extension": ["1.2.3", "2.0.1", "3.1.2"] }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", Some("1.2.3"), None, None, None)).is_ok());
}

#[test]
fn should_allow_platform_specific_version_of_an_extension_when_configured_to_platform_specific_version() {
    let test_object = service(json!({ "test.extension": ["1.2.3@darwin-x64"] }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", Some("1.2.3"), None, Some(TargetPlatform::DarwinX64), None)).is_ok());
}

#[test]
fn should_allow_universal_platform_specific_version_of_an_extension_when_configured_to_platform_specific_version() {
    let test_object = service(json!({ "test.extension": ["1.2.3@darwin-x64"] }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", Some("1.2.3"), None, Some(TargetPlatform::Universal), None)).is_ok());
}

#[test]
fn should_allow_specific_version_of_an_extension_when_configured_to_platform_specific_version() {
    let test_object = service(json!({ "test.extension": ["1.2.3@darwin-x64"] }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", Some("1.2.3"), None, None, None)).is_ok());
}

#[test]
fn should_allow_platform_specific_version_of_an_extension_when_configured_to_multiple_versions() {
    let test_object = service(json!({ "test.extension": ["1.0.0", "1.2.3@darwin-x64", "1.2.3@darwin-arm64"] }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", Some("1.2.3"), None, Some(TargetPlatform::DarwinX64), None)).is_ok());
}

#[test]
fn should_not_allow_platform_specific_version_of_an_extension_when_configured_to_different_platform_specific_version() {
    let test_object = service(json!({ "test.extension": ["1.2.3@darwin-x64"] }), &[]);
    assert!(!test_object.is_allowed(&fields("test.extension", Some("1.2.3"), None, Some(TargetPlatform::DarwinArm64), None)).is_ok());
}

#[test]
fn should_specific_version_of_an_extension_when_configured_to_different_versions() {
    let test_object = service(json!({ "test.extension": ["1.0.0", "1.2.3@darwin-x64", "1.2.3@darwin-arm64"] }), &[]);
    assert!(!test_object.is_allowed(&fields("test.extension", Some("1.0.1"), None, None, None)).is_ok());
}

#[test]
fn should_allow_extension_if_publisher_is_in_allowed_list() {
    let test_object = service(json!({ "test": true }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_allow_extension_if_publisher_is_not_in_allowed_list_and_has_publisher_mapping() {
    let test_object = service(json!({ "hello": true }), &["hello"]);
    assert!(test_object.is_allowed(&fields("test.extension", None, None, None, Some("Hello"))).is_ok());
}

#[test]
fn should_allow_extension_if_publisher_is_not_in_allowed_list_and_has_different_publisher_mapping() {
    let test_object = service(json!({ "hello": true }), &["bar"]);
    assert!(!test_object.is_allowed(&fields("test.extension", None, None, None, Some("Hello"))).is_ok());
}

#[test]
fn should_not_allow_extension_if_publisher_is_not_in_allowed_list() {
    let test_object = service(json!({ "test": false }), &[]);
    assert!(!test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_not_allow_prerelease_extension_if_publisher_is_allowed_only_to_stable() {
    let test_object = service(json!({ "test": "stable" }), &[]);
    assert!(!test_object.is_allowed(&fields("test.extension", None, Some(true), None, None)).is_ok());
}

#[test]
fn should_allow_extension_if_publisher_is_set_to_random_value() {
    let test_object = service(json!({ "test": "hello" }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", None, Some(true), None, None)).is_ok());
}

#[test]
fn should_allow_extension_if_only_wildcard_is_in_allowed_list() {
    let test_object = service(json!({ "*": true }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_allow_extension_if_wildcard_is_in_allowed_list() {
    let test_object = service(json!({ "*": true, "hello": false }), &[]);
    assert!(test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_not_allow_extension_if_wildcard_is_not_in_allowed_list() {
    let test_object = service(json!({ "*": false, "hello": true }), &[]);
    assert!(!test_object.is_allowed(&fields("test.extension", None, None, None, None)).is_ok());
}

#[test]
fn should_allow_a_gallery_extension() {
    let test_object = service(json!({ "pub": true }), &[]);
    assert!(test_object.is_allowed(&AllowedCandidate::Gallery(&a_gallery_extension("name"))).is_ok());
}

#[test]
fn should_allow_a_local_extension() {
    let test_object = service(json!({ "pub": true }), &[]);
    assert!(test_object.is_allowed(&AllowedCandidate::Local(&a_local_extension("pub.name"))).is_ok());
}

#[test]
fn should_trigger_change_event_when_allowed_list_change() {
    let config = Rc::new(RefCell::new(json!({ "*": false })));
    let reader = config.clone();
    let test_object = AllowedExtensionsService::new(&[], move || Some(reader.borrow().clone()));
    let fired = Rc::new(Cell::new(0));
    let counter = fired.clone();
    let _listener = test_object.on_did_change_allowed_extensions_config_value().listen(move |_| counter.set(counter.get() + 1));

    test_object.on_configuration_changed();

    assert_eq!(fired.get(), 1);
}
