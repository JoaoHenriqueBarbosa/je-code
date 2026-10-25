//! Porte de `src/vs/editor/contrib/stickyScroll/browser/stickyScrollModelProvider.ts`: o provedor que calcula o
//! modelo de linhas fixas (`StickyModelProvider`) a partir do outline (símbolos), das dobras de sintaxe ou da
//! indentação, nessa ordem de preferência.
//!
//! Representação:
//! - `IStickyModelProvider` é o trait [`IStickyModelProvider`]; `IStickyModelCandidateProvider<T>` é o trait privado
//!   `IStickyModelCandidateProvider`, sem o parâmetro `T`, que só aparece no tipo do `modelPromise`. Esse tipo vira
//!   `ModelPromise`, uma alça apagada de tipo com o que o original faz com a promessa: cancelar e comparar a
//!   identidade (`this._modelPromise !== modelPromise`).
//! - `abstract class StickyModelCandidateProvider<T>` é a struct genérica `StickyModelCandidateProvider<S>` mais o
//!   trait `CandidateSource`, que reúne os métodos que as subclasses sobrescrevem (`isProviderValid`, `isModelValid`,
//!   `createModelFromProvider`, `createStickyModel`). O `_editor` protegido da base fica na base, e o modelo de texto
//!   que as subclasses leem com `this._editor.getModel()` é entregue por ela como argumento. O
//!   `this._stickyModel?.outlineProviderId`, que só o outline lê, chega como o argumento `previous` de
//!   `create_sticky_model`.
//! - `StickyModelFromCandidateFoldingProvider`, `StickyModelFromCandidateIndentationFoldingProvider` e
//!   `StickyModelFromCandidateSyntaxFoldingProvider` são uma [`FoldingSource`] só: a subclasse de indentação e a de
//!   sintaxe diferem apenas no `RangeProvider` que usam (`IndentRangeProvider` e o provedor de sintaxe, que troca o
//!   `SyntaxRangeProvider` quando os provedores de dobra mudam), e esse é o parâmetro de
//!   `folding_candidate_provider`.
//! - O `MutableDisposable<SyntaxRangeProvider>` é um `RefCell<Option<Rc<SyntaxRangeProvider>>>` que descarta o valor
//!   antigo ao trocar, porque o `SyntaxRangeProvider` do porte descarta por `RangeProvider::dispose`.
//! - Os `async`/`await` são cadeias de [`Promise`]: o laço sobre os provedores candidatos é `run_from`, recursivo no
//!   índice, e o `await` de um valor que não é promessa (o `Status` síncrono) cede uma microtarefa como no original.
//! - O `enum ModelProvider` do arquivo é o [`StickyScrollDefaultModel`] das opções (mesmas três cadeias).
//!
//! Divergências:
//! - **`@IInstantiationService`.** O parâmetro do construtor de `StickyModelProvider` que o original declara com o
//!   decorador errado (`@IInstantiationService _languageConfigurationService: ILanguageConfigurationService`) é o
//!   `ILanguageConfigurationService` que o nome diz.
//! - **`IActiveCodeEditor`.** O editor ativo sempre tem modelo. O construtor de [`StickyModelProvider`] entra em
//!   pânico sem modelo, e a base trata o editor sem modelo, no momento do cálculo, como o provedor inválido (no
//!   original o `getModel()` nulo lançaria dentro do `createCancelablePromise` e viraria `Status.CANCELED`).
//! - **`isProviderValid` da sintaxe.** O original compara o `MutableDisposable` (sempre definido) com `undefined`, o
//!   que dá sempre verdadeiro: é o valor padrão da base, sem sobrescrita.

use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::{Rc, Weak};

use je_base::async_::{Delayer, create_cancelable_promise};
use je_base::cancellation::CancellationToken;
use je_base::errors::on_unexpected_error;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, to_disposable};
use je_base::promise::{CancelablePromise, MaybePromise, Promise, Thenable};
use je_base::symbols::Delay;

use super::sticky_scroll_element::{StickyElement, StickyModel, StickyRange};
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::config::editor_options::sticky_scroll::{STICKY_SCROLL, StickyScrollDefaultModel};
use crate::folding::folding_controller::{FoldingController, RangesLimitReporter};
use crate::folding::folding_ranges::FoldingRegions;
use crate::folding::indent_range_provider::IndentRangeProvider;
use crate::folding::syntax_range_provider::SyntaxRangeProvider;
use crate::folding::{FoldingLimitReporter, RangeProvider};
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::language_features_service::LanguageFeaturesService;
use crate::multicursor::listen_weak;
use crate::outline_model::{Children, OutlineElement, OutlineModel, TreeElement};
use crate::text_model::TextModel;

/// `Status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Valid,
    Invalid,
    Canceled,
}

/// `IStickyModelProvider`.
pub trait IStickyModelProvider: IDisposable {
    /// Atualiza o modelo de linhas fixas e o devolve (`None` é o `null`).
    fn update(&self, token: &CancellationToken) -> Promise<Option<StickyModel>>;
}

/// Alça apagada de tipo da `CancelablePromise<T | null>` do `modelPromise`: cancelar e identidade.
#[derive(Clone)]
struct ModelPromise {
    identity: usize,
    cancel: Rc<dyn Fn()>,
}

impl ModelPromise {
    fn of<T: Clone + 'static>(promise: &CancelablePromise<T>) -> Self {
        let promise = promise.clone();
        Self { identity: promise.promise.identity(), cancel: Rc::new(move || promise.cancel()) }
    }

    fn cancel(&self) {
        (self.cancel)();
    }
}

/// `this._modelPromise !== modelPromise` negado: duas ausências são iguais, como `null === null`.
fn same_model_promise(a: &Option<ModelPromise>, b: &Option<ModelPromise>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => a.identity == b.identity,
        _ => false,
    }
}

/// `{ statusPromise, modelPromise }` de `computeStickyModel`.
struct ComputedStickyModel {
    status: MaybePromise<Status>,
    model_promise: Option<ModelPromise>,
}

/// `IStickyModelCandidateProvider<T>`.
trait IStickyModelCandidateProvider: IDisposable {
    /// `stickyModel`.
    fn sticky_model(&self) -> Option<StickyModel>;

    /// Calcula o modelo de linhas fixas e devolve o estado que diz se ele foi encontrado, junto da promessa do modelo
    /// do provedor.
    fn compute_sticky_model(&self, token: &CancellationToken) -> ComputedStickyModel;
}

struct ProviderState {
    model_providers: Vec<Box<dyn IStickyModelCandidateProvider>>,
    model_promise: RefCell<Option<ModelPromise>>,
    update_scheduler: Delayer<Option<StickyModel>>,
    update_operation: Rc<DisposableStore>,
}

impl ProviderState {
    /// `_cancelModelPromise()`.
    fn cancel_model_promise(&self) {
        let model_promise = self.model_promise.borrow_mut().take();
        if let Some(model_promise) = model_promise {
            model_promise.cancel();
        }
    }

    /// O corpo do `async` passado ao `_updateScheduler.trigger`, a partir do provedor candidato `index`.
    fn run_from(self: &Rc<Self>, index: usize, token: CancellationToken) -> Promise<Option<StickyModel>> {
        let Some(model_provider) = self.model_providers.get(index) else {
            return Promise::resolved(None);
        };
        let ComputedStickyModel { status, model_promise } = model_provider.compute_sticky_model(&token);
        *self.model_promise.borrow_mut() = model_promise.clone();
        let status_promise = match status {
            MaybePromise::Value(status) => Promise::resolved(status),
            MaybePromise::Promise(promise) => promise,
        };
        let this = Rc::clone(self);
        status_promise.then_promise(move |status| {
            if !same_model_promise(&this.model_promise.borrow(), &model_promise) {
                return Promise::resolved(None);
            }
            match status {
                Status::Canceled => {
                    this.update_operation.clear();
                    Promise::resolved(None)
                }
                Status::Valid => Promise::resolved(this.model_providers[index].sticky_model()),
                Status::Invalid => this.run_from(index + 1, token),
            }
        })
    }
}

/// `StickyModelProvider`.
pub struct StickyModelProvider {
    state: Rc<ProviderState>,
}

impl StickyModelProvider {
    /// `new StickyModelProvider(editor, onProviderUpdate, languageConfigurationService, languageFeaturesService)`.
    pub fn new(
        editor: &Rc<CodeEditorWidget>,
        on_provider_update: Rc<dyn Fn()>,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
        language_features_service: Rc<LanguageFeaturesService>,
    ) -> Self {
        let default_model = editor.get_options().get(STICKY_SCROLL).default_model;
        let mut model_providers: Vec<Box<dyn IStickyModelCandidateProvider>> = Vec::new();
        // Os `case` do `switch` caem um no outro: o outline cai na sintaxe, que cai na indentação.
        if default_model == StickyScrollDefaultModel::OutlineModel {
            model_providers.push(Box::new(StickyModelCandidateProvider::new(
                editor,
                OutlineSource { language_features_service: Rc::clone(&language_features_service) },
            )));
        }
        if matches!(default_model, StickyScrollDefaultModel::OutlineModel | StickyScrollDefaultModel::FoldingProviderModel) {
            model_providers.push(syntax_folding_candidate_provider(editor, on_provider_update, language_features_service));
        }
        model_providers.push(indentation_folding_candidate_provider(editor, language_configuration_service));

        Self {
            state: Rc::new(ProviderState {
                model_providers,
                model_promise: RefCell::new(None),
                update_scheduler: Delayer::new(Delay::Millis(300)),
                update_operation: DisposableStore::new(),
            }),
        }
    }
}

impl IDisposable for StickyModelProvider {
    fn dispose(&self) {
        let state = &self.state;
        for model_provider in &state.model_providers {
            model_provider.dispose();
        }
        state.update_operation.clear();
        state.cancel_model_promise();
        // `super.dispose()`: descarta o que foi registrado com `_register`.
        state.update_scheduler.dispose();
        state.update_operation.dispose();
    }
}

impl IStickyModelProvider for StickyModelProvider {
    fn update(&self, token: &CancellationToken) -> Promise<Option<StickyModel>> {
        let state = &self.state;
        state.update_operation.clear();
        let weak_state = Rc::downgrade(state);
        state.update_operation.add(to_disposable(move || {
            if let Some(state) = weak_state.upgrade() {
                state.cancel_model_promise();
                state.update_scheduler.cancel();
            }
        }));
        state.cancel_model_promise();

        let running = Rc::clone(state);
        let token = token.clone();
        state
            .update_scheduler
            .trigger(move || MaybePromise::Promise(running.run_from(0, token)), None)
            .then_result(|result| match result {
                Ok(sticky_model) => Ok(sticky_model),
                Err(error) => {
                    on_unexpected_error(&error);
                    Ok(None)
                }
            })
    }
}

/// O que as subclasses de `StickyModelCandidateProvider<T>` sobrescrevem (ver o topo do módulo).
trait CandidateSource: 'static {
    /// `T`.
    type Model: Clone + 'static;

    /// `isProviderValid()`: por padrão verdadeiro.
    fn is_provider_valid(&self) -> bool {
        true
    }

    /// `isModelValid(model)`: por padrão verdadeiro.
    fn is_model_valid(&self, _model: &Self::Model) -> bool {
        true
    }

    /// `createModelFromProvider(token)`.
    fn create_model_from_provider(&self, text_model: &Rc<TextModel>, token: &CancellationToken) -> Promise<Self::Model>;

    /// `createStickyModel(token, model)`.
    fn create_sticky_model(&self, text_model: &TextModel, model: &Self::Model, previous: Option<&StickyModel>) -> StickyModel;

    /// O descarte do que a subclasse registrou com `_register`.
    fn dispose(&self) {}
}

struct CandidateState<S: CandidateSource> {
    editor: Weak<CodeEditorWidget>,
    source: S,
    sticky_model: RefCell<Option<StickyModel>>,
}

impl<S: CandidateSource> CandidateState<S> {
    /// `this._editor.getModel()`.
    fn active_text_model(&self) -> Option<Rc<TextModel>> {
        self.editor.upgrade()?.get_model()
    }

    /// `_invalid()`.
    fn invalid(&self) -> Status {
        *self.sticky_model.borrow_mut() = None;
        Status::Invalid
    }

    /// O primeiro `then` de `computeStickyModel`.
    fn apply_provider_model(&self, provider_model: &S::Model, token: &CancellationToken) -> Status {
        if !self.source.is_model_valid(provider_model) {
            return self.invalid();
        }
        if token.is_cancellation_requested() {
            return Status::Canceled;
        }
        let Some(text_model) = self.active_text_model() else {
            return Status::Canceled;
        };
        let previous = self.sticky_model.borrow().clone();
        let sticky_model = self.source.create_sticky_model(&text_model, provider_model, previous.as_ref());
        *self.sticky_model.borrow_mut() = Some(sticky_model);
        Status::Valid
    }
}

/// `StickyModelCandidateProvider<T>`.
struct StickyModelCandidateProvider<S: CandidateSource> {
    state: Rc<CandidateState<S>>,
}

impl<S: CandidateSource> StickyModelCandidateProvider<S> {
    fn new(editor: &Rc<CodeEditorWidget>, source: S) -> Self {
        Self { state: Rc::new(CandidateState { editor: Rc::downgrade(editor), source, sticky_model: RefCell::new(None) }) }
    }
}

impl<S: CandidateSource> IDisposable for StickyModelCandidateProvider<S> {
    fn dispose(&self) {
        self.state.source.dispose();
    }
}

impl<S: CandidateSource> IStickyModelCandidateProvider for StickyModelCandidateProvider<S> {
    fn sticky_model(&self) -> Option<StickyModel> {
        self.state.sticky_model.borrow().clone()
    }

    fn compute_sticky_model(&self, token: &CancellationToken) -> ComputedStickyModel {
        let state = &self.state;
        let text_model = if token.is_cancellation_requested() || !state.source.is_provider_valid() {
            None
        } else {
            state.active_text_model()
        };
        let Some(text_model) = text_model else {
            return ComputedStickyModel { status: MaybePromise::Value(state.invalid()), model_promise: None };
        };

        let creating = Rc::clone(state);
        let provider_model_promise =
            create_cancelable_promise(move |token| creating.source.create_model_from_provider(&text_model, &token));

        let applying = Rc::clone(state);
        let token = token.clone();
        let status = provider_model_promise
            .promise
            .then(move |provider_model| applying.apply_provider_model(&provider_model, &token))
            .then_result(|result| match result {
                Ok(status) => Ok(status),
                Err(error) => {
                    on_unexpected_error(&error);
                    Ok(Status::Canceled)
                }
            });
        ComputedStickyModel { status: MaybePromise::Promise(status), model_promise: Some(ModelPromise::of(&provider_model_promise)) }
    }
}

/// `StickyModelFromCandidateOutlineProvider`.
struct OutlineSource {
    language_features_service: Rc<LanguageFeaturesService>,
}

impl CandidateSource for OutlineSource {
    type Model = Rc<OutlineModel>;

    fn create_model_from_provider(&self, text_model: &Rc<TextModel>, token: &CancellationToken) -> Promise<Rc<OutlineModel>> {
        OutlineModel::create(&self.language_features_service.document_symbol_provider, text_model, token)
    }

    fn create_sticky_model(&self, text_model: &TextModel, model: &Rc<OutlineModel>, previous: Option<&StickyModel>) -> StickyModel {
        let preferred_provider = previous.and_then(|previous| previous.outline_provider_id.clone());
        let (sticky_outline_element, provider_id) = sticky_model_from_outline_model(model, preferred_provider);
        StickyModel {
            uri: text_model.uri(),
            version: text_model.get_version_id(),
            element: Some(sticky_outline_element),
            outline_provider_id: provider_id,
        }
    }

    fn is_model_valid(&self, model: &Rc<OutlineModel>) -> bool {
        let children = model.children();
        let has_children = !children.borrow().is_empty();
        has_children
    }
}

/// Os valores de um mapa de filhos do outline, numa cópia.
fn tree_elements(children: &Children) -> Vec<TreeElement> {
    children.borrow().values().cloned().collect()
}

/// Os filhos de um elemento do outline, como elementos.
fn element_children(element: &OutlineElement) -> Vec<Rc<OutlineElement>> {
    element.children.borrow().values().filter_map(TreeElement::as_element).collect()
}

/// `_stickyModelFromOutlineModel(outlineModel, preferredProvider)`.
fn sticky_model_from_outline_model(outline_model: &OutlineModel, preferred_provider: Option<String>) -> (Rc<StickyElement>, Option<String>) {
    let mut preferred_provider = preferred_provider;
    let children = tree_elements(&outline_model.children());
    let first_is_group = matches!(children.first(), Some(TreeElement::Group(_)));

    let outline_elements: Vec<TreeElement> = if first_is_group {
        // When several possible outline providers
        let preferred = children
            .iter()
            .find(|group| preferred_provider.as_deref().is_some_and(|preferred| group.id() == preferred));
        match preferred {
            Some(provider) => tree_elements(&provider.children()),
            None => {
                let mut temp_id = String::new();
                let mut max_total_sum_of_ranges = -1_i64;
                let mut optimal_outline_group: Option<&TreeElement> = None;
                for outline_group in &children {
                    let total_sum_ranges = find_sum_of_ranges_of_group(outline_group);
                    if total_sum_ranges > max_total_sum_of_ranges {
                        optimal_outline_group = Some(outline_group);
                        max_total_sum_of_ranges = total_sum_ranges;
                        temp_id = outline_group.id();
                    }
                }
                preferred_provider = Some(temp_id);
                optimal_outline_group.map(|group| tree_elements(&group.children())).unwrap_or_default()
            }
        }
    } else {
        children
    };

    let mut outline_elements: Vec<Rc<OutlineElement>> = outline_elements.iter().filter_map(TreeElement::as_element).collect();
    outline_elements.sort_by(|element1, element2| {
        let range1 = StickyRange::new(element1.symbol.range.start_line_number, element1.symbol.range.end_line_number);
        let range2 = StickyRange::new(element2.symbol.range.start_line_number, element2.symbol.range.end_line_number);
        compare_sticky_ranges(range1, range2)
    });
    let sticky_children: Vec<Rc<StickyElement>> = outline_elements
        .iter()
        .map(|outline_element| sticky_model_from_outline_element(outline_element, outline_element.symbol.selection_range.start_line_number))
        .collect();
    (StickyElement::new(None, sticky_children, None), preferred_provider)
}

/// `_stickyModelFromOutlineElement(outlineElement, previousStartLine)`.
fn sticky_model_from_outline_element(outline_element: &OutlineElement, previous_start_line: i32) -> Rc<StickyElement> {
    let mut children: Vec<Rc<StickyElement>> = Vec::new();
    for child in element_children(outline_element) {
        let child_start_line = child.symbol.selection_range.start_line_number;
        if child_start_line != child.symbol.range.end_line_number {
            if child_start_line != previous_start_line {
                children.push(sticky_model_from_outline_element(&child, child_start_line));
            } else {
                for subchild in element_children(&child) {
                    children.push(sticky_model_from_outline_element(&subchild, child_start_line));
                }
            }
        }
    }
    children.sort_by(|child1, child2| compare_sticky_ranges(element_range(child1), element_range(child2)));
    let range = StickyRange::new(outline_element.symbol.selection_range.start_line_number, outline_element.symbol.range.end_line_number);
    StickyElement::new(Some(range), children, None)
}

/// O `range!` de um elemento criado a partir do outline, que sempre tem intervalo.
fn element_range(element: &StickyElement) -> StickyRange {
    element.range.expect("o elemento criado a partir do outline tem intervalo")
}

/// `_comparator(range1, range2)`: pela linha de início e, nas iguais, o intervalo maior primeiro.
fn compare_sticky_ranges(range1: StickyRange, range2: StickyRange) -> Ordering {
    range1.start_line_number.cmp(&range2.start_line_number).then(range2.end_line_number.cmp(&range1.end_line_number))
}

/// `_findSumOfRangesOfGroup(outline)`.
fn find_sum_of_ranges_of_group(outline: &TreeElement) -> i64 {
    let mut res: i64 = tree_elements(&outline.children()).iter().map(find_sum_of_ranges_of_group).sum();
    if let TreeElement::Element(element) = outline {
        res += i64::from(element.symbol.range.end_line_number) - i64::from(element.symbol.selection_range.start_line_number);
    }
    res
}

/// `StickyModelFromCandidateFoldingProvider` com o `RangeProvider` que a subclasse usa.
struct FoldingSource {
    limit_reporter: Rc<RangesLimitReporter>,
    provider: Rc<dyn RangeProvider>,
}

impl CandidateSource for FoldingSource {
    type Model = Option<Rc<FoldingRegions>>;

    fn create_model_from_provider(&self, _text_model: &Rc<TextModel>, token: &CancellationToken) -> Promise<Option<Rc<FoldingRegions>>> {
        self.provider.compute(token)
    }

    fn create_sticky_model(&self, text_model: &TextModel, model: &Option<Rc<FoldingRegions>>, _previous: Option<&StickyModel>) -> StickyModel {
        let folding_regions = model.as_ref().expect("o modelo de dobras foi validado por is_model_valid");
        StickyModel {
            uri: text_model.uri(),
            version: text_model.get_version_id(),
            element: Some(from_folding_regions(folding_regions)),
            outline_provider_id: None,
        }
    }

    fn is_model_valid(&self, model: &Option<Rc<FoldingRegions>>) -> bool {
        model.is_some()
    }

    fn dispose(&self) {
        self.provider.dispose();
        self.limit_reporter.dispose();
    }
}

/// `_fromFoldingRegions(foldingRegions)`.
fn from_folding_regions(folding_regions: &FoldingRegions) -> Rc<StickyElement> {
    let mut ordered_sticky_elements: Vec<Rc<StickyElement>> = Vec::new();

    // The root sticky outline element
    let sticky_outline_element = StickyElement::new(None, Vec::new(), None);

    for i in 0..folding_regions.length() {
        // Finding the parent index of the current range
        let parent_index = folding_regions.get_parent_index(i);

        let parent_node = if parent_index != -1 {
            // Access the reference of the parent node
            Rc::clone(&ordered_sticky_elements[parent_index as usize])
        } else {
            // In that case the parent node is the root node
            Rc::clone(&sticky_outline_element)
        };

        let child = StickyElement::new(
            Some(StickyRange::new(
                folding_regions.get_start_line_number(i) as i32,
                folding_regions.get_end_line_number(i) as i32 + 1,
            )),
            Vec::new(),
            Some(&parent_node),
        );
        parent_node.children.borrow_mut().push(Rc::clone(&child));
        ordered_sticky_elements.push(child);
    }
    sticky_outline_element
}

/// O construtor de `StickyModelFromCandidateFoldingProvider`: o relator de limite do editor e o `RangeProvider` que a
/// subclasse monta com ele.
fn folding_candidate_provider(
    editor: &Rc<CodeEditorWidget>,
    make_provider: impl FnOnce(Rc<dyn FoldingLimitReporter>) -> Rc<dyn RangeProvider>,
) -> Box<dyn IStickyModelCandidateProvider> {
    let limit_reporter = RangesLimitReporter::new(editor);
    let limit: Rc<dyn FoldingLimitReporter> = limit_reporter.clone();
    let provider = make_provider(limit);
    Box::new(StickyModelCandidateProvider::new(editor, FoldingSource { limit_reporter, provider }))
}

/// `StickyModelFromCandidateIndentationFoldingProvider`.
fn indentation_folding_candidate_provider(
    editor: &Rc<CodeEditorWidget>,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
) -> Box<dyn IStickyModelCandidateProvider> {
    folding_candidate_provider(editor, |limit| {
        let text_model = editor.get_model().expect("o editor ativo tem modelo");
        let provider: Rc<dyn RangeProvider> = Rc::new(IndentRangeProvider::new(text_model, language_configuration_service, limit));
        provider
    })
}

/// `StickyModelFromCandidateSyntaxFoldingProvider`.
fn syntax_folding_candidate_provider(
    editor: &Rc<CodeEditorWidget>,
    on_provider_update: Rc<dyn Fn()>,
    language_features_service: Rc<LanguageFeaturesService>,
) -> Box<dyn IStickyModelCandidateProvider> {
    folding_candidate_provider(editor, |limit| {
        let provider: Rc<dyn RangeProvider> = SyntaxProviderSlot::new(editor, language_features_service, on_provider_update, limit);
        provider
    })
}

/// O `provider: MutableDisposable<SyntaxRangeProvider>` de `StickyModelFromCandidateSyntaxFoldingProvider`, com a
/// escuta de `foldingRangeProvider.onDidChange` e o `compute` que a subclasse sobrescreve.
struct SyntaxProviderSlot {
    editor: Weak<CodeEditorWidget>,
    language_features_service: Rc<LanguageFeaturesService>,
    on_provider_update: Rc<dyn Fn()>,
    limit: Rc<dyn FoldingLimitReporter>,
    provider: RefCell<Option<Rc<SyntaxRangeProvider>>>,
    listener: RefCell<Option<DisposableRef>>,
}

impl SyntaxProviderSlot {
    fn new(
        editor: &Rc<CodeEditorWidget>,
        language_features_service: Rc<LanguageFeaturesService>,
        on_provider_update: Rc<dyn Fn()>,
        limit: Rc<dyn FoldingLimitReporter>,
    ) -> Rc<Self> {
        let slot = Rc::new(Self {
            editor: Rc::downgrade(editor),
            language_features_service,
            on_provider_update,
            limit,
            provider: RefCell::new(None),
            listener: RefCell::new(None),
        });
        let listener = listen_weak(
            &Rc::downgrade(&slot),
            slot.language_features_service.folding_range_provider.on_did_change(),
            |this: &SyntaxProviderSlot, _| this.update_provider(),
        );
        *slot.listener.borrow_mut() = Some(listener);
        slot.update_provider();
        slot
    }

    /// `_updateProvider(editor, onProviderUpdate)`.
    fn update_provider(&self) {
        let Some(text_model) = self.editor.upgrade().and_then(|editor| editor.get_model()) else {
            return;
        };
        let selected_providers = FoldingController::get_folding_range_providers(&self.language_features_service, &text_model);
        if selected_providers.is_empty() {
            return;
        }
        let provider = Rc::new(SyntaxRangeProvider::new(
            text_model,
            selected_providers,
            Rc::clone(&self.on_provider_update),
            Rc::clone(&self.limit),
            None,
        ));
        let previous = self.provider.borrow_mut().replace(provider);
        if let Some(previous) = previous {
            previous.dispose();
        }
    }
}

impl RangeProvider for SyntaxProviderSlot {
    fn id(&self) -> &str {
        "syntax"
    }

    /// `this.provider.value?.compute(token) ?? null`.
    fn compute(&self, token: &CancellationToken) -> Promise<Option<Rc<FoldingRegions>>> {
        let provider = self.provider.borrow().clone();
        match provider {
            Some(provider) => provider.compute(token),
            None => Promise::resolved(None),
        }
    }

    fn dispose(&self) {
        let listener = self.listener.borrow_mut().take();
        if let Some(listener) = listener {
            listener.dispose();
        }
        let provider = self.provider.borrow_mut().take();
        if let Some(provider) = provider {
            provider.dispose();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_are_ordered_by_start_and_then_by_larger_range_first() {
        let outer = StickyRange::new(1, 10);
        let inner = StickyRange::new(1, 5);
        let later = StickyRange::new(2, 3);
        assert_eq!(compare_sticky_ranges(outer, inner), Ordering::Less);
        assert_eq!(compare_sticky_ranges(inner, outer), Ordering::Greater);
        assert_eq!(compare_sticky_ranges(inner, later), Ordering::Less);
        assert_eq!(compare_sticky_ranges(outer, outer), Ordering::Equal);
    }

    #[test]
    fn folding_regions_become_a_tree_by_parent_index() {
        let regions = FoldingRegions::new(vec![1, 2, 8], vec![6, 4, 9], None);
        let root = from_folding_regions(&regions);
        assert_eq!(root.range, None);

        let top = root.children.borrow().clone();
        assert_eq!(top.len(), 2);
        assert_eq!(top[0].range, Some(StickyRange::new(1, 7)));
        assert_eq!(top[1].range, Some(StickyRange::new(8, 10)));

        let nested = top[0].children.borrow().clone();
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].range, Some(StickyRange::new(2, 5)));
        assert!(nested[0].parent().is_some_and(|parent| Rc::ptr_eq(&parent, &top[0])));
        assert!(top[1].parent().is_some_and(|parent| Rc::ptr_eq(&parent, &root)));
    }

    #[test]
    fn model_promises_are_equal_when_both_are_absent_or_the_same() {
        let first = create_cancelable_promise(|_| Promise::<i32>::resolved(1));
        let second = create_cancelable_promise(|_| Promise::<i32>::resolved(1));
        let first_handle = Some(ModelPromise::of(&first));
        let second_handle = Some(ModelPromise::of(&second));
        assert!(same_model_promise(&None, &None));
        assert!(same_model_promise(&first_handle, &first_handle.clone()));
        assert!(!same_model_promise(&first_handle, &second_handle));
        assert!(!same_model_promise(&first_handle, &None));
    }
}
