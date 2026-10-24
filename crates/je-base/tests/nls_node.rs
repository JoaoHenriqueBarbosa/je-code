//! Porte de `src/vs/base/test/node/nls.test.ts` (suíte `NLS configuration`), caso a caso e na mesma ordem.
//!
//! Notas de tradução: o `getRandomTestPath(tmpdir(), 'vsctests', 'nls')` é um diretório aleatório no `tmpdir` e o
//! `Promises.rm(testDir)` do `teardown` é o `Drop` do `Fixture` (que também roda se o teste falhar). O `delete
//! process.env['VSCODE_DEV']` do `setup` é um [`Environment`] sem essa variável, pois mexer no ambiente do processo exige
//! `unsafe`; o `process.env` restante é o do processo real. As funções do original são bloqueantes aqui (ver
//! `je_base::node::nls`), então o `await` some.

mod common;

use std::fs;
use std::path::PathBuf;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use indexmap::IndexMap;
use je_base::node::nls::{
    LanguagePack, LanguagePackExtension, LanguagePacks, NlsConfiguration, ResolveNlsConfigurationContext, resolve_nls_configuration,
};
use je_base::path::{Environment, PathModule, Platform, ProcessEnvironment};
use je_base::uuid::generate_uuid;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const COMMIT: &str = "test-commit";
const LANGUAGE_PACK_ID: &str = "test-language-pack.de";

/// O ambiente do processo sem o `VSCODE_DEV`.
struct NoDevEnvironment(ProcessEnvironment);

impl Environment for NoDevEnvironment {
    fn platform(&self) -> Platform {
        self.0.platform()
    }

    fn cwd(&self) -> String {
        self.0.cwd()
    }

    fn env_var(&self, name: &str) -> Option<String> {
        if name == "VSCODE_DEV" { None } else { self.0.env_var(name) }
    }
}

const ENV: NoDevEnvironment = NoDevEnvironment(ProcessEnvironment);

fn join(segments: &[&str]) -> String {
    PathModule::native(&ENV).join(segments)
}

/// `Pick<IResolveNLSConfigurationContext, 'nlsMetadataPath' | 'nlsMetadataHash'>`.
struct NlsMetadata {
    nls_metadata_path: String,
    nls_metadata_hash: Option<String>,
}

/// O `setup` e o `teardown` da suíte.
struct Fixture {
    test_dir: String,
    user_data_path: String,
}

impl Fixture {
    fn new() -> Self {
        let test_dir = join(&[&std::env::temp_dir().to_string_lossy(), "vsctests", "nls", &generate_uuid()]);
        let user_data_path = join(&[&test_dir, "user"]);
        fs::create_dir_all(&user_data_path).unwrap();

        let translations_file = join(&[&test_dir, "main.i18n.json"]);
        let language_packs: LanguagePacks = IndexMap::from([(
            "de".to_owned(),
            Some(LanguagePack {
                hash: "test-language-pack".to_owned(),
                label: Some("Deutsch".to_owned()),
                extensions: Vec::<LanguagePackExtension>::new(),
                translations: IndexMap::from([("vscode".to_owned(), Some(translations_file.clone()))]),
            }),
        )]);
        fs::write(join(&[&user_data_path, "languagepacks.json"]), serde_json::to_string(&language_packs).unwrap()).unwrap();
        fs::write(
            &translations_file,
            json!({
                "contents": {
                    "vs/base/test": { "first": "Erste", "second": "Zweite" },
                    "vs/workbench/api/common/extHostLogService": { "remote": "Entfernt" }
                }
            })
            .to_string(),
        )
        .unwrap();

        Self { test_dir, user_data_path }
    }

    fn write_metadata(&self, name: &str, keys: &[(&str, &[&str])], messages: &[&str]) -> NlsMetadata {
        let nls_metadata_path = join(&[&self.test_dir, name]);
        fs::create_dir_all(&nls_metadata_path).unwrap();
        let keys: Value = keys.iter().map(|(module, names)| json!([module, names])).collect();
        let messages = json!(messages);
        fs::write(join(&[&nls_metadata_path, "nls.keys.json"]), keys.to_string()).unwrap();
        fs::write(join(&[&nls_metadata_path, "nls.messages.json"]), messages.to_string()).unwrap();
        let identity = json!({ "commit": COMMIT, "keys": keys, "messages": messages }).to_string();
        let digest: String = Sha256::digest(identity.as_bytes()).iter().map(|b| format!("{b:02x}")).collect();
        NlsMetadata { nls_metadata_path, nls_metadata_hash: Some(digest) }
    }

    fn resolve_messages(&self, metadata: &NlsMetadata) -> ResolvedMessages {
        let configuration = resolve_nls_configuration(
            &ResolveNlsConfigurationContext {
                user_locale: "de".to_owned(),
                os_locale: "de".to_owned(),
                user_data_path: self.user_data_path.clone(),
                commit: Some(COMMIT.to_owned()),
                nls_metadata_path: metadata.nls_metadata_path.clone(),
                nls_metadata_hash: metadata.nls_metadata_hash.clone(),
            },
            &ENV,
        );
        let language_pack = configuration.language_pack.expect("languagePack");
        let messages: Vec<String> = serde_json::from_str(&fs::read_to_string(&language_pack.messages_file).unwrap()).unwrap();
        ResolvedMessages {
            messages_file: language_pack.messages_file,
            corrupt_marker_file: language_pack.corrupt_marker_file,
            translations_config_file: language_pack.translations_config_file,
            messages,
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(PathBuf::from(&self.test_dir));
    }
}

/// `{ ...configuration.languagePack, messages }`.
struct ResolvedMessages {
    messages_file: String,
    corrupt_marker_file: String,
    translations_config_file: String,
    messages: Vec<String>,
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|v| (*v).to_owned()).collect()
}

#[test]
fn switches_between_server_and_server_web_tables_at_the_same_commit() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new();
    let server_metadata = fixture.write_metadata("server", &[("vs/workbench/api/common/extHostLogService", &["remote"])], &["Remote"]);
    let server_web_metadata = fixture.write_metadata(
        "server-web",
        &[("vs/base/test", &["first", "missing"]), ("vs/workbench/api/common/extHostLogService", &["remote"])],
        &["First", "Fallback", "Remote"],
    );

    let server = fixture.resolve_messages(&server_metadata);
    let server_web = fixture.resolve_messages(&server_web_metadata);
    let server_again = fixture.resolve_messages(&server_metadata);

    assert_eq!(
        (
            vec![server.messages.clone(), server_web.messages.clone(), server_again.messages.clone()],
            server.messages_file != server_web.messages_file,
            server.messages_file == server_again.messages_file
        ),
        (vec![strings(&["Entfernt"]), strings(&["Erste", "Fallback", "Entfernt"]), strings(&["Entfernt"])], true, true)
    );
}

#[test]
fn distinguishes_key_order_when_the_default_messages_are_identical() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new();
    let first_metadata = fixture.write_metadata("first", &[("vs/base/test", &["first", "second"])], &["Same", "Same"]);
    let second_metadata = fixture.write_metadata("second", &[("vs/base/test", &["second", "first"])], &["Same", "Same"]);

    let first = fixture.resolve_messages(&first_metadata);
    let second = fixture.resolve_messages(&second_metadata);

    assert_eq!((first.messages, second.messages), (strings(&["Erste", "Zweite"]), strings(&["Zweite", "Erste"])));
}

#[test]
fn refreshes_fallback_messages_after_rebuilding_at_the_same_commit() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new();
    let original_metadata = fixture.write_metadata("server", &[("vs/base/test", &["missing"])], &["Original"]);
    let original = fixture.resolve_messages(&original_metadata);
    let updated_metadata = fixture.write_metadata("server", &[("vs/base/test", &["missing"])], &["Updated"]);
    let updated = fixture.resolve_messages(&updated_metadata);

    assert_eq!((original.messages, updated.messages), (strings(&["Original"]), strings(&["Updated"])));
}

#[test]
fn reuses_the_cache_for_identical_tables_in_different_locations() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new();
    let first_metadata = fixture.write_metadata("first", &[("vs/base/test", &["first"])], &["First"]);
    let second_metadata = fixture.write_metadata("second", &[("vs/base/test", &["first"])], &["First"]);
    let first = fixture.resolve_messages(&first_metadata);
    fs::write(&first.messages_file, json!(["Cached translation"]).to_string()).unwrap();
    let second = fixture.resolve_messages(&second_metadata);

    assert_eq!((first.messages_file == second.messages_file, second.messages), (true, strings(&["Cached translation"])));
}

#[test]
fn a_cache_hit_does_not_read_the_nls_tables() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new();
    let metadata = fixture.write_metadata("server", &[("vs/base/test", &["first"])], &["First"]);
    let first = fixture.resolve_messages(&metadata);
    fs::remove_file(join(&[&metadata.nls_metadata_path, "nls.keys.json"])).unwrap();
    fs::remove_file(join(&[&metadata.nls_metadata_path, "nls.messages.json"])).unwrap();
    let cached = fixture.resolve_messages(&metadata);

    assert_eq!((first.messages_file == cached.messages_file, cached.messages), (true, strings(&["Erste"])));
}

#[test]
fn does_not_reuse_or_overwrite_a_legacy_commit_only_cache() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new();
    let legacy_cache_path = join(&[&fixture.user_data_path, "clp", LANGUAGE_PACK_ID, COMMIT]);
    fs::create_dir_all(&legacy_cache_path).unwrap();
    let legacy_messages_file = join(&[&legacy_cache_path, "nls.messages.json"]);
    fs::write(&legacy_messages_file, json!(["Legacy translation"]).to_string()).unwrap();
    let metadata = fixture.write_metadata("server", &[("vs/base/test", &["first"])], &["First"]);
    let result = fixture.resolve_messages(&metadata);

    let legacy: Vec<String> = serde_json::from_str(&fs::read_to_string(&legacy_messages_file).unwrap()).unwrap();
    assert_eq!(
        (result.messages, result.messages_file != legacy_messages_file, legacy),
        (strings(&["Erste"]), true, strings(&["Legacy translation"]))
    );
}

#[test]
fn preserves_the_legacy_cache_path_for_products_without_an_identity() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new();
    let metadata = fixture.write_metadata("server", &[("vs/base/test", &["first"])], &["First"]);
    let result = fixture.resolve_messages(&NlsMetadata { nls_metadata_path: metadata.nls_metadata_path, nls_metadata_hash: None });

    assert_eq!(
        (result.messages, result.messages_file),
        (strings(&["Erste"]), join(&[&fixture.user_data_path, "clp", LANGUAGE_PACK_ID, COMMIT, "nls.messages.json"]))
    );
}

#[test]
fn regenerates_all_table_caches_after_a_corruption_marker() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new();
    let first_metadata = fixture.write_metadata("first", &[("vs/base/test", &["first"])], &["First"]);
    let second_metadata = fixture.write_metadata("second", &[("vs/base/test", &["second"])], &["Second"]);
    let first = fixture.resolve_messages(&first_metadata);
    let second = fixture.resolve_messages(&second_metadata);
    fs::write(&first.corrupt_marker_file, "corrupt").unwrap();
    let regenerated = fixture.resolve_messages(&first_metadata);

    assert_eq!(
        (
            regenerated.messages,
            std::path::Path::new(&second.messages_file).exists(),
            std::path::Path::new(&regenerated.translations_config_file).exists()
        ),
        (strings(&["Erste"]), false, true)
    );
}

#[test]
fn does_not_require_nls_metadata_for_the_default_locale() {
    let _store = ensure_no_disposables_are_leaked_in_test_suite();
    let fixture = Fixture::new();
    let nls_metadata_path = join(&[&fixture.test_dir, "missing"]);
    let result = resolve_nls_configuration(
        &ResolveNlsConfigurationContext {
            user_locale: "en".to_owned(),
            os_locale: "en".to_owned(),
            user_data_path: fixture.user_data_path.clone(),
            commit: Some(COMMIT.to_owned()),
            nls_metadata_path: nls_metadata_path.clone(),
            nls_metadata_hash: None,
        },
        &ENV,
    );

    assert_eq!(
        result,
        NlsConfiguration {
            user_locale: "en".to_owned(),
            os_locale: "en".to_owned(),
            resolved_language: "en".to_owned(),
            language_pack: None,
            default_messages_file: join(&[&nls_metadata_path, "nls.messages.json"]),
            locale: "en".to_owned(),
            available_languages: IndexMap::new(),
            language_pack_support: None,
            language_pack_id: None,
            translations_config_file: None,
            cache_root: None,
            resolved_language_pack_core_location: None,
            corrupted_file: None,
        }
    );
}
