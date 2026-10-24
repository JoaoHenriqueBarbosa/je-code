//! Porte de `src/vs/workbench/contrib/onboarding/browser/spotlight/spotlightTypes.ts`: os passos de um tour de
//! spotlight (dados puros) e o payload do cenário.
//!
//! Divergências:
//! - `description: string | IMarkdownString` é [`SpotlightDescription`].
//! - `advanceOnTargetClick: boolean | 'advanceOnly'` é [`SpotlightTargetClickBehavior`], e
//!   `openTarget: boolean | 'ifUnselected'` é [`SpotlightOpenTarget`].
//! - Os membros que são funções (`primaryAction.run`, `onBeforeShow`, `onDidShow`, `resolveSteps`) são `Rc<dyn Fn>`.
//!   O `===` do original entre dois passos compara a identidade da função, que o `PartialEq` de [`SpotlightStep`]
//!   reproduz com `Rc::ptr_eq`; as expressões de contexto comparam por `equals`, e o markdown por todos os campos.
//! - `timeoutMs: number` é `f64`.
//! - Os retornos `Promise<void> | void` são [`MaybePromise`] de `()`.
//! - `Default` em [`SpotlightStep`] faz o papel do objeto literal parcial do original (`{ id, targetId, ... }`).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::html_content::IMarkdownString;
use je_base::promise::{MaybePromise, Promise};
use je_platform::contextkey::contextkey::ContextKeyExpression;

/// A `kind` de apresentação que a apresentação de spotlight trata.
pub const SPOTLIGHT_PRESENTATION_KIND: &str = "spotlight";

/// `SpotlightPlacement`: posição preferida do balão em relação ao alvo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightPlacement {
    Above,
    Below,
    Left,
    Right,
    Auto,
}

/// `SpotlightTargetClickBehavior`: `advanceOnly` consome a ativação do alvo em vez de rodar a ação dele.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightTargetClickBehavior {
    Bool(bool),
    AdvanceOnly,
}

/// O `'skip' | 'abort'` de `onTimeout`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightTimeoutAction {
    Skip,
    Abort,
}

/// `SpotlightMissingTargetBehavior`: o que fazer quando o alvo não está renderizado ao chegar no passo. Esperar
/// pula o passo no tempo limite, a menos que `on_timeout` seja `Abort`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SpotlightMissingTargetBehavior {
    Skip,
    Abort,
    Wait { timeout_ms: f64, on_timeout: Option<SpotlightTimeoutAction> },
}

/// `boolean | 'ifUnselected'`: abre o alvo pelo dono dele; `IfUnselected` só abre controles sem valor selecionado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpotlightOpenTarget {
    Bool(bool),
    IfUnselected,
}

/// `string | IMarkdownString`: o corpo do balão.
#[derive(Debug, Clone)]
pub enum SpotlightDescription {
    Text(String),
    Markdown(IMarkdownString),
}

impl Default for SpotlightDescription {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl PartialEq for SpotlightDescription {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Text(a), Self::Text(b)) => a == b,
            // `IMarkdownString` não define igualdade: o `Debug` derivado cobre todos os campos.
            (Self::Markdown(a), Self::Markdown(b)) => format!("{a:?}") == format!("{b:?}"),
            _ => false,
        }
    }
}

impl From<&str> for SpotlightDescription {
    fn from(text: &str) -> Self {
        Self::Text(text.to_string())
    }
}

/// A `run` da ação primária: recebe o token cancelado quando o passo termina ou o alvo é trocado.
pub type SpotlightPrimaryActionRun = Rc<dyn Fn(&CancellationToken) -> MaybePromise<()>>;

/// `primaryAction`: ação explícita oferecida no lugar de Próximo ou Concluir. Só roda no clique do botão.
#[derive(Clone)]
pub struct SpotlightPrimaryAction {
    pub label: String,
    pub run: SpotlightPrimaryActionRun,
}

/// `ISpotlightStep`: um passo de um tour de spotlight. Passos são dados puros; a apresentação os transforma no
/// overlay escurecido, no recorte e no balão ancorado.
#[derive(Clone, Default)]
pub struct SpotlightStep {
    /// Identificador estável (único dentro do tour).
    pub id: String,

    /// O `data-onboarding-id` do elemento a destacar, resolvido sob demanda.
    pub target_id: String,

    /// Título do balão (localizado).
    pub title: String,

    /// Corpo do balão (texto ou markdown localizado).
    pub description: SpotlightDescription,

    /// Rótulo localizado do botão primário, no lugar do Próximo ou Concluir padrão.
    pub next_button_label: Option<String>,

    /// Ação explícita oferecida no lugar de Próximo ou Concluir.
    pub primary_action: Option<SpotlightPrimaryAction>,

    /// Posição preferida do balão. Padrão `auto`.
    pub placement: Option<SpotlightPlacement>,

    /// Quando presente e insatisfeita, o passo é pulado.
    pub when: Option<ContextKeyExpression>,

    /// Padrão: esperar e pular; `abort` encerra a execução na hora se o alvo faltar.
    pub missing_target: Option<SpotlightMissingTargetBehavior>,

    /// Abre o alvo pelo dono dele.
    pub open_target: Option<SpotlightOpenTarget>,

    /// Mantém o elemento destacado interativo. Padrão `false`.
    pub allow_target_interaction: Option<bool>,

    /// Avança na ativação do alvo.
    pub advance_on_target_click: Option<SpotlightTargetClickBehavior>,

    /// Avança quando o alvo reporta uma seleção, sem esconder o Próximo.
    pub advance_on_target_selection: Option<bool>,

    /// Sobrescreve o esconder do Próximo ao avançar por clique no alvo. `primaryAction` sempre fica visível.
    pub hide_next: Option<bool>,

    /// Esconde o Próximo e avança quando a expressão de contexto passa a valer.
    pub advance_when: Option<ContextKeyExpression>,

    /// Margem extra (px) em volta do alvo ao abrir o buraco do destaque.
    pub padding: Option<f64>,

    /// Gancho que roda logo antes de o passo aparecer, por exemplo para abrir a view que hospeda o alvo.
    pub on_before_show: Option<Rc<dyn Fn() -> MaybePromise<()>>>,

    /// Chamado depois de o spotlight estar visível, para efeitos só de apresentação.
    pub on_did_show: Option<Rc<dyn Fn()>>,
}

fn context_eq(a: &Option<ContextKeyExpression>, b: &Option<ContextKeyExpression>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.equals(b),
        _ => false,
    }
}

fn function_eq<T: ?Sized>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        _ => false,
    }
}

/// Os dados do passo; os ganchos (`on_before_show`, `on_did_show`, `run` da ação primária) são funções e só aparecem como presentes ou
/// ausentes.
impl std::fmt::Debug for SpotlightStep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpotlightStep")
            .field("id", &self.id)
            .field("target_id", &self.target_id)
            .field("title", &self.title)
            .field("description", &self.description)
            .field("next_button_label", &self.next_button_label)
            .field("primary_action_label", &self.primary_action.as_ref().map(|action| &action.label))
            .field("placement", &self.placement)
            .field("has_when", &self.when.is_some())
            .field("missing_target", &self.missing_target)
            .field("open_target", &self.open_target)
            .field("allow_target_interaction", &self.allow_target_interaction)
            .field("advance_on_target_click", &self.advance_on_target_click)
            .field("advance_on_target_selection", &self.advance_on_target_selection)
            .field("hide_next", &self.hide_next)
            .field("has_advance_when", &self.advance_when.is_some())
            .field("padding", &self.padding)
            .field("has_on_before_show", &self.on_before_show.is_some())
            .field("has_on_did_show", &self.on_did_show.is_some())
            .finish()
    }
}

impl PartialEq for SpotlightStep {
    fn eq(&self, other: &Self) -> bool {
        let primary_action_eq = match (&self.primary_action, &other.primary_action) {
            (None, None) => true,
            (Some(a), Some(b)) => a.label == b.label && Rc::ptr_eq(&a.run, &b.run),
            _ => false,
        };
        self.id == other.id
            && self.target_id == other.target_id
            && self.title == other.title
            && self.description == other.description
            && self.next_button_label == other.next_button_label
            && primary_action_eq
            && self.placement == other.placement
            && context_eq(&self.when, &other.when)
            && self.missing_target == other.missing_target
            && self.open_target == other.open_target
            && self.allow_target_interaction == other.allow_target_interaction
            && self.advance_on_target_click == other.advance_on_target_click
            && self.advance_on_target_selection == other.advance_on_target_selection
            && self.hide_next == other.hide_next
            && context_eq(&self.advance_when, &other.advance_when)
            && self.padding == other.padding
            && function_eq(&self.on_before_show, &other.on_before_show)
            && function_eq(&self.on_did_show, &other.on_did_show)
    }
}

/// O `resolveSteps` do payload.
pub type SpotlightResolveSteps = Rc<dyn Fn() -> Promise<Vec<SpotlightStep>>>;

/// `ISpotlightPayload`: o payload de um cenário de spotlight, uma lista ordenada de passos.
#[derive(Clone, Default)]
pub struct SpotlightPayload {
    pub steps: Vec<SpotlightStep>,

    /// Resolve passos escolhidos por experimento quando o tour roda, no lugar dos passos padrão.
    pub resolve_steps: Option<SpotlightResolveSteps>,
}
