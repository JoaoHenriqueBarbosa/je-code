//! Porte de `src/vs/platform/theme/common/themeService.ts`, a parte dos participantes de tema: `IThemingParticipant`,
//! `ICssStyleCollector`, `registerThemingParticipant` e `IThemingRegistry.getThemingParticipants`.
//!
//! Notas de tradução:
//! - O participante recebe o `ColorThemeData` do workbench no lugar da interface `IColorTheme` (a plataforma de temas
//!   ainda não tem a interface; o único tema que os participantes recebem é o dos fixtures e do workbench).
//! - O terceiro argumento do participante (`IEnvironmentService`) não existe: nenhum participante portado o lê.
//! - O registro é por thread, como o `tokenization_registry()`: cada teste roda na sua thread e começa sem participantes;
//!   o `import` de um módulo que registra um participante vira a chamada explícita da função de registro dele.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::{DisposableRef, to_disposable};

use crate::services::themes::color_theme_data::ColorThemeData;

/// `ICssStyleCollector`: onde o participante escreve as regras CSS que o tema precisa.
pub trait ICssStyleCollector {
    fn add_rule(&mut self, rule: &str);
}

/// O conjunto de regras na ordem de inserção, sem repetição (o `Set<string>` de `generateColorThemeCSS`).
#[derive(Default)]
pub struct CssRuleSet {
    rules: Vec<String>,
}

impl CssRuleSet {
    pub fn rules(&self) -> &[String] {
        &self.rules
    }
}

impl ICssStyleCollector for CssRuleSet {
    fn add_rule(&mut self, rule: &str) {
        if !self.rules.iter().any(|existing| existing == rule) {
            self.rules.push(rule.to_owned());
        }
    }
}

/// `IThemingParticipant`.
pub type ThemingParticipant = Rc<dyn Fn(&ColorThemeData, &mut dyn ICssStyleCollector)>;

struct ThemingRegistry {
    participants: Vec<(u64, ThemingParticipant)>,
    next_id: u64,
}

thread_local! {
    static THEMING_REGISTRY: RefCell<ThemingRegistry> = const { RefCell::new(ThemingRegistry { participants: Vec::new(), next_id: 0 }) };
}

/// `registerThemingParticipant(participant)`: descartar o resultado tira o participante do registro.
pub fn register_theming_participant(participant: impl Fn(&ColorThemeData, &mut dyn ICssStyleCollector) + 'static) -> DisposableRef {
    let participant: ThemingParticipant = Rc::new(participant);
    let id = THEMING_REGISTRY.with(|registry| {
        let mut registry = registry.borrow_mut();
        let id = registry.next_id;
        registry.next_id += 1;
        registry.participants.push((id, participant));
        id
    });
    to_disposable(move || {
        THEMING_REGISTRY.with(|registry| registry.borrow_mut().participants.retain(|(existing, _)| *existing != id));
    })
}

/// `getThemingParticipants()`: na ordem de registro.
pub fn get_theming_participants() -> Vec<ThemingParticipant> {
    THEMING_REGISTRY.with(|registry| registry.borrow().participants.iter().map(|(_, participant)| participant.clone()).collect())
}
