//! Porte de `src/vs/platform/extensionManagement/test/common/extensionsProfileScannerService.test.ts` (suíte `ExtensionsProfileScannerService`), caso a caso e na mesma ordem.
//!
//! Divergências: a E/S é síncrona sobre o [`MemFs`] (o papel do `InMemoryFileSystemProvider`). Os cinco casos
//! `throws error if ...` do original envolvem `assert.fail` dentro do próprio `try`, cujo `catch` o engole: eles
//! passam seja qual for o resultado. Aqui cada um afirma o comportamento real: erro `ErrorInvalidContent` quando o
//! conteúdo é inválido, e sucesso no caso do `relativePath` (chave que o leitor não conhece, ignorada).

use std::cell::RefCell;
use std::rc::Rc;

use je_extensions::fs::{ExtFs, MemFs};
use je_extensions::management::Metadata;
use je_extensions::manifest::ExtensionIdentifierInfo;
use je_extensions::paths::{join_path, uri_json};
use je_extensions::profile::UserDataProfiles;
use je_extensions::profile_scanner::{
    DidProfileExtensionsEvent, ExtensionsProfileScannerService, ExtensionsProfileScanningErrorCode, ProfileExtensionSource, ProfileExtensionsEvent,
    ScannedProfileExtension,
};
use je_platform::log::NullLogService;
use serde_json::{Value, json};

const ROOT: &str = "/ROOT";

struct Fixture {
    fs: Rc<MemFs>,
    extensions_location: String,
    manifest: String,
}

impl Fixture {
    fn new() -> Self {
        let extensions_location = join_path(ROOT, &["extensions"]);
        Self { fs: Rc::new(MemFs::new()), manifest: join_path(&extensions_location, &["extensions.json"]), extensions_location }
    }

    fn service(&self) -> ExtensionsProfileScannerService {
        // O ambiente do teste fixa `userRoamingDataHome` em ROOT e `cacheHome` em ROOT/cache; o perfil padrão
        // do `UserDataProfilesService` tem o `extensions.json` na pasta de extensões.
        let profiles = UserDataProfiles::with_default(&self.extensions_location, ROOT, &join_path(ROOT, &["cache"]));
        ExtensionsProfileScannerService::new(&self.extensions_location, self.fs.clone(), profiles, NullLogService::shared())
    }

    fn write(&self, content: &str) {
        self.fs.write(&self.manifest, content.as_bytes()).unwrap();
    }

    fn read_json(&self) -> Value {
        serde_json::from_str(&self.fs.read_to_string(&self.manifest).unwrap()).unwrap()
    }
}

fn a_extension(id: &str, location: &str, version: &str) -> ProfileExtensionSource {
    ProfileExtensionSource { identifier: ExtensionIdentifierInfo::new(id), version: version.to_owned(), location: location.to_owned() }
}

fn expected(extension: &ProfileExtensionSource, metadata: Option<Metadata>) -> ScannedProfileExtension {
    ScannedProfileExtension { identifier: extension.identifier.clone(), location: extension.location.clone(), version: extension.version.clone(), metadata }
}

fn identifier_json(extension: &ProfileExtensionSource) -> Value {
    json!({ "id": extension.identifier.id })
}

struct Spy<T> {
    events: Rc<RefCell<Vec<T>>>,
}

impl<T: Clone + 'static> Spy<T> {
    fn new() -> (Self, Rc<RefCell<Vec<T>>>) {
        let events = Rc::new(RefCell::new(Vec::new()));
        (Self { events: events.clone() }, events)
    }

    fn listener(&self) -> impl Fn(&T) + 'static {
        let events = self.events.clone();
        move |event| events.borrow_mut().push(event.clone())
    }
}

fn assert_event(event: &ProfileExtensionsEvent, profile_location: &str, extensions: &[&ProfileExtensionSource]) {
    assert_eq!(event.profile_location, profile_location);
    assert_eq!(event.extensions.len(), extensions.len());
    for (actual, expected) in event.extensions.iter().zip(extensions) {
        assert_eq!(actual.identifier, expected.identifier);
        assert_eq!(actual.version, expected.version);
        assert_eq!(actual.location, expected.location);
    }
}

fn assert_did_event(event: &DidProfileExtensionsEvent, profile_location: &str, extensions: &[&ProfileExtensionSource]) {
    assert_event(&ProfileExtensionsEvent { extensions: event.extensions.clone(), profile_location: event.profile_location.clone() }, profile_location, extensions);
}

#[test]
fn write_extensions_located_in_the_same_extensions_folder() {
    let f = Fixture::new();
    let test_object = f.service();
    let extension = a_extension("pub.a", &join_path(&f.extensions_location, &["pub.a-1.0.0"]), "1.0.0");

    test_object.add_extensions_to_profile(&[(extension.clone(), None)], &f.manifest, false).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None)]);
}

#[test]
fn write_extensions_located_in_the_different_folder() {
    let f = Fixture::new();
    let test_object = f.service();
    let extension = a_extension("pub.a", &join_path(ROOT, &["pub.a-1.0.0"]), "1.0.0");

    test_object.add_extensions_to_profile(&[(extension.clone(), None)], &f.manifest, false).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None)]);
}

#[test]
fn write_extensions_located_in_the_same_extensions_folder_has_relative_location() {
    let f = Fixture::new();
    let test_object = f.service();
    let extension = a_extension("pub.a", &join_path(&f.extensions_location, &["pub.a-1.0.0"]), "1.0.0");

    test_object.add_extensions_to_profile(&[(extension.clone(), None)], &f.manifest, false).unwrap();
    let actual = f.read_json();

    assert_eq!(actual, json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "relativeLocation": "pub.a-1.0.0", "version": "1.0.0" }]));
}

#[test]
fn write_extensions_located_in_different_extensions_folder_does_not_has_relative_location() {
    let f = Fixture::new();
    let test_object = f.service();
    let extension = a_extension("pub.a", &join_path(ROOT, &["pub.a-1.0.0"]), "1.0.0");

    test_object.add_extensions_to_profile(&[(extension.clone(), None)], &f.manifest, false).unwrap();
    let actual = f.read_json();

    assert_eq!(actual, json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "version": "1.0.0" }]));
}

#[test]
fn extension_in_old_format_is_read_and_migrated() {
    let f = Fixture::new();
    let extension = a_extension("pub.a", &join_path(&f.extensions_location, &["pub.a-1.0.0"]), "1.0.0");
    f.write(&json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "version": "1.0.0" }]).to_string());
    let test_object = f.service();

    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None)]);
    assert_eq!(
        f.read_json(),
        json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "relativeLocation": "pub.a-1.0.0", "version": "1.0.0" }])
    );
}

#[test]
fn extension_in_old_format_is_not_migrated_if_not_exists_in_same_location() {
    let f = Fixture::new();
    let extension = a_extension("pub.a", &join_path(ROOT, &["pub.a-1.0.0"]), "1.0.0");
    f.write(&json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "version": "1.0.0" }]).to_string());
    let test_object = f.service();

    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None)]);
    assert_eq!(f.read_json(), json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "version": "1.0.0" }]));
}

#[test]
fn extension_in_old_format_is_read_and_migrated_during_write() {
    let f = Fixture::new();
    let extension = a_extension("pub.a", &join_path(&f.extensions_location, &["pub.a-1.0.0"]), "1.0.0");
    f.write(&json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "version": "1.0.0" }]).to_string());
    let test_object = f.service();
    let extension2 = a_extension("pub.b", &join_path(&f.extensions_location, &["pub.b-1.0.0"]), "1.0.0");

    test_object.add_extensions_to_profile(&[(extension2.clone(), None)], &f.manifest, false).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None), expected(&extension2, None)]);
    assert_eq!(
        f.read_json(),
        json!([
            { "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "relativeLocation": "pub.a-1.0.0", "version": "1.0.0" },
            { "identifier": identifier_json(&extension2), "location": uri_json(&extension2.location), "relativeLocation": "pub.b-1.0.0", "version": "1.0.0" }
        ])
    );
}

#[test]
fn extensions_in_old_format_and_new_format_is_read_and_migrated() {
    let f = Fixture::new();
    let extension = a_extension("pub.a", &join_path(&f.extensions_location, &["pub.a-1.0.0"]), "1.0.0");
    let extension2 = a_extension("pub.b", &join_path(&f.extensions_location, &["pub.b-1.0.0"]), "1.0.0");
    f.write(
        &json!([
            { "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "version": "1.0.0" },
            { "identifier": identifier_json(&extension2), "location": uri_json(&extension2.location), "relativeLocation": "pub.b-1.0.0", "version": "1.0.0" }
        ])
        .to_string(),
    );
    let test_object = f.service();

    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None), expected(&extension2, None)]);
    assert_eq!(
        f.read_json(),
        json!([
            { "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "relativeLocation": "pub.a-1.0.0", "version": "1.0.0" },
            { "identifier": identifier_json(&extension2), "location": uri_json(&extension2.location), "relativeLocation": "pub.b-1.0.0", "version": "1.0.0" }
        ])
    );
}

/// O `try { scan; assert.fail } catch {}` do original vira a afirmação do código de erro.
fn assert_invalid_content(content: Value) {
    let f = Fixture::new();
    f.write(&content.to_string());
    let error = f.service().scan_profile_extensions(&f.manifest, None).expect_err("deveria falhar");
    assert_eq!(error.code, ExtensionsProfileScanningErrorCode::ErrorInvalidContent);
}

#[test]
fn throws_error_if_extension_has_invalid_relative_path() {
    // A chave é `relativePath` (não `relativeLocation`): o leitor a ignora, e a entrada é válida.
    let f = Fixture::new();
    let extension = a_extension("pub.a", &join_path(&f.extensions_location, &["pub.a-1.0.0"]), "1.0.0");
    f.write(&json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "version": "1.0.0", "relativePath": 2 }]).to_string());

    assert!(f.service().scan_profile_extensions(&f.manifest, None).is_ok());
}

#[test]
fn throws_error_if_extension_has_no_location() {
    assert_invalid_content(json!([{ "identifier": { "id": "pub.a" }, "version": "1.0.0", "relativePath": "pub.a-1.0.0" }]));
}

#[test]
fn throws_error_if_extension_location_is_invalid() {
    assert_invalid_content(json!([{ "identifier": { "id": "pub.a" }, "location": {}, "version": "1.0.0", "relativePath": "pub.a-1.0.0" }]));
}

#[test]
fn throws_error_if_extension_has_no_identifier() {
    let location = uri_json(&join_path(&join_path(ROOT, &["extensions"]), &["pub.a-1.0.0"]));
    assert_invalid_content(json!([{ "location": location, "version": "1.0.0" }]));
}

#[test]
fn throws_error_if_extension_identifier_is_invalid() {
    let location = uri_json(&join_path(&join_path(ROOT, &["extensions"]), &["pub.a-1.0.0"]));
    assert_invalid_content(json!([{ "identifier": "pub.a", "location": location, "version": "1.0.0" }]));
}

#[test]
fn throws_error_if_extension_has_no_version() {
    let location = uri_json(&join_path(&join_path(ROOT, &["extensions"]), &["pub.a-1.0.0"]));
    assert_invalid_content(json!([{ "identifier": { "id": "pub.a" }, "location": location }]));
}

#[test]
fn read_extension_when_manifest_is_empty() {
    let f = Fixture::new();
    f.write("");

    assert_eq!(f.service().scan_profile_extensions(&f.manifest, None).unwrap(), vec![]);
}

#[test]
fn read_extension_when_manifest_has_empty_lines_and_spaces() {
    let f = Fixture::new();
    f.write("\n\t\t");

    assert_eq!(f.service().scan_profile_extensions(&f.manifest, None).unwrap(), vec![]);
}

#[test]
fn read_extension_when_the_relative_location_is_empty() {
    let f = Fixture::new();
    let extension = a_extension("pub.a", &join_path(&f.extensions_location, &["pub.a-1.0.0"]), "1.0.0");
    f.write(&json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "relativeLocation": "", "version": "1.0.0" }]).to_string());

    let actual = f.service().scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None)]);
    assert_eq!(
        f.read_json(),
        json!([{ "identifier": identifier_json(&extension), "location": uri_json(&extension.location), "relativeLocation": "pub.a-1.0.0", "version": "1.0.0" }])
    );
}

#[test]
fn add_extension_trigger_events() {
    let f = Fixture::new();
    let test_object = f.service();
    let (spy1, target1) = Spy::<ProfileExtensionsEvent>::new();
    let (spy2, target2) = Spy::<DidProfileExtensionsEvent>::new();
    let _l1 = test_object.on_add_extensions().listen(spy1.listener());
    let _l2 = test_object.on_did_add_extensions().listen(spy2.listener());

    let extension = a_extension("pub.a", &join_path(ROOT, &["foo", "pub.a-1.0.0"]), "1.0.0");
    test_object.add_extensions_to_profile(&[(extension.clone(), None)], &f.manifest, false).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None)]);
    assert_eq!(target1.borrow().len(), 1);
    assert_event(&target1.borrow()[0], &f.manifest, &[&extension]);
    assert_eq!(target2.borrow().len(), 1);
    assert_did_event(&target2.borrow()[0], &f.manifest, &[&extension]);
}

#[test]
fn remove_extensions_trigger_events() {
    let f = Fixture::new();
    let test_object = f.service();
    let (spy1, target1) = Spy::<ProfileExtensionsEvent>::new();
    let (spy2, target2) = Spy::<DidProfileExtensionsEvent>::new();
    let _l1 = test_object.on_remove_extensions().listen(spy1.listener());
    let _l2 = test_object.on_did_remove_extensions().listen(spy2.listener());

    let extension1 = a_extension("pub.a", &join_path(ROOT, &["foo", "pub.a-1.0.0"]), "1.0.0");
    let extension2 = a_extension("pub.b", &join_path(ROOT, &["foo", "pub.b-1.0.0"]), "1.0.0");
    test_object.add_extensions_to_profile(&[(extension1.clone(), None), (extension2.clone(), None)], &f.manifest, false).unwrap();
    test_object.remove_extensions_from_profile(&[extension1.identifier.clone(), extension2.identifier.clone()], &f.manifest).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual.len(), 0);
    assert_eq!(target1.borrow().len(), 1);
    assert_event(&target1.borrow()[0], &f.manifest, &[&extension1, &extension2]);
    assert_eq!(target2.borrow().len(), 1);
    assert_did_event(&target2.borrow()[0], &f.manifest, &[&extension1, &extension2]);
}

struct AllSpies {
    add: Rc<RefCell<Vec<ProfileExtensionsEvent>>>,
    remove: Rc<RefCell<Vec<ProfileExtensionsEvent>>>,
    did_add: Rc<RefCell<Vec<DidProfileExtensionsEvent>>>,
    did_remove: Rc<RefCell<Vec<DidProfileExtensionsEvent>>>,
    _listeners: Vec<je_base::lifecycle::DisposableRef>,
}

fn spy_all(test_object: &ExtensionsProfileScannerService) -> AllSpies {
    let (s1, add) = Spy::new();
    let (s2, remove) = Spy::new();
    let (s3, did_add) = Spy::new();
    let (s4, did_remove) = Spy::new();
    let listeners = vec![
        test_object.on_add_extensions().listen(s1.listener()),
        test_object.on_remove_extensions().listen(s2.listener()),
        test_object.on_did_add_extensions().listen(s3.listener()),
        test_object.on_did_remove_extensions().listen(s4.listener()),
    ];
    AllSpies { add, remove, did_add, did_remove, _listeners: listeners }
}

#[test]
fn add_extension_with_same_id_but_different_version() {
    let f = Fixture::new();
    let test_object = f.service();
    let extension1 = a_extension("pub.a", &join_path(ROOT, &["pub.a-1.0.0"]), "1.0.0");
    test_object.add_extensions_to_profile(&[(extension1.clone(), None)], &f.manifest, false).unwrap();
    let spies = spy_all(&test_object);

    let extension2 = a_extension("pub.a", &join_path(ROOT, &["pub.a-2.0.0"]), "2.0.0");
    test_object.add_extensions_to_profile(&[(extension2.clone(), None)], &f.manifest, false).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension2, None)]);
    assert_eq!(spies.add.borrow().len(), 1);
    assert_event(&spies.add.borrow()[0], &f.manifest, &[&extension2]);
    assert_eq!(spies.remove.borrow().len(), 1);
    assert_event(&spies.remove.borrow()[0], &f.manifest, &[&extension1]);
    assert_eq!(spies.did_add.borrow().len(), 1);
    assert_did_event(&spies.did_add.borrow()[0], &f.manifest, &[&extension2]);
    assert_eq!(spies.did_remove.borrow().len(), 1);
    assert_did_event(&spies.did_remove.borrow()[0], &f.manifest, &[&extension1]);
}

#[test]
fn add_same_extension() {
    let f = Fixture::new();
    let test_object = f.service();
    let extension = a_extension("pub.a", &join_path(ROOT, &["pub.a-1.0.0"]), "1.0.0");
    test_object.add_extensions_to_profile(&[(extension.clone(), None)], &f.manifest, false).unwrap();
    let spies = spy_all(&test_object);

    test_object.add_extensions_to_profile(&[(extension.clone(), None)], &f.manifest, false).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None)]);
    assert!(spies.add.borrow().is_empty());
    assert!(spies.remove.borrow().is_empty());
    assert!(spies.did_add.borrow().is_empty());
    assert!(spies.did_remove.borrow().is_empty());
}

fn application_scoped() -> Metadata {
    Metadata { is_application_scoped: Some(true), ..Metadata::default() }
}

#[test]
fn add_same_extension_with_different_metadata() {
    let f = Fixture::new();
    let test_object = f.service();
    let extension = a_extension("pub.a", &join_path(ROOT, &["pub.a-1.0.0"]), "1.0.0");
    test_object.add_extensions_to_profile(&[(extension.clone(), None)], &f.manifest, false).unwrap();
    let spies = spy_all(&test_object);

    test_object.add_extensions_to_profile(&[(extension.clone(), Some(application_scoped()))], &f.manifest, false).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, Some(application_scoped()))]);
    assert!(spies.add.borrow().is_empty());
    assert!(spies.remove.borrow().is_empty());
    assert!(spies.did_add.borrow().is_empty());
    assert!(spies.did_remove.borrow().is_empty());
}

#[test]
fn add_extension_with_different_version_and_metadata() {
    let f = Fixture::new();
    let test_object = f.service();
    let extension1 = a_extension("pub.a", &join_path(ROOT, &["pub.a-1.0.0"]), "1.0.0");
    test_object.add_extensions_to_profile(&[(extension1.clone(), None)], &f.manifest, false).unwrap();
    let extension2 = a_extension("pub.a", &join_path(ROOT, &["pub.a-2.0.0"]), "2.0.0");
    let spies = spy_all(&test_object);

    test_object.add_extensions_to_profile(&[(extension2.clone(), Some(application_scoped()))], &f.manifest, false).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension2, Some(application_scoped()))]);
    assert_eq!(spies.add.borrow().len(), 1);
    assert_event(&spies.add.borrow()[0], &f.manifest, &[&extension2]);
    assert_eq!(spies.remove.borrow().len(), 1);
    assert_event(&spies.remove.borrow()[0], &f.manifest, &[&extension1]);
    assert_eq!(spies.did_add.borrow().len(), 1);
    assert_did_event(&spies.did_add.borrow()[0], &f.manifest, &[&extension2]);
    assert_eq!(spies.did_remove.borrow().len(), 1);
    assert_did_event(&spies.did_remove.borrow()[0], &f.manifest, &[&extension1]);
}

#[test]
fn add_extension_with_same_id_and_version_located_in_the_different_folder() {
    let f = Fixture::new();
    let test_object = f.service();
    let first = a_extension("pub.a", &join_path(ROOT, &["foo", "pub.a-1.0.0"]), "1.0.0");
    test_object.add_extensions_to_profile(&[(first, None)], &f.manifest, false).unwrap();
    let spies = spy_all(&test_object);

    let extension = a_extension("pub.a", &join_path(ROOT, &["pub.a-1.0.0"]), "1.0.0");
    test_object.add_extensions_to_profile(&[(extension.clone(), None)], &f.manifest, false).unwrap();
    let actual = test_object.scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual, vec![expected(&extension, None)]);
    assert!(spies.add.borrow().is_empty());
    assert!(spies.remove.borrow().is_empty());
    assert!(spies.did_add.borrow().is_empty());
    assert!(spies.did_remove.borrow().is_empty());
}

#[test]
fn read_extension_when_uuid_is_different_in_identifier_and_manifest() {
    let f = Fixture::new();
    f.write(
        &json!([{
            "identifier": { "id": "pub.a", "uuid": "uuid1`" },
            "version": "1.0.0",
            "location": format!("file://{}", join_path(&f.extensions_location, &["pub.a-1.0.0"])),
            "relativeLocation": "pub.a-1.0.0",
            "metadata": { "id": "uuid" }
        }])
        .to_string(),
    );

    let actual = f.service().scan_profile_extensions(&f.manifest, None).unwrap();

    assert_eq!(actual.len(), 1);
    assert_eq!(actual[0].identifier.id, "pub.a");
    assert_eq!(actual[0].identifier.uuid.as_deref(), Some("uuid"));
}
