//! Porte de `SignatureHelp` e dos tipos vizinhos de `src/vs/editor/common/languages.ts` (linhas 1259 a 1355 do
//! original): `ParameterInformation`, `SignatureInformation`, `SignatureHelp`, `SignatureHelpResult`,
//! `SignatureHelpTriggerKind`, `SignatureHelpContext` e `SignatureHelpProvider`, os tipos que o modelo de dicas de
//! parâmetro (`parameterHints`) consome. Ficam junto da contribuição, como os de `code_action/languages.rs`.
//!
//! Representação:
//! - `string | [number, number]` do `label` de um parâmetro é o [`ParameterLabel`] e `string | IMarkdownString` da
//!   documentação é o [`StringOrMarkdown`] de `languages.rs`.
//! - `SignatureHelp` é um objeto compartilhado no original (o modelo guarda o mesmo objeto que o provedor devolveu, e o
//!   `activeSignatureHelp` que o provedor recebe é esse objeto): aqui é o `Rc<SignatureHelp>`, e a identidade
//!   (`===`) é o [`Rc::ptr_eq`].
//! - `SignatureHelpResult extends IDisposable` é o [`SignatureHelpResult`], que guarda o `value` e o `dispose` de quem o
//!   criou. O resultado é compartilhado (`Rc`): o provedor pode devolver o mesmo resultado várias vezes.
//! - `ProviderResult<SignatureHelpResult>` é a [`ProviderResult`] de `Rc<SignatureHelpResult>`: `undefined` e `null`
//!   viram `None`. `signatureHelpTriggerCharacters?` e `signatureHelpRetriggerCharacters?` opcionais são os métodos do
//!   provedor, com a lista vazia como padrão (o original lê `|| []`).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::IDisposable;

use crate::language_providers::ProviderResult;
use crate::languages::StringOrMarkdown;
use crate::position::Position;
use crate::text_model::TextModel;

/// `ParameterInformation.label`: `string | [number, number]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParameterLabel {
    /// O texto do parâmetro.
    Text(String),
    /// O início e o fim do parâmetro dentro do rótulo da assinatura.
    Range(u32, u32),
}

/// `ParameterInformation`: um parâmetro de uma assinatura, com rótulo e documentação.
#[derive(Clone, Debug)]
pub struct ParameterInformation {
    /// O rótulo, mostrado na interface.
    pub label: ParameterLabel,
    /// A documentação, mostrada na interface e opcional.
    pub documentation: Option<StringOrMarkdown>,
}

/// `SignatureInformation`: a assinatura de algo chamável, com rótulo, documentação e parâmetros.
#[derive(Clone, Debug)]
pub struct SignatureInformation {
    /// O rótulo, como o nome da função, mostrado na interface.
    pub label: String,
    /// A documentação, mostrada na interface e opcional.
    pub documentation: Option<StringOrMarkdown>,
    /// Os parâmetros da assinatura.
    pub parameters: Vec<ParameterInformation>,
    /// O índice do parâmetro ativo; se vier, vale no lugar do `SignatureHelp.activeSignature`.
    pub active_parameter: Option<i32>,
}

/// `SignatureHelp`: as assinaturas de algo chamável, uma ativa e um parâmetro ativo.
#[derive(Clone, Debug)]
pub struct SignatureHelp {
    /// Uma ou mais assinaturas.
    pub signatures: Vec<SignatureInformation>,
    /// A assinatura ativa.
    pub active_signature: i32,
    /// O parâmetro ativo da assinatura ativa.
    pub active_parameter: i32,
}

/// `SignatureHelpResult`: o resultado de um provedor, que o consumidor descarta quando não o usa mais.
#[derive(Clone)]
pub struct SignatureHelpResult {
    pub value: Rc<SignatureHelp>,
    dispose: Rc<dyn Fn()>,
}

impl SignatureHelpResult {
    pub fn new(value: Rc<SignatureHelp>, dispose: impl Fn() + 'static) -> Self {
        Self { value, dispose: Rc::new(dispose) }
    }
}

impl IDisposable for SignatureHelpResult {
    fn dispose(&self) {
        (self.dispose)();
    }
}

/// `SignatureHelpTriggerKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignatureHelpTriggerKind {
    Invoke = 1,
    TriggerCharacter = 2,
    ContentChange = 3,
}

/// `SignatureHelpContext`.
#[derive(Clone, Debug)]
pub struct SignatureHelpContext {
    pub trigger_kind: SignatureHelpTriggerKind,
    pub trigger_character: Option<String>,
    pub is_retrigger: bool,
    pub active_signature_help: Option<Rc<SignatureHelp>>,
}

/// `SignatureHelpProvider`: o contrato entre as extensões e as dicas de parâmetro.
pub trait SignatureHelpProvider {
    /// `signatureHelpTriggerCharacters`.
    fn signature_help_trigger_characters(&self) -> &[String] {
        &[]
    }

    /// `signatureHelpRetriggerCharacters`.
    fn signature_help_retrigger_characters(&self) -> &[String] {
        &[]
    }

    /// `provideSignatureHelp(model, position, token, context)`: a ajuda para a assinatura na posição, ou `None`.
    fn provide_signature_help(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        token: &CancellationToken,
        context: &SignatureHelpContext,
    ) -> ProviderResult<Rc<SignatureHelpResult>>;
}
