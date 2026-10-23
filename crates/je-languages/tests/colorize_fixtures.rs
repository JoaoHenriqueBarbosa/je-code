//! Ponta a ponta da H14: abre as fixtures de `extensions/vscode-colorize-tests` como `TextModel`, com a
//! linguagem escolhida pelo `LanguagesRegistry`, tokeniza pelo `TokenizationRegistry` (gramática TextMate
//! do `ferriki-textmate`) e confere, caractere a caractere, a cor resolvida em cada um dos 10 temas
//! padrão contra as goldens `colorize-results` (geradas pelo `_workbench.captureSyntaxTokens` do VS Code).
//!
//! Depende de `ref/vscode` (fora do git), então roda com `cargo test -p je-languages --test colorize_fixtures -- --ignored`.
//!
//! O que a golden dá e o que se confere: a golden guarda, por token agrupado por escopos, o texto (`c`), os
//! escopos (`t`) e, por tema, o seletor e a cor (`r`). O modelo guarda tokens codificados (cor, tipo,
//! linguagem), sem escopos. Por isso a conferência é por unidade UTF-16: o texto concatenado das goldens
//! tem de ser o das linhas do modelo, e a cor do modelo em cada posição tem de ser a da golden. Os escopos
//! (`t`) e os seletores (`r`) são conferidos pela sonda `testbench/textmate-probe`, que usa a API de escopos.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use je_base::lifecycle::IDisposable;
use je_base::runtime;
use je_base::uri::URI;
use je_editor::encoded_token_attributes::TokenMetadata;
use je_editor::language_service::{ILanguageService, LanguageService};
use je_editor::line_tokens::IViewLineTokens;
use je_editor::language_configuration_registry::{ILanguageConfigurationService, LanguageConfigurationService};
use je_editor::text_model::{TextModel, TextModelServices, TextSource};
use je_editor::tokenization_registry::tokenization_registry;
use je_editor::undo_redo_service::UndoRedoService;
use je_languages::extensions::ExtensionsCatalog;
use je_languages::jsonc;
use je_languages::text_mate_tokenization_feature::TextMateTokenizationFeature;
use je_languages::token_theme::TokenTheme;
use serde_json::Value;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("raiz do workspace")
}

struct GoldenToken {
    text: String,
    /// Tema para a cor (maiúsculas), só nos tokens que têm cor.
    colors: HashMap<String, String>,
}

fn load_golden(path: &Path) -> Vec<GoldenToken> {
    let value: Value = serde_json::from_str(&fs::read_to_string(path).expect("golden")).expect("json da golden");
    value
        .as_array()
        .expect("lista de tokens")
        .iter()
        .map(|t| GoldenToken {
            text: t["c"].as_str().unwrap_or_default().to_owned(),
            colors: t["r"]
                .as_object()
                .map(|m| {
                    m.iter()
                        .filter_map(|(theme, v)| {
                            let explanation = v.as_str()?;
                            Some((theme.clone(), explanation.rsplit(": ").next()?.to_uppercase()))
                        })
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

fn load_themes(extensions: &Path) -> Vec<TokenTheme> {
    let dir = extensions.join("theme-defaults");
    let manifest = jsonc::parse(&fs::read_to_string(dir.join("package.json")).expect("package.json")).expect("json");
    manifest["contributes"]["themes"]
        .as_array()
        .expect("temas")
        .iter()
        .map(|entry| TokenTheme::load(&dir, entry).unwrap_or_else(|e| panic!("tema: {e}")))
        .collect()
}

/// A cor de cada unidade UTF-16 do modelo, no tema atual.
fn model_colors(model: &TextModel, color_map: &[String]) -> Vec<Option<String>> {
    let mut colors = Vec::new();
    for line_number in 1..=model.get_line_count() {
        let line_tokens = model.tokenization().get_line_tokens(line_number);
        for index in 0..line_tokens.get_count() {
            let start = line_tokens.get_start_offset(index);
            let end = line_tokens.get_end_offset(index);
            let foreground = TokenMetadata::get_foreground(line_tokens.get_metadata(index)) as usize;
            let color = color_map.get(foreground).filter(|c| foreground != 0 && !c.is_empty()).map(|c| c.to_uppercase());
            for _ in start..end {
                colors.push(color.clone());
            }
        }
    }
    colors
}

fn golden_colors(golden: &[GoldenToken], theme: &str) -> Vec<Option<String>> {
    let mut colors = Vec::new();
    for token in golden {
        let color = token.colors.get(theme).cloned();
        for _ in token.text.encode_utf16() {
            colors.push(color.clone());
        }
    }
    colors
}

#[test]
#[ignore = "depende de ref/vscode (fora do git)"]
fn colorize_fixtures_match_goldens_in_every_theme() {
    let extensions = root().join("ref/vscode/extensions");
    let tests = extensions.join("vscode-colorize-tests/test");

    let catalog = ExtensionsCatalog::load(&extensions).expect("catálogo");
    let language_service = LanguageService::new(false);
    language_service.set_dynamic_languages(catalog.languages.clone());
    let service: Rc<dyn ILanguageService> = language_service.clone();
    let configuration_service: Rc<dyn ILanguageConfigurationService> = LanguageConfigurationService::new(service.clone());
    let feature = TextMateTokenizationFeature::new(service.clone(), &catalog.grammars, catalog.max_tokenization_line_length.clone());
    let mut themes = load_themes(&extensions);

    let mut fixtures: Vec<PathBuf> =
        fs::read_dir(tests.join("colorize-fixtures")).expect("fixtures").filter_map(|e| e.ok().map(|e| e.path())).collect();
    fixtures.sort();

    let (mut passed, mut failures) = (0, Vec::new());
    let (mut tokens_total, mut chars_total) = (0usize, 0usize);
    let started = Instant::now();
    for fixture in &fixtures {
        let name = fixture.file_name().expect("nome").to_string_lossy().to_string();
        let golden = load_golden(&tests.join("colorize-results").join(format!("{}.json", name.replacen('.', "_", 1))));
        let text = String::from_utf8_lossy(&fs::read(fixture).expect("fixture")).into_owned();

        let uri = URI::file(format!("/{name}"), je_base::path::Platform::Posix);
        let language_id = service
            .guess_language_id_by_filepath_or_first_line(Some(&uri), None)
            .unwrap_or_else(|| "plaintext".to_owned());
        // Resolve a fábrica de tokenização da linguagem (assíncrona no original) antes de abrir o modelo.
        let _ = tokenization_registry().get_or_create(&language_id);
        runtime::run_microtasks();

        let model = TextModel::new_with_language_service(
            TextSource::from(text.as_str()),
            &language_id,
            TextModel::default_creation_options(),
            Some(uri),
            UndoRedoService::for_tests(),
            Some(TextModelServices {
                language_service: service.clone(),
                language_configuration_service: configuration_service.clone(),
            }),
        );

        let golden_text: String = golden.iter().map(|t| t.text.as_str()).collect();
        let model_text: String = (1..=model.get_line_count()).map(|n| model.get_line_content(n).to_string_lossy()).collect();
        let mut problem = if golden_text != model_text {
            Some(format!("texto diferente: golden {} unidades, modelo {}", golden_text.encode_utf16().count(), model_text.encode_utf16().count()))
        } else {
            None
        };
        if problem.is_none() {
            for theme in &mut themes {
                feature.set_theme(theme);
                let color_map = tokenization_registry().get_color_map().unwrap_or_default();
                model.tokenization().force_tokenization(model.get_line_count());
                let (got, expected) = (model_colors(&model, &color_map), golden_colors(&golden, &theme.id));
                chars_total += expected.len();
                if got != expected {
                    let at = got.iter().zip(&expected).position(|(g, e)| g != e).unwrap_or(got.len().min(expected.len()));
                    problem = Some(format!(
                        "tema {}: cor diferente na unidade {at} ({:?}): modelo {:?}, golden {:?}",
                        theme.id,
                        golden_text.encode_utf16().skip(at.saturating_sub(10)).take(30).collect::<Vec<_>>(),
                        got.get(at),
                        expected.get(at)
                    ));
                    break;
                }
            }
        }
        tokens_total += golden.len();
        match problem {
            None => passed += 1,
            Some(p) => failures.push(format!("{name} ({language_id}): {p}")),
        }
        model.dispose();
    }
    println!(
        "{passed} de {} fixtures iguais em {} temas ({tokens_total} tokens de golden, {chars_total} unidades conferidas) em {:.1}s",
        fixtures.len(),
        themes.len(),
        started.elapsed().as_secs_f64()
    );
    for f in &failures {
        println!("FALHA {f}");
    }
    feature.dispose();
    language_service.dispose();
    assert!(failures.is_empty(), "{} fixtures divergem", failures.len());
    assert_eq!(passed, 109);
}
