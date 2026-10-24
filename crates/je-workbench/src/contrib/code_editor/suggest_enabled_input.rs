//! Porte de `src/vs/workbench/contrib/codeEditor/browser/suggestEnabledInput/suggestEnabledInput.ts`: a lógica de
//! completação do campo de texto de uma linha com sugestões.
//!
//! Recorte e divergências:
//! - **Portados**: o `SuggestResultsProvider`, o modelo do campo (`modelService.createModel('', null, scopeHandle, true)`),
//!   o `setValue`/`getValue` sobre o modelo, a validação do provedor (`validatedSuggestProvider`) e o provedor de
//!   completação registrado para o esquema do recurso (`completionProvider.register`), com a regra de só responder para o
//!   recurso exato do campo.
//! - **Fora do recorte (DOM e widgets)**: `CodeEditorWidget` do campo, contêiner e placeholder, estilos, eventos de
//!   foco, teclado e colagem, `layout`, `focus`, `onHide`, `updateAriaLabel`, `setPlaceHolder` e
//!   `getScopedContextKeyService`. Sem o widget, `inputWidget.getModel()` é [`SuggestEnabledInput::input_model`], e o
//!   `setValue` troca o conteúdo do modelo pelo `push_edit_operations` (sem rolagem nem posição do cursor).
//! - **`provideResults` devolve `(Partial<CompletionItem> & { label }) | string`**: é a lista de [`SuggestResult`]; o
//!   `{ ...rest }` que sobrescreve o item padrão é o fecho `apply`, que roda depois de montar o item.
//! - **`wordDefinition?: RegExp`** é um `Rc<JsRegExp>`, validado por `ensure_valid_word_definition`.
//! - **`query.lastIndexOf(' ', column - 1) + 1`** é a busca para trás do espaço a partir de `column - 1` (índice negativo
//!   vale 0, como no JavaScript).
//! - **`isEqual(model.uri, scopeHandle)`** é o `is_equal` do `ExtUri` sem viés de caixa (o `resources.isEqual`).

use std::rc::Rc;

use je_base::cancellation::CancellationToken;
use je_base::lifecycle::{to_disposable, Disposable, IDisposable};
use je_base::promise::Promise;
use je_base::resources::ExtUri;
use je_base::uri::URI;
use je_base::js_regexp::JsRegExp;
use je_base::JsString;
use je_editor::edit_operation::EditOperation;
use je_editor::language_features_service::LanguageFeaturesService;
use je_editor::language_providers::ProviderResult;
use je_editor::language_selector::{LanguageFilter, LanguageSelector};
use je_editor::languages::{CompletionContext, CompletionItem, CompletionItemKind, CompletionItemProvider, CompletionItemRange, CompletionLabel, CompletionList};
use je_editor::model_service::ModelService;
use je_editor::position::Position;
use je_editor::range::Range;
use je_editor::text_model::TextModel;
use je_editor::word_helper::{ensure_valid_word_definition, get_word_at_text};
use je_platform::resources::host_environment;

/// Um resultado do `provideResults`: o rótulo puro (`string`) ou o item parcial (`Partial<CompletionItem> & { label }`),
/// cujo `apply` sobrescreve os campos do item padrão.
#[derive(Clone)]
pub enum SuggestResult {
    Label(String),
    Item { label: String, apply: Rc<dyn Fn(&mut CompletionItem)> },
}

impl From<&str> for SuggestResult {
    fn from(label: &str) -> Self {
        SuggestResult::Label(label.to_string())
    }
}

/// `SuggestResultsProvider`.
#[derive(Clone)]
pub struct SuggestResultsProvider {
    /// `provideResults(query)`: `query` é o texto inteiro do campo.
    pub provide_results: Rc<dyn Fn(&str) -> Vec<SuggestResult>>,
    /// Trigger characters for this input. Defaults to the empty array.
    pub trigger_characters: Option<Vec<String>>,
    /// Optional regular expression that describes what a word is. Defaults to space separated words.
    pub word_definition: Option<Rc<JsRegExp>>,
    /// Show suggestions even if the trigger character is not present. Defaults to false.
    pub always_show_suggestions: bool,
    /// Defines the sorting function used when showing results. Defaults to the identity function.
    pub sort_key: Option<Rc<dyn Fn(&str) -> String>>,
}

impl SuggestResultsProvider {
    pub fn new(provide_results: impl Fn(&str) -> Vec<SuggestResult> + 'static) -> Self {
        Self { provide_results: Rc::new(provide_results), trigger_characters: None, word_definition: None, always_show_suggestions: false, sort_key: None }
    }
}

/// O provedor de completação que `completionProvider.register` recebe.
struct SuggestEnabledInputCompletionProvider {
    debug_display_name: String,
    scope_handle: URI,
    suggestion_provider: SuggestResultsProvider,
    sort_key: Rc<dyn Fn(&str) -> String>,
    trigger_characters: Vec<String>,
    word_definition: Option<Rc<JsRegExp>>,
    always_show_suggestions: bool,
}

impl CompletionItemProvider for SuggestEnabledInputCompletionProvider {
    fn debug_display_name(&self) -> &str {
        &self.debug_display_name
    }

    fn trigger_characters(&self) -> Option<&[String]> {
        Some(&self.trigger_characters)
    }

    fn provide_completion_items(
        &self,
        model: &Rc<TextModel>,
        position: Position,
        _context: &CompletionContext,
        _token: &CancellationToken,
    ) -> ProviderResult<Rc<CompletionList>> {
        let (model_uri, scope_handle) = (model.uri(), self.scope_handle.clone());
        if !ExtUri::unbiased(host_environment()).is_equal(Some(&model_uri), Some(&scope_handle), false) {
            return Promise::resolved(None);
        }

        let query = model.get_value();
        let query_units = query.units();

        let zero_indexed_column = position.column - 1;
        let (already_typed_count, zero_indexed_word_start);

        if let Some(word_definition) = &self.word_definition {
            let word_at_text = get_word_at_text(position.column, word_definition, query_units, 0, None);
            already_typed_count = word_at_text.as_ref().map_or(0, |word| word.word.units().len() as i32);
            zero_indexed_word_start = word_at_text.as_ref().map_or(0, |word| word.start_column - 1);
        } else {
            // `query.lastIndexOf(' ', zeroIndexedColumn - 1) + 1`
            let last_space = if query_units.is_empty() {
                -1
            } else {
                let from = (zero_indexed_column - 1).max(0) as usize;
                let end = from.min(query_units.len() - 1);
                query_units[..=end].iter().rposition(|unit| *unit == u16::from(b' ')).map_or(-1, |index| index as i32)
            };
            zero_indexed_word_start = last_space + 1;
            already_typed_count = zero_indexed_column - zero_indexed_word_start;
        }

        // dont show suggestions if the user has typed something, but hasn't used the trigger character
        if !self.always_show_suggestions && already_typed_count > 0 {
            let typed_character = usize::try_from(zero_indexed_word_start).ok().and_then(|index| query_units.get(index)).map(|unit| String::from_utf16_lossy(&[*unit]));
            if !typed_character.is_some_and(|character| self.trigger_characters.contains(&character)) {
                return Promise::resolved(Some(Rc::new(CompletionList { suggestions: Vec::new(), incomplete: None, dispose: None, duration: None })));
            }
        }

        let suggestions = (self.suggestion_provider.provide_results)(&query.to_string_lossy())
            .into_iter()
            .map(|result| {
                let (label, apply) = match result {
                    SuggestResult::Label(label) => (label, None),
                    SuggestResult::Item { label, apply } => (label, Some(apply)),
                };

                let mut item = CompletionItem {
                    insert_text: label.clone(),
                    range: CompletionItemRange::Single(Range::from_positions(position.delta(0, -already_typed_count), position)),
                    sort_text: Some((self.sort_key)(&label)),
                    kind: CompletionItemKind::Keyword,
                    label: CompletionLabel::Text(label),
                    tags: None,
                    detail: None,
                    documentation: None,
                    filter_text: None,
                    preselect: None,
                    insert_text_rules: None,
                    commit_characters: None,
                    additional_text_edits: None,
                    command: None,
                    action: None,
                    extension_id: None,
                    internal_id: None,
                };
                if let Some(apply) = apply {
                    apply(&mut item);
                }
                item
            })
            .collect();

        Promise::resolved(Some(Rc::new(CompletionList { suggestions, incomplete: None, dispose: None, duration: None })))
    }
}

/// `SuggestEnabledInput` (a parte sem DOM).
pub struct SuggestEnabledInput {
    base: Rc<Disposable>,
    input_model: Rc<TextModel>,
}

impl SuggestEnabledInput {
    /// `constructor(id, parent, suggestionProvider, ariaLabel, resourceHandle, options, ...)`: o `parent`, o `ariaLabel` e
    /// as opções de estilo são de DOM e ficam de fora; `value` é o `options.value`.
    pub fn new(
        id: &str,
        suggestion_provider: SuggestResultsProvider,
        resource_handle: &str,
        value: Option<&str>,
        language_features_service: &LanguageFeaturesService,
        model_service: &ModelService,
    ) -> Rc<Self> {
        let base = Disposable::new();

        let scope_handle = URI::parse(resource_handle, false);
        let input_model = model_service.create_model("", None, Some(&scope_handle), true);
        {
            let input_model = input_model.clone();
            base.register(to_disposable(move || input_model.dispose()));
        }

        let this = Rc::new(Self { base, input_model });

        let validated_provider = SuggestEnabledInputCompletionProvider {
            debug_display_name: format!("suggestEnabledInput/{id}"),
            scope_handle: scope_handle.clone(),
            sort_key: suggestion_provider.sort_key.clone().unwrap_or_else(|| Rc::new(|label: &str| label.to_string())),
            trigger_characters: suggestion_provider.trigger_characters.clone().unwrap_or_default(),
            word_definition: suggestion_provider.word_definition.as_ref().map(|word_definition| ensure_valid_word_definition(Some(word_definition))),
            always_show_suggestions: suggestion_provider.always_show_suggestions,
            suggestion_provider,
        };

        this.set_value(value.unwrap_or(""));

        let selector = LanguageSelector::Filter(LanguageFilter {
            scheme: Some(scope_handle.scheme().to_string_lossy()),
            has_access_to_all_models: true,
            ..LanguageFilter::default()
        });
        this.base.register(language_features_service.completion_provider.register(selector, Rc::new(validated_provider)));

        this
    }

    /// `inputWidget.getModel()`.
    pub fn input_model(&self) -> &Rc<TextModel> {
        &self.input_model
    }

    /// `setValue(val)`: os espaços em branco viram espaço simples e o conteúdo inteiro do modelo é trocado.
    pub fn set_value(&self, value: &str) {
        let cleansed: String = value.chars().map(|character| if character.is_whitespace() { ' ' } else { character }).collect();
        let full_range = self.input_model.get_full_model_range();
        self.input_model.push_edit_operations(None, &[EditOperation::replace(full_range, Some(JsString::from(cleansed.as_str())))], None, None, None);
    }

    /// `getValue()`.
    pub fn get_value(&self) -> String {
        self.input_model.get_value().to_string_lossy()
    }
}

impl IDisposable for SuggestEnabledInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}
