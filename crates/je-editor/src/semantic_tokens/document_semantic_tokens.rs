//! Porte de `src/vs/editor/contrib/semanticTokens/browser/documentSemanticTokens.ts`: a contribuição que pede os tokens
//! semânticos do documento aos provedores e os entrega ao modelo (`DocumentSemanticTokensFeature` e o
//! `ModelSemanticColoring` de cada modelo).
//!
//! Representação:
//! - Os serviços injetados (`@IModelService`, `@IThemeService`...) são os `Rc` dos contratos do porte, passados ao
//!   [`DocumentSemanticTokensFeature::new`]; o `ILanguageFeaturesService` é o [`LanguageFeaturesService`] (só o registro
//!   `documentSemanticTokensProvider` é lido). O `ModelSemanticColoring` recebe os mesmos serviços direto, no lugar do
//!   `instantiationService`.
//! - O estado compartilhado da contribuição (o `_watchers`, os serviços e as operações `register`, `deregister` e
//!   `handleSettingOrThemeChange`, que no original são fechos do construtor) é um [`FeatureContext`], que os ouvintes
//!   capturam; assim nenhum ouvinte guarda a própria contribuição.
//! - `Set<DocumentSemanticTokensProvider>` é um `Vec` comparado pela identidade do provedor.
//! - O `then(onFulfilled, onRejected)` do pedido é um `then_result`; o fecho guarda o `ModelSemanticColoring` (forte), como o
//!   fecho do original guarda o `this`: um pedido que termina depois do `dispose` ainda libera os tokens que chegaram.
//! - `new StopWatch(false)` é o `StopWatch` de `Date.now()` (o relógio do laço, que os testes controlam).
//! - O atraso do `RunOnceScheduler` é inteiro (`u64`): o do `debounceInformation.get(model)`, que é fracionário, é
//!   truncado.
//! - Um provedor que lança de dentro do `releaseDocumentSemanticTokens` (pânico de [`errors::throw`]) atravessa o
//!   `_setDocumentSemanticTokens` como a exceção do original, e o `try/catch` da troca de tema o captura.
//! - `registerEditorFeature(DocumentSemanticTokensFeature)` não existe: o registro de contribuições do editor ainda não foi
//!   portado, e a contribuição é criada por quem a usa.
//! - [`DocumentSemanticTokensFeature::watcher_count_for_testing`] é só para teste (o `sinon.spy` do original sobre o
//!   `onDidChangeContent` do modelo não tem equivalente).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::RunOnceScheduler;
use je_base::cancellation::CancellationTokenSource;
use je_base::errors;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable};
use je_base::map::ResourceMap;
use je_base::stopwatch::StopWatch;
use je_platform::configuration::service::IConfigurationService;

use crate::language_feature_debounce::{DebounceConfig, IFeatureDebounceInformation, ILanguageFeatureDebounceService};
use crate::language_feature_registry::{AnyLanguageFeatureRegistry, LanguageFeatureRegistry};
use crate::language_features_service::{DocumentSemanticTokensProvider, LanguageFeaturesService, SemanticTokens, SemanticTokensResult};
use crate::semantic_tokens::get_semantic_tokens::{get_document_semantic_tokens, has_document_semantic_tokens_provider};
use crate::semantic_tokens::semantic_tokens_config::{SEMANTIC_HIGHLIGHTING_SETTING_ID, is_semantic_coloring_enabled};
use crate::semantic_tokens_provider_styling::{SemanticTokensProviderStyling, to_multiline_tokens2};
use crate::semantic_tokens_styling::{DocumentTokensProvider, ISemanticTokensStylingService};
use crate::services::theme_service::IThemeService;
use crate::services_model::IModelService;
use crate::text_model::TextModel;
use crate::text_model_events::ModelContentChangedEvent;

type ProviderRef = Rc<dyn DocumentSemanticTokensProvider>;
type Registry = Rc<LanguageFeatureRegistry<dyn DocumentSemanticTokensProvider>>;

/// O estado compartilhado da contribuição (ver as notas do módulo).
struct FeatureContext {
    watchers: RefCell<ResourceMap<Rc<ModelSemanticColoring>>>,
    provider: Registry,
    semantic_tokens_styling_service: Rc<dyn ISemanticTokensStylingService>,
    model_service: Rc<dyn IModelService>,
    theme_service: Rc<dyn IThemeService>,
    configuration_service: Rc<dyn IConfigurationService>,
    language_feature_debounce_service: Rc<dyn ILanguageFeatureDebounceService>,
}

impl FeatureContext {
    fn is_enabled(&self, model: &TextModel) -> bool {
        is_semantic_coloring_enabled(model, &*self.theme_service, &*self.configuration_service)
    }

    fn watcher_of(&self, model: &TextModel) -> Option<Rc<ModelSemanticColoring>> {
        self.watchers.borrow().get(&model.uri()).cloned()
    }

    fn all_watchers(&self) -> Vec<Rc<ModelSemanticColoring>> {
        self.watchers.borrow().values().cloned().collect()
    }

    /// `register(model)`.
    fn register(&self, model: &Rc<TextModel>) {
        if let Some(previous) = self.watcher_of(model) {
            previous.dispose();
        }
        let watcher = ModelSemanticColoring::new(
            Rc::clone(model),
            Rc::clone(&self.semantic_tokens_styling_service),
            &*self.language_feature_debounce_service,
            Rc::clone(&self.provider),
        );
        self.watchers.borrow_mut().set(model.uri(), watcher);
    }

    /// `deregister(model, modelSemanticColoring)`.
    fn deregister(&self, model: &TextModel, model_semantic_coloring: &ModelSemanticColoring) {
        model_semantic_coloring.dispose();
        self.watchers.borrow_mut().delete(&model.uri());
    }

    /// `handleSettingOrThemeChange(themeChanged)`.
    fn handle_setting_or_theme_change(&self, theme_changed: bool) {
        for model in self.model_service.get_models() {
            // Updating tokens can synchronously dispose another model in this snapshot.
            if model.is_disposed() {
                continue;
            }
            let curr = self.watcher_of(&model);
            if self.is_enabled(&model) {
                match curr {
                    None => self.register(&model),
                    Some(curr) if theme_changed => {
                        if let Err(err) = errors::catch(|| curr.handle_theme_change()) {
                            errors::on_unexpected_error(&err);
                        }
                    }
                    Some(_) => {}
                }
            } else if let Some(curr) = curr {
                self.deregister(&model, &curr);
            }
        }
    }

    /// `bindProviderChangeListeners()`.
    fn bind_provider_change_listeners(self: &Rc<Self>, provider_change_listeners: &DisposableStore) {
        provider_change_listeners.clear();
        for p in self.provider.all_no_model() {
            if let Some(on_did_change) = p.on_did_change() {
                let context = Rc::clone(self);
                let changed_provider = Rc::clone(&p);
                provider_change_listeners.add(on_did_change.listen(move |_| {
                    for watcher in context.all_watchers() {
                        watcher.handle_provider_did_change(&changed_provider);
                    }
                }));
            }
        }
    }
}

/// `DocumentSemanticTokensFeature`.
pub struct DocumentSemanticTokensFeature {
    base: Rc<Disposable>,
    context: Rc<FeatureContext>,
}

impl DocumentSemanticTokensFeature {
    pub fn new(
        semantic_tokens_styling_service: Rc<dyn ISemanticTokensStylingService>,
        model_service: Rc<dyn IModelService>,
        theme_service: Rc<dyn IThemeService>,
        configuration_service: Rc<dyn IConfigurationService>,
        language_feature_debounce_service: Rc<dyn ILanguageFeatureDebounceService>,
        language_features_service: &LanguageFeaturesService,
    ) -> Rc<Self> {
        let provider = Rc::clone(&language_features_service.document_semantic_tokens_provider);
        let base = Disposable::new();
        let provider_change_listeners = DisposableStore::new();
        base.register(provider_change_listeners.clone());

        let context = Rc::new(FeatureContext {
            watchers: RefCell::new(ResourceMap::new()),
            provider: Rc::clone(&provider),
            semantic_tokens_styling_service,
            model_service: Rc::clone(&model_service),
            theme_service: Rc::clone(&theme_service),
            configuration_service: Rc::clone(&configuration_service),
            language_feature_debounce_service,
        });

        for model in model_service.get_models() {
            if context.is_enabled(&model) {
                context.register(&model);
            }
        }
        let on_added = Rc::clone(&context);
        base.register(model_service.on_model_added().listen(move |model| {
            if on_added.is_enabled(model) {
                on_added.register(model);
            }
        }));
        let on_removed = Rc::clone(&context);
        base.register(model_service.on_model_removed().listen(move |model| {
            if let Some(curr) = on_removed.watcher_of(model) {
                on_removed.deregister(model, &curr);
            }
        }));
        let on_configuration = Rc::clone(&context);
        base.register(configuration_service.on_did_change_configuration().listen(move |e| {
            if e.affects_configuration(SEMANTIC_HIGHLIGHTING_SETTING_ID, None) {
                on_configuration.handle_setting_or_theme_change(false);
            }
        }));
        let on_theme = Rc::clone(&context);
        base.register(theme_service.on_did_color_theme_change().listen(move |_| on_theme.handle_setting_or_theme_change(true)));
        context.bind_provider_change_listeners(&provider_change_listeners);
        let on_registry = Rc::clone(&context);
        base.register(provider.on_did_change().listen(move |_| {
            on_registry.bind_provider_change_listeners(&provider_change_listeners);
            for watcher in on_registry.all_watchers() {
                watcher.handle_registry_change();
            }
        }));

        Rc::new(Self { base, context })
    }

    /// Só para teste: quantos modelos têm um `ModelSemanticColoring` registrado.
    #[doc(hidden)]
    pub fn watcher_count_for_testing(&self) -> usize {
        self.context.watchers.borrow().size()
    }
}

impl IDisposable for DocumentSemanticTokensFeature {
    fn dispose(&self) {
        let watchers = self.context.all_watchers();
        for watcher in &watchers {
            watcher.dispose();
        }
        self.context.watchers.borrow_mut().clear();

        IDisposable::dispose(&*self.base);
    }
}

/// `ModelSemanticColoring.REQUEST_MIN_DELAY`.
const REQUEST_MIN_DELAY: u64 = 300;
/// `ModelSemanticColoring.REQUEST_MAX_DELAY`.
const REQUEST_MAX_DELAY: u64 = 2000;

/// `ModelSemanticColoring`.
struct ModelSemanticColoring {
    me: Weak<ModelSemanticColoring>,
    base: Rc<Disposable>,
    is_disposed: Cell<bool>,
    model: Rc<TextModel>,
    provider: Registry,
    semantic_tokens_styling_service: Rc<dyn ISemanticTokensStylingService>,
    debounce_information: Rc<dyn IFeatureDebounceInformation>,
    fetch_document_semantic_tokens: Rc<RunOnceScheduler>,
    current_document_response: RefCell<Option<Rc<SemanticTokensResponse>>>,
    current_document_request_cancellation_token_source: RefCell<Option<CancellationTokenSource>>,
    relevant_providers: RefCell<Vec<ProviderRef>>,
    providers_changed_during_request: Cell<bool>,
}

fn same_provider(a: &ProviderRef, b: &ProviderRef) -> bool {
    std::ptr::addr_eq(Rc::as_ptr(a), Rc::as_ptr(b))
}

impl ModelSemanticColoring {
    fn new(
        model: Rc<TextModel>,
        semantic_tokens_styling_service: Rc<dyn ISemanticTokensStylingService>,
        language_feature_debounce_service: &dyn ILanguageFeatureDebounceService,
        provider: Registry,
    ) -> Rc<Self> {
        let debounce_information = language_feature_debounce_service.for_feature(
            Rc::clone(&provider) as Rc<dyn AnyLanguageFeatureRegistry>,
            "DocumentSemanticTokens",
            Some(DebounceConfig { min: Some(REQUEST_MIN_DELAY as f64), max: Some(REQUEST_MAX_DELAY as f64), key: None }),
        );
        let this = Rc::new_cyclic(|me: &Weak<ModelSemanticColoring>| {
            let base = Disposable::new();
            let runner_target = me.clone();
            let fetch_document_semantic_tokens = RunOnceScheduler::new(
                move || {
                    if let Some(coloring) = runner_target.upgrade() {
                        coloring.fetch_document_semantic_tokens_now();
                    }
                },
                REQUEST_MIN_DELAY,
            );
            base.register(fetch_document_semantic_tokens.clone());
            ModelSemanticColoring {
                me: me.clone(),
                base,
                is_disposed: Cell::new(false),
                model,
                provider,
                semantic_tokens_styling_service,
                debounce_information,
                fetch_document_semantic_tokens,
                current_document_response: RefCell::new(None),
                current_document_request_cancellation_token_source: RefCell::new(None),
                relevant_providers: RefCell::new(Vec::new()),
                providers_changed_during_request: Cell::new(false),
            }
        });
        this.update_relevant_providers();

        let on_content = Rc::downgrade(&this);
        this.base.register(this.model.on_did_change_content().listen(move |_| {
            if let Some(coloring) = on_content.upgrade() {
                coloring.schedule_debounced_fetch_if_idle();
            }
        }));
        let on_attached = Rc::downgrade(&this);
        this.base.register(this.model.on_did_change_attached().listen(move |_| {
            if let Some(coloring) = on_attached.upgrade() {
                coloring.schedule_debounced_fetch_if_idle();
            }
        }));
        let on_language = Rc::downgrade(&this);
        this.base.register(this.model.on_did_change_language().listen(move |_| {
            if let Some(coloring) = on_language.upgrade() {
                coloring.handle_language_change();
            }
        }));

        this.fetch_document_semantic_tokens.schedule(Some(0));
        this
    }

    /// O atraso do `debounceInformation.get(model)`.
    fn debounce_delay(&self) -> Option<u64> {
        Some(self.debounce_information.get(&self.model) as u64)
    }

    fn schedule_debounced_fetch_if_idle(&self) {
        if !self.fetch_document_semantic_tokens.is_scheduled() {
            self.fetch_document_semantic_tokens.schedule(self.debounce_delay());
        }
    }

    fn handle_language_change(&self) {
        // clear any outstanding state
        let response = self.current_document_response.borrow().clone();
        if let Some(response) = response {
            response.dispose();
            *self.current_document_response.borrow_mut() = None;
        }
        let cancellation_token_source = self.current_document_request_cancellation_token_source.borrow_mut().take();
        if let Some(cancellation_token_source) = cancellation_token_source {
            cancellation_token_source.cancel();
        }
        self.set_document_semantic_tokens(None, None, None, &[]);
        self.update_relevant_providers();
        self.fetch_document_semantic_tokens.schedule(Some(0));
    }

    /// `handleThemeChange()`.
    pub fn handle_theme_change(&self) {
        self.set_document_semantic_tokens(None, None, None, &[]);
        self.fetch_document_semantic_tokens.schedule(self.debounce_delay());
    }

    /// `handleRegistryChange()`.
    pub fn handle_registry_change(&self) {
        self.update_relevant_providers();
        self.fetch_document_semantic_tokens.schedule(self.debounce_delay());
    }

    /// `handleProviderDidChange(provider)`.
    pub fn handle_provider_did_change(&self, provider: &ProviderRef) {
        if !self.relevant_providers.borrow().iter().any(|relevant| same_provider(relevant, provider)) {
            return;
        }
        if self.current_document_request_cancellation_token_source.borrow().is_some() {
            // there is already a request running,
            self.providers_changed_during_request.set(true);
            return;
        }
        self.fetch_document_semantic_tokens.schedule(Some(0));
    }

    fn update_relevant_providers(&self) {
        *self.relevant_providers.borrow_mut() = self.provider.all(&self.model);
    }

    fn fetch_document_semantic_tokens_now(&self) {
        if self.current_document_request_cancellation_token_source.borrow().is_some() {
            // there is already a request running, let it finish...
            return;
        }

        if !has_document_semantic_tokens_provider(&self.provider, &self.model) {
            // there is no provider
            if self.current_document_response.borrow().is_some() {
                // there are semantic tokens set
                self.model.tokenization().set_semantic_tokens(None, false);
            }
            return;
        }

        if !self.model.is_attached_to_editor() {
            // this document is not visible, there is no need to fetch semantic tokens for it
            return;
        }

        let cancellation_token_source = CancellationTokenSource::new(None);
        let (last_provider, last_result_id) = match &*self.current_document_response.borrow() {
            Some(response) => (Some(Rc::clone(&response.provider)), response.result_id.clone().filter(|id| !id.is_empty())),
            None => (None, None),
        };
        let request =
            get_document_semantic_tokens(&self.provider, &self.model, last_provider.as_ref(), last_result_id.as_deref(), &cancellation_token_source.token());
        *self.current_document_request_cancellation_token_source.borrow_mut() = Some(cancellation_token_source);
        self.providers_changed_during_request.set(false);

        let pending_changes: Rc<RefCell<Vec<ModelContentChangedEvent>>> = Rc::default();
        let collected_changes = Rc::clone(&pending_changes);
        let content_change_listener = self.model.on_did_change_content().listen(move |e| collected_changes.borrow_mut().push(e.clone()));

        let sw = StopWatch::new(Some(false));
        let coloring = self.me.upgrade().expect("o ModelSemanticColoring está vivo enquanto roda um método seu");
        let _ = request.then_result(move |result| {
            let pending_changes = std::mem::take(&mut *pending_changes.borrow_mut());
            match result {
                Ok(res) => {
                    coloring.debounce_information.update(&coloring.model, sw.elapsed());
                    *coloring.current_document_request_cancellation_token_source.borrow_mut() = None;
                    content_change_listener.dispose();

                    match res {
                        None => coloring.set_document_semantic_tokens(None, None, None, &pending_changes),
                        Some(res) => {
                            let styling = coloring.semantic_tokens_styling_service.get_styling(&DocumentTokensProvider::Document(Rc::clone(&res.provider)));
                            coloring.set_document_semantic_tokens(Some(res.provider), res.tokens, Some(styling), &pending_changes);
                        }
                    }
                }
                Err(err) => {
                    let is_expected_error = errors::is_cancellation_error(&err) || err.message.contains("busy");
                    if !is_expected_error {
                        errors::on_unexpected_error(&err);
                    }

                    // Semantic tokens eats up all errors and considers errors to mean that the result is temporarily not available
                    // The API does not have a special error kind to express this...
                    *coloring.current_document_request_cancellation_token_source.borrow_mut() = None;
                    content_change_listener.dispose();

                    if !pending_changes.is_empty() || coloring.providers_changed_during_request.get() {
                        // More changes occurred while the request was running
                        coloring.schedule_debounced_fetch_if_idle();
                    }
                }
            }
            Ok(())
        });
    }

    /// `_copy(src, srcOffset, dest, destOffset, length)`: copia sem estourar nenhum dos dois vetores. Um deslocamento fora
    /// do vetor de origem lê `undefined` (zero no `Uint32Array`) e um fora do de destino não escreve nada, como o
    /// `Uint32Array` do original.
    fn copy(src: &[u32], src_offset: i64, dest: &mut [u32], dest_offset: i64, length: i64) {
        // protect against overflows
        let length = length.min(dest.len() as i64 - dest_offset).min(src.len() as i64 - src_offset);
        for i in 0..length.max(0) {
            let dest_index = dest_offset + i;
            if dest_index < 0 || dest_index >= dest.len() as i64 {
                continue;
            }
            let src_index = src_offset + i;
            dest[dest_index as usize] = if src_index < 0 { 0 } else { src.get(src_index as usize).copied().unwrap_or(0) };
        }
    }

    fn set_document_semantic_tokens(
        &self,
        provider: Option<ProviderRef>,
        tokens: Option<SemanticTokensResult>,
        styling: Option<Rc<SemanticTokensProviderStyling>>,
        pending_changes: &[ModelContentChangedEvent],
    ) {
        let current_response = self.current_document_response.borrow().clone();
        let reschedule_if_needed = || {
            if (!pending_changes.is_empty() || self.providers_changed_during_request.get()) && !self.fetch_document_semantic_tokens.is_scheduled() {
                self.fetch_document_semantic_tokens.schedule(self.debounce_delay());
            }
        };

        if let Some(response) = &current_response {
            response.dispose();
            *self.current_document_response.borrow_mut() = None;
        }
        if self.is_disposed.get() {
            // disposed!
            if let (Some(provider), Some(tokens)) = (&provider, &tokens) {
                provider.release_document_semantic_tokens(result_id_of(tokens).as_deref());
            }
            return;
        }
        let (Some(provider), Some(styling)) = (provider, styling) else {
            self.model.tokenization().set_semantic_tokens(None, false);
            return;
        };
        let Some(tokens) = tokens else {
            self.model.tokenization().set_semantic_tokens(None, true);
            reschedule_if_needed();
            return;
        };

        // O `tokens = { resultId, data }` do original, que troca as edições pelo resultado completo.
        let mut applied_edits: Option<SemanticTokensResult> = None;
        if let SemanticTokensResult::Edits(edits) = &tokens {
            let result_id = edits.result_id.clone();
            let reject_invalid_edits = |request_full_refresh: bool| {
                provider.release_document_semantic_tokens(result_id.as_deref());
                self.model.tokenization().set_semantic_tokens(None, true);
                if request_full_refresh {
                    self.fetch_document_semantic_tokens.schedule(Some(0));
                }
            };
            let Some(current_response) = current_response else {
                // not possible!
                reject_invalid_edits(false);
                return;
            };
            if edits.edits.is_empty() {
                // nothing to do!
                applied_edits = Some(SemanticTokensResult::Tokens(SemanticTokens { result_id: edits.result_id.clone(), data: current_response.data.clone() }));
            } else {
                let mut delta_length: i64 = 0;
                for edit in &edits.edits {
                    delta_length += edit.data.as_ref().map_or(0, |data| data.len() as i64) - i64::from(edit.delete_count);
                }

                let src_data = &current_response.data;
                let dest_data_length = src_data.len() as i64 + delta_length;
                if dest_data_length < 0 {
                    styling.warn_invalid_edit_delete_count(current_response.result_id.as_deref(), result_id.as_deref(), src_data.len(), delta_length);
                    reject_invalid_edits(true);
                    return;
                }
                let mut dest_data: Vec<u32> = vec![0; dest_data_length as usize];

                let mut src_last_start = src_data.len() as i64;
                let mut dest_last_start = dest_data.len() as i64;
                for (i, edit) in edits.edits.iter().enumerate().rev() {
                    if edit.start as usize > src_data.len() {
                        styling.warn_invalid_edit_start(current_response.result_id.as_deref(), result_id.as_deref(), i, edit.start, src_data.len());
                        reject_invalid_edits(true);
                        return;
                    }

                    let copy_count = src_last_start - (i64::from(edit.start) + i64::from(edit.delete_count));
                    if copy_count > 0 {
                        Self::copy(src_data, src_last_start - copy_count, &mut dest_data, dest_last_start - copy_count, copy_count);
                        dest_last_start -= copy_count;
                    }

                    if let Some(data) = &edit.data {
                        Self::copy(data, 0, &mut dest_data, dest_last_start - data.len() as i64, data.len() as i64);
                        dest_last_start -= data.len() as i64;
                    }

                    src_last_start = i64::from(edit.start);
                }

                if src_last_start > 0 {
                    Self::copy(src_data, 0, &mut dest_data, 0, src_last_start);
                }

                applied_edits = Some(SemanticTokensResult::Tokens(SemanticTokens { result_id: result_id.clone(), data: dest_data }));
            }
        }
        let tokens = applied_edits.unwrap_or(tokens);

        if let SemanticTokensResult::Tokens(tokens) = &tokens {
            *self.current_document_response.borrow_mut() =
                Some(Rc::new(SemanticTokensResponse { provider: Rc::clone(&provider), result_id: tokens.result_id.clone(), data: tokens.data.clone() }));

            let mut result = to_multiline_tokens2(tokens, &styling, &self.model.get_language_id());

            // Adjust incoming semantic tokens
            if !pending_changes.is_empty() {
                // More changes occurred while the request was running
                // We need to:
                // 1. Adjust incoming semantic tokens
                // 2. Request them again
                for change in pending_changes {
                    for area in &mut result {
                        for single_change in &change.changes {
                            area.apply_edit(&single_change.range, single_change.text.units());
                        }
                    }
                }
            }

            self.model.tokenization().set_semantic_tokens(Some(result), true);
        } else {
            self.model.tokenization().set_semantic_tokens(None, true);
        }

        reschedule_if_needed();
    }
}

impl IDisposable for ModelSemanticColoring {
    fn dispose(&self) {
        let response = self.current_document_response.borrow().clone();
        if let Some(response) = response {
            response.dispose();
            *self.current_document_response.borrow_mut() = None;
        }
        let cancellation_token_source = self.current_document_request_cancellation_token_source.borrow_mut().take();
        if let Some(cancellation_token_source) = cancellation_token_source {
            cancellation_token_source.cancel();
        }
        self.set_document_semantic_tokens(None, None, None, &[]);
        self.is_disposed.set(true);

        IDisposable::dispose(&*self.base);
    }
}

/// O `resultId` de `SemanticTokens | SemanticTokensEdits`.
fn result_id_of(tokens: &SemanticTokensResult) -> Option<String> {
    match tokens {
        SemanticTokensResult::Tokens(tokens) => tokens.result_id.clone(),
        SemanticTokensResult::Edits(edits) => edits.result_id.clone(),
    }
}

/// `SemanticTokensResponse`.
struct SemanticTokensResponse {
    provider: ProviderRef,
    result_id: Option<String>,
    data: Vec<u32>,
}

impl SemanticTokensResponse {
    fn dispose(&self) {
        self.provider.release_document_semantic_tokens(self.result_id.as_deref());
    }
}
