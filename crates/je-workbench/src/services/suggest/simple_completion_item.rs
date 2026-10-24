//! Porte de `src/vs/workbench/services/suggest/browser/simpleCompletionItem.ts`: `CompletionItemLabel`,
//! `ISimpleCompletion` e `SimpleCompletionItem`.
//!
//! Diferenças do original:
//! - Herança vira composição: `SimpleCompletionItem` é genérico no tipo da completion (`C`), e uma
//!   subclasse (o `TerminalCompletionItem`) guarda o item base e o expõe por `Deref`. O que o
//!   `ISimpleCompletion` e as interfaces que o estendem têm em comum fica no trait
//!   [`ISimpleCompletionData`] (o `ITerminalCompletion` repete os campos em vez de aninhar a base).
//! - `label: string | CompletionItemLabel` é [`SimpleCompletionLabel`].
//! - O texto do rótulo é [`JsString`] nos campos derivados (`text_label`, `label_low`), porque o `FuzzyScore`
//!   e as posições medem UTF-16.
//! - `replacementRange` é `Option<[i64; 2]>` (início inclusive, fim exclusive).
//! - `FuzzyScore.Default` é [`default_score`].

use je_base::JsString;
use je_base::filters::{FUZZY_SCORE_DEFAULT, FuzzyScore};
use je_base::themables::ThemeIcon;
use je_editor::languages::StringOrMarkdown;

/// `CompletionItemLabel` (o mesmo tipo do editor: `string | CompletionItemLabel` é o rótulo de qualquer item de sugestão).
pub use je_editor::languages::CompletionItemLabel;

/// `string | CompletionItemLabel`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SimpleCompletionLabel {
    Text(String),
    Item(CompletionItemLabel),
}

impl SimpleCompletionLabel {
    /// O texto do rótulo (o `label` do objeto, ou o próprio texto).
    pub fn text(&self) -> &str {
        match self {
            Self::Text(text) => text,
            Self::Item(item) => &item.label,
        }
    }
}

impl Default for SimpleCompletionLabel {
    fn default() -> Self {
        Self::Text(String::new())
    }
}

impl From<&str> for SimpleCompletionLabel {
    fn from(text: &str) -> Self {
        Self::Text(text.to_owned())
    }
}

impl From<CompletionItemLabel> for SimpleCompletionLabel {
    fn from(item: CompletionItemLabel) -> Self {
        Self::Item(item)
    }
}

/// `FuzzyScore.Default`: a pontuação de quem não casou com nada.
pub fn default_score() -> FuzzyScore {
    FUZZY_SCORE_DEFAULT.to_vec()
}

/// O que todo `ISimpleCompletion` (e quem o estende) oferece ao item e ao modelo.
pub trait ISimpleCompletionData {
    /// O rótulo, que aparece à esquerda do ícone.
    fn label(&self) -> &SimpleCompletionLabel;

    /// O intervalo de substituição (início inclusive, fim exclusive) do texto da linha.
    fn replacement_range(&self) -> Option<[i64; 2]>;
}

/// `ISimpleCompletion`.
#[derive(Clone, Debug, Default)]
pub struct ISimpleCompletion {
    /// O rótulo da completion, que aparece à esquerda do ícone.
    pub label: SimpleCompletionLabel,
    /// O id do provedor de onde a completion veio.
    pub provider: String,
    /// O ícone à esquerda do widget de sugestões.
    pub icon: Option<ThemeIcon>,
    /// O tipo do item, incluído no rótulo de acessibilidade.
    pub kind_label: Option<String>,
    /// O detalhe, que aparece à direita da lista.
    pub detail: Option<String>,
    /// Texto legível que representa um comentário de documentação.
    pub documentation: Option<StringOrMarkdown>,
    /// Intervalo (início inclusive, fim exclusive) do texto da linha trocado ao aplicar a completion.
    pub replacement_range: Option<[i64; 2]>,
}

impl ISimpleCompletionData for ISimpleCompletion {
    fn label(&self) -> &SimpleCompletionLabel {
        &self.label
    }

    fn replacement_range(&self) -> Option<[i64; 2]> {
        self.replacement_range
    }
}

/// `SimpleCompletionItem`.
pub struct SimpleCompletionItem<C: ISimpleCompletionData = ISimpleCompletion> {
    pub completion: C,

    /// O texto do rótulo (`textLabel`).
    pub text_label: JsString,
    /// O rótulo em minúsculas.
    pub label_low: JsString,

    // ordenação e filtro
    pub score: FuzzyScore,
    pub idx: Option<usize>,
    pub word: Option<JsString>,

    // validação
    pub is_invalid: bool,
}

impl<C: ISimpleCompletionData> SimpleCompletionItem<C> {
    pub fn new(completion: C) -> Self {
        // garante as variantes em minúsculas (desempenho)
        let text_label = JsString::from(completion.label().text());
        let label_low = text_label.to_lower_case();
        Self { completion, text_label, label_low, score: default_score(), idx: None, word: None, is_invalid: false }
    }
}

impl<C: ISimpleCompletionData> AsRef<SimpleCompletionItem<C>> for SimpleCompletionItem<C> {
    fn as_ref(&self) -> &SimpleCompletionItem<C> {
        self
    }
}

impl<C: ISimpleCompletionData> AsMut<SimpleCompletionItem<C>> for SimpleCompletionItem<C> {
    fn as_mut(&mut self) -> &mut SimpleCompletionItem<C> {
        self
    }
}
