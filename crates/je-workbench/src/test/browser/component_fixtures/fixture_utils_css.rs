//! Porte de `src/vs/workbench/test/browser/componentFixtures/fixtureUtilsCss.ts`, a parte que os testes exercitam: a folha
//! de estilo de um tema (`getThemeStyleSheet`), escopada ao elemento raiz do fixture e a seus descendentes.
//!
//! Notas de tradução:
//! - **Não portado** (nenhum caso dos fixtures o exercita e todo ele opera sobre as folhas `<link>` do `index.html` do
//!   explorador web): `ensureGlobalStylesInstalled`, `overrideStylesheetOrder`, `getStylesheetDocumentFiles`,
//!   `ReverseStylesheetsOption` e a análise dos marcadores `@css-source` do bundle, o cache de folhas de ícones e de tema
//!   de ícones de arquivo e o `WeakMap<ColorThemeData, CSSStyleSheet>` (o cache por identidade de objeto; cada chamada
//!   gera a folha, que o chamador guarda).
//! - O `mockEnvironmentService` (`Object.create(null)` repassado aos participantes) não existe: os participantes
//!   portados não leem o serviço (ver `platform/theme/theme_service.rs`).
//! - `agentsColors.js` e `agentsSizes.js` (registros de cores e tamanhos importados antes de gerar o CSS) já estão no
//!   registro de cores e de tamanhos (`registered_colors.rs`, `base_sizes.rs`).
//! - `.replace(/^(\s*):root(?=\s*\{)/, '$1:scope').replaceAll(scopeRootSelector, ':scope')` é [`scope_rule`].

use std::rc::Rc;

use super::fixture_css::Stylesheet;
use crate::platform::theme::theme_service::{CssRuleSet, ICssStyleCollector, ThemingParticipant, get_theming_participants};
use crate::services::themes::color_theme_css::generate_color_theme_css;
use crate::services::themes::color_theme_data::ColorThemeData;

/// A regra com `:root` no começo (depois do recuo) e `scope_root_selector` em qualquer posição trocados por `:scope`.
pub fn scope_rule(rule: &str, scope_root_selector: &str) -> String {
    let trimmed = rule.trim_start();
    let indent = &rule[..rule.len() - trimmed.len()];
    let root_to_scope = match trimmed.strip_prefix(":root") {
        Some(rest) if rest.trim_start().starts_with('{') => format!("{indent}:scope{rest}"),
        _ => rule.to_owned(),
    };
    root_to_scope.replace(scope_root_selector, ":scope")
}

/// `createScopedThemingParticipant(scopeSelector, scopeRootSelector, participants)`: os participantes escrevem as regras
/// normalmente e o resultado vai dentro de um `@scope`, com a raiz do workbench trocada por `:scope`.
fn create_scoped_theming_participant(scope_selector: String, scope_root_selector: String, participants: Vec<ThemingParticipant>) -> ThemingParticipant {
    Rc::new(move |theme: &ColorThemeData, collector: &mut dyn ICssStyleCollector| {
        let mut rules = CssRuleSet::default();
        for participant in &participants {
            (**participant)(theme, &mut rules);
        }
        let scoped_rules: Vec<String> = rules.rules().iter().map(|rule| scope_rule(rule, &scope_root_selector)).collect();
        collector.add_rule(&format!("@scope ({scope_selector}) {{\n{}\n}}", scoped_rules.join("\n")));
    })
}

/// `getThemeStyleSheet(theme)`: o CSS do tema, com as variáveis em `.<tipo>:where(.<classes>, .<classes> *)` (que acha
/// também os editores aninhados, que só levam a primeira classe, sem aumentar a especificidade) e as regras dos
/// participantes de tema vigentes dentro de `@scope (.<classes>)`.
pub fn get_theme_style_sheet(theme: &ColorThemeData) -> Rc<Stylesheet> {
    let class_names = theme.class_names();
    let theme_scope_selector = format!(".{}", class_names.join("."));
    // Keep matching nested editor theme roots without increasing selector specificity.
    let scope_selector = format!(".{}:where({theme_scope_selector}, {theme_scope_selector} *)", class_names[0]);
    let scoped = create_scoped_theming_participant(theme_scope_selector, ".monaco-workbench".to_owned(), get_theming_participants());
    let css = generate_color_theme_css(theme, &scope_selector, &[scoped]);
    Rc::new(Stylesheet::parse(&css.code, None).unwrap_or_else(|error| panic!("o CSS gerado do tema não é válido: {error}")))
}
