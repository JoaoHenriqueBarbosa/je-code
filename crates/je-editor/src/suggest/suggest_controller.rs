//! Porte de `src/vs/editor/contrib/suggest/browser/suggestController.ts`: o controlador de sugestões do editor
//! (`SuggestController`), com a inserção de uma sugestão aceita (`_insertSuggestion`), o cálculo do trecho a
//! sobrescrever (`getOverwriteInfo`), o `LineSuffix` e a disparada manual (`triggerSuggest`).
//!
//! Representação:
//! - `SuggestController implements IEditorContribution` é a struct [`SuggestController`], criada por
//!   [`SuggestController::new`], que devolve `Rc<Self>`: os ouvintes guardam só um [`Weak`] do controlador, e o
//!   `editor` do controlador é um [`Weak`] do widget (o editor é dono da contribuição e um `Rc` formaria um ciclo).
//!   `SuggestController.get(editor)` é [`SuggestController::get`].
//! - Os seis argumentos de injeção do construtor (`ISuggestMemoryService`, `ICommandService`, `IContextKeyService`,
//!   `IInstantiationService`, `ILogService`, `ITelemetryService`) são o [`SuggestControllerServices`]; o
//!   `IInstantiationService` entra como os [`SuggestModelServices`] que o `createInstance(SuggestModel, editor)`
//!   resolveria, e o `alert` global de `aria.ts` como o `aria_alert` (o mesmo padrão do controlador de busca).
//! - `const enum InsertFlags` é o módulo [`insert_flags`] de constantes `u32` (o `|` e o `&` são os do original).
//! - `WindowIdleValue<T>` é o [`GlobalIdleValue`] (o `runWhenWindowIdle` do editor de teste é o `runWhenGlobalIdle`,
//!   e o porte não tem janela). O `this._toDispose.add(new WindowIdleValue(...))` é um descarte que chama o `dispose`
//!   do valor.
//! - `MutableDisposable<LineSuffix>` é o [`MutableDisposable`] de [`LineSuffix`]. O
//!   `ModelDecorationOptions.register({ description, stickiness })` estático é o `thread_local` de uma opção registrada.
//! - `protected _insertSuggestion(event, flags)` é [`SuggestController::insert_suggestion`], público porque o porte
//!   não tem herança (a subclasse `TestCtrl` do teste só o expõe). `event: ISelectedSuggestion | undefined` é um
//!   `Option<&ISelectedSuggestion>`.
//! - `tasks: Promise<unknown>[]` é `Vec<Promise<()>>` e o `cts` um `Rc` do [`CancellationTokenSource`], que os
//!   ouvintes e o `then` compartilham. O `Promise.all(tasks).finally(...)` é [`Promise::all`] seguido de `finally`.
//! - `private _reportSuggestionAcceptedTelemetry` mantém a amostragem de 0,01% e monta o mesmo evento
//!   `suggest.acceptedSuggestion`. `basename(model.uri)` e `extname(model.uri)` são os do `ExtUri` sem viés, e
//!   `hash(...).toString(16)` é o hexadecimal com sinal do `Number.prototype.toString`.
//!
//! Divergências:
//! - Sem o widget, o item em foco (`widget.getFocusedItem()`) é guardado pelos ouvintes de `onDidSuggest` (índice da
//!   memória e `selectionMode`, como o `showSuggestions`) e de `onDidCancel` (como o `hideWidget`), e o
//!   `acceptSelectedSuggestion` o lê. As `_selectors` do `PriorityRegistry` se reduzem ao seletor da memória.
//! - O `SuggestWidget` ainda não tem porte (ver `suggest_widget_contract.rs`), então tudo o que o controlador faz
//!   com ele não existe aqui: o `widget` e o `_wantsForceRenderingAbove`, o `onDidSelect`, o
//!   `CommitCharacterController`, as chaves `suggestionMakesTextEdit`, `suggestionHasInsertAndReplaceRange` e
//!   `suggestionCanResolve` (que o `onDidFocus` do widget escreve), o `showTriggered` do `onDidTrigger`, os
//!   comandos de navegação do widget (`cancelSuggestWidget`, `focusSuggestion`, `selectNextSuggestion`...
//!   `toggleExplainMode`, `resetWidgetSize`,
//!   `forceRenderingAbove`, `stopForceRenderingAbove`) e os registros de ações e comandos
//!   do fim do arquivo, que dependem do widget, do sistema de menus e do `ICommandService`. Entram com o porte do
//!   `suggestWidget.ts`.
//! - O `CompletionItem` de `languages.rs` agora tem `command` e `extensionId`: a execução do `command` do item (com o
//!   caso do `TriggerSuggestAction`, cujo `id` é [`TRIGGER_SUGGEST_ACTION_ID`] porque a ação não tem porte), o
//!   `extensionId` do `EditSources.suggest` e o da telemetria são os do original. O `item.extensionId` é lido de
//!   `item.completion.extension_id` (o `CompletionItem` do porte não o copia para um campo próprio). O `providerId`
//!   do `EditSources.suggest` é aceito mas não guardado, porque o `TextModelEditSource` do porte não tem o campo.
//! - O `triggerSuggestAndAcceptBest` depende do widget e não é portado (o `fallback` executa um comando pelo
//!   `ICommandService`, que agora existe, mas o corpo usa o `SuggestWidget`).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::async_::GlobalIdleValue;
use je_base::cancellation::CancellationTokenSource;
use je_base::dynamic::Dynamic;
use je_base::errors::{on_unexpected_error, on_unexpected_external_error};
use je_base::event::Emitter;
use je_base::hash::hash;
use je_base::lifecycle::{DisposableRef, DisposableStore, IDisposable, MutableDisposable, to_disposable};
use je_base::nls::localize;
use je_base::path::ProcessEnvironment;
use je_base::promise::Promise;
use je_base::random::random_f64;
use je_base::resources::ExtUri;
use je_base::stopwatch::StopWatch;
use je_base::types::assert_type;
use je_platform::commands::ICommandService;
use je_platform::contextkey::{IContextKey, IContextKeyService};
use je_platform::log::{LogArg, LogServiceRef};
use je_platform::telemetry::telemetry::ITelemetryService;

use crate::code_action::languages::ProviderId;
use crate::code_editor_widget::{CodeEditorWidget, ExecuteEditsSource, ICodeEditor, IEditorContribution};
use crate::config::editor_options::registry::ACCEPT_SUGGESTION_ON_ENTER;
use crate::config::editor_options::suggest::{SUGGEST, SuggestInsertMode, SuggestSelectionMode};
use crate::cursor_common::CursorSimpleModel;
use crate::edit_operation::EditOperation;
use crate::editor_common::ScrollType;
use crate::languages::{CompletionTriggerKind, completion_item_insert_text_rule};
use crate::model::{ModelDecorationOptionsInit, TrackedRangeStickiness};
use crate::position::Position;
use crate::range::Range;
use crate::snippet::snippet_controller2::{PartialSnippetInsertOptions, SnippetController2};
use crate::snippet::snippet_parser::SnippetParser;
use crate::snippet::snippet_variables::IOvertypingCapturer;
use crate::stable_editor_scroll::StableEditorScrollState;
use crate::suggest::suggest::{CompletionItem, ProviderSet, suggest_context};
use crate::suggest::suggest_alternatives::SuggestAlternatives;
use crate::suggest::suggest_memory::ISuggestMemoryService;
use crate::suggest::suggest_model::{PartialCompletionOptions, State, SuggestModel, SuggestModelServices, SuggestTriggerOptions};
use crate::suggest::suggest_overtyping_capturer::OvertypingCapturer;
use crate::suggest::suggest_widget_contract::ISelectedSuggestion;
use crate::suggest::word_context_key::WordContextKey;
use crate::text_model::{ModelDecorationOptions, TextModel};
use crate::text_model_edit_source::EditSources;

/// `TriggerSuggestAction.id`: o comando do `item.completion.command` que redispara a sugestão.
const TRIGGER_SUGGEST_ACTION_ID: &str = "editor.action.triggerSuggest";

thread_local! {
    /// `LineSuffix._decorationOptions`.
    static LINE_SUFFIX_DECORATION_OPTIONS: Rc<ModelDecorationOptions> =
        ModelDecorationOptions::register(&ModelDecorationOptionsInit {
            description: "suggest-line-suffix".to_owned(),
            stickiness: Some(TrackedRangeStickiness::NeverGrowsWhenTypingAtEdges),
            ..ModelDecorationOptionsInit::default()
        });
}

/// `LineSuffix`.
pub struct LineSuffix {
    model: Rc<TextModel>,
    position: Position,
    marker: RefCell<Option<String>>,
}

impl LineSuffix {
    pub fn new(model: &Rc<TextModel>, position: Position) -> Rc<Self> {
        // spy on what's happening right of the cursor. two cases:
        // 1. end of line -> check that it's still end of line
        // 2. mid of line -> add a marker and compute the delta
        let suffix = Rc::new(Self { model: Rc::clone(model), position, marker: RefCell::new(None) });
        let max_column = model.get_line_max_column(position.line_number);
        if max_column != position.column {
            let offset = model.get_offset_at(position);
            let end = model.get_position_at(offset as i64 + 1);
            let options = LINE_SUFFIX_DECORATION_OPTIONS.with(Rc::clone);
            model.change_decorations(
                |accessor| {
                    let existing = suffix.marker.borrow().clone();
                    if let Some(existing) = existing {
                        accessor.remove_decoration(&existing);
                    }
                    *suffix.marker.borrow_mut() = Some(accessor.add_decoration(&Range::from_positions(position, end), options));
                },
                0,
            );
        }
        suffix
    }

    pub fn delta(&self, position: Position) -> i32 {
        if self.model.is_disposed() || self.position.line_number != position.line_number {
            // bail out early if things seems fishy
            return 0;
        }
        // read the marker (in case suggest was triggered at line end) or compare
        // the cursor to the line end.
        let marker = self.marker.borrow().clone();
        match marker {
            Some(marker) => {
                let range = self.model.get_decoration_range(&marker).expect("a decoração do sufixo existe enquanto o modelo vive");
                let end = self.model.get_offset_at(range.get_start_position());
                end as i32 - self.model.get_offset_at(position) as i32
            }
            None => self.model.get_line_max_column(position.line_number) - position.column,
        }
    }
}

impl IDisposable for LineSuffix {
    fn dispose(&self) {
        let marker = self.marker.borrow().clone();
        if let Some(marker) = marker
            && !self.model.is_disposed()
        {
            self.model.change_decorations(|accessor| accessor.remove_decoration(&marker), 0);
            *self.marker.borrow_mut() = None;
        }
    }
}

/// `const enum InsertFlags`.
pub mod insert_flags {
    pub const NONE: u32 = 0;
    pub const NO_BEFORE_UNDO_STOP: u32 = 1;
    pub const NO_AFTER_UNDO_STOP: u32 = 2;
    pub const KEEP_ALTERNATIVE_SUGGESTIONS: u32 = 4;
    pub const ALTERNATIVE_OVERWRITE_CONFIG: u32 = 8;
}

/// `{ item: CompletionItem }` de `onWillInsertSuggestItem`.
#[derive(Clone)]
pub struct WillInsertSuggestItem {
    pub item: CompletionItem,
}

/// `{ overwriteBefore, overwriteAfter }` de `getOverwriteInfo`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverwriteInfo {
    pub overwrite_before: i32,
    pub overwrite_after: i32,
}

/// Os argumentos de injeção do construtor (ver "Representação").
pub struct SuggestControllerServices {
    /// `@ISuggestMemoryService`.
    pub memory_service: Rc<dyn ISuggestMemoryService>,
    /// `@ICommandService`.
    pub command_service: Rc<dyn ICommandService>,
    /// `@IContextKeyService`.
    pub context_key_service: Rc<dyn IContextKeyService>,
    /// `@ILogService`.
    pub log_service: LogServiceRef,
    /// O que o `createInstance(SuggestModel, editor)` resolve (inclusive o `@ITelemetryService`, que o controlador
    /// também lê).
    pub model_services: SuggestModelServices,
    /// `alert(message)` de `aria.js`.
    pub aria_alert: Rc<dyn Fn(&str)>,
}

/// `SuggestController`.
pub struct SuggestController {
    me: Weak<SuggestController>,
    editor: Weak<CodeEditorWidget>,
    pub model: Rc<SuggestModel>,

    memory_service: Rc<dyn ISuggestMemoryService>,
    command_service: Rc<dyn ICommandService>,
    log_service: LogServiceRef,
    telemetry_service: Rc<dyn ITelemetryService>,
    aria_alert: Rc<dyn Fn(&str)>,

    alternatives: Rc<GlobalIdleValue<Rc<SuggestAlternatives>>>,
    line_suffix: Rc<MutableDisposable<LineSuffix>>,
    to_dispose: Rc<DisposableStore>,
    overtyping_capturer: Rc<GlobalIdleValue<Rc<OvertypingCapturer>>>,
    /// O item que o `SuggestWidget` teria em foco (`widget.getFocusedItem()`), mantido pelos ouvintes de `onDidSuggest`
    /// e `onDidCancel` enquanto o widget não tem porte.
    focused_item: Rc<RefCell<Option<ISelectedSuggestion>>>,

    on_will_insert_suggest_item: Rc<Emitter<WillInsertSuggestItem>>,
}

impl SuggestController {
    pub const ID: &'static str = "editor.contrib.suggestController";

    /// `SuggestController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<SuggestController>> {
        editor.get_contribution_as::<SuggestController>(Self::ID)
    }

    pub fn new(editor: &Rc<CodeEditorWidget>, services: SuggestControllerServices) -> Rc<Self> {
        let SuggestControllerServices { memory_service, command_service, context_key_service, log_service, model_services, aria_alert } =
            services;
        let telemetry_service = Rc::clone(&model_services.telemetry_service);
        let model = SuggestModel::new(editor, model_services);
        let to_dispose = DisposableStore::new();

        // context key: update insert/replace mode
        let ctx_insert_mode: Rc<dyn IContextKey> = suggest_context().insert_mode.bind_to(&*context_key_service);
        ctx_insert_mode.set(Dynamic::string(editor.get_option(SUGGEST).insert_mode.as_str()));
        {
            let ctx_insert_mode = Rc::clone(&ctx_insert_mode);
            let weak_editor = Rc::downgrade(editor);
            model.on_did_trigger().listen_in(
                move |_| {
                    if let Some(editor) = weak_editor.upgrade() {
                        ctx_insert_mode.set(Dynamic::string(editor.get_option(SUGGEST).insert_mode.as_str()));
                    }
                },
                &to_dispose,
            );
        }

        // Wire up text overtyping capture
        let overtyping_capturer = {
            let (weak_editor, model, to_dispose) = (Rc::downgrade(editor), Rc::clone(&model), Rc::clone(&to_dispose));
            Rc::new(GlobalIdleValue::new(move || {
                let editor = weak_editor.upgrade().expect("o editor vive enquanto o controlador vive");
                to_dispose.add_rc(OvertypingCapturer::new(&editor, &model))
            }))
        };
        Self::dispose_with(&to_dispose, &overtyping_capturer);

        let alternatives = {
            let (weak_editor, context_key_service, to_dispose) =
                (Rc::downgrade(editor), Rc::clone(&context_key_service), Rc::clone(&to_dispose));
            Rc::new(GlobalIdleValue::new(move || {
                let editor = weak_editor.upgrade().expect("o editor vive enquanto o controlador vive");
                to_dispose.add_rc(SuggestAlternatives::new(&editor, &*context_key_service))
            }))
        };
        Self::dispose_with(&to_dispose, &alternatives);

        to_dispose.add_rc(WordContextKey::new(editor, &*context_key_service));

        let line_suffix = MutableDisposable::<LineSuffix>::new();
        {
            let (line_suffix, weak_editor) = (Rc::clone(&line_suffix), Rc::downgrade(editor));
            model.on_did_trigger().listen_in(
                move |e| {
                    if let Some(editor) = weak_editor.upgrade() {
                        let model = editor.get_model().expect("o editor tem modelo ao disparar");
                        line_suffix.set(Some(LineSuffix::new(&model, e.position)));
                    }
                },
                &to_dispose,
            );
        }
        // `onDidSuggest`: a escolha do índice e o foco do `showSuggestions` do widget (sem o widget, só o item em foco)
        let focused_item: Rc<RefCell<Option<ISelectedSuggestion>>> = Rc::new(RefCell::new(None));
        {
            let (weak_editor, weak_model, memory_service, focused_item) =
                (Rc::downgrade(editor), Rc::downgrade(&model), Rc::clone(&memory_service), Rc::clone(&focused_item));
            model.on_did_suggest().listen_in(
                move |e| {
                    if e.trigger_options.shy {
                        return;
                    }
                    let (Some(editor), Some(suggest_model)) = (weak_editor.upgrade(), weak_model.upgrade()) else {
                        return;
                    };
                    let text_model = editor.get_model().expect("o editor tem modelo ao sugerir");
                    let position = editor.get_position().expect("o editor com modelo tem posição");
                    let items = e.completion_model.items();
                    // o seletor da memória é o único registrado e nunca devolve -1 no porte
                    let index = memory_service.select(&text_model, position, &items);
                    if suggest_model.state() == State::Idle {
                        // selecting an item can "pump" out selection/cursor change events
                        // which can cancel suggest halfway through this function. therefore
                        // we need to check again and bail if the session has been canceled
                        return;
                    }
                    let mut no_focus = false;
                    if e.trigger_options.auto {
                        // don't "focus" item when configured to do
                        let options = editor.get_option(SUGGEST);
                        match options.selection_mode {
                            // simple: always or never
                            SuggestSelectionMode::Always => no_focus = false,
                            SuggestSelectionMode::Never => no_focus = true,
                            // on with trigger character
                            SuggestSelectionMode::WhenTriggerCharacter => {
                                no_focus = e.trigger_options.trigger_kind != Some(CompletionTriggerKind::TriggerCharacter);
                            }
                            // without trigger character or when refiltering
                            SuggestSelectionMode::WhenQuickSuggestion => {
                                no_focus = e.trigger_options.trigger_kind == Some(CompletionTriggerKind::TriggerCharacter)
                                    && e.trigger_options.refilter != Some(true);
                            }
                        }
                    }
                    let focused = if no_focus {
                        None
                    } else {
                        items.get(index).map(|item| ISelectedSuggestion {
                            item: item.clone(),
                            index,
                            model: Rc::clone(&e.completion_model),
                        })
                    };
                    *focused_item.borrow_mut() = focused;
                },
                &to_dispose,
            );
        }
        {
            let focused_item = Rc::clone(&focused_item);
            model.on_did_cancel().listen_in(
                move |e| {
                    if !e.retrigger {
                        // `hideWidget()`
                        *focused_item.borrow_mut() = None;
                    }
                },
                &to_dispose,
            );
        }
        {
            let weak_model = Rc::downgrade(&model);
            editor.on_did_blur_editor_widget().listen_in(
                move |_| {
                    // `_sticky` é `false`
                    if let Some(model) = weak_model.upgrade() {
                        model.cancel(false);
                        model.clear();
                    }
                },
                &to_dispose,
            );
        }

        // Manage the acceptSuggestionsOnEnter context key
        let accept_suggestions_on_enter: Rc<dyn IContextKey> =
            suggest_context().accept_suggestions_on_enter.bind_to(&*context_key_service);
        let update_from_config = {
            let weak_editor = Rc::downgrade(editor);
            Rc::new(move || {
                if let Some(editor) = weak_editor.upgrade() {
                    let accept_suggestion_on_enter = editor.get_option(ACCEPT_SUGGESTION_ON_ENTER);
                    accept_suggestions_on_enter
                        .set(Dynamic::Bool(accept_suggestion_on_enter == "on" || accept_suggestion_on_enter == "smart"));
                }
            })
        };
        {
            let update_from_config = Rc::clone(&update_from_config);
            editor.on_did_change_configuration().listen_in(move |_| update_from_config(), &to_dispose);
        }
        update_from_config();

        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            editor: Rc::downgrade(editor),
            model,
            memory_service,
            command_service,
            log_service,
            telemetry_service,
            aria_alert,
            alternatives,
            line_suffix,
            to_dispose,
            overtyping_capturer,
            focused_item,
            on_will_insert_suggest_item: Emitter::new(),
        })
    }

    /// `this._toDispose.add(new WindowIdleValue(...))`: o descarte do valor.
    fn dispose_with<T: Clone + 'static>(to_dispose: &DisposableStore, value: &Rc<GlobalIdleValue<T>>) {
        let value = Rc::clone(value);
        to_dispose.add(to_disposable(move || value.dispose()));
    }

    /// `this.editor`.
    fn editor(&self) -> Rc<CodeEditorWidget> {
        self.editor.upgrade().expect("o editor vive enquanto o controlador vive")
    }

    /// `onWillInsertSuggestItem`.
    pub fn on_will_insert_suggest_item(&self) -> je_base::event::Event<WillInsertSuggestItem> {
        self.on_will_insert_suggest_item.event()
    }

    /// `_insertSuggestion(event, flags)`.
    pub fn insert_suggestion(&self, event: Option<&ISelectedSuggestion>, flags: u32) {
        let Some(event) = event else {
            self.alternatives.value().reset();
            self.model.cancel(false);
            self.model.clear();
            return;
        };
        let editor = self.editor();
        if !editor.has_model() {
            return;
        }
        let Some(snippet_controller) = SnippetController2::get(&editor) else {
            return;
        };

        self.on_will_insert_suggest_item.fire(WillInsertSuggestItem { item: event.item.clone() });

        let model = editor.get_model().expect("o editor tem modelo");
        let model_version_now = model.get_alternative_version_id();
        let item = event.item.clone();

        //
        let tasks: Rc<RefCell<Vec<Promise<()>>>> = Rc::new(RefCell::new(Vec::new()));
        let cts = Rc::new(CancellationTokenSource::new(None));
        let mut flags = flags;

        // pushing undo stops *before* additional text edits and
        // *after* the main edit
        if flags & insert_flags::NO_BEFORE_UNDO_STOP == 0 {
            editor.push_undo_stop();
        }

        // compute overwrite[Before|After] deltas BEFORE applying extra edits
        let info = self.get_overwrite_info(&item, flags & insert_flags::ALTERNATIVE_OVERWRITE_CONFIG != 0);

        // keep item in memory
        self.memory_service.memorize(&model, editor.get_position().expect("o editor com modelo tem posição"), &item);

        let is_resolved = item.is_resolved();

        // telemetry data points: duration of command execution, info about async additional edits (-1=n/a, -2=none, 1=success, 0=failed)
        let command_execution_duration: Rc<Cell<f64>> = Rc::new(Cell::new(-1.0));
        let additional_edits_applied_async: Rc<Cell<i32>> = Rc::new(Cell::new(-1));

        let additional_text_edits = item.completion.borrow().additional_text_edits.clone();
        if let Some(additional_text_edits) = additional_text_edits {
            // cancel -> stops all listening and closes widget
            self.model.cancel(false);

            // sync additional edits
            let scroll_state = StableEditorScrollState::capture(&editor);
            let edits = additional_text_edits
                .iter()
                .map(|edit| {
                    let mut range = edit.range;
                    if range.start_line_number == item.position.line_number && range.start_column > item.position.column {
                        // shift additional edit when it is "after" the completion insertion position
                        let column_delta = editor.get_position().expect("o editor com modelo tem posição").column - item.position.column;
                        let start_column_delta = column_delta;
                        let end_column_delta = if range.spans_multiple_lines() { 0 } else { column_delta };
                        range = Range::new(
                            range.start_line_number,
                            range.start_column + start_column_delta,
                            range.end_line_number,
                            range.end_column + end_column_delta,
                        );
                    }
                    EditOperation::replace_move(range, edit.text.clone())
                })
                .collect();
            editor.execute_edits(ExecuteEditsSource::Name(Some("suggestController.additionalTextEdits.sync")), edits, None);
            scroll_state.restore_relative_vertical_position_of_cursor(&editor);
        } else if !is_resolved {
            // async additional edits
            let sw = StopWatch::new(Some(true));
            let position: Rc<Cell<Option<Position>>> = Rc::new(Cell::new(None));

            let doc_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
            {
                let (cts, position, doc_listener_holder) = (Rc::clone(&cts), Rc::clone(&position), Rc::clone(&doc_listener));
                let listener = model.on_did_change_content().listen(move |e| {
                    if e.is_flush {
                        cts.cancel();
                        let listener = doc_listener_holder.borrow_mut().take();
                        if let Some(listener) = listener {
                            listener.dispose();
                        }
                        return;
                    }
                    for change in &e.changes {
                        let this_position = change.range.get_end_position();
                        if position.get().is_none_or(|current| this_position.is_before(&current)) {
                            position.set(Some(this_position));
                        }
                    }
                });
                *doc_listener.borrow_mut() = Some(listener);
            }

            let old_flags = flags;
            flags |= insert_flags::NO_AFTER_UNDO_STOP;
            let did_type = Rc::new(Cell::new(false));
            let type_listener: Rc<RefCell<Option<DisposableRef>>> = Rc::new(RefCell::new(None));
            {
                let (type_listener_holder, did_type, weak_editor) =
                    (Rc::clone(&type_listener), Rc::clone(&did_type), self.editor.clone());
                let listener = editor.on_will_type().listen(move |_| {
                    let listener = type_listener_holder.borrow_mut().take();
                    if let Some(listener) = listener {
                        listener.dispose();
                    }
                    did_type.set(true);
                    if old_flags & insert_flags::NO_AFTER_UNDO_STOP == 0
                        && let Some(editor) = weak_editor.upgrade()
                    {
                        editor.push_undo_stop();
                    }
                });
                *type_listener.borrow_mut() = Some(listener);
            }

            let resolved = {
                let (item, cts, position, did_type, weak_editor) =
                    (item.clone(), Rc::clone(&cts), Rc::clone(&position), Rc::clone(&did_type), self.editor.clone());
                item.resolve(&cts.token()).then(move |()| -> Option<bool> {
                    let additional_text_edits = item.completion.borrow().additional_text_edits.clone();
                    let Some(additional_text_edits) = additional_text_edits else {
                        return None;
                    };
                    if cts.token().is_cancellation_requested() {
                        return None;
                    }
                    let position = position.get();
                    if position.is_some_and(|position| {
                        additional_text_edits.iter().any(|edit| position.is_before(&edit.range.get_start_position()))
                    }) {
                        return Some(false);
                    }
                    let editor = weak_editor.upgrade()?;
                    if did_type.get() {
                        editor.push_undo_stop();
                    }
                    let scroll_state = StableEditorScrollState::capture(&editor);
                    editor.execute_edits(
                        ExecuteEditsSource::Name(Some("suggestController.additionalTextEdits.async")),
                        additional_text_edits.iter().map(|edit| EditOperation::replace_move(edit.range, edit.text.clone())).collect(),
                        None,
                    );
                    scroll_state.restore_relative_vertical_position_of_cursor(&editor);
                    if did_type.get() || old_flags & insert_flags::NO_AFTER_UNDO_STOP == 0 {
                        editor.push_undo_stop();
                    }
                    Some(true)
                })
            };
            let applied_logged = {
                let (log_service, additional_edits_applied_async) = (Rc::clone(&self.log_service), Rc::clone(&additional_edits_applied_async));
                resolved.then(move |applied: Option<bool>| {
                    let applied_value = match applied {
                        Some(applied) => Dynamic::Bool(applied),
                        None => Dynamic::Undefined,
                    };
                    log_service.trace_args(
                        "[suggest] async resolving of edits DONE (ms, applied?)",
                        &[LogArg::from(sw.elapsed()), LogArg::from(applied_value)],
                    );
                    additional_edits_applied_async.set(match applied {
                        Some(true) => 1,
                        Some(false) => 0,
                        None => -2,
                    });
                })
            };
            let finished = applied_logged.finally(move || {
                let listener = doc_listener.borrow_mut().take();
                if let Some(listener) = listener {
                    listener.dispose();
                }
                let listener = type_listener.borrow_mut().take();
                if let Some(listener) = listener {
                    listener.dispose();
                }
            });
            tasks.borrow_mut().push(finished);
        }

        let (insert_text, insert_text_rules) = {
            let completion = item.completion.borrow();
            (completion.insert_text.clone(), completion.insert_text_rules.unwrap_or(0))
        };
        let insert_text = JsString::from(insert_text);
        let insert_text = if insert_text_rules & completion_item_insert_text_rule::INSERT_AS_SNIPPET == 0 {
            SnippetParser::escape(insert_text.units())
        } else {
            insert_text
        };

        // cancel -> stops all listening and closes widget
        self.model.cancel(false);

        let overtyping_capturer: Rc<dyn IOvertypingCapturer> = self.overtyping_capturer.value();
        let provider_id = ProviderId::from_extension_id(item.completion.borrow().extension_id.as_ref().map(|id| id.value.as_str()));
        snippet_controller.insert(
            insert_text,
            Some(PartialSnippetInsertOptions {
                overwrite_before: Some(info.overwrite_before),
                overwrite_after: Some(info.overwrite_after),
                undo_stop_before: Some(false),
                undo_stop_after: Some(false),
                adjust_whitespace: Some(insert_text_rules & completion_item_insert_text_rule::KEEP_WHITESPACE == 0),
                clipboard_text: event.model.clipboard_text.clone().map(JsString::from),
                overtyping_capturer: Some(overtyping_capturer),
                reason: Some(EditSources::suggest(&provider_id)),
            }),
        );

        if flags & insert_flags::NO_AFTER_UNDO_STOP == 0 {
            editor.push_undo_stop();
        }

        let command = item.completion.borrow().command.clone();
        if let Some(command) = command {
            if command.id == TRIGGER_SUGGEST_ACTION_ID {
                // retigger
                self.model.trigger(SuggestTriggerOptions { auto: true, retrigger: Some(true), ..SuggestTriggerOptions::default() });
            } else {
                // exec command, done
                let sw = StopWatch::new(Some(true));
                let has_extension = item.completion.borrow().extension_id.is_some();
                let duration = Rc::clone(&command_execution_duration);
                tasks.borrow_mut().push(
                    self.command_service
                        .execute_command(&command.id, command.arguments.clone().unwrap_or_default())
                        .catch(move |error| {
                            if has_extension {
                                on_unexpected_external_error(&error);
                            } else {
                                on_unexpected_error(&error);
                            }
                            Dynamic::Undefined
                        })
                        .finally(move || {
                            duration.set(sw.elapsed());
                        })
                        .then(|_| ()),
                );
            }
        }

        if flags & insert_flags::KEEP_ALTERNATIVE_SUGGESTIONS != 0 {
            let (weak, cts, model) = (self.me.clone(), Rc::clone(&cts), Rc::clone(&model));
            self.alternatives.value().set(
                event,
                Rc::new(move |next: &ISelectedSuggestion| {
                    // cancel resolving of additional edits
                    cts.cancel();

                    // this is not so pretty. when inserting the 'next'
                    // suggestion we undo until we are at the state at
                    // which we were before inserting the previous suggestion...
                    if model.can_undo() {
                        if model_version_now != model.get_alternative_version_id() {
                            model.undo();
                        }
                        if let Some(this) = weak.upgrade() {
                            this.insert_suggestion(
                                Some(next),
                                insert_flags::NO_BEFORE_UNDO_STOP
                                    | insert_flags::NO_AFTER_UNDO_STOP
                                    | if flags & insert_flags::ALTERNATIVE_OVERWRITE_CONFIG != 0 {
                                        insert_flags::ALTERNATIVE_OVERWRITE_CONFIG
                                    } else {
                                        0
                                    },
                            );
                        }
                    }
                }),
            );
        }

        self.alert_completion_item(&item);

        // clear only now - after all tasks are done
        let tasks = tasks.borrow().clone();
        let (weak, cts, event_index, event_items) = (self.me.clone(), Rc::clone(&cts), event.index, event.model.items());
        Promise::<()>::all(tasks).finally(move || {
            if let Some(this) = weak.upgrade() {
                this.report_suggestion_accepted_telemetry(
                    &item,
                    &model,
                    is_resolved,
                    command_execution_duration.get(),
                    additional_edits_applied_async.get(),
                    event_index,
                    &event_items,
                );

                this.model.clear();
            }
            cts.dispose();
        });
    }

    fn report_suggestion_accepted_telemetry(
        &self,
        item: &CompletionItem,
        model: &Rc<TextModel>,
        item_resolved: bool,
        command_execution_duration: f64,
        additional_edits_applied_async: i32,
        index: usize,
        completion_items: &[CompletionItem],
    ) {
        if random_f64() > 0.0001 {
            // 0.01%
            return;
        }

        let mut label_map: HashMap<&str, Vec<usize>> = HashMap::new();

        for (i, completion_item) in completion_items.iter().take(30).enumerate() {
            label_map.entry(completion_item.text_label.as_str()).or_default().push(i);
        }

        let first_index_array = label_map.get(item.text_label.as_str());
        let has_duplicates = first_index_array.is_some_and(|array| array.len() > 1);
        let first_index = match first_index_array {
            Some(array) if has_duplicates => array[0] as f64,
            _ => -1.0,
        };

        let ext_uri = ExtUri::unbiased(Rc::new(ProcessEnvironment));
        let uri = model.uri();
        let basename_hash = hash(&Dynamic::string(&ext_uri.basename(&uri, None)));
        let resolve_info = if !item.provider.has_resolve_completion_item() {
            -1.0
        } else if item_resolved {
            1.0
        } else {
            0.0
        };
        let kind = item.completion.borrow().kind.to_index();

        self.telemetry_service.public_log2(
            "suggest.acceptedSuggestion",
            Some(Dynamic::object([
                // `item.extensionId?.value ?? 'unknown'`
                ("extensionId", Dynamic::string(item.completion.borrow().extension_id.as_ref().map_or("unknown", |id| id.value.as_str()))),
                ("providerId", Dynamic::string(item.provider.debug_display_name())),
                ("kind", Dynamic::Number(kind as f64)),
                ("basenameHash", Dynamic::string(&to_js_hex_string(basename_hash))),
                ("languageId", Dynamic::string(&model.get_language_id())),
                ("fileExtension", Dynamic::string(&ext_uri.extname(&uri))),
                ("resolveInfo", Dynamic::Number(resolve_info)),
                ("resolveDuration", Dynamic::Number(item.resolve_duration())),
                ("commandDuration", Dynamic::Number(command_execution_duration)),
                ("additionalEditsAsync", Dynamic::Number(f64::from(additional_edits_applied_async))),
                ("index", Dynamic::Number(index as f64)),
                ("firstIndex", Dynamic::Number(first_index)),
            ])),
        );
    }

    pub fn get_overwrite_info(&self, item: &CompletionItem, toggle_mode: bool) -> OverwriteInfo {
        let editor = self.editor();
        assert_type(&editor.has_model(), None);

        let mut replace = editor.get_option(SUGGEST).insert_mode == SuggestInsertMode::Replace;
        if toggle_mode {
            replace = !replace;
        }
        let overwrite_before = item.position.column - item.edit_start.column;
        let overwrite_after =
            (if replace { item.edit_replace_end.column } else { item.edit_insert_end.column }) - item.position.column;
        let position = editor.get_position().expect("o editor com modelo tem posição");
        let column_delta = position.column - item.position.column;
        let suffix_delta = self.line_suffix.value().map_or(0, |line_suffix| line_suffix.delta(position));

        OverwriteInfo { overwrite_before: overwrite_before + column_delta, overwrite_after: overwrite_after + suffix_delta }
    }

    fn alert_completion_item(&self, item: &CompletionItem) {
        let count = item.completion.borrow().additional_text_edits.as_ref().map_or(0, Vec::len);
        if count > 0 {
            let msg = localize(
                "aria.alert.snippet",
                "Accepting '{0}' made {1} additional edits",
                &[item.text_label.as_str(), count.to_string().as_str()],
            );
            (self.aria_alert)(&msg);
        }
    }

    /// `triggerSuggest(onlyFrom?, auto?, noFilter?)`.
    pub fn trigger_suggest(&self, only_from: Option<ProviderSet>, auto: Option<bool>, no_filter: Option<bool>) {
        let editor = self.editor();
        if editor.has_model() {
            self.model.trigger(SuggestTriggerOptions {
                auto: auto.unwrap_or(false),
                completion_options: Some(PartialCompletionOptions {
                    provider_filter: only_from,
                    kind_filter: no_filter.unwrap_or(false).then(std::collections::HashSet::new),
                    ..PartialCompletionOptions::default()
                }),
                ..SuggestTriggerOptions::default()
            });
            editor.reveal_position(editor.get_position().expect("o editor com modelo tem posição"), Some(ScrollType::Smooth));
            editor.focus();
        }
    }

    /// `acceptSelectedSuggestion(keepAlternativeSuggestions, alternativeOverwriteConfig)`.
    pub fn accept_selected_suggestion(&self, keep_alternative_suggestions: bool, alternative_overwrite_config: bool) {
        let item = self.focused_item.borrow().clone();
        let mut flags = 0;
        if keep_alternative_suggestions {
            flags |= insert_flags::KEEP_ALTERNATIVE_SUGGESTIONS;
        }
        if alternative_overwrite_config {
            flags |= insert_flags::ALTERNATIVE_OVERWRITE_CONFIG;
        }
        self.insert_suggestion(item.as_ref(), flags);
    }

    /// `acceptNextSuggestion()`.
    pub fn accept_next_suggestion(&self) {
        self.alternatives.value().next();
    }

    /// `acceptPrevSuggestion()`.
    pub fn accept_prev_suggestion(&self) {
        self.alternatives.value().prev();
    }
}

/// `Number.prototype.toString(16)` de um inteiro de 32 bits: o hexadecimal em minúsculas, com `-` para o negativo.
fn to_js_hex_string(value: i32) -> String {
    if value < 0 { format!("-{:x}", value.unsigned_abs()) } else { format!("{value:x}") }
}

impl IDisposable for SuggestController {
    fn dispose(&self) {
        self.alternatives.dispose();
        self.to_dispose.dispose();
        self.model.dispose();
        self.line_suffix.dispose();
        self.on_will_insert_suggest_item.dispose();
    }
}

impl IEditorContribution for SuggestController {}
