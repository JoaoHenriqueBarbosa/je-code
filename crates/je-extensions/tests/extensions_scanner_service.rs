//! Porte de `src/vs/platform/extensionManagement/test/node/extensionsScannerService.test.ts` (suítes `NativeExtensionsScanerService Test` e `ExtensionScannerInput`), caso a caso e na mesma ordem.
//!
//! Divergências: a E/S é síncrona sobre o [`MemFs`]. Nos casos de `manifest cache`, o `await timeout(CACHE_VALIDATION_DELAY)`
//! sob `runWithFakedTimers` vira a chamada explícita de `validate_pending_caches` (o limitador de 3 s é do laço de
//! eventos do produto, ver o módulo `scanner`).

use std::cell::RefCell;
use std::rc::Rc;

use je_extensions::fs::{ExtFs, MemFs};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionManifest, ExtensionType, TargetPlatform, get_manifest_cache_file_name};
use je_extensions::paths::{dirname, join_path, to_fs_path};
use je_extensions::profile::UserDataProfiles;
use je_extensions::profile_scanner::{ExtensionsProfileScannerService, ProfileExtensionsScanOptions};
use je_extensions::scanner::{
    ExtensionScannerInput, ExtensionsProduct, ExtensionsScannerService, ExtensionsScannerServiceParts, ManifestMetadata, ScanOptions, ScannedExtension,
    ScannerEnvironment, ScannerLocations, SystemExtensionsScanOptions, TranslationsMap, UserExtensionsScanOptions,
};
use je_platform::log::NullLogService;
use serde_json::{Map, Value, json};

const ROOT: &str = "/ROOT";

struct Fixture {
    fs: Rc<MemFs>,
    service: ExtensionsScannerService,
    translations: Rc<RefCell<TranslationsMap>>,
    profiles: UserDataProfiles,
}

fn product(quality: Option<&str>, auto_update: &[&str], built_in: &[&str]) -> ExtensionsProduct {
    ExtensionsProduct {
        version: "1.66.0".to_owned(),
        quality: quality.map(str::to_owned),
        built_in_extensions_enabled_with_auto_updates: auto_update.iter().map(|s| (*s).to_owned()).collect(),
        built_in_extensions: built_in.iter().map(|s| (*s).to_owned()).collect(),
        ..ExtensionsProduct::default()
    }
}

fn setup(product: ExtensionsProduct, is_built: bool) -> Fixture {
    let fs = Rc::new(MemFs::new());
    let translations = Rc::new(RefCell::new(TranslationsMap::new()));
    let system = join_path(ROOT, &["system"]);
    let user = join_path(ROOT, &["extensions"]);
    let profiles = UserDataProfiles::with_default(&user, ROOT, &join_path(ROOT, &["cache"]));
    let logger = NullLogService::shared();
    let profile_scanner = Rc::new(ExtensionsProfileScannerService::new(&user, fs.clone(), profiles.clone(), logger.clone()));
    let translations_for_closure = translations.clone();
    let service = ExtensionsScannerService::new(ExtensionsScannerServiceParts {
        locations: ScannerLocations {
            system_extensions_location: system.clone(),
            user_extensions_location: user.clone(),
            extensions_control_location: join_path(ROOT, &[".vscode-oss-dev", "extensions", "control.json"]),
            dev_builtin_extensions_location: join_path(ROOT, &[".build", "builtInExtensions"]),
        },
        environment: ScannerEnvironment { is_built, language: "en".to_owned(), ..ScannerEnvironment::default() },
        product,
        current_profile: profiles.default_profile.clone(),
        profiles: profiles.clone(),
        fs: fs.clone(),
        logger,
        profile_scanner,
        get_translations: Box::new(move |_| translations_for_closure.borrow().clone()),
    });
    fs.create_dir_all(&system).unwrap();
    fs.create_dir_all(&user).unwrap();
    Fixture { fs, service, translations, profiles }
}

fn default_setup() -> Fixture {
    setup(product(None, &[], &[]), false)
}

/// `anExtensionManifest`: `{ engines, version, main, activationEvents, ...overrides }`.
fn an_extension_manifest(overrides: Value) -> Value {
    let mut manifest = Map::new();
    manifest.insert("engines".into(), json!({ "vscode": "^1.66.0" }));
    manifest.insert("version".into(), json!("1.0.0"));
    manifest.insert("main".into(), json!("main.js"));
    manifest.insert("activationEvents".into(), json!(["*"]));
    for (key, value) in overrides.as_object().expect("objeto") {
        manifest.insert(key.clone(), value.clone());
    }
    Value::Object(manifest)
}

fn an_extension(f: &Fixture, manifest: &Value, root: &str) -> String {
    let text = |key: &str| manifest.get(key).map_or("undefined".to_owned(), |v| v.as_str().map_or(v.to_string(), str::to_owned));
    let location = join_path(root, &[&format!("{}.{}-{}", text("publisher"), text("name"), text("version"))]);
    f.fs.write(&join_path(&location, &["package.json"]), manifest.to_string().as_bytes()).unwrap();
    location
}

fn a_user_extension(f: &Fixture, manifest: &Value) -> String {
    an_extension(f, manifest, &join_path(ROOT, &["extensions"]))
}

fn a_system_extension(f: &Fixture, manifest: &Value) -> String {
    an_extension(f, manifest, &join_path(ROOT, &["system"]))
}

fn typed(manifest: &Value) -> ExtensionManifest {
    ExtensionManifest::from_value(manifest.clone())
}

fn assert_plain_user_extension(actual: &ScannedExtension, location: &str, manifest: &Value) {
    assert_eq!(actual.identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual.location, location);
    assert!(!actual.is_builtin);
    assert_eq!(actual.extension_type, ExtensionType::User);
    assert!(actual.is_valid);
    assert_eq!(actual.validations, vec![]);
    assert_eq!(actual.metadata, None);
    assert_eq!(actual.target_platform, TargetPlatform::Undefined);
    assert_eq!(actual.manifest, typed(manifest));
}

#[test]
fn scan_system_extension() {
    let f = default_setup();
    let manifest = an_extension_manifest(json!({ "name": "name", "publisher": "pub" }));
    let location = a_system_extension(&f, &manifest);

    let actual = f.service.scan_system_extensions(&SystemExtensionsScanOptions::default()).unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual[0].location, location);
    assert!(actual[0].is_builtin);
    assert_eq!(actual[0].extension_type, ExtensionType::System);
    assert!(actual[0].is_valid);
    assert_eq!(actual[0].validations, vec![]);
    assert_eq!(actual[0].metadata, None);
    assert_eq!(actual[0].target_platform, TargetPlatform::Undefined);
    assert_eq!(actual[0].manifest, typed(&manifest));
}

#[test]
fn scan_user_extensions() {
    let f = default_setup();
    let manifest = an_extension_manifest(json!({ "name": "name", "publisher": "pub" }));
    let location = a_user_extension(&f, &manifest);

    let actual = f.service.scan_all_user_extensions().unwrap();

    assert_eq!(actual.len(), 1);
    assert_plain_user_extension(&actual[0], &location, &manifest);
}

#[test]
fn scan_existing_extension() {
    let f = default_setup();
    let manifest = an_extension_manifest(json!({ "name": "name", "publisher": "pub" }));
    let location = a_user_extension(&f, &manifest);

    let actual = f.service.scan_existing_extension(&location, ExtensionType::User, &ScanOptions::default()).unwrap();

    assert_plain_user_extension(&actual.expect("a extensão existe"), &location, &manifest);
}

#[test]
fn scan_single_extension() {
    let f = default_setup();
    let manifest = an_extension_manifest(json!({ "name": "name", "publisher": "pub" }));
    let location = a_user_extension(&f, &manifest);

    let actual = f.service.scan_one_or_multiple_extensions(&location, ExtensionType::User, &ScanOptions::default());

    assert_eq!(actual.len(), 1);
    assert_plain_user_extension(&actual[0], &location, &manifest);
}

#[test]
fn scan_multiple_extensions() {
    let f = default_setup();
    let location = a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name2", "publisher": "pub" })));

    let actual = f.service.scan_one_or_multiple_extensions(&dirname(&location), ExtensionType::User, &ScanOptions::default());

    assert_eq!(actual.len(), 2);
    assert_eq!(actual[0].identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual[1].identifier, ExtensionIdentifierInfo::new("pub.name2"));
}

#[test]
fn scan_all_user_extensions_with_different_versions() {
    let f = default_setup();
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.0.1" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.0.2" })));

    let actual = f.service.scan_all_user_extensions_with(false, false).unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual[0].manifest.version, "1.0.2");
}

#[test]
fn scan_all_user_extensions_include_all_versions() {
    let f = default_setup();
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.0.1" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.0.2" })));

    let actual = f.service.scan_all_user_extensions().unwrap();

    assert_eq!(actual.len(), 2);
    assert_eq!(actual[0].identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual[0].manifest.version, "1.0.1");
    assert_eq!(actual[1].identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual[1].manifest.version, "1.0.2");
}

#[test]
fn scan_all_user_extensions_with_different_versions_and_higher_version_is_not_compatible() {
    let f = default_setup();
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.0.1" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.0.2", "engines": { "vscode": "^1.67.0" } })));

    let actual = f.service.scan_all_user_extensions_with(false, false).unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual[0].manifest.version, "1.0.1");
}

#[test]
fn scan_all_user_extensions_exclude_invalid_extensions() {
    let f = default_setup();
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name2", "publisher": "pub", "engines": { "vscode": "^1.67.0" } })));

    let actual = f.service.scan_all_user_extensions_with(false, false).unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].identifier, ExtensionIdentifierInfo::new("pub.name"));
}

#[test]
fn scan_all_user_extensions_include_invalid_extensions() {
    let f = default_setup();
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name2", "publisher": "pub", "engines": { "vscode": "^1.67.0" } })));

    let actual = f.service.scan_all_user_extensions_with(true, false).unwrap();

    assert_eq!(actual.len(), 2);
    assert_eq!(actual[0].identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual[1].identifier, ExtensionIdentifierInfo::new("pub.name2"));
}

#[test]
fn scan_system_extensions_include_additional_builtin_extensions() {
    let f = setup(product(None, &[], &["pub.name2", "pub.name"]), false);
    a_extension_in(&f, "name2", "1.0.0", &join_path(ROOT, &["additional"]));
    let extension_location = a_extension_in(&f, "name", "1.0.0", &join_path(ROOT, &["additional"]));
    a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.0.1" })));
    f.fs
        .write(
            &join_path(ROOT, &[".vscode-oss-dev", "extensions", "control.json"]),
            json!({ "pub.name2": "disabled", "pub.name": to_fs_path(&extension_location) }).to_string().as_bytes(),
        )
        .unwrap();

    let actual = f.service.scan_system_extensions(&SystemExtensionsScanOptions { check_control_file: true, ..SystemExtensionsScanOptions::default() }).unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual[0].manifest.version, "1.0.0");
}

fn a_extension_in(f: &Fixture, name: &str, version: &str, root: &str) -> String {
    an_extension(f, &an_extension_manifest(json!({ "name": name, "publisher": "pub", "version": version })), root)
}

#[test]
fn scan_all_user_extensions_with_default_nls_replacements() {
    let f = default_setup();
    let location = a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "displayName": "%displayName%" })));
    f.fs.write(&join_path(&location, &["package.nls.json"]), json!({ "displayName": "Hello World" }).to_string().as_bytes()).unwrap();

    let actual = f.service.scan_all_user_extensions().unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual[0].manifest.display_name.as_deref(), Some("Hello World"));
}

#[test]
fn scan_extension_with_en_nls_replacements() {
    let f = default_setup();
    let location = a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "displayName": "%displayName%" })));
    f.fs.write(&join_path(&location, &["package.nls.json"]), json!({ "displayName": "Hello World" }).to_string().as_bytes()).unwrap();
    let nls_location = join_path(&location, &["package.en.json"]);
    f.fs.write(&nls_location, json!({ "contents": { "package": { "displayName": "Hello World EN" } } }).to_string().as_bytes()).unwrap();
    *f.translations.borrow_mut() = TranslationsMap::from([("pub.name".to_owned(), to_fs_path(&nls_location))]);

    let actual = f.service.scan_existing_extension(&location, ExtensionType::User, &ScanOptions { language: Some("en".into()), ..ScanOptions::default() }).unwrap();

    let actual = actual.expect("a extensão existe");
    assert_eq!(actual.identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual.manifest.display_name.as_deref(), Some("Hello World EN"));
}

#[test]
fn scan_extension_falls_back_to_default_nls_replacements() {
    let f = default_setup();
    let location = a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "displayName": "%displayName%" })));
    f.fs.write(&join_path(&location, &["package.nls.json"]), json!({ "displayName": "Hello World" }).to_string().as_bytes()).unwrap();
    let nls_location = join_path(&location, &["package.en.json"]);
    f.fs.write(&nls_location, json!({ "contents": { "package": { "displayName": "Hello World EN" } } }).to_string().as_bytes()).unwrap();
    *f.translations.borrow_mut() = TranslationsMap::from([("pub.name2".to_owned(), to_fs_path(&nls_location))]);

    let actual = f.service.scan_existing_extension(&location, ExtensionType::User, &ScanOptions { language: Some("en".into()), ..ScanOptions::default() }).unwrap();

    let actual = actual.expect("a extensão existe");
    assert_eq!(actual.identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual.manifest.display_name.as_deref(), Some("Hello World"));
}

#[test]
fn scan_single_extension_with_manifest_metadata_retains_manifest_metadata() {
    let f = default_setup();
    let manifest = an_extension_manifest(json!({ "name": "name", "publisher": "pub" }));
    let expected_metadata = json!({ "size": 12345, "installedTimestamp": 1234567890, "targetPlatform": "darwin-arm64" });
    let mut with_metadata = manifest.clone();
    with_metadata["__metadata"] = expected_metadata;
    let location = a_user_extension(&f, &with_metadata);

    let actual = f.service.scan_existing_extension(&location, ExtensionType::User, &ScanOptions::default()).unwrap().expect("a extensão existe");

    assert_eq!(actual.identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(actual.location, location);
    assert!(!actual.is_builtin);
    assert_eq!(actual.extension_type, ExtensionType::User);
    assert!(actual.is_valid);
    assert_eq!(actual.validations, vec![]);
    let metadata = actual.metadata.clone().expect("metadata");
    assert_eq!((metadata.size, metadata.installed_timestamp, metadata.target_platform), (Some(12345.0), Some(1234567890.0), Some(TargetPlatform::DarwinArm64)));
    assert_eq!(actual.manifest, typed(&manifest));
}

fn default_profile_scan(f: &Fixture) -> UserExtensionsScanOptions {
    UserExtensionsScanOptions { profile_location: f.profiles.default_profile.extensions_resource.clone(), ..UserExtensionsScanOptions::default() }
}

#[test]
fn scan_user_extension_with_matching_product_version_is_included() {
    let f = setup(product(Some("stable"), &["pub.name"], &[]), false);
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.66.1" })));

    let actual = f.service.scan_user_extensions(&default_profile_scan(&f)).unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].manifest.version, "1.66.1");
}

#[test]
fn scan_user_extension_with_different_version_is_included_when_force_auto_update_is_enabled() {
    let f = setup(product(Some("stable"), &["pub.name"], &[]), false);
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.67.0" })));

    let actual = f.service.scan_user_extensions(&default_profile_scan(&f)).unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].manifest.version, "1.67.0");
}

#[test]
fn scan_user_extension_not_in_auto_update_builtin_extensions_is_not_filtered() {
    let f = setup(product(Some("stable"), &["pub.other"], &[]), false);
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.67.0" })));

    let actual = f.service.scan_user_extensions(&default_profile_scan(&f)).unwrap();

    assert_eq!(actual.len(), 1);
}

#[test]
fn scan_picks_latest_version_when_multiple_versions_exist() {
    let f = setup(product(Some("stable"), &["pub.name"], &[]), false);
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.66.1" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.67.0" })));

    let actual = f.service.scan_user_extensions(&default_profile_scan(&f)).unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].manifest.version, "1.67.0");
}

#[test]
fn scan_all_extensions_prefers_matching_user_extension_over_system_extension() {
    let f = setup(product(Some("stable"), &["pub.name"], &[]), false);
    a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.66.0" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.66.1" })));

    let actual = f.service.scan_all_extensions(&SystemExtensionsScanOptions::default(), &default_profile_scan(&f)).unwrap();

    let extension = actual.iter().find(|e| e.identifier.id == "pub.name").expect("a extensão existe");
    assert_eq!(extension.manifest.version, "1.66.1");
    assert!(!extension.is_builtin);
}

#[test]
fn scan_all_extensions_picks_user_extension_with_newer_version_over_system_extension() {
    let f = setup(product(Some("stable"), &["pub.name"], &[]), false);
    a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.66.0" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.67.0" })));

    let actual = f.service.scan_all_extensions(&SystemExtensionsScanOptions::default(), &default_profile_scan(&f)).unwrap();

    let extension = actual.iter().find(|e| e.identifier.id == "pub.name").expect("a extensão existe");
    assert_eq!(extension.manifest.version, "1.67.0");
    assert_eq!(extension.extension_type, ExtensionType::User);
}

#[test]
fn system_extension_has_auto_update_set_to_true_when_in_auto_update_builtin_extensions_and_quality_is_stable() {
    let f = setup(product(Some("stable"), &["pub.name"], &[]), false);
    a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));

    let actual = f.service.scan_system_extensions(&SystemExtensionsScanOptions::default()).unwrap();

    assert_eq!(actual.len(), 1);
    assert!(actual[0].force_auto_update);
}

#[test]
fn system_extension_has_auto_update_set_to_false_when_not_in_auto_update_builtin_extensions() {
    let f = setup(product(Some("stable"), &["pub.other"], &[]), false);
    a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));

    let actual = f.service.scan_system_extensions(&SystemExtensionsScanOptions::default()).unwrap();

    assert_eq!(actual.len(), 1);
    assert!(!actual[0].force_auto_update);
}

#[test]
fn system_extension_has_auto_update_set_to_false_when_quality_is_not_stable() {
    let f = setup(product(Some("insider"), &["pub.name"], &[]), false);
    a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));

    let actual = f.service.scan_system_extensions(&SystemExtensionsScanOptions::default()).unwrap();

    assert_eq!(actual.len(), 1);
    assert!(!actual[0].force_auto_update);
}

#[test]
fn scan_user_extension_is_excluded_when_auto_update_is_disabled_non_stable_quality() {
    let f = setup(product(Some("insider"), &["pub.name"], &[]), false);
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.66.1" })));

    let actual = f.service.scan_user_extensions(&default_profile_scan(&f)).unwrap();

    assert_eq!(actual.len(), 0);
}

#[test]
fn scan_all_extensions_uses_system_version_when_auto_update_is_disabled_non_stable_quality() {
    let f = setup(product(Some("insider"), &["pub.name"], &[]), false);
    a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.66.0" })));
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": "1.66.1" })));

    let actual = f.service.scan_all_extensions(&SystemExtensionsScanOptions::default(), &default_profile_scan(&f)).unwrap();

    let extension = actual.iter().find(|e| e.identifier.id == "pub.name").expect("a extensão existe");
    assert_eq!(extension.manifest.version, "1.66.0");
    assert_eq!(extension.extension_type, ExtensionType::System);
}

fn cache_file_names(f: &Fixture) -> Vec<String> {
    let mut names: Vec<String> = f.fs.read_dir(&f.profiles.default_profile.cache_home).map(|entries| entries.into_iter().map(|e| e.name).collect()).unwrap_or_default();
    names.sort();
    names
}

fn system_scan(f: &Fixture, language: Option<&str>) -> Vec<ScannedExtension> {
    f.service.scan_system_extensions(&SystemExtensionsScanOptions { language: language.map(str::to_owned), ..SystemExtensionsScanOptions::default() }).unwrap()
}

fn cached_user_scan(f: &Fixture, language: &str) -> Vec<ScannedExtension> {
    f.service
        .scan_user_extensions(&UserExtensionsScanOptions { language: Some(language.to_owned()), use_cache: true, ..default_profile_scan(f) })
        .unwrap()
}

fn built_setup() -> Fixture {
    // The cache is only used for built products, development always scans from disk
    setup(product(None, &[], &[]), true)
}

#[test]
fn system_extensions_are_cached_per_scan_language() {
    let f = built_setup();
    a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));

    system_scan(&f, None);
    system_scan(&f, Some("en"));
    system_scan(&f, Some("zh-CN"));

    let mut expected = vec![
        get_manifest_cache_file_name(ExtensionType::System, None),
        get_manifest_cache_file_name(ExtensionType::System, Some("en")),
        get_manifest_cache_file_name(ExtensionType::System, Some("zh-CN")),
    ];
    expected.sort();
    assert_eq!(cache_file_names(&f), expected);
}

fn rewrite_manifest(f: &Fixture, extension_location: &str, version: &str) {
    f.fs
        .write(&join_path(extension_location, &["package.json"]), an_extension_manifest(json!({ "name": "name", "publisher": "pub", "version": version })).to_string().as_bytes())
        .unwrap();
}

#[test]
fn system_extension_scan_is_only_served_from_the_cache_of_the_same_language() {
    let f = built_setup();
    let location = a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));
    system_scan(&f, Some("en"));

    // Rewrite the manifest, which leaves the mtime of the scanned location untouched
    rewrite_manifest(&f, &location, "2.0.0");
    let cached = system_scan(&f, Some("en"));
    let rescanned = system_scan(&f, Some("de"));

    assert_eq!((cached[0].manifest.version.as_str(), rescanned[0].manifest.version.as_str()), ("1.0.0", "2.0.0"));
}

#[test]
fn user_extensions_are_cached_per_scan_language() {
    let f = built_setup();
    a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));

    cached_user_scan(&f, "en");

    assert_eq!(cache_file_names(&f), vec![get_manifest_cache_file_name(ExtensionType::User, Some("en"))]);
}

#[test]
fn a_system_cache_hit_validates_the_cache_by_scanning_again() {
    let f = built_setup();
    let location = a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));
    system_scan(&f, Some("en"));

    // Leaves the mtime of the scanned location untouched, so the next scan still hits the cache
    rewrite_manifest(&f, &location, "2.0.0");
    system_scan(&f, Some("en"));
    f.service.validate_pending_caches();

    assert_eq!(cache_file_names(&f), Vec::<String>::new());
}

#[test]
fn a_user_cache_hit_validates_the_cache_by_scanning_again() {
    let f = built_setup();
    let location = a_user_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));
    // The first scan creates the profile resource, so scan twice to cache a stable input
    cached_user_scan(&f, "en");
    cached_user_scan(&f, "en");

    // Leaves the mtime of the scanned profile resource untouched, so the next scan still hits the cache
    rewrite_manifest(&f, &location, "2.0.0");
    cached_user_scan(&f, "en");
    f.service.validate_pending_caches();

    assert_eq!(cache_file_names(&f), Vec::<String>::new());
}

#[test]
fn cache_hits_in_several_languages_are_all_validated_not_just_the_last_one() {
    let f = built_setup();
    let location = a_system_extension(&f, &an_extension_manifest(json!({ "name": "name", "publisher": "pub" })));
    system_scan(&f, Some("en"));
    system_scan(&f, Some("de"));

    // Leaves the mtime of the scanned location untouched, so both scans still hit their cache
    rewrite_manifest(&f, &location, "2.0.0");
    system_scan(&f, Some("en"));
    system_scan(&f, Some("de"));
    f.service.validate_pending_caches();

    assert_eq!(cache_file_names(&f), Vec::<String>::new());
}

fn an_input(location: &str, mtime: Option<u64>) -> ExtensionScannerInput {
    ExtensionScannerInput {
        location: location.to_owned(),
        mtime,
        application_extensions_location: None,
        application_extensions_location_mtime: None,
        profile: false,
        profile_scan_options: None,
        extension_type: ExtensionType::User,
        validate: true,
        product_version: "1.1.1".to_owned(),
        product_date: None,
        product_commit: None,
        dev_mode: true,
        language: None,
        translations: TranslationsMap::new(),
    }
}

#[test]
fn compare_inputs_location() {
    let foo = join_path(ROOT, &["foo"]);
    assert!(ExtensionScannerInput::equals(&an_input(ROOT, None), &an_input(ROOT, None)));
    assert!(ExtensionScannerInput::equals(&an_input(ROOT, Some(100)), &an_input(ROOT, Some(100))));
    assert!(!ExtensionScannerInput::equals(&an_input(&foo, None), &an_input(ROOT, None)));
    assert!(!ExtensionScannerInput::equals(&an_input(ROOT, Some(100)), &an_input(ROOT, Some(200))));
    assert!(!ExtensionScannerInput::equals(&an_input(ROOT, None), &an_input(ROOT, Some(200))));
}

#[test]
fn compare_inputs_application_location() {
    let an_input = |location: &str, mtime: Option<u64>| ExtensionScannerInput {
        application_extensions_location: Some(location.to_owned()),
        application_extensions_location_mtime: mtime,
        ..an_input(ROOT, None)
    };
    let foo = join_path(ROOT, &["foo"]);
    assert!(ExtensionScannerInput::equals(&an_input(ROOT, None), &an_input(ROOT, None)));
    assert!(ExtensionScannerInput::equals(&an_input(ROOT, Some(100)), &an_input(ROOT, Some(100))));
    assert!(!ExtensionScannerInput::equals(&an_input(&foo, None), &an_input(ROOT, None)));
    assert!(!ExtensionScannerInput::equals(&an_input(ROOT, Some(100)), &an_input(ROOT, Some(200))));
    assert!(!ExtensionScannerInput::equals(&an_input(ROOT, None), &an_input(ROOT, Some(200))));
}

#[test]
fn compare_inputs_profile() {
    let an_input = |profile: bool, options: Option<ProfileExtensionsScanOptions>| ExtensionScannerInput { profile, profile_scan_options: options, ..an_input(ROOT, None) };
    let bail = |value: Option<bool>| Some(ProfileExtensionsScanOptions { bail_out_when_file_not_found: value });
    assert!(ExtensionScannerInput::equals(&an_input(true, bail(Some(true))), &an_input(true, bail(Some(true)))));
    assert!(ExtensionScannerInput::equals(&an_input(false, bail(Some(true))), &an_input(false, bail(Some(true)))));
    assert!(ExtensionScannerInput::equals(&an_input(true, bail(Some(false))), &an_input(true, bail(Some(false)))));
    assert!(ExtensionScannerInput::equals(&an_input(true, bail(None)), &an_input(true, bail(None))));
    assert!(!ExtensionScannerInput::equals(&an_input(true, bail(Some(true))), &an_input(true, bail(Some(false)))));
    assert!(!ExtensionScannerInput::equals(&an_input(true, bail(None)), &an_input(true, bail(Some(true)))));
    assert!(!ExtensionScannerInput::equals(&an_input(true, None), &an_input(true, bail(None))));
    assert!(!ExtensionScannerInput::equals(&an_input(false, bail(Some(true))), &an_input(true, bail(Some(true)))));
}

#[test]
fn compare_inputs_extension_type() {
    let an_input = |extension_type: ExtensionType| ExtensionScannerInput { extension_type, ..an_input(ROOT, None) };
    assert!(ExtensionScannerInput::equals(&an_input(ExtensionType::System), &an_input(ExtensionType::System)));
    assert!(ExtensionScannerInput::equals(&an_input(ExtensionType::User), &an_input(ExtensionType::User)));
    assert!(!ExtensionScannerInput::equals(&an_input(ExtensionType::User), &an_input(ExtensionType::System)));
}

// Os dois ajudantes que o original expõe só para os testes de `ManifestMetadata` ficam no escopo do módulo.
#[allow(dead_code)]
fn _manifest_metadata_is_part_of_the_api(_: ManifestMetadata) {}
