//! Porte de `src/vs/workbench/contrib/extensions/test/electron-browser/extension.test.ts` (suíte `Extension Test`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso; o `TestFileService` que o
//!   `Extension` recebe é entregue a ele.
//! - `TestInstantiationService.stub(IProductService, { quality })` e `createInstance(Extension, ...)` viram a construção
//!   direta da `Extension` com os serviços de `support::extension_services_support` (a galeria, a telemetria, o log e o
//!   `FileAccess` que o original deixa `undefined` porque nenhum caso os chama) e um `ProductService` com a `quality` dada.
//! - `Partial<IExtensionManifest>` é o JSON do manifesto, mesclado sobre `{ name, publisher: 'pub', version: '1.0.0' }`;
//!   `Partial<ILocalExtension>` e `Partial<IGalleryExtension>` são as structs [`LocalProperties`] e [`GalleryProperties`]
//!   com os campos que os casos sobrescrevem.
//! - `Object.create({ ... })` (campos no protótipo) é a struct preenchida: o que o original deixa `undefined` fica no
//!   valor neutro do tipo (`false`, vazio).
//! - `URI.file('pub.<name>')` é o caminho `pub.<name>`, que é como a `LocalExtension` do porte guarda a pasta.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::uuid::generate_uuid;
use je_extensions::management::{
    GalleryExtension, GalleryExtensionAsset, GalleryExtensionAssets, GalleryExtensionProperties, InstallSource, LocalExtension,
};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest, ExtensionType, TargetPlatform};
use je_extensions::util::get_gallery_extension_id;
use je_platform::product::ProductService;
use je_workbench::contrib::extensions::extensions::{ExtensionState, IExtension};
use je_workbench::contrib::extensions::extensions_workbench_service::Extension;
use je_workbench::test::common::workbench_test_services::TestFileService;
use serde_json::{Map, Value, json};
use support::extension_services_support::extension_services;

/// O que `Partial<ILocalExtension>` sobrescreve nos casos.
#[derive(Default)]
struct LocalProperties {
    extension_type: Option<ExtensionType>,
    target_platform: Option<TargetPlatform>,
    pre_release: Option<bool>,
    is_pre_release_version: Option<bool>,
}

/// O que `Partial<IGalleryExtension>` e `Partial<IGalleryExtensionProperties>` sobrescrevem nos casos.
#[derive(Default)]
struct GalleryProperties {
    version: Option<&'static str>,
    target_platform: Option<TargetPlatform>,
    is_pre_release_version: Option<bool>,
}

/// `instantiationService.stub(IProductService, { quality })`.
fn product_service(quality: &str) -> ProductService {
    ProductService { quality: Some(quality.to_owned()), ..ProductService::default() }
}

/// `instantiationService.createInstance(Extension, () => ExtensionState.Installed, () => undefined, undefined, local, gallery, undefined)`.
fn create_extension(guard: &LeakGuard, quality: &str, local: Option<LocalExtension>, gallery: Option<GalleryExtension>) -> Extension {
    let file_service = guard.add_rc(TestFileService::new());
    Extension::new(
        Rc::new(|_: &Extension| ExtensionState::Installed),
        Rc::new(|_: &Extension| None),
        None,
        local.map(Rc::new),
        gallery.map(Rc::new),
        None,
        extension_services(file_service, product_service(quality)),
    )
}

/// `aLocalExtension(name, manifest, properties)`.
fn a_local_extension(name: &str, manifest: Value, properties: LocalProperties) -> LocalExtension {
    let Value::Object(patch) = manifest else { panic!("o manifesto é um objeto") };
    let mut manifest = Map::new();
    manifest.insert("name".to_owned(), json!(name));
    manifest.insert("publisher".to_owned(), json!("pub"));
    manifest.insert("version".to_owned(), json!("1.0.0"));
    manifest.extend(patch);
    let manifest = ExtensionManifest::from_value(Value::Object(manifest));
    LocalExtension {
        extension_type: properties.extension_type.unwrap_or(ExtensionType::User),
        is_builtin: false,
        identifier: ExtensionIdentifierInfo::new(get_gallery_extension_id(Some(&manifest.publisher), &manifest.name)),
        location: format!("pub.{name}"),
        target_platform: properties.target_platform.unwrap_or(TargetPlatform::Undefined),
        manifest,
        publisher_display_name: None,
        readme_url: None,
        changelog_url: None,
        is_valid: true,
        validations: Vec::new(),
        pre_release: properties.pre_release.unwrap_or(false),
        is_workspace_scoped: false,
        is_machine_scoped: false,
        is_application_scoped: false,
        publisher_id: None,
        installed_timestamp: None,
        is_pre_release_version: properties.is_pre_release_version.unwrap_or(false),
        has_pre_release_version: false,
        private: false,
        updated: false,
        pinned: false,
        force_auto_update: false,
        source: InstallSource::Gallery,
        size: 0.0,
    }
}

fn some_local_extension() -> LocalExtension {
    a_local_extension("someext", json!({}), LocalProperties::default())
}

/// `aGalleryExtension(name, properties, galleryExtensionProperties)`.
fn a_gallery_extension(name: &str, properties: GalleryProperties) -> GalleryExtension {
    let target_platform = properties.target_platform.unwrap_or(TargetPlatform::Undefined);
    let asset = || GalleryExtensionAsset { uri: String::new(), fallback_uri: String::new() };
    GalleryExtension {
        name: name.to_owned(),
        identifier: ExtensionIdentifierInfo { id: get_gallery_extension_id(Some("pub"), name), uuid: Some(generate_uuid()) },
        version: properties.version.unwrap_or("1.0.0").to_owned(),
        display_name: String::new(),
        publisher_id: String::new(),
        publisher: "pub".to_owned(),
        publisher_display_name: String::new(),
        publisher_domain: None,
        publisher_link: None,
        publisher_sponsor_link: None,
        description: String::new(),
        install_count: 0.0,
        rating: 0.0,
        rating_count: 0.0,
        categories: Vec::new(),
        tags: Vec::new(),
        release_date: 0.0,
        last_updated: 0.0,
        preview: false,
        private: false,
        blocking_info: None,
        has_pre_release_version: false,
        has_release_version: false,
        is_signed: false,
        all_target_platforms: vec![target_platform],
        assets: GalleryExtensionAssets {
            manifest: None,
            readme: None,
            changelog: None,
            license: None,
            repository: None,
            download: asset(),
            icon: None,
            signature: None,
            core_translations: Vec::new(),
        },
        properties: GalleryExtensionProperties {
            dependencies: Some(Vec::new()),
            extension_pack: None,
            engine: None,
            enabled_api_proposals: None,
            localized_languages: None,
            target_platform,
            is_pre_release_version: properties.is_pre_release_version.unwrap_or(false),
            executes_code: None,
        },
        details_link: None,
        rating_link: None,
        support_link: None,
    }
}

fn some_gallery_extension() -> GalleryExtension {
    a_gallery_extension("somext", GalleryProperties::default())
}

#[test]
fn extension_is_not_outdated_when_there_is_no_local_and_gallery() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(&guard, "insiders", None, None);
    assert!(!extension.outdated());
}

#[test]
fn extension_is_not_outdated_when_there_is_local_and_no_gallery() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(&guard, "insiders", Some(some_local_extension()), None);
    assert!(!extension.outdated());
}

#[test]
fn extension_is_not_outdated_when_there_is_no_local_and_has_gallery() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(&guard, "insiders", None, Some(some_gallery_extension()));
    assert!(!extension.outdated());
}

#[test]
fn extension_is_not_outdated_when_local_and_gallery_are_on_same_version() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(&guard, "insiders", Some(some_local_extension()), Some(some_gallery_extension()));
    assert!(!extension.outdated());
}

#[test]
fn extension_is_outdated_when_local_is_older_than_gallery() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension("somext", json!({ "version": "1.0.0" }), LocalProperties::default())),
        Some(a_gallery_extension("somext", GalleryProperties { version: Some("1.0.1"), ..GalleryProperties::default() })),
    );
    assert!(extension.outdated());
}

#[test]
fn extension_is_outdated_when_local_is_built_in_and_older_than_gallery() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension(
            "somext",
            json!({ "version": "1.0.0" }),
            LocalProperties { extension_type: Some(ExtensionType::System), ..LocalProperties::default() },
        )),
        Some(a_gallery_extension("somext", GalleryProperties { version: Some("1.0.1"), ..GalleryProperties::default() })),
    );
    assert!(extension.outdated());
}

#[test]
fn extension_is_not_outdated_when_local_is_built_in_and_older_than_gallery_but_product_quality_is_stable() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "stable",
        Some(a_local_extension(
            "somext",
            json!({ "version": "1.0.0" }),
            LocalProperties { extension_type: Some(ExtensionType::System), ..LocalProperties::default() },
        )),
        Some(a_gallery_extension("somext", GalleryProperties { version: Some("1.0.1"), ..GalleryProperties::default() })),
    );
    assert!(!extension.outdated());
}

#[test]
fn extension_is_outdated_when_local_and_gallery_are_on_same_version_but_on_different_target_platforms() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension("somext", json!({}), LocalProperties { target_platform: Some(TargetPlatform::Win32Arm64), ..LocalProperties::default() })),
        Some(a_gallery_extension("somext", GalleryProperties { target_platform: Some(TargetPlatform::Win32X64), ..GalleryProperties::default() })),
    );
    assert!(extension.outdated());
}

#[test]
fn extension_is_not_outdated_when_local_and_gallery_are_on_same_version_and_local_is_on_web() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension("somext", json!({}), LocalProperties { target_platform: Some(TargetPlatform::Web), ..LocalProperties::default() })),
        Some(a_gallery_extension("somext", GalleryProperties::default())),
    );
    assert!(!extension.outdated());
}

#[test]
fn extension_is_not_outdated_when_local_and_gallery_are_on_same_version_and_gallery_is_on_web() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension("somext", json!({}), LocalProperties::default())),
        Some(a_gallery_extension("somext", GalleryProperties { target_platform: Some(TargetPlatform::Web), ..GalleryProperties::default() })),
    );
    assert!(!extension.outdated());
}

#[test]
fn extension_is_not_outdated_when_local_is_not_pre_release_but_gallery_is_pre_release() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension("somext", json!({ "version": "1.0.0" }), LocalProperties::default())),
        Some(a_gallery_extension("somext", GalleryProperties { version: Some("1.0.1"), is_pre_release_version: Some(true), ..GalleryProperties::default() })),
    );
    assert!(!extension.outdated());
}

#[test]
fn extension_is_outdated_when_local_and_gallery_are_pre_releases() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension(
            "somext",
            json!({ "version": "1.0.0" }),
            LocalProperties { pre_release: Some(true), is_pre_release_version: Some(true), ..LocalProperties::default() },
        )),
        Some(a_gallery_extension("somext", GalleryProperties { version: Some("1.0.1"), is_pre_release_version: Some(true), ..GalleryProperties::default() })),
    );
    assert!(extension.outdated());
}

#[test]
fn extension_is_outdated_when_local_was_opted_to_pre_release_but_current_version_is_not_pre_release() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension(
            "somext",
            json!({ "version": "1.0.0" }),
            LocalProperties { pre_release: Some(true), is_pre_release_version: Some(false), ..LocalProperties::default() },
        )),
        Some(a_gallery_extension("somext", GalleryProperties { version: Some("1.0.1"), is_pre_release_version: Some(true), ..GalleryProperties::default() })),
    );
    assert!(extension.outdated());
}

#[test]
fn extension_is_outdated_when_local_is_pre_release_but_gallery_is_not() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension(
            "somext",
            json!({ "version": "1.0.0" }),
            LocalProperties { pre_release: Some(true), is_pre_release_version: Some(true), ..LocalProperties::default() },
        )),
        Some(a_gallery_extension("somext", GalleryProperties { version: Some("1.0.1"), ..GalleryProperties::default() })),
    );
    assert!(extension.outdated());
}

#[test]
fn extension_is_outdated_when_local_was_opted_pre_release_but_current_version_is_not_and_gallery_is_not() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let extension = create_extension(
        &guard,
        "insiders",
        Some(a_local_extension(
            "somext",
            json!({ "version": "1.0.0" }),
            LocalProperties { pre_release: Some(true), is_pre_release_version: Some(false), ..LocalProperties::default() },
        )),
        Some(a_gallery_extension("somext", GalleryProperties { version: Some("1.0.1"), ..GalleryProperties::default() })),
    );
    assert!(extension.outdated());
}
