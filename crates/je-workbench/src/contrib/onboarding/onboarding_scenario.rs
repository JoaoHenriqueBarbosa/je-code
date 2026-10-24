//! Porte de `src/vs/workbench/contrib/onboarding/common/onboardingScenario.ts`: o cenário de onboarding
//! declarativo (dados puros), o gatilho, a apresentação e o resultado de uma execução.
//!
//! Divergências:
//! - `IOnboardingScenario<TPayload = unknown>` e `IOnboardingPresentationRef<TPayload = unknown>` são genéricos
//!   com `Dynamic` (o `unknown` do original) como padrão.
//! - `OnboardingTrigger` é o enum [`OnboardingTrigger`]; o `signal: IObservable<boolean>` é o
//!   `IObservable<bool>` de `je_base::observable`.
//! - `readonly developerModeVariations?: readonly string[]` e `tryout?` são `Option<Rc<...>>`: o registro compara
//!   as duas por identidade (`===`) em `registerDescriptor`, e o `Rc` guarda essa identidade.
//! - `const enum` de texto ([`OnboardingOutcome`], [`OnboardingDismissReason`]) tem `as_str` com o valor do original.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::observable::base::IObservable;
use je_platform::contextkey::contextkey::ContextKeyExpression;

use super::onboarding_tryout::OnboardingTryoutMetadata;

/// `OnboardingTrigger`: quando um cenário passa a poder rodar sozinho.
#[derive(Clone)]
pub enum OnboardingTrigger {
    /// Elegível assim que a cláusula `when` do cenário é satisfeita.
    Auto,
    /// Só começa explicitamente por um comando (nunca roda sozinho).
    Command { command_id: String },
    /// Elegível sempre que o observável fornecido vale `true`.
    Observable { signal: IObservable<bool> },
}

/// `IOnboardingPresentationRef<TPayload>`: referência a uma apresentação registrada, com o payload opaco que ela
/// entende (por exemplo, os passos de um spotlight).
#[derive(Debug, Clone, PartialEq)]
pub struct OnboardingPresentationRef<P = Dynamic> {
    /// O tipo da apresentação, por exemplo `'spotlight'`.
    pub kind: String,
    /// Dados específicos da apresentação. O motor nunca os inspeciona.
    pub payload: P,
}

/// Prefixo reservado que o identificador de contexto de atribuição de todo experimento de onboarding precisa usar
/// (por exemplo `onb-newsession-2026q3`).
///
/// ### Por que existe
/// A plataforma de experimentação carimba todo evento de telemetria com o contexto de atribuição do usuário assim
/// que as atribuições são buscadas na partida. Para experimentos de onboarding o identificador NÃO pode sair antes
/// de o usuário chegar ao ponto em que o onboarding seria mostrado: senão o placar contaria atividade de antes de o
/// experimento poder ter efeito.
///
/// As flags de tratamento que dizem ao cliente QUAL variante vale para um tour resolvem de forma assíncrona,
/// depois de o contexto de atribuição já ter ido para a telemetria, então chegam tarde para bloquear os primeiros
/// eventos. O prefixo resolve a corrida: por ser uma cadeia estática e conhecida, o cliente exclui de antemão
/// QUALQUER entrada do contexto que comece com ele desde o primeiro evento e depois libera só a variante
/// específica quando o usuário chega ao momento do onboarding.
///
/// ### Contrato para quem escreve experimentos
/// - Todo experimento de onboarding configurado no ExP DEVE usar nomes de variante que comecem com este prefixo.
/// - O nome da variante de cada braço DEVE ser devolvido pela flag de tratamento
///   `assignment_context_id_flag` do experimento.
///
/// O motor reforça isso por defesa: um id sem o prefixo é rejeitado (o experimento é tratado como inativo e a
/// configuração errada é reportada por `onUnexpectedError`), então um id ruim nunca vaza para a telemetria sem
/// bloqueio. Um id com prefixo que nenhum tour registrado reivindica fica bloqueado para sempre.
pub const ONBOARDING_ASSIGNMENT_CONTEXT_PREFIX: &str = "onb-";

/// `IOnboardingExperiment`: a configuração de experimento de um cenário. Os dois NOMES de flag são resolvidos no
/// serviço de experimentação; o experimento só é considerado ATIVO quando as duas resolvem: a booleana para um
/// booleano e o id para um texto não vazio que começa com [`ONBOARDING_ASSIGNMENT_CONTEXT_PREFIX`]. Se uma
/// faltar, ou o id não tiver o prefixo, o experimento fica INATIVO e o cenário não roda sozinho (ainda pode ser
/// iniciado por um comando ou `runScenario`, que ignora o gate de experimento e não abre o gate de telemetria; ou
/// ligando `onboarding.developerMode` para o cenário, que mostra o tour sem abrir o gate de telemetria).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnboardingExperiment {
    /// Nome da flag booleana de tratamento que decide a EXPERIÊNCIA: `true` mostra o tour (braço de tratamento),
    /// `false` não (controle). Nos dois braços o identificador do contexto de atribuição passa a fluir quando o
    /// usuário chega ao momento do onboarding.
    pub behavior_flag: String,

    /// Nome da flag de texto cujo valor é o nome da variante ExP do braço atual. A entrada de telemetria acrescenta
    /// `:<allocationId>`; o valor em si DEVE começar com [`ONBOARDING_ASSIGNMENT_CONTEXT_PREFIX`]; um id sem o
    /// prefixo é rejeitado (o experimento fica inativo) para nunca vazar para a telemetria sem bloqueio.
    pub assignment_context_id_flag: String,
}

/// `IOnboardingScenario<TPayload>`: um cenário de onboarding declarativo. Cenários são dados puros: descrevem
/// QUANDO o onboarding acontece e QUAL apresentação o desenha, mas não COMO é desenhado (isso é da apresentação).
#[derive(Clone)]
pub struct OnboardingScenario<P = Dynamic> {
    /// Identificador estável. Chave de persistência do estado "mostrado uma vez".
    pub id: String,

    /// Chave de persistência do estado "mostrado" uma vez por usuário. Cenários que compartilham a `seen_key`
    /// valem como o MESMO onboarding para o gate de uma vez por usuário: mostrar um marca todos como vistos, e os
    /// outros nunca rodam sozinhos. Serve para variações do mesmo tour que devem aparecer no máximo uma vez entre
    /// elas. Padrão: [`Self::id`].
    pub seen_key: Option<String>,

    /// Portão de elegibilidade. Combinado com E às verificações do próprio motor.
    pub when: Option<ContextKeyExpression>,

    /// Como o cenário passa a poder rodar sozinho.
    pub trigger: OnboardingTrigger,

    /// Cenários de maior prioridade rodam primeiro quando vários estão elegíveis. Padrão `0`.
    pub priority: Option<f64>,

    /// A apresentação que desenha este cenário, com o payload dela.
    pub presentation: OnboardingPresentationRef<P>,

    /// Expõe um exemplo de recurso só manual pelo serviço de tryout com guarda.
    pub tryout: Option<Rc<OnboardingTryoutMetadata>>,

    /// Experimento opcional que libera e mede este cenário. Quando presente, o cenário só roda se o experimento
    /// estiver ativo (as duas flags de tratamento definidas). Ver [`OnboardingExperiment`].
    pub experiment: Option<OnboardingExperiment>,

    /// Variações expostas pelo override de variação do modo desenvolvedor do onboarding.
    pub developer_mode_variations: Option<Rc<[String]>>,

    /// Quando `true`, o cenário roda em toda sessão elegível em vez de uma vez por usuário.
    pub repeatable: Option<bool>,
}

/// `OnboardingOutcome`: o resultado de rodar a apresentação de um cenário.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingOutcome {
    /// O usuário chegou ao fim do cenário.
    Completed,
    /// O usuário pulou o cenário explicitamente.
    Skipped,
    /// O cenário foi dispensado (por exemplo, fechado sem pular explicitamente).
    Dismissed,
    /// O cenário foi abortado pelo motor (por exemplo, fim da janela, alvo perdido).
    Aborted,
}

impl OnboardingOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::Skipped => "skipped",
            Self::Dismissed => "dismissed",
            Self::Aborted => "aborted",
        }
    }
}

/// `OnboardingDismissReason`: o mecanismo concreto que encerrou a execução de um cenário. Refina
/// [`OnboardingOutcome`] com a ação específica do usuário, principalmente para telemetria (por exemplo, separar o
/// botão Pular da tecla Escape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnboardingDismissReason {
    /// O usuário chegou ao último passo e avançou além dele.
    Completed,
    /// O usuário apertou o botão Pular.
    SkipButton,
    /// O usuário apertou a tecla Escape.
    EscapeKey,
    /// O usuário avançou clicando no alvo destacado do último passo.
    TargetClick,
    /// O motor abortou a execução (fim da janela, alvo perdido...).
    Aborted,
}

impl OnboardingDismissReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Completed => "completed",
            Self::SkipButton => "skipButton",
            Self::EscapeKey => "escapeKey",
            Self::TargetClick => "targetClick",
            Self::Aborted => "aborted",
        }
    }
}

/// `IOnboardingRunResult`: o resultado rico que o `run` de uma apresentação devolve, com o desfecho e os dados de
/// que o motor precisa para a telemetria por tour (até onde o usuário foi e como a execução terminou).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OnboardingRunResult {
    /// Desfecho grosso, no nível do motor.
    pub outcome: OnboardingOutcome,

    /// Se a apresentação mostrou algo ao usuário de fato. `false` para uma execução degenerada que não desenhou UI
    /// (por exemplo, cenário sem passos, ou todos os passos pulados por falta de alvo ou de `when`). O motor só
    /// emite telemetria de desfecho quando um tour foi realmente mostrado.
    pub shown: bool,

    /// O motivo/mecanismo concreto que encerrou a execução.
    pub dismiss_reason: OnboardingDismissReason,

    /// O maior índice de passo (base 0) que o usuário alcançou (`0` se o primeiro passo nunca apareceu).
    pub last_step_index: f64,

    /// O total de passos que o cenário declarou.
    pub step_count: f64,
}
