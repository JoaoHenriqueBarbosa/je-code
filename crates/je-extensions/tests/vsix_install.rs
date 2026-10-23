//! Teste de integração do instalador de VSIX (`management_service` e `watcher`), sobre o `MemFs`. Não há equivalente no
//! original: no VS Code o fluxo é coberto pelos testes de integração do produto. Confere o layout em disco, o
//! `extensions.json`, o `.obsolete` e a limpeza.

mod common;

use std::rc::Rc;

use common::{FakeGallery, a_manifest, build_vsix, setup};
use je_extensions::fs::ExtFs;
use je_extensions::management::{ExtensionManagementErrorCode, InstallOptions, InstallSource, UninstallOptions};
use je_extensions::manifest::{ExtensionIdentifierInfo, ExtensionType};
use je_extensions::paths::join_path;
use je_extensions::watcher::ExtensionsWatcher;
use je_platform::log::NullLogService;
use serde_json::{Value, json};

fn put_vsix(env: &common::Env, name: &str, bytes: &[u8]) -> String {
    let path = join_path("/ROOT/downloads", &[name]);
    env.fs.write(&path, bytes).unwrap();
    path
}

fn extensions_json(env: &common::Env) -> Value {
    serde_json::from_str(&env.fs.read_to_string(&env.profiles.default_profile.extensions_resource).unwrap()).unwrap()
}

fn watcher(env: &common::Env) -> ExtensionsWatcher {
    let watcher = ExtensionsWatcher::new(env.management.clone(), env.scanner.clone(), env.profile_scanner.clone(), env.profiles.clone(), NullLogService::shared());
    watcher.initialize();
    watcher
}

#[test]
fn installs_a_vsix_into_the_extensions_folder_and_the_profile() {
    let env = setup(Rc::new(FakeGallery::new(false)), None);
    let vsix = put_vsix(&env, "a.vsix", &build_vsix(&a_manifest("pub", "name", "1.2.3"), &[("out/main.js", b"module.exports = {};")]));

    let local = env.management.install(&vsix, &InstallOptions::default()).unwrap();

    let folder = join_path(&env.user_dir, &["pub.name-1.2.3"]);
    assert_eq!(local.location, folder);
    assert_eq!(local.identifier, ExtensionIdentifierInfo::new("pub.name"));
    assert_eq!(local.source, InstallSource::Vsix);
    assert!(local.is_valid);
    assert_eq!(env.fs.read(&join_path(&folder, &["out/main.js"])).unwrap(), b"module.exports = {};");
    // O `__metadata` é gravado no package.json instalado, com indentação de tabulação.
    let manifest_text = env.fs.read_to_string(&join_path(&folder, &["package.json"])).unwrap();
    assert!(manifest_text.contains("\n\t\"__metadata\""));
    let manifest: Value = serde_json::from_str(&manifest_text).unwrap();
    assert_eq!(manifest["__metadata"]["targetPlatform"], "undefined");
    assert!(manifest["__metadata"]["installedTimestamp"].as_f64().unwrap() > 0.0);
    assert!(manifest["__metadata"]["size"].as_f64().unwrap() > 0.0);
    // O extensions.json guarda o `relativeLocation` e o JSON da URI, como o VS Code.
    let listed = extensions_json(&env);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["identifier"], json!({ "id": "pub.name" }));
    assert_eq!(listed[0]["version"], "1.2.3");
    assert_eq!(listed[0]["relativeLocation"], "pub.name-1.2.3");
    assert_eq!(listed[0]["location"]["scheme"], "file");
    assert_eq!(listed[0]["location"]["path"], folder);
    assert_eq!(listed[0]["metadata"]["source"], "vsix");
    // A varredura enxerga a extensão instalada.
    let installed = env.management.get_installed(Some(ExtensionType::User), None, None, None).unwrap();
    assert_eq!(installed.len(), 1);
    assert_eq!(installed[0].manifest.version, "1.2.3");
}

#[test]
fn rejects_a_vsix_that_is_not_compatible_with_the_product_version() {
    let env = setup(Rc::new(FakeGallery::new(false)), None);
    let mut manifest = a_manifest("pub", "name", "1.0.0");
    manifest["engines"] = json!({ "vscode": "^1.200.0" });
    let vsix = put_vsix(&env, "a.vsix", &build_vsix(&manifest, &[]));

    let error = env.management.install(&vsix, &InstallOptions::default()).expect_err("incompatível");

    assert!(error.message.contains("not compatible with VS Code '1.100.0'"));
    assert!(!env.fs.exists(&join_path(&env.user_dir, &["pub.name-1.0.0"])));
}

#[test]
fn rejects_a_corrupt_zip() {
    let env = setup(Rc::new(FakeGallery::new(false)), None);
    let vsix = put_vsix(&env, "a.vsix", b"isto nao e um zip");

    let error = env.management.install(&vsix, &InstallOptions::default()).expect_err("zip corrompido");

    assert_eq!(error.code, ExtensionManagementErrorCode::CorruptZip);
}

#[test]
fn rejects_a_vsix_whose_extension_is_not_allowed() {
    let env = setup(Rc::new(FakeGallery::new(false)), Some(json!({ "other.thing": true })));
    let vsix = put_vsix(&env, "a.vsix", &build_vsix(&a_manifest("pub", "name", "1.0.0"), &[]));

    let error = env.management.install(&vsix, &InstallOptions::default()).expect_err("não permitida");

    assert_eq!(error.code, ExtensionManagementErrorCode::NotAllowed);
}

#[test]
fn updating_to_a_new_version_marks_the_old_folder_for_removal_and_the_cleanup_deletes_it() {
    let env = setup(Rc::new(FakeGallery::new(false)), None);
    let _watcher = watcher(&env);
    let v1 = put_vsix(&env, "1.vsix", &build_vsix(&a_manifest("pub", "name", "1.0.0"), &[]));
    let v2 = put_vsix(&env, "2.vsix", &build_vsix(&a_manifest("pub", "name", "2.0.0"), &[]));

    env.management.install(&v1, &InstallOptions::default()).unwrap();
    let updated = env.management.install(&v2, &InstallOptions::default()).unwrap();

    assert_eq!(updated.manifest.version, "2.0.0");
    let listed = extensions_json(&env);
    assert_eq!(listed.as_array().unwrap().len(), 1);
    assert_eq!(listed[0]["version"], "2.0.0");
    // A pasta antiga continua até a limpeza, marcada no `.obsolete`.
    let old = join_path(&env.user_dir, &["pub.name-1.0.0"]);
    assert!(env.fs.exists(&old));
    let obsolete: Value = serde_json::from_str(&env.fs.read_to_string(&join_path(&env.user_dir, &[".obsolete"])).unwrap()).unwrap();
    assert_eq!(obsolete, json!({ "pub.name-1.0.0": true }));

    env.management.clean_up();

    assert!(!env.fs.exists(&old));
    assert!(env.fs.exists(&join_path(&env.user_dir, &["pub.name-2.0.0"])));
    assert!(!env.fs.exists(&join_path(&env.user_dir, &[".obsolete"])));
}

#[test]
fn uninstalling_removes_the_extension_from_the_profile_and_the_cleanup_deletes_the_folder() {
    let env = setup(Rc::new(FakeGallery::new(false)), None);
    let _watcher = watcher(&env);
    let vsix = put_vsix(&env, "a.vsix", &build_vsix(&a_manifest("pub", "name", "1.0.0"), &[]));
    let local = env.management.install(&vsix, &InstallOptions::default()).unwrap();
    let storage = join_path("/ROOT/User/globalStorage", &["pub.name"]);
    env.fs.write(&join_path(&storage, &["state.json"]), b"{}").unwrap();

    env.management.uninstall(&local, &UninstallOptions::default()).unwrap();

    assert_eq!(extensions_json(&env), json!([]));
    assert!(!env.fs.exists(&storage), "a pasta de armazenamento global some junto");
    assert!(env.fs.exists(&local.location));
    env.management.clean_up();
    assert!(!env.fs.exists(&local.location));
}

#[test]
fn reinstalling_the_same_version_of_a_vsix_replaces_the_folder() {
    let env = setup(Rc::new(FakeGallery::new(false)), None);
    let first = put_vsix(&env, "1.vsix", &build_vsix(&a_manifest("pub", "name", "1.0.0"), &[("marker.txt", b"first")]));
    let second = put_vsix(&env, "2.vsix", &build_vsix(&a_manifest("pub", "name", "1.0.0"), &[("marker.txt", b"second")]));

    env.management.install(&first, &InstallOptions::default()).unwrap();
    let local = env.management.install(&second, &InstallOptions::default()).unwrap();

    assert_eq!(env.fs.read(&join_path(&local.location, &["marker.txt"])).unwrap(), b"second");
    assert_eq!(extensions_json(&env).as_array().unwrap().len(), 1);
}

#[test]
fn an_extension_that_another_extension_depends_on_cannot_be_uninstalled() {
    let env = setup(Rc::new(FakeGallery::new(false)), None);
    let base = put_vsix(&env, "base.vsix", &build_vsix(&a_manifest("pub", "base", "1.0.0"), &[]));
    let mut dependent_manifest = a_manifest("pub", "dependent", "1.0.0");
    dependent_manifest["extensionDependencies"] = json!(["pub.base"]);
    let dependent = put_vsix(&env, "dependent.vsix", &build_vsix(&dependent_manifest, &[]));
    let base_local = env.management.install(&base, &InstallOptions { donot_include_pack_and_dependencies: true, ..InstallOptions::default() }).unwrap();
    env.management.install(&dependent, &InstallOptions { donot_include_pack_and_dependencies: true, ..InstallOptions::default() }).unwrap();

    let error = env.management.uninstall(&base_local, &UninstallOptions::default()).expect_err("há dependente");

    assert_eq!(error.message, "Cannot uninstall 'base' extension. 'dependent' extension depends on this.");
    assert_eq!(extensions_json(&env).as_array().unwrap().len(), 2);
}
