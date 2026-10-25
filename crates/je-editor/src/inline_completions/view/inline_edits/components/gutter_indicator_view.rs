//! Porte de `src/vs/editor/contrib/inlineCompletions/browser/view/inlineEdits/components/gutterIndicatorView.ts`:
//! `InlineEditsGutterIndicator`, o indicador da margem (a pílula com o número da linha e o ícone de aceitar ou saltar),
//! mais `InlineEditsGutterIndicatorData`, `InlineSuggestionGutterMenuData` e `SimpleInlineSuggestModel`.
//!
//! Notas de tradução:
//! - o `n.div` da raiz (a área da margem coberta pela edição, a pílula, o número da linha e o ícone) é o
//!   [`GutterIndicatorVisual`], lido pelo pintor nativo em [`InlineEditsGutterIndicator::visual`] (ver as notas de
//!   `inline_edits_views`). O `mapOutFalsy(this._layout)` do original só mantém a árvore DOM estável entre recálculos; o
//!   `!layout ? [] : [...]` é o `None` do visual. O `keepUpdated` é o `recompute_initially_and_on_change`. O `rectToProps`
//!   é o retângulo direto: quem pinta lê `left`, `top`, `width()` e `height()`. O `style` fixo do original vira constantes
//!   (`position: absolute`, `zIndex: 20` em [`PILL_Z_INDEX`], `cursor: pointer`, `tabIndex: 0`, `boxSizing: border-box`,
//!   borda de [`PILL_BORDER_WIDTH`] e os `transition` de [`TRANSITION_SECONDS`]). A transição só vale com
//!   [`GutterIndicatorVisual::animate_transition`] (sem o modificador pressionado, como o `transition: ''` do original);
//! - `applyFontInfo(indicator.element)` copia a fonte do editor para o elemento, por causa do número da linha: o pintor
//!   nativo desenha o número com as opções de fonte do editor, então o `autorun` do original não existe aqui;
//! - `renderIcon(icon)` é o [`ThemeIcon`] em [`GutterIconVisual::icon`], que o pintor desenha como codicon. Os ícones do
//!   original (`Codicon.keyboardTab`, `arrowRight`, `check`, `keyboardTabAbove`, `keyboardTabBelow`) são os ids dos
//!   codicons, criados uma vez por view (a identidade que o `===` do original compara). `asCssVariable(color.id)` é o
//!   id do tema em [`GutterIconVisual::color`]. O `--vscodeIconForeground` é o [`GutterIndicatorVisual::foreground`];
//! - `withStyles(..., { fontSize })` é o [`GutterIconVisual::font_size`] em pixels. `rotate(Ndeg)` é o
//!   [`GutterIconVisual::rotation_degrees`];
//! - as cores `getEditorBlendedColor(...).toString()` ficam como [`Color`] (o pintor serializa);
//! - o `onclick`, o `onmouseenter` e o `onmouseleave` do ícone são [`InlineEditsGutterIndicator::notify_click`],
//!   [`InlineEditsGutterIndicator::notify_mouse_enter`] e [`InlineEditsGutterIndicator::notify_mouse_leave`], que o pintor
//!   chama (o `focus()` do editor no clique é feito aqui). `readModifierKeyStatus(...).shiftKey` é o
//!   [`InlineEditsGutterIndicator::notify_shift_key`], como nas views irmãs (o estado do shift enquanto o editor tem o foco);
//! - `StickyScrollController.get(editor)` (e o evento `onDidChangeStickyScrollHeight`) ainda não tem porte: a altura do
//!   sticky scroll chega pelo [`IGutterIndicatorHost::sticky_scroll_height`] (`0` sem controller, como o `constObservable(0)`
//!   do original);
//! - **ponto de abertura do menu.** `_showHover` cria o `GutterIndicatorMenuContent` (fatia seguinte), mede o foco nele com
//!   `createFocusTracker` e o mostra com `IHoverService.showInstantHover`. Essas três peças são do menu, então a view só
//!   conhece o contrato [`IGutterIndicatorHost::show_menu`]: recebe o [`GutterIndicatorMenuRequest`] (o editor, o
//!   [`InlineSuggestionGutterMenuData`], o retângulo da pílula que serve de alvo e o `onClose` do original, que repassa o
//!   `commandId` por [`InlineEditsGutterIndicator::on_did_close_with_command`]) e devolve a [`IGutterIndicatorMenuSession`]
//!   (o foco dentro do menu, o descarte e o evento de descarte), ou `None` quando o hover não aparece (o `showInstantHover`
//!   devolve `undefined`). A view mantém o resto do `_showHover`: a guarda `_hoverVisible`, o descarte ao rolar o editor e o
//!   `_focusIsInMenu`. A fatia do `gutterIndicatorMenu` implementa o contrato;
//! - `@IInstantiationService`, `@IHoverService` e `@IUserInteractionService` saem do construtor pelos motivos acima;
//!   `@IThemeService` é o `Rc<dyn IThemeService>` e `@IAccessibilityService` o `Rc<dyn IAccessibilityService>`;
//! - `triggerAnimation()` anima o elemento com `Element.animate` (Web Animations) e devolve `animation.finished`. Aqui o
//!   pintor nativo é o dono do relógio, como na view colapsada: [`InlineEditsGutterIndicator::trigger_animation`] publica
//!   um [`GutterIndicatorPulse`] em [`InlineEditsGutterIndicator::pulse`] e devolve a promessa que o `finish` dele resolve.
//!   O pintor mede o tempo desde que viu o pulso, aplica o contorno de [`GutterIndicatorPulse::outline_offset_at`] e
//!   [`GutterIndicatorPulse::outline_opacity_at`] à pílula e chama `finish` quando passa de [`PULSE_DURATION_MS`]. Com
//!   movimento reduzido o original devolve `new Animation(null, null).finished`, uma promessa já resolvida. Um pulso novo
//!   substitui o anterior (o elemento é um só) e resolve a promessa dele. Os keyframes são interpolados linearmente, e a
//!   cor do contorno vai da borda opaca à transparente (interpolação com alfa pré-multiplicado: o RGB fica e o alfa cai);
//! - `InlineSuggestionGutterMenuData.setModelId` é `provider.setModelId?.bind(provider)`: o trait do provedor tem o método
//!   com corpo vazio por padrão, então o `undefined` do original não existe e o campo é sempre `Some`; quem esconde o seletor
//!   de modelo é a ausência do `modelInfo`;
//! - `SimpleInlineSuggestModel.fromInlineCompletionModel(model)` chama `model.accept()`, que no original usa o editor do
//!   modelo (`editor = this._editor`): aqui é o [`InlineCompletionsModel::editor`]. A promessa do `accept` é descartada,
//!   como no `() => model.accept()`;
//! - `originalRange` é um [`LineRange`] por valor: o `===` do `mapOutFalsy` do original compara o objeto, e o observável
//!   daqui só avisa quando o intervalo mudou de fato;
//! - o `Math.round` de `observeLineOffsetRange` já está no [`ObservableCodeEditor`]; o `Rect.fromRanges` de
//!   `gutterEditArea` é montado direto com as bordas, já que o `OffsetRange` do crate é inteiro e o recuo da margem é
//!   fracionário.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::dynamic::{Dynamic, number_to_js_string};
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, to_disposable};
use je_base::nls::localize;
use je_base::observable::observables::observable_value::ObservableValue;
use je_base::observable::utils::utils::{DebounceMs, debounced_observable};
use je_base::observable::{
    IObservable, Observable, ObservableExt, SettableExt, autorun, const_observable, derived, derived_with_store, observable_value, run_on_change,
};
use je_base::promise::{Promise, Resolver};
use je_base::themables::ThemeIcon;
use je_platform::accessibility::IAccessibilityService;
use je_theme::color::Color;

use crate::code_action::languages::Command;
use crate::code_editor_widget::ICodeEditor;
use crate::config::editor_options::editor_font_info::FONT_INFO;
use crate::config::editor_options::line_numbers::{LINE_NUMBERS, RenderLineNumbersType};
use crate::core_2d::rect::Rect;
use crate::cursor_common::CursorSimpleModel;
use crate::inline_completions::inline_completions_model::InlineCompletionsModel;
use crate::inline_completions::inline_suggest_alternative_action::InlineSuggestAlternativeAction;
use crate::inline_completions::inline_suggestion_item::{InlineSuggestionAction, InlineSuggestionItem, InlineSuggestionItemBase};
use crate::languages::{IInlineCompletionModelInfo, InlineCompletionCommand};
use crate::line_range::LineRange;
use crate::observable_code_editor::{ObservableCodeEditor, ValueOrObservable};
use crate::offset_range::OffsetRange;
use crate::services::theme_service::IThemeService;

use super::super::inline_edits_view_interface::InlineEditTabAction;
use super::super::inline_edits_views::NonOverflowOverlayWidget;
use super::super::utils::utils::map_out_falsy;
use super::super::theme::{
    INLINE_EDIT_INDICATOR_BACKGROUND, INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND, INLINE_EDIT_INDICATOR_PRIMARY_BORDER,
    INLINE_EDIT_INDICATOR_PRIMARY_FOREGROUND, INLINE_EDIT_INDICATOR_SECONDARY_BACKGROUND, INLINE_EDIT_INDICATOR_SECONDARY_BORDER,
    INLINE_EDIT_INDICATOR_SECONDARY_FOREGROUND, INLINE_EDIT_INDICATOR_SUCCESSFUL_BACKGROUND, INLINE_EDIT_INDICATOR_SUCCESSFUL_BORDER,
    INLINE_EDIT_INDICATOR_SUCCESSFUL_FOREGROUND, INLINE_EDITS_BORDER_RADIUS, get_editor_blended_color,
};

/// `CODICON_SIZE_PX`.
const CODICON_SIZE_PX: f64 = 16.0;
/// `CODICON_PADDING_PX`.
const CODICON_PADDING_PX: f64 = 2.0;
/// `idealIconAreaWidth`.
const IDEAL_ICON_AREA_WIDTH: f64 = 22.0;
/// `gutterViewPortPaddingLeft`.
const GUTTER_VIEWPORT_PADDING_LEFT: f64 = 1.0;
/// `gutterViewPortPaddingTop`.
const GUTTER_VIEWPORT_PADDING_TOP: f64 = 2.0;

/// O `zIndex: '20'` da pílula.
pub const PILL_Z_INDEX: i32 = 20;
/// A borda de `1px solid` da pílula.
pub const PILL_BORDER_WIDTH: f64 = 1.0;
/// O raio de borda da área da margem e da pílula (`INLINE_EDITS_BORDER_RADIUS`).
pub const GUTTER_BORDER_RADIUS: f64 = INLINE_EDITS_BORDER_RADIUS;
/// A duração (`0.2s`) das transições da pílula (`background-color` e `width`) e do ícone (`rotate` e `opacity`).
pub const TRANSITION_SECONDS: f64 = 0.2;
/// O deslocamento à direita do ícone quando ele aponta para cima (`right: '1px'`).
pub const ICON_TOP_RIGHT_OFFSET: f64 = 1.0;

/// O atraso do `debouncedObservable` do passar do mouse (em ms).
const HOVER_DEBOUNCE_MS: u64 = 100;

/// `{ duration: 500 }` do pulso.
pub const PULSE_DURATION_MS: f64 = 500.0;
/// O `outline: 2px solid ...` do pulso.
pub const PULSE_OUTLINE_WIDTH: f64 = 2.0;
/// O `outlineOffset` do primeiro keyframe.
pub const PULSE_OUTLINE_OFFSET_FROM: f64 = -1.0;
/// O `outlineOffset` do último keyframe.
pub const PULSE_OUTLINE_OFFSET_TO: f64 = 10.0;

/// `GutterIndicatorCustomization`: as opções de aparência e comportamento do indicador.
#[derive(Clone, Debug, Default)]
pub struct GutterIndicatorCustomization {
    /// Override the default icon.
    pub icon: Option<ThemeIcon>,
}

/// `SimpleInlineSuggestModel`. O original anota que esta classe ainda não faz muito sentido.
pub struct SimpleInlineSuggestModel {
    pub accept: Rc<dyn Fn()>,
    pub jump: Rc<dyn Fn()>,
}

impl SimpleInlineSuggestModel {
    pub fn new(accept: impl Fn() + 'static, jump: impl Fn() + 'static) -> Self {
        Self { accept: Rc::new(accept), jump: Rc::new(jump) }
    }

    /// `SimpleInlineSuggestModel.fromInlineCompletionModel(model)`.
    pub fn from_inline_completion_model(model: &Rc<InlineCompletionsModel>) -> Self {
        let accept_model = Rc::clone(model);
        let jump_model = Rc::clone(model);
        Self::new(
            move || {
                let editor = Rc::clone(accept_model.editor());
                let _ = accept_model.accept(&editor, false);
            },
            move || jump_model.jump(),
        )
    }
}

/// `InlineSuggestionGutterMenuData`.
#[derive(Clone)]
pub struct InlineSuggestionGutterMenuData {
    pub action: Option<Command>,
    pub display_name: String,
    pub extension_commands: Vec<Vec<InlineCompletionCommand>>,
    pub alternative_action: Option<InlineSuggestAlternativeAction>,
    pub model_info: Option<IInlineCompletionModelInfo>,
    pub set_model_id: Option<Rc<dyn Fn(&str) -> Promise<()>>>,
    pub extension_commands_only: bool,
}

impl InlineSuggestionGutterMenuData {
    /// `InlineSuggestionGutterMenuData.fromInlineSuggestion(suggestion)`.
    pub fn from_inline_suggestion(suggestion: &InlineSuggestionItem) -> Self {
        let alternative_action = match suggestion.action() {
            Some(InlineSuggestionAction::Edit(edit)) => edit.alternative_action,
            _ => None,
        };
        let source = suggestion.source();
        let commands = source.inline_suggestions.commands.clone().unwrap_or_default();
        let provider = Rc::clone(&source.provider);
        let display_name = provider.display_name().map_or_else(|| localize("inlineSuggestion", "Inline Suggestion", &[]), str::to_owned);
        let model_info = provider.model_info();
        let set_model_id: Rc<dyn Fn(&str) -> Promise<()>> = Rc::new(move |model_id: &str| provider.set_model_id(model_id));
        Self::new(
            suggestion.gutter_menu_link_action(),
            display_name,
            if commands.is_empty() { Vec::new() } else { vec![commands] },
            alternative_action,
            model_info,
            Some(set_model_id),
            false,
        )
    }

    pub fn new(
        action: Option<Command>,
        display_name: String,
        extension_commands: Vec<Vec<InlineCompletionCommand>>,
        alternative_action: Option<InlineSuggestAlternativeAction>,
        model_info: Option<IInlineCompletionModelInfo>,
        set_model_id: Option<Rc<dyn Fn(&str) -> Promise<()>>>,
        extension_commands_only: bool,
    ) -> Self {
        Self { action, display_name, extension_commands, alternative_action, model_info, set_model_id, extension_commands_only }
    }
}

/// `InlineEditsGutterIndicatorData`.
pub struct InlineEditsGutterIndicatorData {
    pub gutter_menu_data: Rc<InlineSuggestionGutterMenuData>,
    pub original_range: LineRange,
    pub model: SimpleInlineSuggestModel,
    pub alt_action: Option<InlineSuggestAlternativeAction>,
    pub customization: Option<GutterIndicatorCustomization>,
}

impl InlineEditsGutterIndicatorData {
    pub fn new(
        gutter_menu_data: Rc<InlineSuggestionGutterMenuData>,
        original_range: LineRange,
        model: SimpleInlineSuggestModel,
        alt_action: Option<InlineSuggestAlternativeAction>,
        customization: Option<GutterIndicatorCustomization>,
    ) -> Self {
        Self { gutter_menu_data, original_range, model, alt_action, customization }
    }
}

/// O callback `(focusEditor, commandId) => void` que o menu chama ao se fechar.
pub type GutterMenuCloseCallback = Rc<dyn Fn(bool, Option<String>)>;

/// O que o menu do indicador recebe para abrir (ver as notas do módulo).
pub struct GutterIndicatorMenuRequest {
    pub editor: Rc<ObservableCodeEditor>,
    pub data: Rc<InlineSuggestionGutterMenuData>,
    /// O retângulo da pílula (o `this._iconRef.element` que o original passa como `target` do hover), nas coordenadas do
    /// overlay do editor.
    pub target: Rect,
    pub on_close: GutterMenuCloseCallback,
}

/// O menu aberto: o conteúdo (`GutterIndicatorMenuContent`) dentro do hover.
pub trait IGutterIndicatorMenuSession {
    /// O foco dentro do conteúdo do menu (o `createFocusTracker(content.element)` do original).
    fn is_focused(&self) -> IObservable<bool>;

    /// `h.onDispose`: o hover foi descartado.
    fn on_did_dispose(&self) -> Event<()>;

    /// `h.dispose()`.
    fn dispose(&self);
}

/// O que o indicador não tem em si e a fatia do menu (e o sticky scroll) fornece.
pub trait IGutterIndicatorHost {
    /// `stickyScrollWidgetHeight` do `StickyScrollController` do editor, que muda com `onDidChangeStickyScrollHeight`
    /// (`const_observable(0.0)` sem controller).
    fn sticky_scroll_height(&self) -> IObservable<f64>;

    /// O corpo de `_showHover` que cria e mostra o menu: `None` quando o hover não aparece.
    fn show_menu(&self, request: GutterIndicatorMenuRequest) -> Option<Rc<dyn IGutterIndicatorMenuSession>>;
}

/// Os serviços que o construtor recebe.
pub struct GutterIndicatorServices {
    pub theme_service: Rc<dyn IThemeService>,
    pub accessibility_service: Rc<dyn IAccessibilityService>,
    pub host: Rc<dyn IGutterIndicatorHost>,
}

/// `iconDirection`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GutterIconDirection {
    Top,
    Bottom,
    Right,
}

impl GutterIconDirection {
    /// `getRotationFromDirection(direction)`.
    pub fn rotation_degrees(self) -> f64 {
        match self {
            Self::Top => 90.0,
            Self::Bottom => -90.0,
            Self::Right => 0.0,
        }
    }
}

/// O `justifyContent` da pílula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GutterJustifyContent {
    FlexStart,
    FlexEnd,
}

/// O `_layout` do original.
pub struct GutterIndicatorLayout {
    pub gutter_edit_area: Rect,
    pub icon: IObservable<Rc<ThemeIcon>>,
    pub icon_direction: GutterIconDirection,
    pub icon_rect: Rect,
    pub icon_visible: bool,
    pub pill_rect: Rect,
    pub line_number_rect: Option<Rect>,
}

/// O número da linha dentro da pílula (o `div.line-number`).
#[derive(Clone)]
pub struct GutterLineNumberVisual {
    pub width: f64,
    /// O `lineHeight` (a altura do retângulo do número).
    pub line_height: f64,
    /// `_lineNumberToRender` (vazio quando o número não é mostrado).
    pub text: String,
}

/// O ícone dentro da pílula.
#[derive(Clone)]
pub struct GutterIconVisual {
    pub icon: Rc<ThemeIcon>,
    pub rotation_degrees: f64,
    /// `iconVisible ? '1' : '0'`.
    pub opacity: f64,
    pub margin_right: f64,
    pub width: f64,
    /// `right: iconDirection === 'top' ? '1px' : '0'`.
    pub right_offset: f64,
    pub font_size: f64,
    /// O id da cor do tema do ícone personalizado (`asCssVariable(color.id)`).
    pub color: Option<String>,
}

/// O que o pintor nativo lê da view: a área da margem, a pílula e o que há dentro dela.
#[derive(Clone)]
pub struct GutterIndicatorVisual {
    pub gutter_edit_area: Rect,
    /// O identificador do tema do fundo da área (`inlineEditIndicatorBackground`).
    pub gutter_edit_area_background: &'static str,
    pub pill_rect: Rect,
    pub background: Rc<Color>,
    /// A cor do número da linha e do ícone (`--vscodeIconForeground`).
    pub foreground: Rc<Color>,
    pub border: Rc<Color>,
    pub justify_content: GutterJustifyContent,
    /// `false` quando o modificador está pressionado (o `transition` vazio).
    pub animate_transition: bool,
    /// `None` quando o `lineNumberRect` não existe (o `display: 'none'`).
    pub line_number: Option<GutterLineNumberVisual>,
    pub icon: GutterIconVisual,
}

/// `_gutterIndicatorStyles`.
struct GutterIndicatorStyles {
    background: Rc<Color>,
    foreground: Rc<Color>,
    border: Rc<Color>,
}

/// O pulso em andamento, que o pintor nativo anima (ver as notas do módulo).
pub struct GutterIndicatorPulse {
    /// A cor da borda no começo do pulso (o `outline` do primeiro keyframe).
    pub border: Rc<Color>,
    finished: Resolver<()>,
}

impl GutterIndicatorPulse {
    fn progress(elapsed_ms: f64) -> f64 {
        (elapsed_ms / PULSE_DURATION_MS).clamp(0.0, 1.0)
    }

    /// O `outlineOffset` (em pixels) `elapsed_ms` depois do início: de [`PULSE_OUTLINE_OFFSET_FROM`] a
    /// [`PULSE_OUTLINE_OFFSET_TO`].
    pub fn outline_offset_at(elapsed_ms: f64) -> f64 {
        PULSE_OUTLINE_OFFSET_FROM + (PULSE_OUTLINE_OFFSET_TO - PULSE_OUTLINE_OFFSET_FROM) * Self::progress(elapsed_ms)
    }

    /// O fator que multiplica o alfa da borda no contorno: de `1` (borda opaca) a `0` (transparente).
    pub fn outline_opacity_at(elapsed_ms: f64) -> f64 {
        1.0 - Self::progress(elapsed_ms)
    }

    /// O `animation.finished` resolve: o pintor chama quando o pulso terminou.
    pub fn finish(&self) {
        self.finished.resolve(());
    }
}

/// A parte de `triggerAnimation` que o callback do `runOnChange` também usa.
struct PulseController {
    accessibility_service: Rc<dyn IAccessibilityService>,
    styles: IObservable<Rc<GutterIndicatorStyles>>,
    pulse: Rc<ObservableValue<Option<Rc<GutterIndicatorPulse>>, ()>>,
}

impl PulseController {
    fn trigger(&self) -> Promise<()> {
        if self.accessibility_service.is_motion_reduced() {
            return Promise::resolved(());
        }

        // PULSE ANIMATION:
        let (finished, resolver) = Promise::new();
        if let Some(previous) = self.pulse.get() {
            previous.finish();
        }
        let border = Rc::clone(&self.styles.get().border);
        self.pulse.set(Some(Rc::new(GutterIndicatorPulse { border, finished: resolver })), None);
        finished
    }
}

/// Os ícones do original (`Codicon.keyboardTab`, `arrowRight`, `check`, `keyboardTabAbove`, `keyboardTabBelow`), um
/// objeto por ícone.
struct StockIcons {
    keyboard_tab: Rc<ThemeIcon>,
    arrow_right: Rc<ThemeIcon>,
    check: Rc<ThemeIcon>,
    keyboard_tab_above: Rc<ThemeIcon>,
    keyboard_tab_below: Rc<ThemeIcon>,
}

impl StockIcons {
    fn new() -> Rc<Self> {
        Rc::new(Self {
            keyboard_tab: Rc::new(ThemeIcon::from_id("keyboard-tab")),
            arrow_right: Rc::new(ThemeIcon::from_id("arrow-right")),
            check: Rc::new(ThemeIcon::from_id("check")),
            keyboard_tab_above: Rc::new(ThemeIcon::from_id("keyboard-tab-above")),
            keyboard_tab_below: Rc::new(ThemeIcon::from_id("keyboard-tab-below")),
        })
    }

    fn is_check(icon: &ThemeIcon) -> bool {
        icon.id == "check"
    }
}

/// O `_state` do original.
struct GutterState {
    range: IObservable<LineRange>,
    line_offset_range: IObservable<OffsetRange>,
}

/// `InlineEditsGutterIndicator`.
pub struct InlineEditsGutterIndicator {
    base: Rc<Disposable>,
    editor: Rc<ObservableCodeEditor>,
    data: IObservable<Option<Rc<InlineEditsGutterIndicatorData>>>,
    layout: IObservable<Option<Rc<GutterIndicatorLayout>>>,
    visual: IObservable<Option<Rc<GutterIndicatorVisual>>>,
    host: Rc<dyn IGutterIndicatorHost>,
    focus_is_in_menu: Rc<ObservableValue<bool, ()>>,
    shift_pressed: Rc<ObservableValue<bool, ()>>,
    hover_visible: Rc<ObservableValue<bool, ()>>,
    is_hovered_over_icon: Rc<ObservableValue<bool, ()>>,
    is_hovered_over_icon_debounced: IObservable<bool>,
    pulse_controller: Rc<PulseController>,
    on_did_close_with_command: Rc<Emitter<String>>,
    pub is_visible: IObservable<bool>,
}

impl InlineEditsGutterIndicator {
    pub fn new(
        editor: Rc<ObservableCodeEditor>,
        data: IObservable<Option<Rc<InlineEditsGutterIndicatorData>>>,
        tab_action: IObservable<InlineEditTabAction>,
        vertical_offset: IObservable<f64>,
        is_hovering_over_inline_edit: IObservable<bool>,
        focus_is_in_menu: Rc<ObservableValue<bool, ()>>,
        services: GutterIndicatorServices,
    ) -> Rc<Self> {
        let GutterIndicatorServices { theme_service, accessibility_service, host } = services;
        let base = Disposable::new();
        let icons = StockIcons::new();

        let original_range_obs = map_out_falsy(&data.map(|d, _| d.map(|d| d.original_range)));

        let sticky_scroll_height = host.sticky_scroll_height();

        let is_hovered_over_inline_edit_debounced = debounced_observable(is_hovering_over_inline_edit, DebounceMs::Fixed(HOVER_DEBOUNCE_MS));

        base.register(editor.create_overlay_widget(NonOverflowOverlayWidget::new(const_observable(None), const_observable(0.0))));

        let is_hovered_over_icon = observable_value::<bool, ()>("isHoveredOverIcon", false);
        base.register(editor.editor.on_did_scroll_change().listen({
            let is_hovered_over_icon = Rc::clone(&is_hovered_over_icon);
            move |_| is_hovered_over_icon.set(false, None)
        }));
        let is_hovered_over_icon_debounced = debounced_observable(is_hovered_over_icon.to_observable(), DebounceMs::Fixed(HOVER_DEBOUNCE_MS));

        let shift_pressed = observable_value::<bool, ()>("modifierPressed", false);
        let modifier_pressed = shift_pressed.to_observable();

        let styles: IObservable<Rc<GutterIndicatorStyles>> = {
            let (tab_action, data, modifier_pressed) = (Rc::clone(&tab_action), Rc::clone(&data), Rc::clone(&modifier_pressed));
            derived(move |reader| {
                let mut action = tab_action.read(reader);

                // Falta uma fonte única da verdade para a ação alternativa ativa e a tecla pressionada.
                let has_alt_action = data.read(reader).is_some_and(|d| d.alt_action.is_some());
                let modifier_pressed = modifier_pressed.read(reader);
                if has_alt_action && modifier_pressed {
                    action = InlineEditTabAction::Inactive;
                }

                let (background, foreground, border) = match action {
                    InlineEditTabAction::Inactive => (
                        INLINE_EDIT_INDICATOR_SECONDARY_BACKGROUND,
                        INLINE_EDIT_INDICATOR_SECONDARY_FOREGROUND,
                        INLINE_EDIT_INDICATOR_SECONDARY_BORDER,
                    ),
                    InlineEditTabAction::Jump => {
                        (INLINE_EDIT_INDICATOR_PRIMARY_BACKGROUND, INLINE_EDIT_INDICATOR_PRIMARY_FOREGROUND, INLINE_EDIT_INDICATOR_PRIMARY_BORDER)
                    }
                    InlineEditTabAction::Accept => (
                        INLINE_EDIT_INDICATOR_SUCCESSFUL_BACKGROUND,
                        INLINE_EDIT_INDICATOR_SUCCESSFUL_FOREGROUND,
                        INLINE_EDIT_INDICATOR_SUCCESSFUL_BORDER,
                    ),
                };
                let blended = |id: &'static str| get_editor_blended_color(ValueOrObservable::Value(id), &theme_service).read(reader);
                Rc::new(GutterIndicatorStyles { background: blended(background), foreground: blended(foreground), border: blended(border) })
            })
        };

        let pulse_controller = Rc::new(PulseController {
            accessibility_service,
            styles: Rc::clone(&styles),
            pulse: observable_value::<Option<Rc<GutterIndicatorPulse>>, ()>("pulse", None),
        });

        // pulse animation when hovering inline edit
        base.register(run_on_change::<bool, ()>(Rc::clone(&is_hovered_over_inline_edit_debounced), {
            let pulse_controller = Rc::clone(&pulse_controller);
            move |is_hovering, _, _| {
                if *is_hovering {
                    let _ = pulse_controller.trigger();
                }
            }
        }));

        let state: IObservable<Option<Rc<GutterState>>> = {
            let editor = Rc::clone(&editor);
            derived_with_store(move |reader, store| {
                let range = original_range_obs.read(reader)?;
                let line_offset_range = editor.observe_line_offset_range(&range, store);
                Some(Rc::new(GutterState { range, line_offset_range }))
            })
        };

        let line_number_to_render: IObservable<String> = {
            let (editor, data, vertical_offset) = (Rc::clone(&editor), Rc::clone(&data), Rc::clone(&vertical_offset));
            derived(move |reader| {
                if vertical_offset.read(reader) != 0.0 {
                    return String::new();
                }

                let line_number = data.read(reader).map(|d| d.original_range.start_line_number);
                let line_number_options = editor.get_option(LINE_NUMBERS).read(reader);

                let Some(line_number) = line_number else {
                    return String::new();
                };
                if line_number_options.render_type == RenderLineNumbersType::Off {
                    return String::new();
                }

                if line_number_options.render_type == RenderLineNumbersType::Interval {
                    let cursor_position = editor.cursor_position.read(reader);
                    if line_number % 10 == 0 || cursor_position.is_some_and(|p| p.line_number == line_number) {
                        return line_number.to_string();
                    }
                    return String::new();
                }

                if line_number_options.render_type == RenderLineNumbersType::Relative {
                    let Some(cursor_position) = editor.cursor_position.read(reader) else {
                        return String::new();
                    };
                    let relative_line_number = (line_number - cursor_position.line_number).abs();
                    if relative_line_number == 0 {
                        return line_number.to_string();
                    }
                    return relative_line_number.to_string();
                }

                if line_number_options.render_type == RenderLineNumbersType::Custom {
                    return match &line_number_options.render_fn {
                        Some(render_fn) => match render_fn.call(f64::from(line_number)) {
                            Dynamic::String(text) => text.to_string_lossy(),
                            Dynamic::Number(number) => number_to_js_string(number),
                            _ => String::new(),
                        },
                        None => String::new(),
                    };
                }

                line_number.to_string()
            })
        };

        let available_width_for_icon: IObservable<Rc<dyn Fn(f64) -> f64>> = {
            let editor = Rc::clone(&editor);
            derived(move |reader| -> Rc<dyn Fn(f64) -> f64> {
                let text_model = editor.editor.get_model();
                let layout = editor.layout_info.read(reader);
                let gutter_width = layout.decorations_left + layout.decorations_width - layout.glyph_margin_left;

                let Some(text_model) = text_model.filter(|_| gutter_width > 0.0) else {
                    return Rc::new(|_: f64| 0.0);
                };

                // no glyph margin => the entire gutter width is available as there is no optimal place to put the icon
                if layout.line_numbers_left == 0.0 {
                    return Rc::new(move |_: f64| gutter_width);
                }

                let line_number_options = editor.get_option(LINE_NUMBERS).read(reader);
                if line_number_options.render_type == RenderLineNumbersType::Relative /* likely to flicker */
                    || line_number_options.render_type == RenderLineNumbersType::Off
                {
                    return Rc::new(move |_: f64| gutter_width);
                }

                let w = editor.editor.get_option(FONT_INFO).typical_halfwidth_character_width;
                let right_of_line_number = layout.line_numbers_left + layout.line_numbers_width;
                let total_lines = CursorSimpleModel::get_line_count(&*text_model);
                // 0 based to 1 based
                let total_lines_digits = u32::try_from((i64::from(total_lines) + 1).to_string().len()).unwrap_or(u32::MAX);

                // (topOfLineNumber, usableWidthLeftOfLineNumber)
                let mut offset_digits: Vec<(f64, f64)> = Vec::new();

                // We only need to pre compute the usable width left of the line number for the first line number with a given digit count
                for digits in 1..=total_lines_digits {
                    let first_line_number_with_digit_count = 10_i32.saturating_pow(digits - 1);
                    let top_of_line_number = editor.editor.get_top_for_line_number(first_line_number_with_digit_count, None);
                    let digits_width = f64::from(digits) * w;
                    let usable_width_left_of_line_number =
                        gutter_width.min((right_of_line_number - digits_width - layout.glyph_margin_left).max(0.0));
                    offset_digits.push((top_of_line_number, usable_width_left_of_line_number));
                }

                Rc::new(move |top_offset: f64| {
                    for &(top_of_line_number, usable_width) in offset_digits.iter().rev() {
                        if top_offset >= top_of_line_number {
                            return usable_width;
                        }
                    }
                    errors::throw(Error::bug_indicating(Some("Could not find avilable width for icon")))
                })
            })
        };

        let layout: IObservable<Option<Rc<GutterIndicatorLayout>>> = {
            let (editor, data, tab_action, vertical_offset, icons) =
                (Rc::clone(&editor), Rc::clone(&data), Rc::clone(&tab_action), Rc::clone(&vertical_offset), Rc::clone(&icons));
            let (is_hovered_over_icon_debounced, is_hovered_over_inline_edit_debounced) =
                (Rc::clone(&is_hovered_over_icon_debounced), Rc::clone(&is_hovered_over_inline_edit_debounced));
            derived(move |reader| {
                let s = state.read(reader)?;

                let layout = editor.layout_info.read(reader);

                let line_height =
                    editor.observe_line_height_for_line(ValueOrObservable::Observable(s.range.map(|r, _| r.start_line_number))).read(reader);

                // Entire gutter view from top left to bottom right
                let gutter_width_without_padding =
                    layout.decorations_left + layout.decorations_width - layout.glyph_margin_left - 2.0 * GUTTER_VIEWPORT_PADDING_LEFT;
                let gutter_height_without_padding = layout.height - 2.0 * GUTTER_VIEWPORT_PADDING_TOP;
                let gutter_view_port_with_sticky_scroll = Rect::from_left_top_width_height(
                    GUTTER_VIEWPORT_PADDING_LEFT,
                    GUTTER_VIEWPORT_PADDING_TOP,
                    gutter_width_without_padding,
                    gutter_height_without_padding,
                );
                let gutter_view_port_without_sticky_scroll_without_padding_top =
                    gutter_view_port_with_sticky_scroll.with_top(sticky_scroll_height.read(reader));
                let gutter_view_port_without_sticky_scroll = gutter_view_port_with_sticky_scroll
                    .with_top(gutter_view_port_without_sticky_scroll_without_padding_top.top + GUTTER_VIEWPORT_PADDING_TOP);

                // The glyph margin area across all relevant lines
                let vertical_edit_range = s.line_offset_range.read(reader);
                let gutter_edit_area = Rect::from_left_top_right_bottom(
                    gutter_view_port_without_sticky_scroll.left,
                    f64::from(vertical_edit_range.start),
                    gutter_view_port_without_sticky_scroll.right,
                    f64::from(vertical_edit_range.end_exclusive),
                );

                // The gutter view container (pill)
                let pill_height = line_height;
                let pill_offset = vertical_offset.read(reader);
                let pill_fully_docked_rect = gutter_edit_area.with_height(pill_height).translate_y(pill_offset);
                let pill_is_fully_docked = gutter_view_port_without_sticky_scroll_without_padding_top.contains_rect(&pill_fully_docked_rect);

                // The icon which will be rendered in the pill
                let custom_icon: Option<Rc<ThemeIcon>> = data.read(reader).and_then(|d| d.customization.as_ref().and_then(|c| c.icon.clone())).map(Rc::new);
                let icon_none_docked: IObservable<Rc<ThemeIcon>> = match &custom_icon {
                    Some(icon) => const_observable(Rc::clone(icon)),
                    None => {
                        let icons = Rc::clone(&icons);
                        tab_action.map(move |action, _| {
                            if action == InlineEditTabAction::Accept { Rc::clone(&icons.keyboard_tab) } else { Rc::clone(&icons.arrow_right) }
                        })
                    }
                };
                let icon_docked: IObservable<Rc<ThemeIcon>> = match &custom_icon {
                    Some(icon) => const_observable(Rc::clone(icon)),
                    None => {
                        let (icons, editor, tab_action, range) = (Rc::clone(&icons), Rc::clone(&editor), Rc::clone(&tab_action), Rc::clone(&s.range));
                        let (hovered_over_icon, hovered_over_inline_edit) =
                            (Rc::clone(&is_hovered_over_icon_debounced), Rc::clone(&is_hovered_over_inline_edit_debounced));
                        derived(move |reader| {
                            if hovered_over_icon.read(reader) || hovered_over_inline_edit.read(reader) {
                                return Rc::clone(&icons.check);
                            }
                            if tab_action.read(reader) == InlineEditTabAction::Accept {
                                return Rc::clone(&icons.keyboard_tab);
                            }
                            let cursor_line_number = editor.cursor_line_number.read(reader).unwrap_or(0);
                            let edit_start_line_number = range.read(reader).start_line_number;
                            if cursor_line_number <= edit_start_line_number {
                                Rc::clone(&icons.keyboard_tab_above)
                            } else {
                                Rc::clone(&icons.keyboard_tab_below)
                            }
                        })
                    }
                };

                let icon_width = |pill_rect: &Rect| -> f64 {
                    let available_width = available_width_for_icon.get();
                    let available_icon_area_width =
                        available_width(pill_rect.bottom + editor.editor.get_scroll_top()) - GUTTER_VIEWPORT_PADDING_LEFT;
                    available_icon_area_width.min(IDEAL_ICON_AREA_WIDTH).max(CODICON_SIZE_PX)
                };

                if pill_is_fully_docked {
                    let pill_rect = pill_fully_docked_rect;

                    let width_until_line_number_end = if layout.line_numbers_width == 0.0 {
                        (layout.line_numbers_left - gutter_view_port_with_sticky_scroll.left)
                            .max(0.0)
                            .min(pill_rect.width() - IDEAL_ICON_AREA_WIDTH)
                            .max(0.0)
                    } else {
                        (layout.line_numbers_left + layout.line_numbers_width - gutter_view_port_with_sticky_scroll.left).max(0.0)
                    };

                    let line_number_rect = pill_rect.with_width(width_until_line_number_end);
                    let minimal_icon_width_with_padding = CODICON_SIZE_PX + CODICON_PADDING_PX;
                    let docked_icon_width = (pill_rect.width() - width_until_line_number_end).min(IDEAL_ICON_AREA_WIDTH);
                    let icon_rect = pill_rect.with_width(docked_icon_width.max(minimal_icon_width_with_padding)).translate_x(width_until_line_number_end);
                    let icon_visible = docked_icon_width >= minimal_icon_width_with_padding;

                    return Some(Rc::new(GutterIndicatorLayout {
                        gutter_edit_area,
                        icon: icon_docked,
                        icon_direction: GutterIconDirection::Right,
                        icon_rect,
                        icon_visible,
                        pill_rect,
                        line_number_rect: Some(line_number_rect),
                    }));
                }

                // The area in which the pill could be partially docked
                let pill_partially_docked_possible_area = gutter_view_port_with_sticky_scroll.intersect(&gutter_edit_area);
                if let Some(possible_area) = pill_partially_docked_possible_area.filter(|area| area.height() >= pill_height) {
                    // pillFullyDockedRect is outside viewport, move it into the viewport under sticky scroll as we prefer the pill to not be on top of the sticky scroll
                    // then move it into the possible area which will only cause it to move if it has to be rendered on top of the sticky scroll
                    let pill_rect_moved =
                        pill_fully_docked_rect.move_to_be_contained_in(&gutter_view_port_without_sticky_scroll).move_to_be_contained_in(&possible_area);
                    let pill_rect = pill_rect_moved.with_width(icon_width(&pill_rect_moved));
                    let icon_rect = pill_rect;

                    return Some(Rc::new(GutterIndicatorLayout {
                        gutter_edit_area,
                        icon: icon_docked,
                        icon_direction: GutterIconDirection::Right,
                        icon_rect,
                        icon_visible: true,
                        pill_rect,
                        line_number_rect: None,
                    }));
                }

                // pillFullyDockedRect is outside viewport, so move it into viewport
                let pill_rect_moved = pill_fully_docked_rect.move_to_be_contained_in(&gutter_view_port_with_sticky_scroll);
                let pill_rect = pill_rect_moved.with_width(icon_width(&pill_rect_moved));
                let icon_rect = pill_rect;

                // docked = pill was already in the viewport
                let icon_direction = if pill_rect.top < pill_fully_docked_rect.top { GutterIconDirection::Top } else { GutterIconDirection::Bottom };

                Some(Rc::new(GutterIndicatorLayout {
                    gutter_edit_area,
                    icon: icon_none_docked,
                    icon_direction,
                    icon_rect,
                    icon_visible: true,
                    pill_rect,
                    line_number_rect: None,
                }))
            })
        };

        let visual: IObservable<Option<Rc<GutterIndicatorVisual>>> = {
            let (layout, data, styles, modifier_pressed) = (Rc::clone(&layout), Rc::clone(&data), Rc::clone(&styles), Rc::clone(&modifier_pressed));
            derived(move |reader| {
                let layout = layout.read(reader)?;
                let styles = styles.read(reader);

                let line_number = layout.line_number_rect.map(|rect| GutterLineNumberVisual {
                    width: rect.width(),
                    line_height: rect.height(),
                    text: line_number_to_render.read(reader),
                });

                let icon = layout.icon.read(reader);
                let icon_color = data.read(reader).and_then(|d| d.customization.as_ref().and_then(|c| c.icon.as_ref().and_then(|i| i.color.as_ref().map(|c| c.id.clone()))));
                let font_size = (layout.icon_rect.width() - CODICON_PADDING_PX).min(CODICON_SIZE_PX);

                Some(Rc::new(GutterIndicatorVisual {
                    gutter_edit_area: layout.gutter_edit_area,
                    gutter_edit_area_background: INLINE_EDIT_INDICATOR_BACKGROUND,
                    pill_rect: layout.pill_rect,
                    background: Rc::clone(&styles.background),
                    foreground: Rc::clone(&styles.foreground),
                    border: Rc::clone(&styles.border),
                    justify_content: if layout.icon_direction == GutterIconDirection::Bottom {
                        GutterJustifyContent::FlexStart
                    } else {
                        GutterJustifyContent::FlexEnd
                    },
                    animate_transition: !modifier_pressed.read(reader),
                    line_number,
                    icon: GutterIconVisual {
                        icon,
                        rotation_degrees: layout.icon_direction.rotation_degrees(),
                        opacity: if layout.icon_visible { 1.0 } else { 0.0 },
                        margin_right: layout.pill_rect.width()
                            - layout.icon_rect.width()
                            - layout.line_number_rect.map_or(0.0, |rect| rect.width()),
                        width: layout.icon_rect.width(),
                        right_offset: if layout.icon_direction == GutterIconDirection::Top { ICON_TOP_RIGHT_OFFSET } else { 0.0 },
                        font_size,
                        color: icon_color,
                    },
                }))
            })
            .recompute_initially_and_on_change(base.store(), None)
        };

        let is_visible = layout.map(|l, _| l.is_some());

        Rc::new(Self {
            base,
            editor,
            data,
            layout,
            visual,
            host,
            focus_is_in_menu,
            shift_pressed,
            hover_visible: observable_value::<bool, ()>("hoverVisible", false),
            is_hovered_over_icon,
            is_hovered_over_icon_debounced,
            pulse_controller,
            on_did_close_with_command: Emitter::new(),
            is_visible,
        })
    }

    /// O dado que o pintor nativo lê (`None` quando o indicador está escondido).
    pub fn visual(&self) -> IObservable<Option<Rc<GutterIndicatorVisual>>> {
        Rc::clone(&self.visual)
    }

    /// `onDidCloseWithCommand`.
    pub fn on_did_close_with_command(&self) -> Event<String> {
        self.on_did_close_with_command.event()
    }

    /// `isHoverVisible`.
    pub fn is_hover_visible(&self) -> IObservable<bool> {
        self.hover_visible.to_observable()
    }

    /// `isHoveredOverIcon` (com o atraso de 100 ms do original).
    pub fn is_hovered_over_icon(&self) -> IObservable<bool> {
        Rc::clone(&self.is_hovered_over_icon_debounced)
    }

    /// O pulso em andamento, que o pintor nativo anima (`None` antes do primeiro `trigger_animation`).
    pub fn pulse(&self) -> IObservable<Option<Rc<GutterIndicatorPulse>>> {
        self.pulse_controller.pulse.to_observable()
    }

    /// `triggerAnimation()`: a promessa resolve quando o pulso termina.
    pub fn trigger_animation(&self) -> Promise<()> {
        self.pulse_controller.trigger()
    }

    /// O `readModifierKeyStatus(...).shiftKey`: o pintor avisa o estado do shift enquanto o editor tem o foco.
    pub fn notify_shift_key(&self, pressed: bool) {
        self.shift_pressed.set(pressed, None);
    }

    /// O `onclick` do ícone: aceita quando o ícone é o de confirmar, senão salta.
    pub fn notify_click(&self) {
        let accept_on_click = self.layout.get().is_some_and(|layout| StockIcons::is_check(&layout.icon.get()));

        let Some(data) = self.data.get() else {
            errors::throw(Error::bug_indicating(Some("Gutter indicator data not available")));
        };

        self.editor.editor.focus();
        if accept_on_click {
            (data.model.accept)();
        } else {
            (data.model.jump)();
        }
    }

    /// O `onmouseenter` do ícone.
    pub fn notify_mouse_enter(&self) {
        self.is_hovered_over_icon.set(true, None);
        // Falta mostrar o hover também ao passar sobre o ghost text e afins (anotação do original).
        self.show_hover();
    }

    /// O `onmouseleave` do ícone.
    pub fn notify_mouse_leave(&self) {
        self.is_hovered_over_icon.set(false, None);
    }

    /// `_showHover()`.
    fn show_hover(&self) {
        if self.hover_visible.get() {
            return;
        }

        let Some(data) = self.data.get() else {
            errors::throw(Error::bug_indicating(Some("Gutter indicator data not available")));
        };
        let Some(layout) = self.layout.get() else {
            return;
        };

        let session_slot: Rc<RefCell<Option<Rc<dyn IGutterIndicatorMenuSession>>>> = Rc::default();
        let on_close: GutterMenuCloseCallback = {
            let (editor, on_did_close_with_command, session_slot) =
                (Rc::clone(&self.editor), Rc::clone(&self.on_did_close_with_command), Rc::clone(&session_slot));
            Rc::new(move |focus_editor: bool, command_id: Option<String>| {
                if focus_editor {
                    editor.editor.focus();
                }
                if let Some(command_id) = command_id.filter(|id| !id.is_empty()) {
                    on_did_close_with_command.fire(command_id);
                }
                let session = session_slot.borrow_mut().take();
                if let Some(session) = session {
                    session.dispose();
                }
            })
        };

        let session = self.host.show_menu(GutterIndicatorMenuRequest {
            editor: Rc::clone(&self.editor),
            data: Rc::clone(&data.gutter_menu_data),
            target: layout.pill_rect,
            on_close,
        });
        let Some(session) = session else {
            return;
        };
        *session_slot.borrow_mut() = Some(Rc::clone(&session));

        let store = DisposableStore::new();
        store.add(autorun({
            let (focus_is_in_menu, session) = (Rc::clone(&self.focus_is_in_menu), Rc::clone(&session));
            move |reader| focus_is_in_menu.set(session.is_focused().read(reader), None)
        }));
        store.add(to_disposable({
            let focus_is_in_menu = Rc::clone(&self.focus_is_in_menu);
            move || focus_is_in_menu.set(false, None)
        }));

        self.hover_visible.set(true, None);
        store.add(self.editor.editor.on_did_scroll_change().listen({
            let session = Rc::clone(&session);
            move |_| session.dispose()
        }));
        store.add(session.on_did_dispose().listen({
            let (hover_visible, store) = (Rc::clone(&self.hover_visible), Rc::clone(&store));
            move |_| {
                hover_visible.set(false, None);
                store.dispose();
            }
        }));
    }
}

impl IDisposable for InlineEditsGutterIndicator {
    fn dispose(&self) {
        self.base.dispose();
    }
}
