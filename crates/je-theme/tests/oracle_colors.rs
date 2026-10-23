//! Cores resolvidas contra o oráculo (VS Code 1.143.1): para cada tema, todas as variáveis
//! `--vscode-*` de cor que o workbench real escreve na raiz (`generateColorThemeCSS`, que usa
//! `theme.getColor(id, true)` e `Color.toString()`) contra o que o porte resolve para as mesmas cores.
//!
//! As golden em `tests/golden/<tema>.json` vêm do oráculo: `getComputedStyle(.monaco-workbench)`
//! com o tema escolhido em `workbench.colorTheme`, filtrado para valores `#rrggbb` e `rgba(...)`.
//! Os temas em `tests/golden/themes` são cópias sem alteração de `extensions/theme-defaults/themes`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use je_base::json::{self, Value};
use je_theme::registry::{as_css_variable_name, color_registry};
use je_theme::{ColorTheme, ThemeType};

fn golden_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden")
}

fn read_golden(theme: &str) -> BTreeMap<String, String> {
    let path = golden_dir().join(format!("{theme}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut errors = Vec::new();
    let Some(Value::Object(map)) = json::parse_str(&text, &mut errors) else {
        panic!("{}: não é um objeto JSON", path.display())
    };
    assert!(errors.is_empty(), "{}: JSON inválido", path.display());
    map.iter()
        .map(|(key, value)| {
            let Value::String(value) = value else {
                panic!(
                    "{}: valor que não é string em {}",
                    path.display(),
                    key.to_string_lossy()
                )
            };
            (key.to_string_lossy(), value.to_string_lossy())
        })
        .collect()
}

/// Resolve todas as cores registradas e compara com a golden. Devolve (registradas, batem, divergências).
fn compare(golden_name: &str, theme_file: &str) -> (usize, usize, Vec<String>) {
    let golden = read_golden(golden_name);
    let theme_path = golden_dir().join("themes").join(theme_file);
    let theme = ColorTheme::load(&theme_path)
        .unwrap()
        .with_type(ThemeType::Dark);

    let mut matched = 0;
    let mut mismatches = Vec::new();
    let colors = color_registry().get_colors();
    for contribution in colors {
        let var = as_css_variable_name(contribution.id);
        let ours = theme
            .resolve_color(contribution.id, true)
            .map(|c| c.to_string());
        let oracle = golden.get(&var);
        if ours.as_ref() == oracle {
            matched += 1;
        } else {
            mismatches.push(format!("{var}: porte {ours:?}, oráculo {oracle:?}"));
        }
    }
    (colors.len(), matched, mismatches)
}

fn assert_all_registered_match(golden_name: &str, theme_file: &str) {
    let (registered, matched, mismatches) = compare(golden_name, theme_file);
    assert!(
        mismatches.is_empty(),
        "{golden_name}: {matched} de {registered} batem; divergências:\n{}",
        mismatches.join("\n")
    );
    eprintln!("{golden_name}: {matched} de {registered} cores registradas batem com o oráculo");
}

#[test]
fn registered_colors_match_oracle_2026_dark() {
    assert_all_registered_match("2026-dark", "2026-dark.json");
}

#[test]
fn registered_colors_match_oracle_dark_modern() {
    assert_all_registered_match("dark_modern", "dark_modern.json");
}

/// O oráculo escreve variáveis que o porte ainda não registra: elas vêm de outros arquivos do VS Code
/// (terminal, debug, scm, chat, notebook, extensões...). Aqui só se confere que o porte não inventa
/// variável que o oráculo não tem.
#[test]
fn every_resolved_color_is_a_variable_of_the_oracle_or_undefined() {
    for (golden_name, theme_file) in [
        ("2026-dark", "2026-dark.json"),
        ("dark_modern", "dark_modern.json"),
    ] {
        let golden = read_golden(golden_name);
        let theme = ColorTheme::load(&golden_dir().join("themes").join(theme_file)).unwrap();
        for contribution in color_registry().get_colors() {
            let var = as_css_variable_name(contribution.id);
            if theme.resolve_color(contribution.id, true).is_some() {
                assert!(
                    golden.contains_key(&var),
                    "{golden_name}: {var} resolvida pelo porte mas ausente no oráculo"
                );
            }
        }
    }
}
