//! Retrocompatibilidade com o `settings.json` real do usuário. NÃO é porte de teste do VS Code.
//!
//! O teste é `#[ignore]` porque lê um arquivo da máquina de quem desenvolve (somente leitura, nunca
//! escrita), fora do repositório; rode com `cargo test -p je-platform --test settings_compat -- --ignored`.
//! O conteúdo do arquivo é dado pessoal e não é copiado nem impresso: a falha só nomeia as chaves.

mod common;

use std::cell::RefCell;

use je_base::dynamic::Dynamic;
use je_base::json;
use je_platform::configuration::models::ConfigurationModelParser;
use je_platform::configuration::{get_configuration_value, to_values_tree};
use je_platform::js::{for_in_entries, from_json, object_keys};

use common::*;

/// O caminho do `settings.json` real vem do ambiente (`JE_USER_SETTINGS`), nunca do código: o
/// repositório é público e não guarda caminho da máquina de quem desenvolve.
const USER_SETTINGS_ENV: &str = "JE_USER_SETTINGS";

#[test]
#[ignore = "lê o settings.json real do usuário apontado por JE_USER_SETTINGS (somente leitura); rode com --ignored"]
fn real_user_settings_parse_without_errors_and_keep_all_keys() {
    let _leaks = ensure_no_disposables_are_leaked_in_test_suite();
    let Ok(path) = std::env::var(USER_SETTINGS_ENV) else {
        eprintln!("{USER_SETTINGS_ENV} não definido: nada a conferir");
        return;
    };
    let user_settings = path.as_str();
    let Ok(bytes) = std::fs::read(user_settings) else {
        eprintln!("{user_settings} não existe: nada a conferir");
        return;
    };
    let text = String::from_utf8(bytes).expect("o settings.json é UTF-8");

    // O parser do VS Code: o `errors` original nunca recebe os erros de sintaxe (o `visit` não lança), por
    // isso o `syntax_errors` e o `json::parse` independente conferem de verdade que o texto é JSONC válido.
    let mut parser = ConfigurationModelParser::new(user_settings, null_log());
    parser.parse(Some(&text), None);
    assert!(parser.errors().is_empty(), "o parser reportou erro");
    assert!(parser.syntax_errors().is_empty(), "o texto tem erro de sintaxe: {:?}", parser.syntax_errors());

    let mut errors = Vec::new();
    let value = json::parse_str(&text, &mut errors).expect("o settings.json tem um valor");
    assert!(errors.is_empty(), "json::parse reportou erro de sintaxe: {errors:?}");
    let raw = from_json(&value);
    assert!(matches!(raw, Dynamic::Object(_)), "o settings.json não é um objeto");

    // `toValuesTree` mantém todas as chaves, sem conflito, e o valor de cada uma é achado pelo caminho.
    let conflicts: RefCell<Vec<String>> = RefCell::new(Vec::new());
    let tree = to_values_tree(&raw, &|message| conflicts.borrow_mut().push(message.to_string()));
    assert!(conflicts.borrow().is_empty(), "conflitos na árvore de valores: {:?}", conflicts.borrow());
    let mut missing: Vec<String> = Vec::new();
    for (key, expected) in for_in_entries(&raw) {
        let found = get_configuration_value(&tree, &key, None);
        if !deep_strict_equal(&found, &expected) {
            missing.push(key.to_string_lossy());
        }
    }
    assert!(missing.is_empty(), "chaves perdidas ou alteradas pela árvore de valores: {missing:?}");

    // O modelo do parser tem as mesmas chaves, na mesma ordem, e o mesmo conteúdo.
    let model = parser.configuration_model();
    assert_eq!(model.keys(), object_keys(&raw));
    assert_deep_strict_equal(&model.contents(), &tree);
}
