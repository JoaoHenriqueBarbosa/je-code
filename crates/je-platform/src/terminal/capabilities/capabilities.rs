//! Porte de `src/vs/platform/terminal/common/capabilities/capabilities.ts`: o enum [`TerminalCapability`],
//! o armazenamento ([`ITerminalCapabilityStore`]) e as interfaces de cada capacidade.
//!
//! Decisões de tradução:
//! - Uma interface do TypeScript vira um trait. O objeto compartilhado é `Rc<dyn Trait>`, e os métodos que
//!   mudam estado recebem `&self` (mutabilidade interior no implementador), como no resto do porte.
//! - `ITerminalCapabilityImplMap` (o mapa `capacidade -> implementação` que tipa `get<T>`) é um trait
//!   implementado por um tipo marcador por capacidade ([`CwdDetection`], [`CommandDetection`], ...), com a
//!   implementação como tipo associado. A união discriminada dos valores é [`TerminalCapabilityImpl`], e o
//!   `AnyTerminalCapabilityChangeEvent` é [`AnyTerminalCapabilityChangeEvent`] (o `id` sempre vale
//!   `capability.capability()`).
//! - Os métodos genéricos do store (`get<T>`, `createOn...OfTypeEvent<T>`) não cabem num trait usável como
//!   `dyn`: o trait tem o método sem genérico ([`ITerminalCapabilityStore::get_capability`]) e
//!   [`super::terminal_capability_store::TerminalCapabilityStoreExt`] dá a forma genérica a todo store.
//! - Os getters memoizados do original (`@memoize`) devolvem `Rc<Event<_>>`: a identidade do `Rc` é a
//!   identidade do evento memoizado.
//! - `number` é `f64` e `string | undefined` é `Option<String>`.
//! - Ficam de fora `IPtyHostProcessReplayEvent` (depende de `ReplayEntry`, de `terminalProcess.ts`) e o
//!   bloco comentado do `IMarker` do original.
//!
//! Os tipos que o original importa de outros arquivos ainda não portados ([`IPromptInputModel`],
//! [`ICurrentPartialCommand`], [`ITerminalOutputMatcher`], [`ITerminalOutputMatch`], o
//! [`ISerializedPromptInputModel`]) aparecem como traits sem membros no fim deste arquivo, só para fechar as
//! assinaturas; o corpo deles vem com o porte dos arquivos de origem.

use std::rc::Rc;

use indexmap::IndexMap;
use je_base::event::Event;
use je_base::lifecycle::IDisposable;
use je_base::promise::Promise;

/// Primariamente guiada pela integração com o shell, uma capacidade do terminal é o mecanismo para
/// melhorar progressivamente recursos que podem não valer para todo terminal ou shell.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TerminalCapability {
    /// O terminal detecta o diretório de trabalho atual assim que a mudança acontece no buffer.
    CwdDetection = 0,
    /// O terminal detecta o diretório de trabalho atual quando é pedido.
    NaiveCwdDetection = 1,
    /// O terminal identifica com confiança prompts, comandos e saídas de comando no buffer.
    CommandDetection = 2,
    /// O terminal costuma identificar prompts, comandos e saídas de comando no buffer, mas pode não
    /// lembrar bem a posição dos comandos que rodaram no passado. Este estado pode ser ligado quando algo
    /// dá errado ou ao usar conpty, por exemplo.
    PartialCommandDetection = 3,
    /// Gerencia marcas do buffer usadas na navegação do terminal. A origem do pedido (tarefa, depuração
    /// etc.) dá um id, um marcador opcional, uma `hoverMessage` e a propriedade `hidden`. Sem `hidden`, uma
    /// decoração genérica entra no buffer e na régua de visão geral.
    BufferMarkDetection = 4,
    /// O terminal detecta o ambiente mais recente do shell atual do usuário.
    ShellEnvDetection = 5,
    /// O terminal detecta o tipo de prompt em uso (por exemplo p10k, posh-git).
    PromptTypeDetection = 6,
}

/// Um objeto que guarda as capacidades adicionais e suas implementações, para recursos que não existem em
/// todo terminal.
pub trait ITerminalCapabilityStore {
    /// Todas as capacidades do store, na ordem em que entraram.
    fn items(&self) -> Box<dyn Iterator<Item = TerminalCapability>>;

    /// Disparado quando uma capacidade é adicionada.
    fn on_did_add_capability(&self) -> Event<AnyTerminalCapabilityChangeEvent>;

    /// Disparado quando uma capacidade é removida.
    fn on_did_remove_capability(&self) -> Event<AnyTerminalCapabilityChangeEvent>;

    /// Disparado quando uma capacidade é adicionada ou removida.
    fn on_did_change_capabilities(&self) -> Rc<Event<()>>;

    /// Disparado quando a capacidade de detecção de comandos é adicionada.
    fn on_did_add_command_detection_capability(&self) -> Rc<Event<Rc<dyn ICommandDetectionCapability>>>;
    /// Disparado quando a capacidade de detecção de comandos é removida.
    fn on_did_remove_command_detection_capability(&self) -> Rc<Event<()>>;
    /// Disparado quando a capacidade de detecção de cwd é adicionada.
    fn on_did_add_cwd_detection_capability(&self) -> Rc<Event<Rc<dyn ICwdDetectionCapability>>>;
    /// Disparado quando a capacidade de detecção de cwd é removida.
    fn on_did_remove_cwd_detection_capability(&self) -> Rc<Event<()>>;

    /// Diz se a capacidade existe no store.
    fn has(&self, capability: TerminalCapability) -> bool;

    /// A implementação de uma capacidade, se ela foi adicionada ao store. A forma tipada é
    /// [`super::terminal_capability_store::TerminalCapabilityStoreExt::get`].
    fn get_capability(&self, capability: TerminalCapability) -> Option<TerminalCapabilityImpl>;
}

/// `TerminalCapabilityChangeEvent<T>`: a mudança de uma capacidade com a implementação tipada.
pub struct TerminalCapabilityChangeEvent<T: ITerminalCapabilityImplMap> {
    pub id: TerminalCapability,
    pub capability: Rc<T::Impl>,
}

/// `AnyTerminalCapabilityChangeEvent`: a união discriminada dos `TerminalCapabilityChangeEvent<K>`. O `id`
/// sempre é o `capability.capability()`.
#[derive(Clone)]
pub struct AnyTerminalCapabilityChangeEvent {
    pub id: TerminalCapability,
    pub capability: TerminalCapabilityImpl,
}

/// Mapeia os tipos de capacidade para a sua implementação, o que permite buscar a implementação com tipo.
/// Cada capacidade tem um tipo marcador que implementa este trait.
pub trait ITerminalCapabilityImplMap: 'static {
    /// A interface da implementação (`ITerminalCapabilityImplMap[K]`).
    type Impl: ?Sized + 'static;
    /// A chave `K` do mapa.
    const CAPABILITY: TerminalCapability;
    /// Põe a implementação na união [`TerminalCapabilityImpl`].
    fn wrap_impl(imp: Rc<Self::Impl>) -> TerminalCapabilityImpl;
    /// Tira a implementação da união, se ela é desta capacidade.
    fn unwrap_impl(imp: &TerminalCapabilityImpl) -> Option<Rc<Self::Impl>>;
}

/// Gera a união [`TerminalCapabilityImpl`] e um marcador por capacidade que implementa
/// [`ITerminalCapabilityImplMap`].
macro_rules! capability_impl_map {
    ($($name:ident => $interface:ident;)*) => {
        /// O valor do mapa [`ITerminalCapabilityImplMap`]: a implementação de uma das capacidades.
        #[derive(Clone)]
        pub enum TerminalCapabilityImpl {
            $($name(Rc<dyn $interface>),)*
        }

        impl TerminalCapabilityImpl {
            /// A capacidade a que a implementação pertence.
            pub fn capability(&self) -> TerminalCapability {
                match self {
                    $(Self::$name(_) => TerminalCapability::$name,)*
                }
            }
        }

        $(
            /// Marcador da capacidade de mesmo nome em [`ITerminalCapabilityImplMap`].
            pub struct $name;

            impl ITerminalCapabilityImplMap for $name {
                type Impl = dyn $interface;
                const CAPABILITY: TerminalCapability = TerminalCapability::$name;

                fn wrap_impl(imp: Rc<Self::Impl>) -> TerminalCapabilityImpl {
                    TerminalCapabilityImpl::$name(imp)
                }

                fn unwrap_impl(imp: &TerminalCapabilityImpl) -> Option<Rc<Self::Impl>> {
                    match imp {
                        TerminalCapabilityImpl::$name(inner) => Some(inner.clone()),
                        _ => None,
                    }
                }
            }
        )*
    };
}

capability_impl_map! {
    CwdDetection => ICwdDetectionCapability;
    CommandDetection => ICommandDetectionCapability;
    NaiveCwdDetection => INaiveCwdDetectionCapability;
    PartialCommandDetection => IPartialCommandDetectionCapability;
    BufferMarkDetection => IBufferMarkCapability;
    ShellEnvDetection => IShellEnvDetectionCapability;
    PromptTypeDetection => IPromptTypeDetectionCapability;
}

pub trait ICwdDetectionCapability {
    /// `type`: sempre [`TerminalCapability::CwdDetection`].
    fn capability_type(&self) -> TerminalCapability;
    fn on_did_change_cwd(&self) -> Event<String>;
    fn cwds(&self) -> Vec<String>;
    /// Diz se o cwd atual veio de uma fonte confiável. Vale `true` só quando o cwd foi informado pela
    /// integração com o shell do VS Code com um nonce válido. As atualizações por OSC 7, OSC 9;9 e OSC 1337
    /// são sempre não confiáveis, porque esses protocolos não têm nonce.
    fn is_trusted(&self) -> bool;
    fn get_cwd(&self) -> String;
    fn update_cwd(&self, cwd: &str, is_trusted: Option<bool>);
}

pub trait IShellEnvDetectionCapability {
    /// `type`: sempre [`TerminalCapability::ShellEnvDetection`].
    fn capability_type(&self) -> TerminalCapability;
    fn on_did_change_env(&self) -> Event<TerminalShellIntegrationEnvironment>;
    fn env(&self) -> TerminalShellIntegrationEnvironment;
    fn set_environment(&self, envs: Option<&IndexMap<String, Option<String>>>, is_trusted: bool);
    fn start_environment_single_var(&self, clear: bool, is_trusted: bool);
    fn set_environment_single_var(&self, key: &str, value: Option<&str>, is_trusted: bool);
    fn delete_environment_single_var(&self, key: &str, value: Option<&str>, is_trusted: bool);
    fn end_environment_single_var(&self, is_trusted: bool);
}

pub trait IPromptTypeDetectionCapability {
    /// `type`: sempre [`TerminalCapability::PromptTypeDetection`].
    fn capability_type(&self) -> TerminalCapability;
    fn prompt_type(&self) -> Option<String>;
    fn on_prompt_type_changed(&self) -> Event<Option<String>>;
    fn set_prompt_type(&self, value: &str);
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalShellIntegrationEnvironment {
    /// O dicionário de variáveis de ambiente.
    pub value: Option<IndexMap<String, Option<String>>>,
    /// Diz se o ambiente veio de uma fonte confiável e, portanto, se os valores podem ser usados de um
    /// jeito que leve à execução de código arbitrário. Se for `false`, `value` não deve ser usado para
    /// nada que possa executar código arbitrário, ou o usuário deve ser avisado antes.
    ///
    /// Vale `true` só quando o ambiente foi informado de forma explícita e usou um nonce para verificação.
    pub is_trusted: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalShellIntegration {
    /// O ambiente do processo do shell. Fica indefinido se o script de integração com o shell não envia o
    /// ambiente.
    pub env: TerminalShellIntegrationEnvironment,
}

/// `CommandInvalidationReason` (`'windows'` e `'noProblemsReported'`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandInvalidationReason {
    Windows,
    NoProblemsReported,
}

impl CommandInvalidationReason {
    /// O valor de texto do original.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Windows => "windows",
            Self::NoProblemsReported => "noProblemsReported",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ICommandInvalidationRequest {
    pub reason: CommandInvalidationReason,
}

pub trait IBufferMarkCapability {
    /// `type`: sempre [`TerminalCapability::BufferMarkDetection`].
    fn capability_type(&self) -> TerminalCapability;
    fn markers(&self) -> Box<dyn Iterator<Item = Rc<dyn IMarker>>>;
    fn on_mark_added(&self) -> Event<IMarkProperties>;
    fn add_mark(&self, properties: Option<IMarkProperties>);
    fn get_mark(&self, id: &str) -> Option<Rc<dyn IMarker>>;
}

/// `'low' | 'medium' | 'high'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalCommandConfidence {
    Low,
    Medium,
    High,
}

/// O resultado de `getCommandForLine`: `ITerminalCommand | ICurrentPartialCommand`.
#[derive(Clone)]
pub enum TerminalCommandForLine {
    Command(Rc<dyn ITerminalCommand>),
    Partial(Rc<dyn ICurrentPartialCommand>),
}

pub trait ICommandDetectionCapability {
    /// `type`: sempre [`TerminalCapability::CommandDetection`].
    fn capability_type(&self) -> TerminalCapability;
    fn prompt_input_model(&self) -> Rc<dyn IPromptInputModel>;
    fn commands(&self) -> Vec<Rc<dyn ITerminalCommand>>;
    /// O comando em execução agora, ou `None`.
    fn executing_command(&self) -> Option<String>;
    fn executing_command_object(&self) -> Option<Rc<dyn ITerminalCommand>>;
    fn executing_command_confidence(&self) -> Option<TerminalCommandConfidence>;
    /// O cwd atual na posição do cursor.
    fn cwd(&self) -> Option<String>;
    fn has_rich_command_detection(&self) -> bool;
    fn current_command(&self) -> Option<Rc<dyn ICurrentPartialCommand>>;
    fn on_command_started(&self) -> Event<Rc<dyn ITerminalCommand>>;
    fn on_command_finished(&self) -> Event<Rc<dyn ITerminalCommand>>;
    fn on_command_executed(&self) -> Event<Rc<dyn ITerminalCommand>>;
    fn on_command_invalidated(&self) -> Event<Vec<Rc<dyn ITerminalCommand>>>;
    fn on_current_command_invalidated(&self) -> Event<ICommandInvalidationRequest>;
    fn on_set_rich_command_detection(&self) -> Event<bool>;
    fn set_continuation_prompt(&self, value: &str);
    fn set_prompt_terminator(&self, value: &str, last_prompt_line: &str);
    fn set_cwd(&self, value: &str);
    fn set_is_windows_pty(&self, value: bool);
    fn set_is_command_storage_disabled(&self);
    /// O diretório de trabalho de uma linha. Devolve `None` se for desconhecido, caso em que vale o cwd
    /// inicial do terminal.
    fn get_cwd_for_line(&self, line: f64) -> Option<String>;
    fn get_command_for_line(&self, line: f64) -> Option<TerminalCommandForLine>;
    fn clear_commands(&self);
    fn handle_prompt_start(&self, options: Option<&IHandleCommandOptions>);
    fn handle_continuation_start(&self);
    fn handle_continuation_end(&self);
    fn handle_right_prompt_start(&self);
    fn handle_right_prompt_end(&self);
    fn handle_command_start(&self, options: Option<&IHandleCommandOptions>);
    fn handle_command_executed(&self, options: Option<&IHandleCommandOptions>);
    fn handle_command_finished(&self, exit_code: Option<f64>, options: Option<&IHandleCommandOptions>);
    fn set_has_rich_command_detection(&self, value: bool);
    /// Define a linha de comando de forma explícita.
    ///
    /// `command_line` é a linha de comando definida. `is_trusted` diz se ela é confiável pelo nonce
    /// opcional enviado para impedir falsificação. Isso importa porque algumas interações não pedem
    /// verificação antes de rodar o comando de novo. O nonce é opcional segundo a especificação, mas
    /// sempre existe ao usar os scripts de integração nativos.
    fn set_command_line(&self, command_line: &str, is_trusted: bool);
    /// Define o id do próximo comando a começar. Permite atribuir um id antes de o shell enviar a
    /// sequência de início de comando, o que serve para ligar comandos entre o renderer e o ptyHost.
    fn set_next_command_id(&self, command: &str, command_id: &str);
    fn serialize(&self) -> ISerializedCommandDetectionCapability;
    fn deserialize(&self, serialized: &ISerializedCommandDetectionCapability);
}

#[derive(Clone, Default)]
pub struct IHandleCommandOptions {
    /// Diz se um comando vazio pode ser registrado. Serve para certos scripts e recursos de integração com
    /// o shell em que acompanhar a linha de comando pode não ser possível.
    pub ignore_command_line: Option<bool>,
    /// O marcador a usar.
    pub marker: Option<Rc<dyn IMarker>>,
    /// As propriedades da marca.
    pub mark_properties: Option<IMarkProperties>,
}

pub trait INaiveCwdDetectionCapability {
    /// `type`: sempre [`TerminalCapability::NaiveCwdDetection`].
    fn capability_type(&self) -> TerminalCapability;
    fn on_did_change_cwd(&self) -> Event<String>;
    fn get_cwd(&self) -> Promise<String>;
}

pub trait IPartialCommandDetectionCapability {
    /// `type`: sempre [`TerminalCapability::PartialCommandDetection`].
    fn capability_type(&self) -> TerminalCapability;
    fn commands(&self) -> Vec<Rc<dyn IMarker>>;
    fn on_command_finished(&self) -> Event<Rc<dyn IMarker>>;
    fn clear_commands(&self);
}

/// `IBaseTerminalCommand`: o que todo comando tem, serializável ou não. Os campos mutáveis do original são
/// lidos por getters; quem implementa guarda o estado.
pub trait IBaseTerminalCommand {
    // Obrigatórios
    fn command(&self) -> String;
    fn command_line_confidence(&self) -> TerminalCommandConfidence;
    fn is_trusted(&self) -> bool;
    fn timestamp(&self) -> f64;
    fn duration(&self) -> f64;
    fn id(&self) -> Option<String>;

    // Opcionais serializáveis
    fn cwd(&self) -> Option<String>;
    fn exit_code(&self) -> Option<f64>;
    fn command_start_line_content(&self) -> Option<String>;
    fn mark_properties(&self) -> Option<IMarkProperties>;
    fn executed_x(&self) -> Option<f64>;
    fn start_x(&self) -> Option<f64>;
}

pub trait ITerminalCommand: IBaseTerminalCommand {
    // Opcionais não serializáveis
    fn prompt_start_marker(&self) -> Option<Rc<dyn IMarker>>;
    fn marker(&self) -> Option<Rc<dyn IMarker>>;
    fn end_marker(&self) -> Option<Rc<dyn IMarker>>;
    fn executed_marker(&self) -> Option<Rc<dyn IMarker>>;
    fn aliases(&self) -> Option<Vec<Vec<String>>>;
    fn was_replayed(&self) -> Option<bool>;

    fn extract_command_line(&self) -> String;
    fn get_output(&self) -> Option<String>;
    fn get_output_match(&self, output_matcher: &dyn ITerminalOutputMatcher) -> Option<Rc<dyn ITerminalOutputMatch>>;
    fn has_output(&self) -> bool;
    fn get_prompt_row_count(&self) -> f64;
    fn get_command_row_count(&self) -> f64;
}

/// `ISerializedTerminalCommand`: `IBaseTerminalCommand` mais as linhas no lugar dos marcadores.
#[derive(Clone)]
pub struct ISerializedTerminalCommand {
    // Obrigatórios
    pub command: String,
    pub command_line_confidence: TerminalCommandConfidence,
    pub is_trusted: bool,
    pub timestamp: f64,
    pub duration: f64,
    pub id: Option<String>,

    // Opcionais serializáveis
    pub cwd: Option<String>,
    pub exit_code: Option<f64>,
    pub command_start_line_content: Option<String>,
    pub mark_properties: Option<IMarkProperties>,
    pub executed_x: Option<f64>,
    pub start_x: Option<f64>,

    // Opcionais não serializáveis, convertidos para a serialização
    pub start_line: Option<f64>,
    pub prompt_start_line: Option<f64>,
    pub end_line: Option<f64>,
    pub executed_line: Option<f64>,
}

#[derive(Clone, Default)]
pub struct IMarkProperties {
    pub hover_message: Option<String>,
    pub disable_command_storage: Option<bool>,
    pub hidden: Option<bool>,
    pub marker: Option<Rc<dyn IMarker>>,
    pub id: Option<String>,
}

#[derive(Clone)]
pub struct ISerializedCommandDetectionCapability {
    pub is_windows_pty: bool,
    pub has_rich_command_detection: bool,
    pub commands: Vec<ISerializedTerminalCommand>,
    pub prompt_input_model: Option<Rc<dyn ISerializedPromptInputModel>>,
}

// #region Tipos de outros arquivos do original

/// `IMarker` do `@xterm/headless`: o marcador de uma linha do buffer.
pub trait IMarker: IDisposable {
    fn id(&self) -> f64;
    fn is_disposed(&self) -> bool;
    fn line(&self) -> f64;
    fn on_dispose(&self) -> Event<()>;
}

/// `IPromptInputModel` de `commandDetection/promptInputModel.ts` (ainda não portado).
pub trait IPromptInputModel {}

/// `ISerializedPromptInputModel` de `commandDetection/promptInputModel.ts` (ainda não portado).
pub trait ISerializedPromptInputModel {}

/// `ICurrentPartialCommand` de `commandDetection/terminalCommand.ts` (ainda não portado).
pub trait ICurrentPartialCommand {}

/// `ITerminalOutputMatcher` de `terminal.ts` (ainda não portado).
pub trait ITerminalOutputMatcher {}

/// `ITerminalOutputMatch` de `terminal.ts` (ainda não portado).
pub trait ITerminalOutputMatch {}

// #endregion
