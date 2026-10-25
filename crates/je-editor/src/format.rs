//! Porte de `src/vs/editor/contrib/format/browser/format.ts`: os formatadores sintéticos, o seletor de conflitos
//! (`FormattingConflicts`), as funções que formatam documento e intervalos com um provedor e as consultas que só
//! pedem edições (`getDocumentRangeFormattingEditsUntilResult`, `getDocumentFormattingEditsUntilResult`,
//! `getDocumentFormattingEditsWithSelectedProvider` e `getOnTypeFormattingEdits`).
//!
//! Decisões de tradução:
//! - O formatador sintético de `getRealAndSyntheticDocumentFormattersOrdered` (o objeto literal que embrulha um
//!   formatador de intervalo) é o [`SyntheticDocumentFormatter`]; `displayName` e `extensionId` vêm do formatador de
//!   intervalo.
//! - `IEditorWorkerService` é o `Rc<dyn IEditorWorkerService>` e `ILanguageFeaturesService` é o
//!   [`LanguageFeaturesService`].
//! - O `ServicesAccessor` das funções `formatDocument...` (`IInstantiationService`, `ILanguageFeaturesService`,
//!   `IEditorWorkerService`, `ILogService`, `IAccessibilitySignalService`) é o [`FormattingServices`]; o
//!   `instaService.invokeFunction(f, ...)` é a chamada direta de `f`. O `EditorStateCancellationTokenSource` ainda lê
//!   o `IEditorCancellationTokens` do editor, que é o `editor_cancellation_tokens` dos serviços.
//! - `ITextModel | IActiveCodeEditor` é o [`EditorOrModel`] (o editor ativo tem modelo); `Range | Range[]` é `Vec<Range>`.
//! - `IProgress<T>` é o `Rc<dyn IProgress<T>>` de `code_action::platform`, e os provedores viajam como `Rc<dyn ...>`.
//! - `IFormattingEditProviderSelector` é um trait com um método por tipo de provedor (o método genérico `<T extends
//!   ...>` do original não tem forma de objeto de trait); o [`FormattingConflicts::select`] escolhe o método pelo
//!   [`SelectableFormatter`]. A lista estática de seletores é por thread, e `unshift` põe o novo no começo.
//! - O `try { ... } finally { cts.dispose() }` é o descarte do [`FormattingCancellation`] ao sair do escopo.
//! - `await Promise.resolve(provider.provideX(...)).catch(onUnexpectedExternalError)` é o `await` da promessa do trait,
//!   com a rejeição enviada a `onUnexpectedExternalError` e lida como `undefined`.
//! - `isNonEmptyArray(rawEdits)` é `Some` de uma lista que não está vazia.
//! - `TextEdit[] | null | undefined` e `TextEdit[] | undefined` são `Option<Vec<TextEdit>>`.
//! - Os `logService.trace(..., range)` levam o intervalo e as edições como objetos dinâmicos.
//! - Ficam de fora os três comandos `_executeFormat*Provider`, que dependem de `CommandsRegistry` e do
//!   `ITextModelService`.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::cancellation::{CancellationToken, CancellationTokenSource};
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, on_unexpected_external_error};
use je_base::lifecycle::{DisposableRef, IDisposable, to_disposable};
use je_base::promise::Promise;
use je_base::JsString;
use je_extensions::manifest::{ExtensionIdentifier, ExtensionIdentifierSet};
use je_platform::log::{LogArg, LogServiceRef, Logger};

use crate::code_action::languages::TextEdit;
use crate::code_action::platform::{
    AccessibilitySignal, AccessibilitySignalOptions, IAccessibilitySignalService, IProgress,
};
use crate::code_editor_widget::{CodeEditorWidget, ICodeEditor};
use crate::edit_operation::SingleEditOperation;
use crate::editor_common::ScrollType;
use crate::editor_state::{EditorStateCancellationTokenSource, TextModelCancellationTokenSource, code_editor_state_flag};
use crate::editor_worker::IEditorWorkerService;
use crate::formatting_edit::FormattingEdit;
use crate::keybinding_cancellation::EditorCancellationTokens;
use crate::language_feature_registry::LanguageFeatureRegistry;
use crate::language_features_service::{
    DocumentFormattingEditProvider, DocumentRangeFormattingEditProvider, FormattingOptions, LanguageFeaturesService,
};
use crate::model::ValidEditOperation;
use crate::position::Position;
use crate::range::Range;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// O objeto literal que `getRealAndSyntheticDocumentFormattersOrdered` cria para cada formatador de intervalo.
struct SyntheticDocumentFormatter {
    formatter: Rc<dyn DocumentRangeFormattingEditProvider>,
}

impl DocumentFormattingEditProvider for SyntheticDocumentFormatter {
    fn extension_id(&self) -> Option<&ExtensionIdentifier> {
        self.formatter.extension_id()
    }

    fn display_name(&self) -> Option<&str> {
        self.formatter.display_name()
    }

    fn provide_document_formatting_edits(
        &self,
        model: &Rc<TextModel>,
        options: &FormattingOptions,
        token: &CancellationToken,
    ) -> crate::language_providers::ProviderResult<Vec<TextEdit>> {
        self.formatter.provide_document_range_formatting_edits(model, model.get_full_model_range(), options, token)
    }
}

/// `getRealAndSyntheticDocumentFormattersOrdered(documentFormattingEditProvider, documentRangeFormattingEditProvider, model)`.
pub fn get_real_and_synthetic_document_formatters_ordered(
    document_formatting_edit_provider: &LanguageFeatureRegistry<dyn DocumentFormattingEditProvider>,
    document_range_formatting_edit_provider: &LanguageFeatureRegistry<dyn DocumentRangeFormattingEditProvider>,
    model: &Rc<TextModel>,
) -> Vec<Rc<dyn DocumentFormattingEditProvider>> {
    let mut result: Vec<Rc<dyn DocumentFormattingEditProvider>> = Vec::new();
    let mut seen = ExtensionIdentifierSet::default();

    // (1) add all document formatter
    for formatter in document_formatting_edit_provider.ordered(model, false) {
        if let Some(extension_id) = formatter.extension_id() {
            seen.add(&extension_id.value);
        }
        result.push(formatter);
    }

    // (2) add all range formatter as document formatter (unless the same extension already did that)
    for formatter in document_range_formatting_edit_provider.ordered(model, false) {
        if let Some(extension_id) = formatter.extension_id() {
            if seen.has(&extension_id.value) {
                continue;
            }
            seen.add(&extension_id.value);
        }
        result.push(Rc::new(SyntheticDocumentFormatter { formatter }));
    }
    result
}

/// O `await Promise.resolve(...).catch(onUnexpectedExternalError)`: a rejeição vira `undefined`.
fn report_failure(result: Result<Option<Vec<TextEdit>>, je_base::errors::Error>) -> Option<Vec<TextEdit>> {
    result.unwrap_or_else(|err| {
        on_unexpected_external_error(&err);
        None
    })
}

/// `getDocumentRangeFormattingEditsUntilResult(workerService, languageFeaturesService, model, range, options, token)`.
pub fn get_document_range_formatting_edits_until_result(
    worker_service: &Rc<dyn IEditorWorkerService>,
    language_features_service: &LanguageFeaturesService,
    model: &Rc<TextModel>,
    range: Range,
    options: FormattingOptions,
    token: &CancellationToken,
) -> Promise<Option<Vec<TextEdit>>> {
    let providers = language_features_service.document_range_formatting_edit_provider.ordered(model, false);
    let (worker_service, model, token) = (Rc::clone(worker_service), Rc::clone(model), token.clone());
    async_fn(async move {
        for provider in providers {
            let raw_edits = report_failure(provider.provide_document_range_formatting_edits(&model, range, &options, &token).await);
            if raw_edits.as_ref().is_some_and(|edits| !edits.is_empty()) {
                return worker_service.compute_more_minimal_edits(&model.uri(), raw_edits).await;
            }
        }
        Ok(None)
    })
}

/// `getDocumentFormattingEditsUntilResult(workerService, languageFeaturesService, model, options, token)`.
pub fn get_document_formatting_edits_until_result(
    worker_service: &Rc<dyn IEditorWorkerService>,
    language_features_service: &LanguageFeaturesService,
    model: &Rc<TextModel>,
    options: FormattingOptions,
    token: &CancellationToken,
) -> Promise<Option<Vec<TextEdit>>> {
    let providers = get_real_and_synthetic_document_formatters_ordered(
        &language_features_service.document_formatting_edit_provider,
        &language_features_service.document_range_formatting_edit_provider,
        model,
    );
    let (worker_service, model, token) = (Rc::clone(worker_service), Rc::clone(model), token.clone());
    async_fn(async move {
        for provider in providers {
            let raw_edits = report_failure(provider.provide_document_formatting_edits(&model, &options, &token).await);
            if raw_edits.as_ref().is_some_and(|edits| !edits.is_empty()) {
                return worker_service.compute_more_minimal_edits(&model.uri(), raw_edits).await;
            }
        }
        Ok(None)
    })
}

/// `getOnTypeFormattingEdits(workerService, languageFeaturesService, model, position, ch, options, token)`.
pub fn get_on_type_formatting_edits(
    worker_service: &Rc<dyn IEditorWorkerService>,
    language_features_service: &LanguageFeaturesService,
    model: &Rc<TextModel>,
    position: Position,
    ch: &str,
    options: FormattingOptions,
    token: &CancellationToken,
) -> Promise<Option<Vec<TextEdit>>> {
    let providers = language_features_service.on_type_formatting_edit_provider.ordered(model, false);

    let Some(provider) = providers.first() else {
        return Promise::resolved(None);
    };

    if !provider.auto_format_trigger_characters().iter().any(|trigger| trigger.as_str() == ch) {
        return Promise::resolved(None);
    }

    let (worker_service, uri) = (Rc::clone(worker_service), model.uri());
    provider
        .provide_on_type_formatting_edits(model, position, ch, &options, token)
        .then_result_promise(move |result| worker_service.compute_more_minimal_edits(&uri, report_failure(result)))
}

/// `FormattingKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormattingKind {
    File = 1,
    Selection = 2,
}

/// `FormattingMode`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormattingMode {
    Explicit = 1,
    Silent = 2,
}

/// `ITextModel | IActiveCodeEditor`.
#[derive(Clone)]
pub enum EditorOrModel {
    Editor(Rc<CodeEditorWidget>),
    Model(Rc<TextModel>),
}

impl EditorOrModel {
    /// `isCodeEditor(editorOrModel) ? editorOrModel.getModel() : editorOrModel`. O editor ativo sempre tem modelo.
    pub fn model(&self) -> Rc<TextModel> {
        match self {
            Self::Editor(editor) => editor.get_model().expect("IActiveCodeEditor tem modelo"),
            Self::Model(model) => Rc::clone(model),
        }
    }
}

/// Os serviços que as funções `formatDocument...` leem do `ServicesAccessor`.
#[derive(Clone)]
pub struct FormattingServices {
    pub worker_service: Rc<dyn IEditorWorkerService>,
    pub language_features_service: LanguageFeaturesService,
    pub log_service: LogServiceRef,
    pub accessibility_signal_service: Rc<dyn IAccessibilitySignalService>,
    pub editor_cancellation_tokens: Rc<EditorCancellationTokens>,
}

pub type DocumentFormatter = Rc<dyn DocumentFormattingEditProvider>;
pub type DocumentRangeFormatter = Rc<dyn DocumentRangeFormattingEditProvider>;

/// `IFormattingEditProviderSelector` (ver a nota do módulo).
pub trait IFormattingEditProviderSelector {
    fn select_document_formatter(
        &self,
        formatter: Vec<DocumentFormatter>,
        document: &Rc<TextModel>,
        mode: FormattingMode,
        kind: FormattingKind,
    ) -> Promise<Option<DocumentFormatter>>;

    fn select_document_range_formatter(
        &self,
        formatter: Vec<DocumentRangeFormatter>,
        document: &Rc<TextModel>,
        mode: FormattingMode,
        kind: FormattingKind,
    ) -> Promise<Option<DocumentRangeFormatter>>;
}

/// O `T extends (DocumentFormattingEditProvider | DocumentRangeFormattingEditProvider)` de `FormattingConflicts.select`.
pub trait SelectableFormatter: 'static {
    fn select_with(
        selector: &dyn IFormattingEditProviderSelector,
        formatter: Vec<Rc<Self>>,
        document: &Rc<TextModel>,
        mode: FormattingMode,
        kind: FormattingKind,
    ) -> Promise<Option<Rc<Self>>>;
}

impl SelectableFormatter for dyn DocumentFormattingEditProvider {
    fn select_with(
        selector: &dyn IFormattingEditProviderSelector,
        formatter: Vec<DocumentFormatter>,
        document: &Rc<TextModel>,
        mode: FormattingMode,
        kind: FormattingKind,
    ) -> Promise<Option<DocumentFormatter>> {
        selector.select_document_formatter(formatter, document, mode, kind)
    }
}

impl SelectableFormatter for dyn DocumentRangeFormattingEditProvider {
    fn select_with(
        selector: &dyn IFormattingEditProviderSelector,
        formatter: Vec<DocumentRangeFormatter>,
        document: &Rc<TextModel>,
        mode: FormattingMode,
        kind: FormattingKind,
    ) -> Promise<Option<DocumentRangeFormatter>> {
        selector.select_document_range_formatter(formatter, document, mode, kind)
    }
}

thread_local! {
    static SELECTORS: RefCell<Vec<Rc<dyn IFormattingEditProviderSelector>>> = const { RefCell::new(Vec::new()) };
}

/// `FormattingConflicts`.
pub struct FormattingConflicts;

impl FormattingConflicts {
    /// `setFormatterSelector(selector)`.
    pub fn set_formatter_selector(selector: Rc<dyn IFormattingEditProviderSelector>) -> DisposableRef {
        SELECTORS.with(|selectors| selectors.borrow_mut().insert(0, Rc::clone(&selector)));
        to_disposable(move || {
            SELECTORS.with(|selectors| {
                let mut selectors = selectors.borrow_mut();
                if let Some(index) = selectors.iter().position(|candidate| Rc::ptr_eq(candidate, &selector)) {
                    selectors.remove(index);
                }
            });
        })
    }

    /// `select(formatter, document, mode, kind)`.
    pub fn select<T: SelectableFormatter + ?Sized>(
        formatter: Vec<Rc<T>>,
        document: &Rc<TextModel>,
        mode: FormattingMode,
        kind: FormattingKind,
    ) -> Promise<Option<Rc<T>>> {
        if formatter.is_empty() {
            return Promise::resolved(None);
        }
        match SELECTORS.with(|selectors| selectors.borrow().first().cloned()) {
            Some(selector) => T::select_with(&*selector, formatter, document, mode, kind),
            None => Promise::resolved(None),
        }
    }
}

/// O `cts` de `formatDocument...`: a fonte de cancelamento do editor ou do modelo, descartada ao sair do escopo (o
/// `finally`).
struct FormattingCancellation {
    source: CancellationTokenSource,
    owner: Box<dyn IDisposable>,
}

impl FormattingCancellation {
    fn new(services: &FormattingServices, editor_or_model: &EditorOrModel, token: &CancellationToken) -> Self {
        match editor_or_model {
            EditorOrModel::Editor(editor) => {
                let cts = EditorStateCancellationTokenSource::new(
                    editor,
                    &services.editor_cancellation_tokens,
                    code_editor_state_flag::VALUE | code_editor_state_flag::POSITION,
                    None,
                    Some(token),
                );
                Self { source: (*cts).clone(), owner: Box::new(cts) }
            }
            EditorOrModel::Model(model) => {
                let cts = TextModelCancellationTokenSource::new(model, Some(token));
                Self { source: (*cts).clone(), owner: Box::new(cts) }
            }
        }
    }

    fn token(&self) -> CancellationToken {
        self.source.token()
    }
}

impl Drop for FormattingCancellation {
    fn drop(&mut self) {
        self.owner.dispose();
    }
}

fn range_to_dynamic(range: &Range) -> Dynamic {
    Dynamic::object([
        ("startLineNumber", Dynamic::Number(f64::from(range.start_line_number))),
        ("startColumn", Dynamic::Number(f64::from(range.start_column))),
        ("endLineNumber", Dynamic::Number(f64::from(range.end_line_number))),
        ("endColumn", Dynamic::Number(f64::from(range.end_column))),
    ])
}

fn edits_to_dynamic(edits: &[TextEdit]) -> Dynamic {
    Dynamic::array(
        edits.iter().map(|edit| Dynamic::object([("range", range_to_dynamic(&edit.range)), ("text", Dynamic::string(&edit.text))])),
    )
}

fn extension_id_value(extension_id: Option<&ExtensionIdentifier>) -> Dynamic {
    extension_id.map_or(Dynamic::Undefined, |extension_id| Dynamic::string(&extension_id.value))
}

/// Aplica as edições pelo editor (com pilha de desfazer e rolagem) ou pelo modelo (com seleção inicial na primeira
/// edição). `interactive` liga as paradas de desfazer e a revelação do cursor, que o modo `Silent` dispensa.
fn apply_formatting_edits(editor_or_model: &EditorOrModel, edits: &[TextEdit], interactive: bool) {
    match editor_or_model {
        EditorOrModel::Editor(editor) => {
            // use editor to apply edits
            FormattingEdit::execute(editor, edits, interactive);
            if interactive && let Some(position) = editor.get_position() {
                editor.reveal_position_in_center_if_outside_viewport(position, Some(ScrollType::Immediate));
            }
        }
        EditorOrModel::Model(model) => {
            // use model to apply edits
            let range = edits[0].range;
            let initial_selection = Selection::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column);
            let operations: Vec<SingleEditOperation> = edits
                .iter()
                .map(|edit| SingleEditOperation::new(edit.range, Some(JsString::from(edit.text.as_str())), true))
                .collect();
            let mut compute_cursor_state = |undo_edits: &[ValidEditOperation]| -> Option<Vec<Selection>> {
                undo_edits.iter().find(|undo_edit| Range::are_intersecting_or_touching(&undo_edit.range, &initial_selection)).map(|undo_edit| {
                    let range = undo_edit.range;
                    vec![Selection::new(range.start_line_number, range.start_column, range.end_line_number, range.end_column)]
                })
            };
            model.push_edit_operations(Some(vec![initial_selection]), &operations, Some(&mut compute_cursor_state), None, None);
        }
    }
}

/// `formatDocumentRangesWithSelectedProvider(accessor, editorOrModel, rangeOrRanges, mode, progress, token, userGesture)`.
pub fn format_document_ranges_with_selected_provider(
    services: &FormattingServices,
    editor_or_model: &EditorOrModel,
    range_or_ranges: Vec<Range>,
    mode: FormattingMode,
    progress: Rc<dyn IProgress<DocumentRangeFormatter>>,
    token: &CancellationToken,
    user_gesture: bool,
) -> Promise<()> {
    let model = editor_or_model.model();
    let provider = services.language_features_service.document_range_formatting_edit_provider.ordered(&model, false);
    let (services, editor_or_model, token) = (services.clone(), editor_or_model.clone(), token.clone());
    async_fn(async move {
        let selected = FormattingConflicts::select(provider, &model, mode, FormattingKind::Selection).await?;
        if let Some(selected) = selected {
            progress.report(Rc::clone(&selected));
            format_document_ranges_with_provider(&services, &selected, &editor_or_model, range_or_ranges, &token, user_gesture).await?;
        }
        Ok(())
    })
}

/// `computeEdits(range)` de `formatDocumentRangesWithProvider`.
async fn compute_range_edits(
    services: &FormattingServices,
    provider: &DocumentRangeFormatter,
    model: &Rc<TextModel>,
    range: Range,
    token: &CancellationToken,
) -> Result<Vec<TextEdit>, Error> {
    let extension_id = extension_id_value(provider.extension_id());
    services.log_service.trace_args(
        "[format][provideDocumentRangeFormattingEdits] (request)",
        &[LogArg::Value(extension_id.clone()), LogArg::Value(range_to_dynamic(&range))],
    );

    let result = provider
        .provide_document_range_formatting_edits(model, range, &model.get_formatting_options(), token)
        .await?
        .unwrap_or_default();

    services.log_service.trace_args(
        "[format][provideDocumentRangeFormattingEdits] (response)",
        &[LogArg::Value(extension_id), LogArg::Value(edits_to_dynamic(&result))],
    );

    Ok(result)
}

/// `hasIntersectingEdit(a, b)` de `formatDocumentRangesWithProvider`.
fn has_intersecting_edit(a: &[TextEdit], b: &[TextEdit]) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    // quick exit if the list of ranges are completely unrelated [O(n)]
    let merged_a = a.iter().fold(a[0].range, |acc, val| acc.plus_range(&val.range));
    if !b.iter().any(|x| merged_a.intersect_ranges(&x.range).is_some()) {
        return false;
    }
    // fallback to a complete check [O(n^2)]
    a.iter().any(|edit| b.iter().any(|other_edit| edit.range.intersect_ranges(&other_edit.range).is_some()))
}

/// O bloco `try` de `formatDocumentRangesWithProvider`: `None` é o cancelamento (o `return true`).
async fn compute_ranges_edits(
    services: &FormattingServices,
    provider: &DocumentRangeFormatter,
    editor_or_model: &EditorOrModel,
    range_or_ranges: Vec<Range>,
    token: &CancellationToken,
) -> Result<Option<Vec<TextEdit>>, Error> {
    let model = editor_or_model.model();
    let cts = FormattingCancellation::new(services, editor_or_model, token);

    // make sure that ranges don't overlap nor touch each other
    let mut sorted = range_or_ranges;
    sorted.sort_by(|a, b| Range::compare_ranges_using_starts(Some(a), Some(b)).cmp(&0));
    let mut ranges: Vec<Range> = Vec::new();
    for range in sorted {
        match ranges.last_mut() {
            Some(last) if Range::are_intersecting_or_touching(last, &range) => {
                *last = Range::from_positions(last.get_start_position(), range.get_end_position());
            }
            _ => ranges.push(range),
        }
    }

    let mut all_edits: Vec<TextEdit> = Vec::new();
    let mut raw_edits_list: Vec<Vec<TextEdit>> = Vec::new();

    if provider.has_ranges_formatting() {
        let extension_id = extension_id_value(provider.extension_id());
        let ranges_dynamic = Dynamic::array(ranges.iter().map(range_to_dynamic));
        services.log_service.trace_args(
            "[format][provideDocumentRangeFormattingEdits] (request)",
            &[LogArg::Value(extension_id.clone()), LogArg::Value(ranges_dynamic)],
        );
        let result = provider
            .provide_document_ranges_formatting_edits(&model, &ranges, &model.get_formatting_options(), &cts.token())
            .await?
            .unwrap_or_default();
        services.log_service.trace_args(
            "[format][provideDocumentRangeFormattingEdits] (response)",
            &[LogArg::Value(extension_id), LogArg::Value(edits_to_dynamic(&result))],
        );
        raw_edits_list.push(result);
    } else {
        for range in &ranges {
            if cts.token().is_cancellation_requested() {
                return Ok(None);
            }
            raw_edits_list.push(compute_range_edits(services, provider, &model, *range, &cts.token()).await?);
        }

        let mut i = 0;
        while i < ranges.len() {
            let mut j = i + 1;
            while j < ranges.len() {
                if cts.token().is_cancellation_requested() {
                    return Ok(None);
                }
                if has_intersecting_edit(&raw_edits_list[i], &raw_edits_list[j]) {
                    // Merge ranges i and j into a single range, recompute the associated edits
                    let merged_range = ranges[i].plus_range(&ranges[j]);
                    let edits = compute_range_edits(services, provider, &model, merged_range, &cts.token()).await?;
                    ranges.remove(j);
                    ranges.remove(i);
                    ranges.push(merged_range);
                    raw_edits_list.remove(j);
                    raw_edits_list.remove(i);
                    raw_edits_list.push(edits);
                    // Restart scanning
                    i = 0;
                    j = 0;
                }
                j += 1;
            }
            i += 1;
        }
    }

    for raw_edits in raw_edits_list {
        if cts.token().is_cancellation_requested() {
            return Ok(None);
        }
        if let Some(minimal_edits) = services.worker_service.compute_more_minimal_edits(&model.uri(), Some(raw_edits)).await? {
            all_edits.extend(minimal_edits);
        }
    }

    if cts.token().is_cancellation_requested() {
        return Ok(None);
    }
    Ok(Some(all_edits))
}

/// `formatDocumentRangesWithProvider(accessor, provider, editorOrModel, rangeOrRanges, token, userGesture)`.
pub fn format_document_ranges_with_provider(
    services: &FormattingServices,
    provider: &DocumentRangeFormatter,
    editor_or_model: &EditorOrModel,
    range_or_ranges: Vec<Range>,
    token: &CancellationToken,
    user_gesture: bool,
) -> Promise<bool> {
    let (services, provider, editor_or_model, token) = (services.clone(), Rc::clone(provider), editor_or_model.clone(), token.clone());
    async_fn(async move {
        let Some(all_edits) = compute_ranges_edits(&services, &provider, &editor_or_model, range_or_ranges, &token).await? else {
            return Ok(true);
        };

        if all_edits.is_empty() {
            return Ok(false);
        }

        apply_formatting_edits(&editor_or_model, &all_edits, true);
        services
            .accessibility_signal_service
            .play_signal_with_options(AccessibilitySignal::Format, AccessibilitySignalOptions { user_gesture: Some(user_gesture) });
        Ok(true)
    })
}

/// `formatDocumentWithSelectedProvider(accessor, editorOrModel, mode, progress, token, userGesture)`.
pub fn format_document_with_selected_provider(
    services: &FormattingServices,
    editor_or_model: &EditorOrModel,
    mode: FormattingMode,
    progress: Rc<dyn IProgress<DocumentFormatter>>,
    token: &CancellationToken,
    user_gesture: Option<bool>,
) -> Promise<()> {
    let model = editor_or_model.model();
    let provider = get_real_and_synthetic_document_formatters_ordered(
        &services.language_features_service.document_formatting_edit_provider,
        &services.language_features_service.document_range_formatting_edit_provider,
        &model,
    );
    let (services, editor_or_model, token) = (services.clone(), editor_or_model.clone(), token.clone());
    async_fn(async move {
        let selected = FormattingConflicts::select(provider, &model, mode, FormattingKind::File).await?;
        if let Some(selected) = selected {
            progress.report(Rc::clone(&selected));
            format_document_with_provider(&services, &selected, &editor_or_model, mode, &token, user_gesture).await?;
        }
        Ok(())
    })
}

/// O bloco `try` de `formatDocumentWithProvider`: `None` é o cancelamento (o `return true`).
async fn compute_document_edits(
    services: &FormattingServices,
    provider: &DocumentFormatter,
    editor_or_model: &EditorOrModel,
    token: &CancellationToken,
) -> Result<Option<Vec<TextEdit>>, Error> {
    let model = editor_or_model.model();
    let cts = FormattingCancellation::new(services, editor_or_model, token);

    let raw_edits = provider.provide_document_formatting_edits(&model, &model.get_formatting_options(), &cts.token()).await?;
    let edits = services.worker_service.compute_more_minimal_edits(&model.uri(), raw_edits).await?;

    if cts.token().is_cancellation_requested() {
        return Ok(None);
    }
    Ok(Some(edits.unwrap_or_default()))
}

/// `formatDocumentWithProvider(accessor, provider, editorOrModel, mode, token, userGesture)`.
pub fn format_document_with_provider(
    services: &FormattingServices,
    provider: &DocumentFormatter,
    editor_or_model: &EditorOrModel,
    mode: FormattingMode,
    token: &CancellationToken,
    user_gesture: Option<bool>,
) -> Promise<bool> {
    let (services, provider, editor_or_model, token) = (services.clone(), Rc::clone(provider), editor_or_model.clone(), token.clone());
    async_fn(async move {
        let Some(edits) = compute_document_edits(&services, &provider, &editor_or_model, &token).await? else {
            return Ok(true);
        };

        if edits.is_empty() {
            return Ok(false);
        }

        apply_formatting_edits(&editor_or_model, &edits, mode != FormattingMode::Silent);
        services
            .accessibility_signal_service
            .play_signal_with_options(AccessibilitySignal::Format, AccessibilitySignalOptions { user_gesture });
        Ok(true)
    })
}

/// `getDocumentFormattingEditsWithSelectedProvider(workerService, languageFeaturesService, editorOrModel, mode, token)`.
pub fn get_document_formatting_edits_with_selected_provider(
    worker_service: &Rc<dyn IEditorWorkerService>,
    language_features_service: &LanguageFeaturesService,
    editor_or_model: &EditorOrModel,
    mode: FormattingMode,
    token: &CancellationToken,
) -> Promise<Option<Vec<TextEdit>>> {
    let model = editor_or_model.model();
    let provider = get_real_and_synthetic_document_formatters_ordered(
        &language_features_service.document_formatting_edit_provider,
        &language_features_service.document_range_formatting_edit_provider,
        &model,
    );
    let (worker_service, token) = (Rc::clone(worker_service), token.clone());
    async_fn(async move {
        let selected = FormattingConflicts::select(provider, &model, mode, FormattingKind::File).await?;
        let Some(selected) = selected else {
            return Ok(None);
        };
        // O original passa `model.getOptions()` como `FormattingOptions`: só `tabSize` e `insertSpaces` são lidos.
        let model_options = model.get_options();
        let options = FormattingOptions { tab_size: f64::from(model_options.tab_size), insert_spaces: model_options.insert_spaces };
        let raw_edits = report_failure(selected.provide_document_formatting_edits(&model, &options, &token).await);
        worker_service.compute_more_minimal_edits(&model.uri(), raw_edits).await
    })
}
