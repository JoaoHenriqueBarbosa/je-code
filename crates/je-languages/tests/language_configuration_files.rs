//! Carrega os `language-configuration.json` de todas as extensões embutidas pelo
//! `LanguageConfigurationFileHandler` e confere o resultado no `ILanguageConfigurationService`: cada arquivo
//! lido vira configuração registrada, as regex compilam no motor ECMAScript (`zyarr`) e as linguagens mais
//! conhecidas têm os comentários e colchetes que o VS Code lhes dá.
//!
//! Depende de `ref/vscode` (fora do git), então roda com
//! `cargo test -p je-languages --test language_configuration_files -- --ignored`.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use je_editor::language_configuration::CharacterPair;
use je_editor::language_configuration_registry::{ILanguageConfigurationService, LanguageConfigurationService};
use je_editor::language_service::{ILanguageService, LanguageService};
use je_languages::extensions::ExtensionsCatalog;
use je_languages::language_configuration_file::LanguageConfigurationFileHandler;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("raiz do workspace")
}

#[test]
#[ignore = "depende de ref/vscode (fora do git)"]
fn builtin_language_configurations_load_and_register() {
    let catalog = ExtensionsCatalog::load(&root().join("ref/vscode/extensions")).expect("catálogo");
    let language_service = LanguageService::new(false);
    language_service.set_dynamic_languages(catalog.languages.clone());
    let service: Rc<dyn ILanguageService> = language_service.clone();
    let configuration_service: Rc<dyn ILanguageConfigurationService> = LanguageConfigurationService::new(service.clone());
    let _handler = LanguageConfigurationFileHandler::new(service.clone(), configuration_service.clone());

    let language_ids = service.get_registered_language_ids();
    let mut with_files = 0;
    for language_id in &language_ids {
        if !service.get_configuration_files(language_id).is_empty() {
            with_files += 1;
        }
        service.request_basic_language_features(language_id);
    }
    println!("{} linguagens, {with_files} com language-configuration.json", language_ids.len());
    assert!(with_files >= 60, "poucas linguagens com configuração: {with_files}");

    let pair = |a: &str, b: &str| -> CharacterPair { (a.to_owned(), b.to_owned()) };
    let typescript = configuration_service.get_language_configuration("typescript");
    let comments = typescript.comments.as_ref().expect("comentários do typescript");
    assert_eq!(comments.line_comment_token.as_deref(), Some("//"));
    assert_eq!(comments.block_comment_start_token.as_deref(), Some("/*"));
    assert_eq!(comments.block_comment_end_token.as_deref(), Some("*/"));
    let brackets = typescript.underlying_config.brackets.as_ref().expect("colchetes do typescript");
    assert!(brackets.contains(&pair("{", "}")) && brackets.contains(&pair("(", ")")) && brackets.contains(&pair("[", "]")));
    assert!(typescript.indentation_rules.is_some());

    let python = configuration_service.get_language_configuration("python");
    let comments = python.comments.as_ref().expect("comentários do python");
    assert_eq!(comments.line_comment_token.as_deref(), Some("#"));

    let html = configuration_service.get_language_configuration("html");
    let html_comments = html.comments.as_ref().expect("comentários do html");
    assert_eq!(html_comments.block_comment_start_token.as_deref(), Some("<!--"));
    assert_eq!(html_comments.block_comment_end_token.as_deref(), Some("-->"));

    // `getWordDefinition` devolve a regex da linguagem quando há `wordPattern` (css aceita `-` na palavra).
    let css = configuration_service.get_language_configuration("css");
    let word = css.get_word_definition();
    let text: Vec<u16> = "margin-left".encode_utf16().collect();
    word.set_last_index(0);
    let found = word.exec(&text).expect("palavra");
    assert_eq!(found.end() - found.start(), text.len(), "o wordPattern do css deve casar `margin-left` inteiro");
}
