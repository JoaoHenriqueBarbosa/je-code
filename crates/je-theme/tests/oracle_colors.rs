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

/// Variáveis que o porte registra e o oráculo não escreve, com o motivo. Nunca saem da golden: a lista só
/// cresce com motivo, e o teste exige que ela seja exatamente o conjunto de divergências.
///
/// As quatro cores abaixo são registradas só por módulos de `src/vs/sessions` (a janela de Agent Sessions:
/// `mobileDiffColors.ts` e `agentFeedbackOverviewRulerContribution.ts`), que o workbench padrão do oráculo
/// nunca carrega. O porte as registra assim mesmo (decisão do user: o registro é barato e temas de
/// terceiros as definem), então o oráculo do workbench padrão não tem a variável.
const REGISTERED_ONLY_BY_SESSIONS_WINDOW: &[&str] = &[
    "--vscode-agentsMobileDiff-addedForeground",
    "--vscode-agentsMobileDiff-deletedForeground",
    "--vscode-agentsMobileDiff-modifiedForeground",
    "--vscode-editorOverviewRuler-agentFeedbackForeground",
];

fn is_known_divergence(var: &str) -> bool {
    REGISTERED_ONLY_BY_SESSIONS_WINDOW.contains(&var)
}

/// Exige igualdade em TODAS as variáveis de cor da golden e em todas as cores registradas, exceto as
/// divergências conhecidas e justificadas acima (que precisam divergir de fato).
fn assert_all_match_oracle(golden_name: &str, theme_file: &str) {
    let golden = read_golden(golden_name);
    let (registered, matched, mismatches) = compare(golden_name, theme_file);

    let registered_vars: std::collections::BTreeSet<String> = color_registry()
        .get_colors()
        .iter()
        .map(|c| as_css_variable_name(c.id))
        .collect();
    let missing: Vec<&String> = golden
        .keys()
        .filter(|var| !registered_vars.contains(*var))
        .collect();
    assert!(
        missing.is_empty(),
        "{golden_name}: variáveis da golden que o porte não registra: {missing:?}"
    );

    let unexpected: Vec<&String> = mismatches
        .iter()
        .filter(|m| !is_known_divergence(m.split(':').next().unwrap_or("")))
        .collect();
    assert!(
        unexpected.is_empty(),
        "{golden_name}: {matched} de {registered} batem; divergências:\n{}",
        unexpected
            .iter()
            .map(|m| m.as_str())
            .collect::<Vec<_>>()
            .join("\n")
    );
    let expected = REGISTERED_ONLY_BY_SESSIONS_WINDOW.len();
    assert_eq!(
        mismatches.len(),
        expected,
        "{golden_name}: a lista de divergências conhecidas ficou desatualizada"
    );
    // Toda variável da golden está registrada (conferido acima) e `compare` percorre todas as registradas,
    // então cada variável da golden foi comparada e bateu.
    eprintln!(
        "{golden_name}: {} de {} variáveis da golden batem; {matched} de {registered} cores registradas batem ({expected} divergências conhecidas)",
        golden.len(),
        golden.len()
    );
}

#[test]
fn registered_colors_match_oracle_2026_dark() {
    assert_all_match_oracle("2026-dark", "2026-dark.json");
}

#[test]
fn registered_colors_match_oracle_dark_modern() {
    assert_all_match_oracle("dark_modern", "dark_modern.json");
}

/// O porte não inventa variável que o oráculo não tem, salvo as divergências conhecidas.
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
            if theme.resolve_color(contribution.id, true).is_some() && !is_known_divergence(&var) {
                assert!(
                    golden.contains_key(&var),
                    "{golden_name}: {var} resolvida pelo porte mas ausente no oráculo"
                );
            }
        }
    }
}
