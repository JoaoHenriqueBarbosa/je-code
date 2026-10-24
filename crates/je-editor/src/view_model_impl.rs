//! Porte de `src/vs/editor/common/viewModel/viewModelImpl.ts`, linhas 51 a 573: a classe `ViewModel` (campos,
//! construtor, `dispose`, `getEditorOption`, o registro dos eventos do modelo, o tratamento dos eventos de
//! conteúdo, de decoração e de configuração). Os métodos públicos que vêm depois (linha 574 em diante) são a
//! parte B, em `view_model_impl_api.rs`, como outro bloco `impl ViewModel`. O que fica fora da classe
//! (`ViewportStart`, `HiddenAreasModel`, `StableViewport`, `OverviewRulerDecorations`, `IBatchableTarget`) está em
//! `view_model_impl_support.rs`.
//!
//! Representação:
//! - `ViewModel` é compartilhado (`Rc`) e muda de estado por trás de `&self` (`Cell`/`RefCell`), como o resto do
//!   crate. Os callbacks que no original capturam `this` guardam um `Weak<ViewModel<T>>` (campo `me`, preenchido
//!   por `Rc::new_cyclic`), o mesmo padrão de `ViewModelLinesFromProjectedModel`: o ouvinte sobe o `Weak` a cada
//!   chamada e some em silêncio se o view model já foi solto. [`ViewModel::register_listener`] faz isso uma vez
//!   só para todos os ouvintes e registra a inscrição no `Disposable` base (o `this._register(...)`).
//! - `IBatchableTarget.batchChanges<T>` é genérico, então o trait não é um objeto (ver `view_model_impl_support.rs`):
//!   o `ViewModel` é genérico no alvo transacional, `ViewModel<T: IBatchableTarget>`. A parte B usa
//!   `impl<T: IBatchableTarget + 'static> ViewModel<T>`.
//! - Os campos do struct são `pub(crate)` (os `public readonly` do original são `pub`). `viewLayout` e `_cursor`
//!   moram em `OnceCell`, porque o `CursorsController` recebe o próprio view model (`Rc<dyn CursorSimpleModel>`) e o
//!   `ViewLayout` é criado com `this.getLineCount()`: os dois só podem nascer depois de o `Rc` existir. O
//!   construtor preenche os dois antes de devolver; [`ViewModel::layout`] e [`ViewModel::cursor`] são os acessores.
//! - O `CursorsController` guarda o view model, e o view model guarda o controlador: um ciclo forte de `Rc` nunca
//!   seria solto. Por isso o controlador recebe um [`CursorViewModel`] (um `Weak` do view model que implementa
//!   `CursorSimpleModel`), não o `Rc` do próprio view model. A parte B precisa implementar `CursorSimpleModel`
//!   para `ViewModel<T>` (o `IViewModel` herda dele), e o adaptador chama essa implementação.
//! - `cursorConfig` é `RefCell<Rc<CursorConfiguration>>`: o `CursorConfiguration` não é `Clone` e o controlador
//!   recebe o mesmo `Rc`. A refeita do original (quatro cópias idênticas de `new CursorConfiguration(...)` mais
//!   `updateConfiguration`) é [`ViewModel::refresh_cursor_config`].
//! - Os eventos do modelo chegam como `ContentOrInjectedTextEvent` (o que o `TextModel` chama, em `text_model.rs`,
//!   pelo trait `ViewModel` de lá, que a struct implementa) e o cursor espera o `ContentOrInjectedTextChangeEvent`
//!   de `view_model_types.rs`: o mesmo par de variantes. As duas conversões (`From`) estão neste arquivo; a
//!   do cursor clona o evento. `onDidChangeContentOrInjectedText` e `emitContentChangeEvent` são, por isso, os
//!   métodos desse trait e não métodos próprios.
//! - O `ModelInjectedTextChangedEvent.changes` (lista de `ModelRawLineChanged`, que no original tem `changeType =
//!   LineChanged`) é tratado como uma lista de `ModelRawChange::LineChanged`.
//! - `try { ... } finally { endEmitViewEvents() }` é o guarda [`ViewEventsEmission`]: o `end` roda ao sair do
//!   escopo, também por pânico. A parte B o usa em `_emitViewEvent`.
//! - O argumento `domLineBreaksComputerFactory`/`monospaceLineBreaksComputerFactory` é `Rc<dyn LineBreaksComputerFactory>`;
//!   `scheduleAtNextAnimationFrame` é o [`ScheduleAtNextAnimationFrame`] de `je_base::scrollable`.
//! - `createLineBreaksComputer(context?)` tem o ciclo de vida das linhas: o computador empresta o contexto e o
//!   `ViewModel` (ver `IViewModelLines::create_line_breaks_computer`). O `IViewModel` de `view_model_types.rs` foi
//!   ajustado para a mesma assinatura (o método vive no `impl IViewModel`, em `view_model_impl_api.rs`, assim como
//!   `addViewEventHandler`, `removeViewEventHandler`, `setHasFocus`, `setHasWidgetFocus`, `onCompositionStart`,
//!   `onCompositionEnd` e `visibleLinesStabilized`, que a parte A não repete como métodos próprios).
//! - `previousHiddenAreas` é `Option<Vec<Range>>`: `None` é o `[]` inicial do original, que é outra instância (e
//!   por isso nunca `===`) do `[]` do `HiddenAreasModel`.
//! - `getEditorOption<T>(id)` é [`ViewModel::get_editor_option`], com a chave tipada `Opt<V>` do registro.
//! - A fonte (`source: unknown`) do `HiddenAreasModel` é [`HiddenAreasSource`], o id (identidade) do dono, ou
//!   `None` para o `undefined`.
//!
//! Divergências (dependências sem porte, com o trait mínimo necessário aqui):
//! - `IEditorConfiguration` (`config/editorConfiguration.ts`) é o trait completo de
//!   `config/editor_configuration.rs`. Ele estende o [`EditorConfiguration`] de `cursor_common` (as opções), então um
//!   `Rc<dyn IEditorConfiguration>` vira `Rc<dyn EditorConfiguration>` (coerção) para o `ViewLayout`, o
//!   `ViewModelDecorations` e o `CursorConfiguration`.
//! - `IThemeService` (`platform/theme/common/themeService.ts`): [`IThemeService`] traz só
//!   `onDidColorThemeChange`, com o `je_theme::ColorTheme` em `Rc`, como `ViewThemeChangedEvent`.
//! - `CustomLineHeightData.fromDecorations` (`line_heights.rs` o deixa pendente) é
//!   [`ViewModel::custom_line_heights_from_decorations`], com o corpo do original.
//! - `IAttachedView` é o [`AttachedView`] de `attached_views.rs` (`Rc`), e `IGlyphMarginLanesModel` é o
//!   [`GlyphMarginLanesModel`] concreto (o que o original instancia).
//!
//! O que a parte B precisa expor para este arquivo (como `pub(crate)`), e o que ela precisa implementar:
//! - `to_model_visible_ranges(&self, Range) -> Vec<Range>`, `invalidate_decorations_color_cache(&self)` e
//!   `emit_view_event<R>(&self, impl FnOnce(&ViewModelEventsCollector) -> R) -> R`;
//! - `impl CursorSimpleModel for ViewModel<T>` (métodos `getLineCount` e companhia), que [`CursorViewModel`] usa.

use std::borrow::Cow;
use std::cell::{Cell, OnceCell, RefCell};
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::arrays::ArrayQueue;
use je_base::async_::RunOnceScheduler;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::scrollable::{INewScrollPosition, ScheduleAtNextAnimationFrame};
use je_theme::ColorTheme;

use crate::attached_views::AttachedView;
use crate::config::editor_configuration::IEditorConfiguration;
use crate::config::editor_options::effective_options::{
    EFFECTIVE_ALLOW_VARIABLE_FONTS, EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH, FullwidthCharacterWidth,
};
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::line_height::LINE_HEIGHT;
use crate::config::editor_options::registry::{ALLOW_VARIABLE_LINE_HEIGHTS, WORD_BREAK, WRAP_ON_ESCAPED_LINE_FEEDS};
use crate::config::editor_options::wrapping_indent::{WRAPPING_INDENT, WrappingIndent};
use crate::config::editor_options::wrapping_info::WRAPPING_INFO;
use crate::config::editor_options::wrapping_strategy::{WRAPPING_STRATEGY, WrappingStrategyValue};
use crate::config::editor_options::{ComputedEditorOptions, ConfigurationChangedEvent, EditorOption, Opt};
use crate::config::font_info::FontInfo;
use crate::coordinates_converter::ICoordinatesConverter;
use crate::cursor::CursorsController;
use crate::cursor_common::{CursorConfiguration, CursorSimpleModel, EditorConfiguration};
use crate::editor_common::ScrollType;
use crate::glyph_lanes_model::GlyphMarginLanesModel;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::line_heights::CustomLineHeightData;
use crate::minimap_tokens_color_tracker::MinimapTokensColorTracker;
use crate::model::{ModelDecoration, PositionAffinity};
use crate::model_line_projection_data::{
    LineBreaksComputerFactory, WordBreak,
};
use crate::position::Position;
use crate::range::Range;
use crate::text_model::{ContentOrInjectedTextEvent, TextModel, ViewModel as TextModelViewModel};
use crate::text_model_events::{self as model_events, ModelLineHeightChanged, ModelRawChange};
use crate::view_events::{
    ViewConfigurationChangedEvent, ViewDecorationsChangedEvent, ViewEvent, ViewFlushedEvent,
    ViewLanguageConfigurationEvent,
    ViewLineMappingChangedEvent, ViewScrollChangedEvent, ViewThemeChangedEvent, ViewTokensChangedEvent,
    ViewTokensChangedRange, ViewTokensColorsChangedEvent,
};
use crate::view_layout::ViewLayout;
use crate::view_model_decorations::ViewModelDecorations;
use crate::view_model_event_dispatcher::{
    ModelContentChangedEvent, ModelDecorationsChangedEvent, ModelFontChangedEvent,
    ModelLanguageChangedEvent, ModelLanguageConfigurationChangedEvent, ModelLineHeightChangedEvent,
    ModelOptionsChangedEvent, ModelTokensChangedEvent, OutgoingViewModelEvent, ScrollChangedEvent,
    ViewModelEventDispatcher, ViewModelEventsCollector,
};
use crate::view_model_impl_support::{HiddenAreasModel, IBatchableTarget, StableViewport, ViewportStart};
use crate::view_model_lines::{
    IViewModelLines, ViewModelLinesFromModelAsIs, ViewModelLinesFromProjectedModel, WrappingStrategy,
};
use crate::view_model_types::{ContentOrInjectedTextChangeEvent, IViewLayout};

/// `USE_IDENTITY_LINES_COLLECTION`.
const USE_IDENTITY_LINES_COLLECTION: bool = true;

/// `IThemeService`, só o que o `ViewModel` lê (ver "Divergências" no topo do módulo).
pub trait IThemeService {
    /// `onDidColorThemeChange`.
    fn on_did_color_theme_change(&self) -> Event<Rc<ColorTheme>>;
}

/// A fonte (`source: unknown`) de `setHiddenAreas`: a identidade do dono (`id_of`), ou `None` (`undefined`).
pub type HiddenAreasSource = Option<usize>;

/// `try { const eventsCollector = beginEmitViewEvents(); ... } finally { endEmitViewEvents(); }`: o `end` roda ao
/// sair do escopo, por qualquer caminho.
pub(crate) struct ViewEventsEmission<'a> {
    dispatcher: &'a ViewModelEventDispatcher,
    pub(crate) events_collector: Rc<ViewModelEventsCollector>,
}

impl<'a> ViewEventsEmission<'a> {
    pub(crate) fn begin(dispatcher: &'a ViewModelEventDispatcher) -> Self {
        Self { events_collector: dispatcher.begin_emit_view_events(), dispatcher }
    }
}

impl Drop for ViewEventsEmission<'_> {
    fn drop(&mut self) {
        self.dispatcher.end_emit_view_events();
    }
}

impl From<&ContentOrInjectedTextEvent> for ContentOrInjectedTextChangeEvent {
    fn from(e: &ContentOrInjectedTextEvent) -> Self {
        match e {
            ContentOrInjectedTextEvent::Content(e) => ContentOrInjectedTextChangeEvent::Content(e.clone()),
            ContentOrInjectedTextEvent::InjectedText(e) => ContentOrInjectedTextChangeEvent::InjectedText(e.clone()),
        }
    }
}

impl From<ContentOrInjectedTextChangeEvent> for ContentOrInjectedTextEvent {
    fn from(e: ContentOrInjectedTextChangeEvent) -> Self {
        match e {
            ContentOrInjectedTextChangeEvent::Content(e) => ContentOrInjectedTextEvent::Content(e),
            ContentOrInjectedTextChangeEvent::InjectedText(e) => ContentOrInjectedTextEvent::InjectedText(e),
        }
    }
}

/// As opções de quebra de linha que o construtor e `_onConfigurationChanged` leem do mesmo jeito.
struct WrappingSettings {
    font_info: FontInfo,
    wrapping_strategy: WrappingStrategy,
    wrapping_column: f64,
    wrapping_indent: WrappingIndent,
    word_break: WordBreak,
    use_two_cell_fullwidth_characters: bool,
}

impl WrappingSettings {
    fn read(options: &ComputedEditorOptions) -> Self {
        Self {
            font_info: options.get(FONT_INFO).clone(),
            wrapping_strategy: match options.get(WRAPPING_STRATEGY) {
                WrappingStrategyValue::Simple => WrappingStrategy::Simple,
                WrappingStrategyValue::Advanced => WrappingStrategy::Advanced,
            },
            wrapping_column: options.get(WRAPPING_INFO).wrapping_column,
            wrapping_indent: *options.get(WRAPPING_INDENT),
            word_break: if options.get(WORD_BREAK) == "keepAll" { WordBreak::KeepAll } else { WordBreak::Normal },
            use_two_cell_fullwidth_characters: *options.get(EFFECTIVE_FULLWIDTH_CHARACTER_WIDTH)
                == FullwidthCharacterWidth::TwoCells,
        }
    }
}

/// `ViewModel`.
pub struct ViewModel<T: IBatchableTarget + 'static> {
    /// O `Rc` do próprio view model, para os callbacks que no original capturam `this`.
    pub(crate) me: Weak<ViewModel<T>>,
    /// O `Disposable` de que a classe original herda (`_register`).
    pub(crate) base: Rc<Disposable>,

    pub(crate) editor_id: i32,
    pub(crate) configuration: Rc<dyn IEditorConfiguration>,
    pub model: Rc<TextModel>,
    pub(crate) event_dispatcher: Rc<ViewModelEventDispatcher>,
    pub on_event: Event<OutgoingViewModelEvent>,
    /// `cursorConfig`.
    pub(crate) cursor_config: RefCell<Rc<CursorConfiguration>>,
    pub(crate) update_configuration_view_line_count: Rc<RunOnceScheduler>,
    pub(crate) has_focus: Cell<bool>,
    pub(crate) viewport_start: RefCell<ViewportStart>,
    pub(crate) lines: Rc<dyn IViewModelLines>,
    pub coordinates_converter: Rc<dyn ICoordinatesConverter>,
    /// `viewLayout`: preenchido pelo construtor, lido por [`ViewModel::layout`].
    pub(crate) view_layout: OnceCell<Rc<ViewLayout>>,
    /// `_cursor`: preenchido pelo construtor, lido por [`ViewModel::cursor`].
    pub(crate) cursor: OnceCell<Rc<CursorsController>>,
    pub(crate) decorations: ViewModelDecorations,
    pub glyph_lanes: Rc<GlyphMarginLanesModel>,

    // Os parâmetros `private readonly` do construtor.
    pub(crate) language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    pub(crate) theme_service: Rc<dyn IThemeService>,
    pub(crate) attached_view: Rc<AttachedView>,
    pub(crate) transactional_target: T,

    // Os campos declarados depois do construtor (linhas 571 e 572).
    pub(crate) hidden_areas_model: RefCell<HiddenAreasModel<HiddenAreasSource>>,
    /// `previousHiddenAreas`: `None` é o `[]` inicial do original, que não é a mesma instância do `[]` do
    /// `HiddenAreasModel` (então a primeira chamada de `setHiddenAreas` nunca sai cedo).
    pub(crate) previous_hidden_areas: RefCell<Option<Vec<Range>>>,
    /// O que os testes de `cursorMoveCommand.test.ts` fazem por atribuição (`viewModel.getCompletelyVisibleViewRange =
    /// () => new Range(...)`): quando preenchido, é o valor de `getCompletelyVisibleViewRange` (ver
    /// [`ViewModel::override_completely_visible_view_range`]).
    pub(crate) completely_visible_view_range_override: RefCell<Option<Range>>,
}

/// O que o `CursorsController` enxerga do view model: um `Weak`, para o controlador (que o view model possui) não
/// formar um ciclo de `Rc` com ele. Cada método chama o do view model.
pub struct CursorViewModel<T: IBatchableTarget + 'static>(Weak<ViewModel<T>>);

impl<T: IBatchableTarget + 'static> CursorViewModel<T> {
    fn view_model(&self) -> Rc<ViewModel<T>> {
        self.0.upgrade().expect("o controlador de cursores não vive mais que o view model que o possui")
    }
}

impl<T: IBatchableTarget + 'static> CursorSimpleModel for CursorViewModel<T> {
    fn get_line_count(&self) -> i32 {
        CursorSimpleModel::get_line_count(&*self.view_model())
    }

    fn get_line_content(&self, line_number: i32) -> JsString {
        CursorSimpleModel::get_line_content(&*self.view_model(), line_number)
    }

    fn get_line_min_column(&self, line_number: i32) -> i32 {
        CursorSimpleModel::get_line_min_column(&*self.view_model(), line_number)
    }

    fn get_line_max_column(&self, line_number: i32) -> i32 {
        CursorSimpleModel::get_line_max_column(&*self.view_model(), line_number)
    }

    fn get_line_first_non_whitespace_column(&self, line_number: i32) -> i32 {
        CursorSimpleModel::get_line_first_non_whitespace_column(&*self.view_model(), line_number)
    }

    fn get_line_last_non_whitespace_column(&self, line_number: i32) -> i32 {
        CursorSimpleModel::get_line_last_non_whitespace_column(&*self.view_model(), line_number)
    }

    fn normalize_position(&self, position: Position, affinity: PositionAffinity) -> Position {
        CursorSimpleModel::normalize_position(&*self.view_model(), position, affinity)
    }

    fn get_line_indent_column(&self, line_number: i32) -> i32 {
        CursorSimpleModel::get_line_indent_column(&*self.view_model(), line_number)
    }
}

impl<T: IBatchableTarget + 'static> ViewModel<T> {
    /// Substitui o resultado de `getCompletelyVisibleViewRange` pelo intervalo dado (`None` devolve o cálculo
    /// normal): a atribuição `viewModel.getCompletelyVisibleViewRange = () => range` que os testes do original
    /// fazem no objeto.
    pub fn override_completely_visible_view_range(&self, range: Option<Range>) {
        *self.completely_visible_view_range_override.borrow_mut() = range;
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        editor_id: i32,
        configuration: Rc<dyn IEditorConfiguration>,
        model: Rc<TextModel>,
        dom_line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory>,
        monospace_line_breaks_computer_factory: Rc<dyn LineBreaksComputerFactory>,
        schedule_at_next_animation_frame: ScheduleAtNextAnimationFrame,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
        theme_service: Rc<dyn IThemeService>,
        attached_view: Rc<AttachedView>,
        transactional_target: T,
    ) -> Rc<Self> {
        let event_dispatcher = ViewModelEventDispatcher::new();
        let on_event = event_dispatcher.on_event();
        let cursor_config = Self::create_cursor_configuration(&model, &configuration, &language_configuration_service);
        let viewport_start = ViewportStart::create(model.clone());

        let lines: Rc<dyn IViewModelLines> = if USE_IDENTITY_LINES_COLLECTION && model.is_too_large_for_tokenization() {
            Rc::new(ViewModelLinesFromModelAsIs::new(model.clone()))
        } else {
            let options = configuration.options();
            let wrapping = WrappingSettings::read(options);
            ViewModelLinesFromProjectedModel::new(
                editor_id,
                model.clone(),
                dom_line_breaks_computer_factory,
                monospace_line_breaks_computer_factory,
                wrapping.font_info,
                f64::from(model.get_options().tab_size),
                wrapping.wrapping_strategy,
                wrapping.wrapping_column,
                wrapping.wrapping_indent,
                wrapping.word_break,
                *options.get(WRAP_ON_ESCAPED_LINE_FEEDS),
                wrapping.use_two_cell_fullwidth_characters,
            )
        };

        let coordinates_converter: Rc<dyn ICoordinatesConverter> = Rc::from(lines.create_coordinates_converter());
        let editor_configuration: Rc<dyn EditorConfiguration> = configuration.clone();
        let decorations = ViewModelDecorations::new(
            editor_id,
            model.clone(),
            editor_configuration.clone(),
            lines.clone(),
            coordinates_converter.clone(),
        );

        let this = Rc::new_cyclic(|me: &Weak<ViewModel<T>>| {
            let scheduler_target = me.clone();
            Self {
                me: me.clone(),
                base: Disposable::new(),
                editor_id,
                configuration,
                model: model.clone(),
                event_dispatcher,
                on_event,
                cursor_config: RefCell::new(cursor_config),
                update_configuration_view_line_count: RunOnceScheduler::new(
                    move || {
                        if let Some(view_model) = scheduler_target.upgrade() {
                            view_model.update_configuration_view_line_count_now();
                        }
                    },
                    0,
                ),
                has_focus: Cell::new(false),
                viewport_start: RefCell::new(viewport_start),
                lines,
                coordinates_converter: coordinates_converter.clone(),
                view_layout: OnceCell::new(),
                cursor: OnceCell::new(),
                decorations,
                glyph_lanes: Rc::new(GlyphMarginLanesModel::new(0)),
                language_configuration_service,
                theme_service,
                attached_view,
                transactional_target,
                hidden_areas_model: RefCell::new(HiddenAreasModel::default()),
                previous_hidden_areas: RefCell::new(None),
                completely_visible_view_range_override: RefCell::new(None),
            }
        });
        this.base.register(this.update_configuration_view_line_count.clone());

        let cursor = Rc::new(CursorsController::new(
            model.clone(),
            Rc::new(CursorViewModel(this.me.clone())),
            coordinates_converter,
            this.cursor_config.borrow().clone(),
        ));
        this.base.register(cursor.clone());
        if this.cursor.set(cursor).is_err() {
            unreachable!("o cursor só é preenchido aqui");
        }

        let view_layout = ViewLayout::new(
            editor_configuration,
            this.get_line_count(),
            &this.get_custom_line_heights(),
            schedule_at_next_animation_frame,
        );
        this.base.register(view_layout.clone());
        if this.view_layout.set(view_layout).is_err() {
            unreachable!("o layout só é preenchido aqui");
        }

        this.register_listener(this.layout().on_did_scroll(), |view_model, e| {
            if e.scroll_top_changed {
                view_model.handle_visible_lines_changed();
            }
            if e.scroll_top_changed {
                view_model.viewport_start.borrow_mut().invalidate();
            }
            view_model.event_dispatcher.emit_single_view_event(ViewEvent::ScrollChanged(ViewScrollChangedEvent::new(e)));
            view_model.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::ScrollChanged(
                ScrollChangedEvent::new(
                    e.old_scroll_width,
                    e.old_scroll_left,
                    e.old_scroll_height,
                    e.old_scroll_top,
                    e.scroll_width,
                    e.scroll_left,
                    e.scroll_height,
                    e.scroll_top,
                ),
            ));
        });

        this.register_listener(this.layout().on_did_content_size_change(), |view_model, e| {
            view_model.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::ContentSizeChanged(e.clone()));
        });

        this.register_model_events();

        this.register_listener(this.configuration.on_did_change_fast(), |view_model, e| {
            let emission = ViewEventsEmission::begin(&view_model.event_dispatcher);
            view_model.on_configuration_changed(&emission.events_collector, e);
        });

        this.register_listener(MinimapTokensColorTracker::get_instance().on_did_change(), |view_model, _| {
            view_model.event_dispatcher.emit_single_view_event(ViewEvent::TokensColorsChanged(ViewTokensColorsChangedEvent));
        });

        this.register_listener(this.theme_service.on_did_color_theme_change(), |view_model, theme| {
            view_model.invalidate_decorations_color_cache();
            view_model.event_dispatcher.emit_single_view_event(ViewEvent::ThemeChanged(ViewThemeChangedEvent::new(theme.clone())));
        });

        this.update_configuration_view_line_count_now();
        let text_model_view_model: Rc<dyn TextModelViewModel> = this.clone();
        this.model.register_view_model(text_model_view_model);
        this
    }

    /// `viewLayout`.
    pub(crate) fn layout(&self) -> &Rc<ViewLayout> {
        self.view_layout.get().expect("o construtor preenche o layout antes de devolver o view model")
    }

    /// `_cursor`.
    pub(crate) fn cursor(&self) -> &Rc<CursorsController> {
        self.cursor.get().expect("o construtor preenche o cursor antes de devolver o view model")
    }

    /// `this._register(event(listener))`, com o callback recebendo o view model (o `this` do original) em vez de
    /// capturá-lo: o `Weak` sobe a cada chamada, e a chamada some se o view model já foi solto.
    fn register_listener<E: 'static>(&self, event: Event<E>, handler: impl Fn(&ViewModel<T>, &E) + 'static) {
        let me = self.me.clone();
        self.base.register(event.listen(move |e| {
            if let Some(view_model) = me.upgrade() {
                handler(&view_model, e);
            }
        }));
    }

    /// `new CursorConfiguration(this.model.getLanguageId(), this.model.getOptions(), this._configuration,
    /// this.languageConfigurationService)`.
    fn create_cursor_configuration(
        model: &TextModel,
        configuration: &Rc<dyn IEditorConfiguration>,
        language_configuration_service: &Rc<dyn ILanguageConfigurationService>,
    ) -> Rc<CursorConfiguration> {
        Rc::new(CursorConfiguration::new(
            &model.get_language_id(),
            &model.get_options(),
            &**configuration,
            language_configuration_service.clone(),
        ))
    }

    /// Refaz o `cursorConfig` e o entrega ao controlador (as quatro repetições de `this.cursorConfig = new
    /// CursorConfiguration(...); this._cursor.updateConfiguration(this.cursorConfig)` do original).
    pub(crate) fn refresh_cursor_config(&self) {
        let cursor_config =
            Self::create_cursor_configuration(&self.model, &self.configuration, &self.language_configuration_service);
        *self.cursor_config.borrow_mut() = cursor_config.clone();
        self.cursor().update_configuration(cursor_config);
    }

    pub fn get_editor_option<V: 'static>(&self, id: Opt<V>) -> &V {
        self.configuration.options().get(id)
    }

    /// `CustomLineHeightData.fromDecorations(decorations, coordinatesConverter, configuration)`.
    fn custom_line_heights_from_decorations(&self, decorations: &[ModelDecoration]) -> Vec<CustomLineHeightData> {
        let default_line_height = *self.configuration.options().get(LINE_HEIGHT);
        decorations
            .iter()
            .map(|d| {
                let view_range = self.coordinates_converter.convert_model_range_to_view_range(d.range, None);
                let line_height = match d.options.line_height {
                    Some(line_height) if line_height != 0.0 && !line_height.is_nan() => line_height * default_line_height,
                    _ => 0.0,
                };
                CustomLineHeightData::new(&d.id, view_range.start_line_number, view_range.end_line_number, line_height)
            })
            .collect()
    }

    /// `_getCustomLineHeights`.
    pub(crate) fn get_custom_line_heights(&self) -> Vec<CustomLineHeightData> {
        if !*self.configuration.options().get(ALLOW_VARIABLE_LINE_HEIGHTS) {
            return Vec::new();
        }
        let decorations = self.model.get_custom_line_heights_decorations(self.editor_id);
        self.custom_line_heights_from_decorations(&decorations)
    }

    /// `_getCustomLineHeightsForLines`.
    pub(crate) fn get_custom_line_heights_for_lines(&self, from_line_number: i32, to_line_number: i32) -> Vec<CustomLineHeightData> {
        if !*self.configuration.options().get(ALLOW_VARIABLE_LINE_HEIGHTS) {
            return Vec::new();
        }
        let model_range = Range::new(from_line_number, 1, to_line_number, self.model.get_line_max_column(to_line_number));
        let decorations = self.model.get_custom_line_heights_decorations_in_range(&model_range, self.editor_id);
        self.custom_line_heights_from_decorations(&decorations)
    }

    /// `_updateConfigurationViewLineCountNow`.
    pub(crate) fn update_configuration_view_line_count_now(&self) {
        self.configuration.set_view_line_count(self.lines.get_view_line_count());
    }

    /// `getModelVisibleRanges` seguido de `this._attachedView.setVisibleLines(modelVisibleRanges, stabilized)`: as
    /// duas únicas chamadas do original diferem só no `stabilized`.
    pub(crate) fn set_attached_view_visible_lines(&self, stabilized: bool) {
        let lines_viewport_data = self.layout().get_lines_viewport_data();
        let view_visible_range = Range::new(
            lines_viewport_data.start_line_number,
            self.get_line_min_column(lines_viewport_data.start_line_number),
            lines_viewport_data.end_line_number,
            self.get_line_max_column(lines_viewport_data.end_line_number),
        );
        let model_visible_ranges: Vec<(i32, i32)> = self
            .to_model_visible_ranges(view_visible_range)
            .iter()
            .map(|range| (range.start_line_number, range.end_line_number))
            .collect();
        self.attached_view.set_visible_lines(&model_visible_ranges, stabilized);
    }

    /// `_handleVisibleLinesChanged`.
    pub(crate) fn handle_visible_lines_changed(&self) {
        self.set_attached_view_visible_lines(false);
    }

    /// `_captureStableViewport`.
    pub(crate) fn capture_stable_viewport(&self) -> StableViewport {
        // We might need to restore the current start view range, so save it (if available)
        // But only if the scroll position is not at the top of the file
        let (is_valid, view_line_number, start_line_delta) = {
            let viewport_start = self.viewport_start.borrow();
            (viewport_start.is_valid(), viewport_start.view_line_number(), viewport_start.start_line_delta())
        };
        if is_valid && self.layout().get_current_scroll_top() > 0.0 {
            let previous_viewport_start_view_position =
                Position::new(view_line_number, self.get_line_min_column(view_line_number));
            let previous_viewport_start_model_position =
                self.coordinates_converter.convert_view_position_to_model_position(previous_viewport_start_view_position);
            return StableViewport::new(Some(previous_viewport_start_model_position), start_line_delta);
        }
        StableViewport::new(None, 0.0)
    }

    /// Os passos que o original repete depois de um `ViewLineMappingChangedEvent`: o evento, as decorações, o
    /// cursor e o cache de decorações.
    fn emit_line_mapping_changed(&self, events_collector: &ViewModelEventsCollector) {
        events_collector.emit_view_event(ViewEvent::LineMappingChanged(ViewLineMappingChangedEvent));
        events_collector.emit_view_event(ViewEvent::DecorationsChanged(ViewDecorationsChangedEvent::new(None)));
        self.cursor().on_line_mapping_changed(events_collector);
        self.decorations.on_line_mapping_changed();
    }

    /// `ViewFlushedEvent` mais o mapeamento de linhas e o `viewLayout.onFlushed`: a sequência de uma mudança de
    /// quebra de linha ou de tamanho do tab.
    pub(crate) fn emit_flushed_and_line_mapping_changed(&self, events_collector: &ViewModelEventsCollector) {
        events_collector.emit_view_event(ViewEvent::Flushed(ViewFlushedEvent));
        self.emit_line_mapping_changed(events_collector);
        self.layout().on_flushed(self.get_line_count(), &self.get_custom_line_heights());
    }

    /// `_onConfigurationChanged`.
    pub(crate) fn on_configuration_changed(&self, events_collector: &ViewModelEventsCollector, e: &ConfigurationChangedEvent) {
        let stable_viewport = self.capture_stable_viewport();
        let wrapping = WrappingSettings::read(self.configuration.options());

        if self.lines.set_wrapping_settings(
            &wrapping.font_info,
            wrapping.wrapping_strategy,
            wrapping.wrapping_column,
            wrapping.wrapping_indent,
            wrapping.word_break,
            wrapping.use_two_cell_fullwidth_characters,
        ) {
            self.emit_flushed_and_line_mapping_changed(events_collector);

            self.update_configuration_view_line_count.schedule(None);
        }

        if e.has_changed(EditorOption::ReadOnly) {
            // Must read again all decorations due to readOnly filtering
            self.decorations.reset();
            events_collector.emit_view_event(ViewEvent::DecorationsChanged(ViewDecorationsChangedEvent::new(None)));
        }

        if e.has_changed(EditorOption::RenderValidationDecorations) {
            self.decorations.reset();
            events_collector.emit_view_event(ViewEvent::DecorationsChanged(ViewDecorationsChangedEvent::new(None)));
        }

        events_collector.emit_view_event(ViewEvent::ConfigurationChanged(ViewConfigurationChangedEvent::new(e.clone())));
        self.layout().on_configuration_changed(e);

        stable_viewport.recover_viewport_start(&*self.coordinates_converter, &**self.layout());

        if CursorConfiguration::should_recreate(e) {
            self.refresh_cursor_config();
        }
    }

    /// `_registerModelEvents`.
    fn register_model_events(&self) {
        let options = self.configuration.options();

        if *options.get(ALLOW_VARIABLE_LINE_HEIGHTS) {
            self.register_listener(self.model.on_did_change_line_height(), |view_model, e| {
                let filtered_changes: Vec<ModelLineHeightChanged> = e
                    .changes
                    .iter()
                    .filter(|change| change.owner_id == view_model.editor_id || change.owner_id == 0)
                    .cloned()
                    .collect();

                view_model.layout().change_special_line_heights(|accessor| {
                    for change in &filtered_changes {
                        let view_range = view_model.coordinates_converter.convert_model_range_to_view_range(
                            Range::new(
                                change.line_number,
                                1,
                                change.line_number,
                                view_model.model.get_line_max_column(change.line_number),
                            ),
                            None,
                        );
                        match change.line_height_multiplier {
                            Some(line_height_multiplier) => accessor.insert_or_change_custom_line_height(
                                &change.decoration_id,
                                view_range.start_line_number,
                                view_range.end_line_number,
                                line_height_multiplier * *view_model.configuration.options().get(LINE_HEIGHT),
                            ),
                            None => accessor.remove_custom_line_height(&change.decoration_id),
                        }
                    }
                });

                // recreate the model event using the filtered changes
                if !filtered_changes.is_empty() {
                    let filtered_event = model_events::ModelLineHeightChangedEvent { changes: filtered_changes };
                    view_model.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::ModelLineHeightChanged(
                        ModelLineHeightChangedEvent::new(filtered_event),
                    ));
                }
            });
        }

        if *options.get(EFFECTIVE_ALLOW_VARIABLE_FONTS) {
            self.register_listener(self.model.on_did_change_font(), |view_model, e| {
                let filtered_changes: Vec<model_events::ModelFontChanged> = e
                    .changes
                    .iter()
                    .filter(|change| change.owner_id == view_model.editor_id || change.owner_id == 0)
                    .cloned()
                    .collect();
                // recreate the model event using the filtered changes
                if !filtered_changes.is_empty() {
                    let filtered_event = model_events::ModelFontChangedEvent { changes: filtered_changes };
                    view_model.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::ModelFontChanged(
                        ModelFontChangedEvent::new(filtered_event),
                    ));
                }
            });
        }

        self.register_listener(self.model.on_did_change_tokens(), |view_model, e| {
            let view_ranges = e
                .ranges
                .iter()
                .map(|model_range| {
                    let view_start_line_number = view_model
                        .coordinates_converter
                        .convert_model_position_to_view_position(Position::new(model_range.from_line_number, 1), None, None, None)
                        .line_number;
                    let view_end_line_number = view_model
                        .coordinates_converter
                        .convert_model_position_to_view_position(
                            Position::new(model_range.to_line_number, view_model.model.get_line_max_column(model_range.to_line_number)),
                            None,
                            None,
                            None,
                        )
                        .line_number;
                    ViewTokensChangedRange { from_line_number: view_start_line_number, to_line_number: view_end_line_number }
                })
                .collect();
            view_model.event_dispatcher.emit_single_view_event(ViewEvent::TokensChanged(ViewTokensChangedEvent::new(view_ranges)));
            view_model.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::ModelTokensChanged(
                ModelTokensChangedEvent::new(e.clone()),
            ));
        });

        self.register_listener(self.model.on_did_change_language_configuration(), |view_model, e| {
            view_model.event_dispatcher.emit_single_view_event(ViewEvent::LanguageConfigurationChanged(ViewLanguageConfigurationEvent));
            view_model.refresh_cursor_config();
            view_model.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::ModelLanguageConfigurationChanged(
                ModelLanguageConfigurationChangedEvent::new(*e),
            ));
        });

        self.register_listener(self.model.on_did_change_language(), |view_model, e| {
            view_model.refresh_cursor_config();
            view_model.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::ModelLanguageChanged(
                ModelLanguageChangedEvent::new(e.clone()),
            ));
        });

        self.register_listener(self.model.on_did_change_options(), |view_model, e| {
            // A tab size change causes a line mapping changed event => all view parts will repaint OK, no further event needed here
            if view_model.lines.set_tab_size(f64::from(view_model.model.get_options().tab_size)) {
                {
                    let emission = ViewEventsEmission::begin(&view_model.event_dispatcher);
                    view_model.emit_flushed_and_line_mapping_changed(&emission.events_collector);
                }
                view_model.update_configuration_view_line_count.schedule(None);
            }

            view_model.refresh_cursor_config();

            view_model.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::ModelOptionsChanged(
                ModelOptionsChangedEvent::new(*e),
            ));
        });

        self.register_listener(self.model.on_did_change_decorations(), |view_model, e| {
            view_model.decorations.on_model_decorations_changed();
            view_model.event_dispatcher.emit_single_view_event(ViewEvent::DecorationsChanged(ViewDecorationsChangedEvent::new(Some(e))));
            view_model.event_dispatcher.emit_outgoing_event(OutgoingViewModelEvent::ModelDecorationsChanged(
                ModelDecorationsChangedEvent::new(*e),
            ));
        });
    }
}

/// O que o `TextModel` chama no view model: `onDidChangeContentOrInjectedText` e `emitContentChangeEvent` ("Gets
/// called directly by the text model").
impl<T: IBatchableTarget + 'static> TextModelViewModel for ViewModel<T> {
    fn on_did_change_content_or_injected_text(&self, e: &ContentOrInjectedTextEvent) {
        {
            let emission = ViewEventsEmission::begin(&self.event_dispatcher);
            let events_collector: &ViewModelEventsCollector = &emission.events_collector;

            let mut had_other_model_change = false;
            let mut had_model_line_change_that_changed_line_mapping = false;

            let (changes, version_id): (Cow<'_, [ModelRawChange]>, Option<i64>) = match e {
                ContentOrInjectedTextEvent::Content(e) => (
                    Cow::Borrowed(e.raw_content_changed_event.changes.as_slice()),
                    Some(e.raw_content_changed_event.version_id),
                ),
                ContentOrInjectedTextEvent::InjectedText(e) => (
                    Cow::Owned(e.changes.iter().map(|change| ModelRawChange::LineChanged(*change)).collect()),
                    None,
                ),
            };

            // Do a first pass to compute line mappings, and a second pass to actually interpret them
            let line_breaks = {
                let mut line_breaks_computer = self.lines.create_line_breaks_computer(None);
                for change in changes.iter() {
                    match change {
                        ModelRawChange::LinesInserted(change) => {
                            for i in 0..change.count {
                                line_breaks_computer.add_request(change.from_line_number_post_edit + i, None);
                            }
                        }
                        ModelRawChange::LineChanged(change) => {
                            line_breaks_computer.add_request(change.line_number_post_edit, None);
                        }
                        _ => {}
                    }
                }
                line_breaks_computer.finalize()
            };
            let mut line_break_queue = ArrayQueue::new(&line_breaks);

            // Collect model line ranges that need custom line height computation.
            // We defer this until after the loop because the coordinatesConverter
            // relies on projections that may not yet reflect all changes in the batch.
            let mut custom_line_height_ranges_to_insert: Vec<(i32, i32)> = Vec::new();

            for change in changes.iter() {
                match change {
                    ModelRawChange::Flush => {
                        self.lines.on_model_flushed();
                        events_collector.emit_view_event(ViewEvent::Flushed(ViewFlushedEvent));
                        self.decorations.reset();
                        self.layout().on_flushed(self.get_line_count(), &self.get_custom_line_heights());
                        had_other_model_change = true;
                    }
                    ModelRawChange::LinesDeleted(change) => {
                        let lines_deleted_event =
                            self.lines.on_model_lines_deleted(version_id, change.from_line_number, change.to_line_number);
                        if let Some(lines_deleted_event) = lines_deleted_event {
                            events_collector.emit_view_event(ViewEvent::LinesDeleted(lines_deleted_event));
                            self.layout().on_lines_deleted(lines_deleted_event.from_line_number, lines_deleted_event.to_line_number);
                            custom_line_height_ranges_to_insert
                                .push((change.last_untouched_line_post_edit, change.last_untouched_line_post_edit));
                        }
                        had_other_model_change = true;
                    }
                    ModelRawChange::LinesInserted(change) => {
                        let inserted_line_breaks = line_break_queue.take_count(usize::try_from(change.count).unwrap_or_default());
                        let lines_inserted_event = self.lines.on_model_lines_inserted(
                            version_id,
                            change.from_line_number,
                            change.to_line_number(),
                            inserted_line_breaks,
                        );
                        if let Some(lines_inserted_event) = lines_inserted_event {
                            events_collector.emit_view_event(ViewEvent::LinesInserted(lines_inserted_event));
                            self.layout().on_lines_inserted(lines_inserted_event.from_line_number, lines_inserted_event.to_line_number);
                            custom_line_height_ranges_to_insert
                                .push((change.from_line_number_post_edit, change.to_line_number_post_edit()));
                        }
                        had_other_model_change = true;
                    }
                    ModelRawChange::LineChanged(change) => {
                        let changed_line_break_data =
                            line_break_queue.dequeue().expect("há um pedido de quebra de linha por linha alterada");
                        let (line_mapping_changed, lines_changed_event, lines_inserted_event, lines_deleted_event) =
                            self.lines.on_model_line_changed(version_id, change.line_number, changed_line_break_data);
                        had_model_line_change_that_changed_line_mapping = line_mapping_changed;
                        if let Some(lines_changed_event) = lines_changed_event {
                            events_collector.emit_view_event(ViewEvent::LinesChanged(lines_changed_event));
                        }
                        if let Some(lines_inserted_event) = lines_inserted_event {
                            events_collector.emit_view_event(ViewEvent::LinesInserted(lines_inserted_event));
                            self.layout().on_lines_inserted(lines_inserted_event.from_line_number, lines_inserted_event.to_line_number);
                            custom_line_height_ranges_to_insert.push((change.line_number_post_edit, change.line_number_post_edit));
                        }
                        if let Some(lines_deleted_event) = lines_deleted_event {
                            events_collector.emit_view_event(ViewEvent::LinesDeleted(lines_deleted_event));
                            self.layout().on_lines_deleted(lines_deleted_event.from_line_number, lines_deleted_event.to_line_number);
                            custom_line_height_ranges_to_insert.push((change.line_number_post_edit, change.line_number_post_edit));
                        }
                    }
                    ModelRawChange::EolChanged => {
                        // Nothing to do. The new version will be accepted below
                    }
                }
            }

            if let Some(version_id) = version_id {
                self.lines.accept_version_id(version_id);
            }

            // Apply deferred custom line heights now that projections are stable
            if !custom_line_height_ranges_to_insert.is_empty() {
                self.layout().change_special_line_heights(|accessor| {
                    for &(from_line_number, to_line_number) in &custom_line_height_ranges_to_insert {
                        for data in self.get_custom_line_heights_for_lines(from_line_number, to_line_number) {
                            accessor.insert_or_change_custom_line_height(
                                &data.decoration_id,
                                data.start_line_number,
                                data.end_line_number,
                                data.line_height,
                            );
                        }
                    }
                });
            }

            self.layout().on_height_maybe_changed();

            if !had_other_model_change && had_model_line_change_that_changed_line_mapping {
                self.emit_line_mapping_changed(events_collector);
            }
        }

        // Update the configuration and reset the centered view line
        let viewport_start_was_valid = self.viewport_start.borrow().is_valid();
        self.viewport_start.borrow_mut().invalidate();
        self.configuration.set_model_line_count(self.model.get_line_count());
        self.update_configuration_view_line_count_now();

        // Recover viewport
        if !self.has_focus.get() && self.model.get_attached_editor_count() >= 2 && viewport_start_was_valid {
            let (model_tracked_range, start_line_delta) = {
                let viewport_start = self.viewport_start.borrow();
                (viewport_start.model_tracked_range().to_owned(), viewport_start.start_line_delta())
            };
            if let Some(model_range) = self.model.get_tracked_range(&model_tracked_range) {
                let view_position = self
                    .coordinates_converter
                    .convert_model_position_to_view_position(model_range.get_start_position(), None, None, None);
                let view_position_top = self.layout().get_vertical_offset_for_line_number(view_position.line_number, None);
                self.layout().set_scroll_position(
                    INewScrollPosition { scroll_left: None, scroll_top: Some(view_position_top + start_line_delta) },
                    ScrollType::Immediate,
                );
            }
        }

        self.handle_visible_lines_changed();
    }

    fn emit_content_change_event(&self, e: &ContentOrInjectedTextEvent) {
        let cursor_event = ContentOrInjectedTextChangeEvent::from(e);
        self.emit_view_event(|events_collector| {
            if let ContentOrInjectedTextEvent::Content(e) = e {
                events_collector.emit_outgoing_event(OutgoingViewModelEvent::ModelContentChanged(
                    ModelContentChangedEvent::new(e.content_changed_event.clone()),
                ));
            }
            self.cursor().on_model_content_changed(events_collector, &cursor_event);
        });
    }
}

impl<T: IBatchableTarget + 'static> IDisposable for ViewModel<T> {
    fn dispose(&self) {
        // First remove listeners, as disposing the lines might end up sending
        // model decoration changed events ... and we no longer care about them ...
        self.base.dispose();
        self.decorations.dispose();
        self.lines.dispose();
        self.viewport_start.borrow().dispose();
        self.event_dispatcher.dispose();
        if let Some(me) = self.me.upgrade() {
            let text_model_view_model: Rc<dyn TextModelViewModel> = me;
            self.model.unregister_view_model(&text_model_view_model);
        }
    }
}

// A parte B (view_model_impl_api.rs, linhas 574 em diante) acrescenta, em outro `impl ViewModel`, os métodos:
// getFontSizeAtPosition, setHiddenAreas, getVisibleRangesPlusViewportAboveBelow, getVisibleRanges, getHiddenAreas,
// _toModelVisibleRanges, getCompletelyVisibleViewRange, getCompletelyVisibleViewRangeAtScrollTop,
// getViewRangeWithCursorPadding, saveState, reduceRestoreState, _reduceRestoreStateCompatibility, getTabSize,
// getLineCount, setViewport, getActiveIndentGuide, getLinesIndentGuides, getBracketGuidesInRangeByLine,
// getLineContent, getLineLength, getLineMinColumn, getLineMaxColumn, getLineFirstNonWhitespaceColumn,
// getLineLastNonWhitespaceColumn, getMinimapDecorationsInRange, getDecorationsInViewport, getInjectedTextAt,
// _getTextDirection, getTextDirection, getViewportViewLineRenderingData, getViewLineRenderingData,
// _getViewLineRenderingData, getViewLineData, getViewLineContinuesWithWrappedLine, getMinimapLinesRenderingData,
// getAllOverviewRulerDecorations, _invalidateDecorationsColorCache, getValueInRange, getValueLengthInRange,
// modifyPosition, deduceModelPositionRelativeToViewPosition, getPlainTextToCopy, getRichTextToCopy,
// _getHTMLToCopy, _getColorMap, getPrimaryCursorState, getLastAddedCursorIndex, getCursorStates, setCursorStates,
// getCursorColumnSelectData, getCursorAutoClosedCharacters, setCursorColumnSelectData, getPrevEditOperationType,
// setPrevEditOperationType, getSelection, getSelections, getPosition, setSelections, saveCursorState,
// restoreCursorState, _executeCursorEdit, executeEdits, startComposition, endComposition, type, compositionType,
// paste, cut, executeCommand, executeCommands, revealAllCursors, revealPrimaryCursor, revealTopMostCursor,
// revealBottomMostCursor, revealRange, changeWhitespace, _emitViewEvent, batchEvents, normalizePosition,
// getLineIndentColumn.
