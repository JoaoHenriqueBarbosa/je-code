//! Porte de `src/vs/base/browser/ui/inputbox/inputBox.ts` (+ `inputBox.css`): o [`InputBox`], o campo de
//! texto de uma linha do workbench com validação e mensagem, e o [`HistoryInputBox`], que navega pelo
//! histórico com Cima e Baixo.
//!
//! Do original ficam: as opções (`placeholder`, `showPlaceholderOnFocus`, `tooltip`, `ariaLabel`, `type`
//! texto ou senha, `validationOptions`, `inputBoxStyles`, `history`, `hideHoverOnValueChange`), o valor
//! (`value`, `setValue`, `insertAtCursor`, `select`, `getSelection`, `isSelectionAtEnd`), o foco
//! (`focus`, `blur`, `hasFocus`), `enable`/`disable`/`setEnabled`, `width`, `paddingRight`, `height`,
//! `layout`, os eventos (`onDidChange`, `onDidHeightChange`, e `onInput`, `onKeyDown`, `onKeyUp`, `onFocus`,
//! `onBlur`, que no original são ouvintes DOM do `input`), a validação (`validate`, `isInputValid`,
//! `showMessage`, `hideMessage`, `stylesForType`, `MessageType`, `IMessage`) com a mensagem desenhada por um
//! [`IContextViewProvider`], o estado (`idle`/`info`/`warning`/`error` e `synthetic-focus` nas classes) e os
//! estilos ([`InputBoxStyles`], [`default_input_box_styles`], [`unthemed_input_box_styles`]).
//!
//! Como o DOM vira [`je_ui`]:
//!
//! - `.monaco-inputbox > .ibwrapper > input.input` são três nós com as mesmas classes. O `<input>` é o nó
//!   do campo ([`InputBox::input_element`], papel `TextInput`, cursor de texto); a edição que o navegador dá
//!   ao `<input>` é do [`TextField`](crate::text_field::TextField), ligado por `TextInput` e `KeyDown`
//!   (`ui.text_input(..)` e `ui.key_input(..)`). A edição roda como ação padrão do nó
//!   ([`Ui::listen_default`]): depois dos ouvintes dos consumidores, e só se nenhum chamou
//!   `preventDefault`, como o navegador faz (a caixa de busca usa isso para Cima e Baixo do histórico).
//! - O texto, a seleção, o cursor e o texto-guia (`placeholder`) são nós de uma janela (`.inner-editor`,
//!   com `overflow: hidden`) dentro do campo: a janela é a caixa de conteúdo, com o `padding: 4px 6px` do
//!   CSS. A borda de 1px do `border: 1px solid ...` é a borda do `je-ui`, que fica dentro da caixa, então
//!   os filhos são deslocados 1px.
//! - A altura vem do CSS: `line-height: inherit` do workbench (1.4em) mais o `padding` e a borda. Não há
//!   motor de layout: o contêiner que muda de largura chama [`InputBox::layout`] (como o VS Code chama
//!   `inputBox.layout()`), que reposiciona a janela de texto.
//! - O anel de foco (`outline: 1px solid var(--vscode-focusBorder); outline-offset: -1px`, da folha de
//!   estilo do workbench) cobre exatamente a borda; vira a cor da borda, e some com a validação ativa
//!   (`outline: 0 !important` em `.info/.warning/.error.synthetic-focus`), que então mostra a cor da
//!   mensagem. `focus_border`, `input_placeholder_foreground` e `selection_background` são campos de
//!   [`InputBoxStyles`] que o original lê de variáveis CSS globais.
//! - O cursor pisca a cada 500 ms com um `setTimeout` do `je-base` ([`InputBoxOptions::caret_blink`] desliga,
//!   para os testes e para capturas).
//! - Fora de foco o texto longo ganha reticências (`text-overflow: ellipsis` do CSS) e a seleção não é
//!   pintada.
//!
//! Fica de fora, por enquanto:
//!
//! - **`flexibleHeight` e `flexibleWidth`** (o `<textarea>` com o espelho `.mirror`, `flexibleMaxHeight`,
//!   `updateMirror`, `mirrorElement`, `onDidHeightChange` com valor e o `ScrollableElement` vertical):
//!   multilinha precisa de quebra de linha, altura que cresce com o conteúdo e rolagem vertical,
//!   que é um editor de várias linhas, outro controle. O módulo é de linha única, então
//!   [`InputBox::on_did_height_change`] existe, mas nunca dispara (a altura de uma linha não muda).
//! - **`actions`, `actionViewItemProvider`, `setActions`, `actionsWidth`** (a `ActionBar` à direita do
//!   campo): dependem do `ActionBar`, que é de outra frente.
//! - **O alerta ARIA** de `showMessage` (`aria.alert`) e o `aria.status` do histórico: dependem do módulo
//!   `aria`. O rótulo do campo vai no `label` do nó e o `aria-invalid` fica em [`InputBox::is_aria_invalid`].
//! - **`step`** (só tem sentido em `type="number"`).
//! - **IME** e o **`ignoreGesture`** (toque).
//!
//! O `else` do `_showMessage` do original, sem provedor de visão de contexto, não mostra nada (só a cor da
//! borda e as classes). [`InlineContextViewProvider`] é um provedor mínimo, do `je-widgets`, que desenha o
//! bloco da mensagem logo abaixo do campo, para quem ainda não tem a visão de contexto de verdade.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::dynamic::Truthy;
use je_base::event::{Emitter, Event as BaseEvent};
use je_base::history::{HistoryNavigator, IHistory};
use je_base::lifecycle::{Disposable, DisposableRef, IDisposable, to_disposable};
use je_base::navigator::INavigator;
use je_base::runtime::{self, TimerId};
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_text::TextStyle;
use je_theme::ColorTheme;
use je_ui::{Border, Content, Cursor, EventKind, NodeId, PointerEvents, Role, Ui};

use crate::common::registered_emitter;
use crate::context_view_api::{Anchor, AnchorAlignment, IContextViewDelegate, IContextViewProvider};
use crate::dom::{StandardKeyboardEvent, create_el};
use crate::hover_api::{HoverContent, HoverTarget, IHoverAppearanceOptions, IHoverOptions, get_base_layer_hover_delegate};
use crate::text::{UI_FONT_SIZE, ellipsize, measure_text, natural_line_height, ui_font};
use crate::text_field::{ClipboardProvider, MemoryClipboard, TextField};

/// O `padding: 4px 6px` do `.input` (e do `.mirror`).
const PADDING_X: f64 = 6.0;
const PADDING_Y: f64 = 4.0;
/// A borda de 1px do `.monaco-inputbox`.
const BORDER: f64 = 1.0;
/// O `border-radius: 4px`.
const RADIUS: f64 = 4.0;
/// O `line-height: 1.4em` do `.monaco-workbench`, que o `.input` herda.
const LINE_HEIGHT_EM: f64 = 1.4;
/// O intervalo do cursor piscando.
const CARET_BLINK_MS: u64 = 500;
/// O fundo da seleção quando o estilo não diz (o `Highlight` do Chromium para Linux).
const DEFAULT_SELECTION_BACKGROUND: Rgba = Rgba::opaque(0x1e, 0x90, 0xff);
/// A cor do texto quando o estilo não define `inputForeground` (o `color: inherit` do CSS, sem pai).
const FALLBACK_FOREGROUND: Rgba = Rgba::opaque(0xcc, 0xcc, 0xcc);
/// O `font-size: 12px` e o `line-height: 17px` da mensagem, e o `padding: 0.4em`.
const MESSAGE_FONT_SIZE: f32 = 12.0;
const MESSAGE_LINE_HEIGHT: f64 = 17.0;
const MESSAGE_PADDING: f64 = 0.4 * 12.0;

/// O `MessageType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageType {
    Info = 1,
    Warning = 2,
    Error = 3,
}

/// O `IMessage`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Message {
    pub content: Option<String>,
    /// `formatContent`: interpreta `**negrito**`, `__itálico__` e `` `código` ``.
    pub format_content: bool,
    pub r#type: Option<MessageType>,
}

/// O `IInputValidator`: devolve a mensagem de erro, ou `None` se o valor é válido.
pub type InputValidator = Rc<dyn Fn(&str) -> Option<Message>>;

/// O `IInputValidationOptions`.
#[derive(Clone, Default)]
pub struct InputValidationOptions {
    pub validation: Option<InputValidator>,
}

/// O `type` do `<input>` (os que um `InputBox` usa).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputType {
    #[default]
    Text,
    /// Mascara o texto com `•` e não deixa copiar nem recortar.
    Password,
}

/// O `IRange`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range {
    pub start: usize,
    pub end: usize,
}

/// O `IInputBoxStyles`, mais os três valores que o CSS lê de variáveis globais do tema.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputBoxStyles {
    pub input_background: Option<Rgba>,
    pub input_foreground: Option<Rgba>,
    pub input_border: Option<Rgba>,
    pub input_validation_info_border: Option<Rgba>,
    pub input_validation_info_background: Option<Rgba>,
    pub input_validation_info_foreground: Option<Rgba>,
    pub input_validation_warning_border: Option<Rgba>,
    pub input_validation_warning_background: Option<Rgba>,
    pub input_validation_warning_foreground: Option<Rgba>,
    pub input_validation_error_border: Option<Rgba>,
    pub input_validation_error_background: Option<Rgba>,
    pub input_validation_error_foreground: Option<Rgba>,
    /// `--vscode-input-placeholderForeground` (`input::placeholder` do `style.css` do workbench).
    pub input_placeholder_foreground: Option<Rgba>,
    /// `--vscode-focusBorder` (o `outline-color` do `.synthetic-focus`).
    pub focus_border: Option<Rgba>,
    /// O fundo da seleção. Sem valor vale o azul padrão do Chromium.
    pub selection_background: Option<Rgba>,
}

/// O `unthemedInboxStyles`.
pub fn unthemed_input_box_styles() -> InputBoxStyles {
    let hex = |r, g, b| Some(Rgba::opaque(r, g, b));
    InputBoxStyles {
        input_background: hex(0x3c, 0x3c, 0x3c),
        input_foreground: hex(0xcc, 0xcc, 0xcc),
        input_validation_info_border: hex(0x55, 0xaa, 0xff),
        input_validation_info_background: hex(0x06, 0x3b, 0x49),
        input_validation_warning_border: hex(0xb8, 0x95, 0x00),
        input_validation_warning_background: hex(0x35, 0x2a, 0x05),
        input_validation_error_border: hex(0xbe, 0x11, 0x00),
        input_validation_error_background: hex(0x5a, 0x1d, 0x1d),
        ..InputBoxStyles::default()
    }
}

/// O `defaultInputBoxStyles` de `defaultStyles.ts`, com as chaves de tema resolvidas.
pub fn default_input_box_styles(theme: &ColorTheme) -> InputBoxStyles {
    let color = |id: &str| theme.get_color(id);
    InputBoxStyles {
        input_background: color("input.background"),
        input_foreground: color("input.foreground"),
        input_border: color("input.border"),
        input_validation_info_border: color("inputValidation.infoBorder"),
        input_validation_info_background: color("inputValidation.infoBackground"),
        input_validation_info_foreground: color("inputValidation.infoForeground"),
        input_validation_warning_border: color("inputValidation.warningBorder"),
        input_validation_warning_background: color("inputValidation.warningBackground"),
        input_validation_warning_foreground: color("inputValidation.warningForeground"),
        input_validation_error_border: color("inputValidation.errorBorder"),
        input_validation_error_background: color("inputValidation.errorBackground"),
        input_validation_error_foreground: color("inputValidation.errorForeground"),
        input_placeholder_foreground: color("input.placeholderForeground"),
        focus_border: color("focusBorder"),
        selection_background: None,
    }
}

/// O que `stylesForType` devolve.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TypeStyles {
    pub border: Option<Rgba>,
    pub background: Option<Rgba>,
    pub foreground: Option<Rgba>,
}

/// O `IInputOptions`.
#[derive(Clone)]
pub struct InputBoxOptions {
    pub placeholder: Option<String>,
    pub show_placeholder_on_focus: bool,
    pub tooltip: Option<String>,
    pub aria_label: Option<String>,
    pub input_type: InputType,
    pub validation_options: Option<InputValidationOptions>,
    pub input_box_styles: InputBoxStyles,
    pub history: Option<Rc<dyn IHistory<String>>>,
    pub hide_hover_on_value_change: bool,
    /// `font-size: inherit`: o tamanho da fonte do contêiner (13px no workbench).
    pub font_size: f32,
    /// Pisca o cursor. Desligue em testes e capturas.
    pub caret_blink: bool,
    /// A área de transferência de recortar, copiar e colar. Sem ela, uma em memória.
    pub clipboard: Option<Rc<dyn ClipboardProvider>>,
}

impl InputBoxOptions {
    pub fn new(input_box_styles: InputBoxStyles) -> Self {
        Self {
            placeholder: None,
            show_placeholder_on_focus: false,
            tooltip: None,
            aria_label: None,
            input_type: InputType::Text,
            validation_options: None,
            input_box_styles,
            history: None,
            hide_hover_on_value_change: false,
            font_size: UI_FONT_SIZE,
            caret_blink: true,
            clipboard: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MessageState {
    Idle,
    Open,
    Closed,
}

fn truncate_layout_unit(px: f64) -> f64 {
    (px * 64.0).floor() / 64.0
}

/// O `InputBox`. Ver a documentação do módulo.
pub struct InputBox {
    base: Rc<Disposable>,
    weak_self: Weak<InputBox>,
    ui: Ui,
    context_view_provider: Option<Rc<dyn IContextViewProvider>>,
    element: NodeId,
    wrapper: NodeId,
    input: NodeId,
    inner: NodeId,
    selection_node: NodeId,
    text_node: NodeId,
    caret_node: NodeId,
    placeholder_node: NodeId,
    options: InputBoxOptions,
    clipboard: Rc<dyn ClipboardProvider>,
    field: RefCell<TextField>,
    message: RefCell<Option<Message>>,
    /// O `this.placeholder` do original (o texto lógico).
    placeholder: RefCell<String>,
    /// O atributo `placeholder` do `<input>` (o que é pintado quando o campo está vazio).
    placeholder_attribute: RefCell<String>,
    tooltip: Rc<RefCell<String>>,
    has_hover: Cell<bool>,
    aria_label: RefCell<String>,
    validation: Option<InputValidator>,
    aria_invalid: Cell<bool>,
    state: Cell<MessageState>,
    enabled: Cell<bool>,
    synthetic_focus: Cell<bool>,
    caret_visible: Cell<bool>,
    blink_timer: Cell<Option<TimerId>>,
    /// O `input.style.width` posto por `width` (`None` é `width: 100%`).
    input_width: Cell<Option<f64>>,
    padding_right: Cell<f64>,
    message_container: RefCell<Option<NodeId>>,
    on_did_change: Rc<Emitter<String>>,
    on_did_height_change: Rc<Emitter<f64>>,
    on_input: Rc<Emitter<()>>,
    on_key_down: Rc<Emitter<StandardKeyboardEvent>>,
    on_key_up: Rc<Emitter<StandardKeyboardEvent>>,
    on_did_focus: Rc<Emitter<()>>,
    on_did_blur: Rc<Emitter<()>>,
}

impl InputBox {
    /// `new InputBox(container, contextViewProvider, options)`: o campo entra como último filho de
    /// `container`.
    pub fn new(ui: &Ui, container: NodeId, context_view_provider: Option<Rc<dyn IContextViewProvider>>, options: InputBoxOptions) -> Rc<InputBox> {
        let base = Disposable::new();
        let placeholder = options.placeholder.clone().unwrap_or_default();
        let tooltip = options.tooltip.clone().unwrap_or_else(|| placeholder.clone());
        let aria_label = options.aria_label.clone().unwrap_or_default();
        let validation = options.validation_options.as_ref().and_then(|o| o.validation.clone());
        let (element, wrapper, input, inner, selection_node, text_node, caret_node, placeholder_node) = {
            let mut tree = ui.tree_mut();
            let element = create_el(&mut tree, container, Role::Generic, "monaco-inputbox idle");
            tree.set_fill(element, true, false);
            tree.style_mut(element).radius = RADIUS;
            let wrapper = create_el(&mut tree, element, Role::Generic, "ibwrapper");
            let input = create_el(&mut tree, wrapper, Role::TextInput, "input empty");
            tree.style_mut(input).cursor = Some(Cursor::Text);
            let inner = create_el(&mut tree, input, Role::Generic, "inner-editor");
            tree.style_mut(inner).clip = true;
            let selection_node = create_el(&mut tree, inner, Role::Generic, "selection");
            let text_node = create_el(&mut tree, inner, Role::Generic, "text");
            let caret_node = create_el(&mut tree, inner, Role::Generic, "caret");
            let placeholder_node = create_el(&mut tree, inner, Role::Generic, "placeholder");
            for node in [selection_node, text_node, caret_node, placeholder_node] {
                tree.style_mut(node).pointer_events = PointerEvents::None;
            }
            (element, wrapper, input, inner, selection_node, text_node, caret_node, placeholder_node)
        };
        let mut field = TextField::new();
        field.set_password(options.input_type == InputType::Password);
        let clipboard: Rc<dyn ClipboardProvider> = options.clipboard.clone().unwrap_or_else(|| MemoryClipboard::new());
        let this = Rc::new_cyclic(|weak| InputBox {
            weak_self: weak.clone(),
            ui: ui.clone(),
            context_view_provider,
            element,
            wrapper,
            input,
            inner,
            selection_node,
            text_node,
            caret_node,
            placeholder_node,
            clipboard,
            field: RefCell::new(field),
            message: RefCell::new(None),
            placeholder: RefCell::new(placeholder.clone()),
            placeholder_attribute: RefCell::new(String::new()),
            tooltip: Rc::new(RefCell::new(tooltip.clone())),
            has_hover: Cell::new(false),
            aria_label: RefCell::new(aria_label.clone()),
            validation,
            aria_invalid: Cell::new(false),
            state: Cell::new(MessageState::Idle),
            enabled: Cell::new(true),
            synthetic_focus: Cell::new(false),
            caret_visible: Cell::new(true),
            blink_timer: Cell::new(None),
            input_width: Cell::new(None),
            padding_right: Cell::new(0.0),
            message_container: RefCell::new(None),
            on_did_change: registered_emitter(&base),
            on_did_height_change: registered_emitter(&base),
            on_input: registered_emitter(&base),
            on_key_down: registered_emitter(&base),
            on_key_up: registered_emitter(&base),
            on_did_focus: registered_emitter(&base),
            on_did_blur: registered_emitter(&base),
            options,
            base,
        });
        this.connect();
        if !aria_label.is_empty() {
            this.ui.tree_mut().set_label(input, aria_label);
        }
        if !placeholder.is_empty() && !this.options.show_placeholder_on_focus {
            this.set_place_holder(&placeholder);
        }
        if !tooltip.is_empty() {
            this.set_tooltip(&tooltip);
        }
        this.apply_styles();
        this.layout();
        this
    }

    /// Os ouvintes do nó do campo: os eventos públicos (em bolha) e a edição (ação padrão).
    fn connect(&self) {
        let ui = &self.ui;
        let input = self.input;
        let with_self = |weak: &Weak<InputBox>, f: fn(&Rc<InputBox>, &mut je_ui::Event)| {
            let weak = weak.clone();
            move |e: &mut je_ui::Event| {
                if let Some(this) = weak.upgrade() {
                    f(&this, e);
                }
            }
        };
        let weak = &self.weak_self;
        // `onfocus`/`onblur` do `Widget`: a ordem do original é a do registro (classe, depois valor).
        self.base.register(ui.listen(input, EventKind::Focus, false, with_self(weak, |this, _| {
            this.synthetic_focus.set(true);
            this.ui.tree_mut().add_class(this.element, "synthetic-focus");
            this.on_focus();
        })));
        self.base.register(ui.listen(input, EventKind::Blur, false, with_self(weak, |this, _| {
            this.synthetic_focus.set(false);
            this.ui.tree_mut().remove_class(this.element, "synthetic-focus");
            this.on_blur();
        })));
        for (kind, down) in [(EventKind::KeyDown, true), (EventKind::KeyUp, false)] {
            let weak = weak.clone();
            self.base.register(ui.listen(input, kind, false, move |e| {
                let Some(this) = weak.upgrade() else { return };
                if let Some(key) = StandardKeyboardEvent::from_event(e) {
                    let emitter = if down { &this.on_key_down } else { &this.on_key_up };
                    emitter.fire(key);
                }
            }));
        }
        self.base.register(ui.listen_default(input, EventKind::TextInput, with_self(weak, |this, e| {
            if this.enabled.get() && let Some(text) = e.text.as_deref() {
                let changed = this.field.borrow_mut().insert_text(text);
                this.after_edit(changed);
            }
        })));
        self.base.register(ui.listen_default(input, EventKind::KeyDown, with_self(weak, |this, e| {
            let Some(key) = StandardKeyboardEvent::from_event(e) else { return };
            if !this.enabled.get() {
                return;
            }
            let outcome = this.field.borrow_mut().handle_key(&key, this.clipboard.as_ref());
            if outcome.handled {
                e.prevent_default();
                this.after_edit(outcome.changed);
            }
        })));
        self.base.register(ui.listen_default(input, EventKind::PointerDown, with_self(weak, |this, e| {
            let Some(pointer) = e.pointer else { return };
            if !this.enabled.get() {
                return;
            }
            this.ui.focus(this.input);
            let offset = this.offset_at(pointer.x);
            this.field.borrow_mut().pointer_down(offset, pointer.click_count, pointer.modifiers.shift);
            this.ui.set_pointer_capture(this.input);
            this.touch();
        })));
        self.base.register(ui.listen(input, EventKind::PointerMove, false, with_self(weak, |this, e| {
            let Some(pointer) = e.pointer else { return };
            if this.field.borrow().is_dragging() {
                let offset = this.offset_at(pointer.x);
                this.field.borrow_mut().pointer_drag(offset);
                this.touch();
            }
        })));
        self.base.register(ui.listen(input, EventKind::PointerUp, false, with_self(weak, |this, _| {
            if this.field.borrow().is_dragging() {
                this.field.borrow_mut().pointer_up();
                this.ui.release_pointer_capture();
            }
        })));
        let weak = self.weak_self.clone();
        self.base.register(to_disposable(move || {
            if let Some(this) = weak.upgrade() {
                this.stop_blink();
            }
        }));
    }

    // ---- estilo e medidas ---------------------------------------------------------------------------------

    fn font(&self) -> TextStyle {
        ui_font(self.options.font_size, 400)
    }

    fn line_height(&self) -> f64 {
        truncate_layout_unit(LINE_HEIGHT_EM * f64::from(self.options.font_size))
    }

    fn foreground(&self) -> Rgba {
        self.options.input_box_styles.input_foreground.unwrap_or(FALLBACK_FOREGROUND)
    }

    /// A largura do `<input>`: a posta por `width`, ou a do `ibwrapper`.
    fn input_box_width(&self) -> f64 {
        let wrapper_width = self.ui.tree().bounds(self.wrapper).width();
        self.input_width.get().unwrap_or(wrapper_width)
    }

    /// A largura da caixa de conteúdo do `<input>` (o que sobra depois do padding).
    fn viewport_width(&self) -> f64 {
        (self.input_box_width() - 2.0 * PADDING_X - self.padding_right.get()).max(0.0)
    }

    /// `layout()` do original (que só mexe em altura, no `<textarea>`) mais o que o CSS faria sozinho:
    /// reposiciona os nós a partir da largura atual do contêiner.
    pub fn layout(&self) {
        let (line_height, element_width) = (self.line_height(), self.ui.tree().bounds(self.element).width());
        let height = 2.0 * BORDER + 2.0 * PADDING_Y + line_height;
        {
            let mut tree = self.ui.tree_mut();
            let element_bounds = tree.bounds(self.element);
            tree.set_bounds(self.element, Rect::new(element_bounds.x0, element_bounds.y0, element_bounds.x0 + element_width, element_bounds.y0 + height));
            tree.set_bounds(self.wrapper, Rect::new(BORDER, BORDER, element_width - BORDER, height - BORDER));
            tree.set_fill(self.wrapper, false, false);
        }
        let input_width = self.input_box_width();
        let viewport = self.viewport_width();
        {
            let mut tree = self.ui.tree_mut();
            tree.set_bounds(self.input, Rect::new(0.0, 0.0, input_width, height - 2.0 * BORDER));
            tree.set_bounds(self.inner, Rect::new(PADDING_X, PADDING_Y, PADDING_X + viewport, PADDING_Y + line_height));
        }
        self.render();
        self.layout_message();
    }

    /// `height`: a altura total da caixa (a borda, o padding e a linha).
    pub fn height(&self) -> f64 {
        self.ui.tree().bounds(self.element).height()
    }

    /// `width` (get): a largura total do `<input>`.
    pub fn width(&self) -> f64 {
        self.input_box_width()
    }

    /// `width` (set): a largura do `<input>`, em px.
    pub fn set_width(&self, width: f64) {
        self.input_width.set(Some(width));
        self.layout();
    }

    /// `paddingRight`: reserva espaço à direita do texto (para botões sobrepostos).
    pub fn set_padding_right(&self, padding_right: f64) {
        self.padding_right.set(padding_right);
        self.layout();
    }

    /// Reescreve os nós de texto, seleção, cursor e texto-guia a partir do estado do campo.
    fn render(&self) {
        let (style, line_height, viewport) = (self.font(), self.line_height(), self.viewport_width());
        let focused = self.has_focus();
        let foreground = self.foreground();
        let styles = &self.options.input_box_styles;
        if focused {
            self.field.borrow_mut().reveal_caret(viewport, &style);
        } else {
            self.field.borrow_mut().set_scroll_left(0.0, viewport, &style);
        }
        let field = self.field.borrow();
        let scroll = field.scroll_left();
        let natural = natural_line_height(&style);
        let display = field.display_text();
        let empty = field.is_empty();
        let content_width = field.content_width(&style);
        let selection = field.selection_x_range(&style);
        let caret_x = field.x_at_offset(field.caret(), &style);
        let placeholder = self.placeholder_attribute.borrow().clone();
        let ellipsis_text = (!focused && content_width > viewport).then(|| ellipsize(&display, &style, viewport));
        let mut tree = self.ui.tree_mut();
        tree.toggle_class(self.input, "empty", empty);
        tree.style_mut(self.input).cursor = Some(if self.enabled.get() { Cursor::Text } else { Cursor::Default });
        // O texto.
        let shown = ellipsis_text.as_deref().unwrap_or(&display);
        let shown_scroll = if ellipsis_text.is_some() { 0.0 } else { scroll };
        tree.set_bounds(self.text_node, Rect::new(-shown_scroll, 0.0, -shown_scroll + content_width.max(measure_text(shown, &style)), line_height));
        tree.style_mut(self.text_node).content = if empty { Content::None } else { Content::Text { text: shown.to_owned(), style: style.clone(), color: foreground, line_height } };
        // A seleção, só com foco.
        let selection = selection.filter(|_| focused);
        tree.style_mut(self.selection_node).visible = selection.is_some();
        if let Some((x0, x1)) = selection {
            tree.set_bounds(self.selection_node, Rect::new(x0 - scroll, 0.0, x1 - scroll, line_height));
            tree.style_mut(self.selection_node).background = Some(styles.selection_background.unwrap_or(DEFAULT_SELECTION_BACKGROUND));
        }
        // O cursor, só com foco e sem seleção.
        let caret = focused && self.enabled.get() && selection.is_none() && self.caret_visible.get();
        tree.style_mut(self.caret_node).visible = caret;
        tree.set_bounds(self.caret_node, Rect::new(caret_x - scroll, (line_height - natural) / 2.0, caret_x - scroll + 1.0, (line_height + natural) / 2.0));
        tree.style_mut(self.caret_node).background = Some(foreground);
        // O texto-guia.
        let placeholder_visible = empty && !placeholder.is_empty();
        tree.style_mut(self.placeholder_node).visible = placeholder_visible;
        if placeholder_visible {
            let color = styles.input_placeholder_foreground.unwrap_or(foreground);
            let text = ellipsize(&placeholder, &style, viewport);
            let width = measure_text(&text, &style);
            tree.set_bounds(self.placeholder_node, Rect::new(0.0, 0.0, width, line_height));
            tree.style_mut(self.placeholder_node).content = Content::Text { text, style, color, line_height };
        }
        drop(tree);
        drop(field);
        self.refresh_border();
    }

    /// A borda: a da mensagem ativa; senão o anel de foco; senão `inputBorder`; transparente na falta.
    fn refresh_border(&self) {
        let styles = &self.options.input_box_styles;
        let message_type = self.message.borrow().as_ref().map(|m| m.r#type);
        let color = match message_type {
            Some(t) => self.styles_for_type(t).border,
            None if self.synthetic_focus.get() => styles.focus_border.or(styles.input_border),
            None => styles.input_border,
        };
        self.ui.tree_mut().style_mut(self.element).border = Some(Border { width: BORDER, color: color.unwrap_or(Rgba::TRANSPARENT) });
    }

    /// `applyStyles`.
    fn apply_styles(&self) {
        let styles = &self.options.input_box_styles;
        self.ui.tree_mut().style_mut(self.element).background = styles.input_background;
        self.refresh_border();
    }

    /// `stylesForType`.
    pub fn styles_for_type(&self, message_type: Option<MessageType>) -> TypeStyles {
        let s = &self.options.input_box_styles;
        match message_type {
            Some(MessageType::Info) => TypeStyles { border: s.input_validation_info_border, background: s.input_validation_info_background, foreground: s.input_validation_info_foreground },
            Some(MessageType::Warning) => TypeStyles { border: s.input_validation_warning_border, background: s.input_validation_warning_background, foreground: s.input_validation_warning_foreground },
            _ => TypeStyles { border: s.input_validation_error_border, background: s.input_validation_error_background, foreground: s.input_validation_error_foreground },
        }
    }

    fn class_for_type(message_type: Option<MessageType>) -> &'static str {
        match message_type {
            Some(MessageType::Info) => "info",
            Some(MessageType::Warning) => "warning",
            _ => "error",
        }
    }

    // ---- entrada ---------------------------------------------------------------------------------------------

    /// O deslocamento do cursor para o x da janela (`pointer.x`), a partir do início do texto.
    fn offset_at(&self, window_x: f64) -> usize {
        let origin = self.ui.tree().absolute_bounds(self.inner).x0;
        let field = self.field.borrow();
        field.offset_at_x(window_x - origin + field.scroll_left(), &self.font())
    }

    /// Depois de uma edição: avisa a mudança do valor, se houve, e redesenha.
    fn after_edit(&self, changed: bool) {
        if changed {
            self.on_value_change();
            self.on_input.fire(());
        }
        self.touch();
    }

    /// Qualquer atividade do usuário: o cursor volta a aparecer e o piscar recomeça.
    fn touch(&self) {
        self.caret_visible.set(true);
        self.render();
        self.restart_blink();
    }

    fn restart_blink(&self) {
        self.stop_blink();
        if self.options.caret_blink && self.has_focus() && self.enabled.get() && !self.field.borrow().has_selection() {
            let weak = self.weak_self.clone();
            self.blink_timer.set(Some(runtime::set_timeout(
                move || {
                    if let Some(this) = weak.upgrade() {
                        this.blink_timer.set(None);
                        this.caret_visible.set(!this.caret_visible.get());
                        this.render();
                        this.restart_blink();
                    }
                },
                CARET_BLINK_MS,
            )));
        }
    }

    fn stop_blink(&self) {
        if let Some(timer) = self.blink_timer.take() {
            runtime::clear_timeout(timer);
        }
    }

    fn on_value_change(&self) {
        let value = self.value();
        self.on_did_change.fire(value);
        self.validate();
        self.render();
        if self.state.get() == MessageState::Open
            && let Some(provider) = &self.context_view_provider
        {
            provider.layout();
        }
        if self.options.hide_hover_on_value_change {
            get_base_layer_hover_delegate().hide_hover(false);
        }
    }

    fn on_blur(&self) {
        self.hide_message_view();
        if self.options.show_placeholder_on_focus {
            self.placeholder_attribute.borrow_mut().clear();
        }
        self.stop_blink();
        self.caret_visible.set(true);
        self.render();
        self.on_did_blur.fire(());
    }

    fn on_focus(&self) {
        self.show_message_view();
        if self.options.show_placeholder_on_focus {
            *self.placeholder_attribute.borrow_mut() = self.placeholder.borrow().clone();
        }
        self.touch();
        self.on_did_focus.fire(());
    }

    // ---- API do original ---------------------------------------------------------------------------------------

    /// O nó do `.monaco-inputbox`.
    pub fn element(&self) -> NodeId {
        self.element
    }

    /// `inputElement`: o nó do campo (o `<input>`).
    pub fn input_element(&self) -> NodeId {
        self.input
    }

    pub fn on_did_change(&self) -> BaseEvent<String> {
        self.on_did_change.event()
    }

    /// Nunca dispara: a altura de uma linha não muda. Ver a documentação do módulo (`flexibleHeight`).
    pub fn on_did_height_change(&self) -> BaseEvent<f64> {
        self.on_did_height_change.event()
    }

    /// O `input` do DOM: dispara a cada edição do usuário (digitar, colar, apagar, desfazer), não ao
    /// chamar [`InputBox::set_value`].
    pub fn on_input(&self) -> BaseEvent<()> {
        self.on_input.event()
    }

    pub fn on_key_down(&self) -> BaseEvent<StandardKeyboardEvent> {
        self.on_key_down.event()
    }

    pub fn on_key_up(&self) -> BaseEvent<StandardKeyboardEvent> {
        self.on_key_up.event()
    }

    pub fn on_did_focus(&self) -> BaseEvent<()> {
        self.on_did_focus.event()
    }

    pub fn on_did_blur(&self) -> BaseEvent<()> {
        self.on_did_blur.event()
    }

    pub fn value(&self) -> String {
        self.field.borrow().value().to_owned()
    }

    /// `value = newValue`: só age se o valor muda; leva o cursor ao fim.
    pub fn set_value(&self, new_value: &str) {
        let changed = self.field.borrow_mut().set_value(new_value);
        if changed {
            self.on_value_change();
        }
        self.touch();
    }

    /// A seleção, o desfazer e o texto do campo, para quem edita por fora (`input.select()` etc.).
    pub fn text_field(&self) -> std::cell::RefMut<'_, TextField> {
        self.field.borrow_mut()
    }

    /// `setPlaceHolder`.
    pub fn set_place_holder(&self, place_holder: &str) {
        *self.placeholder.borrow_mut() = place_holder.to_owned();
        *self.placeholder_attribute.borrow_mut() = place_holder.to_owned();
        self.render();
    }

    pub fn placeholder(&self) -> String {
        self.placeholder.borrow().clone()
    }

    /// Troca só o texto lógico do `placeholder`, sem tocar no atributo (o `this.placeholder = ...` do
    /// `HistoryInputBox`).
    fn set_logical_placeholder(&self, placeholder: &str) {
        *self.placeholder.borrow_mut() = placeholder.to_owned();
    }

    /// `setTooltip`: o hover com atraso no ponteiro, criado uma vez.
    pub fn set_tooltip(&self, tooltip: &str) {
        *self.tooltip.borrow_mut() = tooltip.to_owned();
        if !self.has_hover.replace(true) {
            let (shared, input) = (self.tooltip.clone(), self.input);
            let options = Rc::new(move || {
                let mut options = IHoverOptions::new(HoverContent::Text(shared.borrow().clone()), HoverTarget::Node(input));
                options.appearance = Some(IHoverAppearanceOptions { compact: Some(true), ..IHoverAppearanceOptions::default() });
                options
            });
            self.base.register(get_base_layer_hover_delegate().setup_delayed_hover_at_mouse(input, options, None));
        }
    }

    pub fn tooltip(&self) -> String {
        self.tooltip.borrow().clone()
    }

    pub fn set_aria_label(&self, label: &str) {
        *self.aria_label.borrow_mut() = label.to_owned();
        let mut tree = self.ui.tree_mut();
        if label.is_empty() {
            tree.clear_label(self.input);
        } else {
            tree.set_label(self.input, label);
        }
    }

    pub fn aria_label(&self) -> String {
        self.aria_label.borrow().clone()
    }

    /// O `aria-invalid="true"` do campo.
    pub fn is_aria_invalid(&self) -> bool {
        self.aria_invalid.get()
    }

    pub fn focus(&self) {
        if self.enabled.get() {
            self.ui.focus(self.input);
        }
    }

    pub fn blur(&self) {
        if self.has_focus() {
            self.ui.blur();
        }
    }

    pub fn has_focus(&self) -> bool {
        self.ui.focused() == Some(self.input)
    }

    /// `select(range)`: seleciona tudo, ou o intervalo (e rola ao fim se o intervalo termina no fim).
    pub fn select(&self, range: Option<Range>) {
        {
            let mut field = self.field.borrow_mut();
            field.select_all();
            if let Some(range) = range {
                field.set_selection_range(range.start, range.end, crate::text_field::SelectionDirection::Forward);
                if range.end == field.len() {
                    let (width, style) = (self.viewport_width(), self.font());
                    field.set_scroll_left(f64::MAX, width, &style);
                }
            }
        }
        self.touch();
    }

    pub fn is_selection_at_end(&self) -> bool {
        let field = self.field.borrow();
        let (start, end) = field.selection();
        end == field.len() && start == end
    }

    pub fn get_selection(&self) -> Option<Range> {
        let (start, end) = self.field.borrow().selection();
        Some(Range { start, end })
    }

    pub fn enable(&self) {
        self.enabled.set(true);
        self.render();
    }

    pub fn disable(&self) {
        self.blur();
        self.enabled.set(false);
        self.hide_message_view();
        self.stop_blink();
        self.render();
    }

    pub fn set_enabled(&self, enabled: bool) {
        if enabled {
            self.enable();
        } else {
            self.disable();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled.get()
    }

    /// `insertAtCursor`: insere `text` no lugar da seleção e deixa o cursor em `start + 1` (como o original,
    /// que soma 1 e não o tamanho do texto).
    pub fn insert_at_cursor(&self, text: &str) {
        let (start, end) = self.field.borrow().selection();
        let value = self.value();
        let units: Vec<u16> = value.encode_utf16().collect();
        let joined: Vec<u16> = units[..start].iter().chain(text.encode_utf16().collect::<Vec<_>>().iter()).chain(units[end..].iter()).copied().collect();
        self.set_value(&String::from_utf16_lossy(&joined));
        self.field.borrow_mut().set_caret(start + 1);
        self.layout();
    }

    // ---- validação e mensagem --------------------------------------------------------------------------------

    /// `showMessage`.
    pub fn show_message(&self, message: Message, force: bool) {
        if self.state.get() == MessageState::Open && self.message.borrow().as_ref() == Some(&message) {
            return;
        }
        let message_type = message.r#type;
        let has_content = message.content.as_deref().is_some_and(|c| !c.is_empty());
        *self.message.borrow_mut() = Some(message);
        {
            let mut tree = self.ui.tree_mut();
            for class in ["idle", "info", "warning", "error"] {
                tree.remove_class(self.element, class);
            }
            tree.add_class(self.element, Self::class_for_type(message_type));
        }
        self.refresh_border();
        if has_content && (self.has_focus() || force) {
            self.show_message_view();
        }
    }

    /// `hideMessage`.
    pub fn hide_message(&self) {
        *self.message.borrow_mut() = None;
        {
            let mut tree = self.ui.tree_mut();
            for class in ["info", "warning", "error"] {
                tree.remove_class(self.element, class);
            }
            tree.add_class(self.element, "idle");
        }
        self.hide_message_view();
        self.apply_styles();
    }

    /// `isInputValid`: tem validador e o valor passa.
    pub fn is_input_valid(&self) -> bool {
        self.validation.as_ref().is_some_and(|validation| validation(&self.value()).is_none())
    }

    /// `validate`: roda o validador e mostra ou esconde a mensagem. Devolve o tipo da mensagem.
    pub fn validate(&self) -> Option<MessageType> {
        let validation = self.validation.as_ref()?;
        let error = validation(&self.value());
        match &error {
            Some(message) => {
                self.aria_invalid.set(true);
                self.show_message(message.clone(), false);
            }
            None if self.aria_invalid.replace(false) => self.hide_message(),
            None => {}
        }
        error.and_then(|m| m.r#type)
    }

    fn show_message_view(&self) {
        let Some(provider) = &self.context_view_provider else { return };
        if self.message.borrow().is_none() {
            return;
        }
        let delegate = Rc::new(MessageDelegate { input: self.weak_self.clone(), element: self.element });
        provider.show_context_view(delegate, None);
        self.state.set(MessageState::Open);
    }

    fn hide_message_view(&self) {
        let Some(provider) = &self.context_view_provider else { return };
        if self.state.get() == MessageState::Open {
            provider.hide_context_view();
        }
        self.state.set(MessageState::Idle);
    }

    fn layout_message(&self) {
        if self.state.get() == MessageState::Open
            && let Some(provider) = &self.context_view_provider
        {
            provider.layout();
        }
    }

    /// A largura total do `.monaco-inputbox` (a do contêiner da mensagem).
    fn element_width(&self) -> f64 {
        self.ui.tree().bounds(self.element).width()
    }
}

impl IDisposable for InputBox {
    fn dispose(&self) {
        self.hide_message_view();
        *self.message.borrow_mut() = None;
        self.stop_blink();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "InputBox"
    }
}

// ---- o bloco da mensagem ------------------------------------------------------------------------------------

/// O `IContextViewDelegate` do `_showMessage`: ancorado no campo, alinhado à direita, com o bloco
/// `.monaco-inputbox-container > span.monaco-inputbox-message`.
struct MessageDelegate {
    input: Weak<InputBox>,
    element: NodeId,
}

impl IContextViewDelegate for MessageDelegate {
    fn get_anchor(&self) -> Anchor {
        Anchor::Node(self.element)
    }

    fn render(&self, container: NodeId) -> Option<DisposableRef> {
        let this = self.input.upgrade()?;
        let message = this.message.borrow().clone()?;
        let width = this.element_width();
        let styles = this.styles_for_type(message.r#type);
        let class_name = InputBox::class_for_type(message.r#type);
        let border = styles.border.map_or(0.0, |_| BORDER);
        let foreground = styles.foreground.unwrap_or_else(|| this.foreground());
        let lines = wrap_message(message.content.as_deref().unwrap_or(""), message.format_content, width - 2.0 * (border + MESSAGE_PADDING));
        let height = lines.len() as f64 * MESSAGE_LINE_HEIGHT + 2.0 * (border + MESSAGE_PADDING);
        let mut tree = this.ui.tree_mut();
        let div = create_el(&mut tree, container, Role::Generic, "monaco-inputbox-container");
        tree.set_bounds(div, Rect::new(0.0, 0.0, width, height - 1.0));
        let span = create_el(&mut tree, div, Role::Alert, &format!("monaco-inputbox-message {class_name}"));
        tree.set_bounds(span, Rect::new(0.0, -1.0, width, height - 1.0));
        {
            let style = tree.style_mut(span);
            style.background = styles.background;
            style.border = styles.border.map(|color| Border { width: BORDER, color });
        }
        let text_style = ui_font(MESSAGE_FONT_SIZE, 400);
        for (index, line) in lines.iter().enumerate() {
            let y = border + MESSAGE_PADDING + index as f64 * MESSAGE_LINE_HEIGHT;
            let mut x = border + MESSAGE_PADDING;
            for piece in line {
                let piece_style = piece.style(&text_style);
                let piece_width = measure_text(&piece.text, &piece_style);
                let node = create_el(&mut tree, span, Role::Generic, if piece.bold { "bold" } else if piece.code { "code" } else { "text" });
                tree.set_bounds(node, Rect::new(x, y, x + piece_width, y + MESSAGE_LINE_HEIGHT));
                tree.style_mut(node).content = Content::Text { text: piece.text.clone(), style: piece_style, color: foreground, line_height: MESSAGE_LINE_HEIGHT };
                x += piece_width;
            }
        }
        *this.message_container.borrow_mut() = Some(div);
        None
    }

    /// `layout`: `div.style.width = getTotalWidth(this.element)`.
    fn layout(&self) {
        let Some(this) = self.input.upgrade() else { return };
        let Some(div) = *this.message_container.borrow() else { return };
        let width = this.element_width();
        let mut tree = this.ui.tree_mut();
        if tree.is_alive(div) {
            let bounds = tree.bounds(div);
            tree.set_bounds(div, Rect::new(bounds.x0, bounds.y0, bounds.x0 + width, bounds.y1));
            for span in tree.children(div).to_vec() {
                let b = tree.bounds(span);
                tree.set_bounds(span, Rect::new(b.x0, b.y0, b.x0 + width, b.y1));
            }
        }
    }

    fn anchor_alignment(&self) -> Option<AnchorAlignment> {
        Some(AnchorAlignment::Right)
    }

    fn on_hide(&self, _did_cancel: Option<bool>) {
        if let Some(this) = self.input.upgrade() {
            this.state.set(MessageState::Closed);
        }
    }
}

/// Um trecho de texto de uma linha da mensagem, com o estilo do `renderFormattedText`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Piece {
    text: String,
    bold: bool,
    code: bool,
}

impl Piece {
    fn style(&self, base: &TextStyle) -> TextStyle {
        if self.code {
            TextStyle::new("monospace", base.size(), 400)
        } else if self.bold {
            TextStyle { weight: 700, ..base.clone() }
        } else {
            base.clone()
        }
    }
}

/// Quebra o conteúdo da mensagem em linhas de no máximo `max_width` px (o `word-wrap: break-word` do CSS):
/// espaços (e quebras de linha) colapsam num só e uma palavra mais larga que a linha quebra no meio. Com
/// `format`, `**x**` vira negrito, `` `x` `` código e `__x__` (itálico no original) fica sem marca.
fn wrap_message(content: &str, format: bool, max_width: f64) -> Vec<Vec<Piece>> {
    let base = ui_font(MESSAGE_FONT_SIZE, 400);
    let mut tokens: Vec<(String, bool, bool)> = Vec::new();
    let (mut bold, mut code) = (false, false);
    let mut rest = content;
    let mut word = String::new();
    let flush = |word: &mut String, tokens: &mut Vec<(String, bool, bool)>, bold: bool, code: bool| {
        if !word.is_empty() {
            tokens.push((std::mem::take(word), bold, code));
        }
    };
    while let Some(c) = rest.chars().next() {
        if format && rest.starts_with("**") {
            flush(&mut word, &mut tokens, bold, code);
            bold = !bold;
            rest = &rest[2..];
        } else if format && rest.starts_with("__") {
            rest = &rest[2..];
        } else if format && c == '`' {
            flush(&mut word, &mut tokens, bold, code);
            code = !code;
            rest = &rest[1..];
        } else if c.is_whitespace() {
            flush(&mut word, &mut tokens, bold, code);
            if tokens.last().is_none_or(|(t, _, _)| t != " ") {
                tokens.push((" ".to_owned(), bold, code));
            }
            rest = &rest[c.len_utf8()..];
        } else {
            word.push(c);
            rest = &rest[c.len_utf8()..];
        }
    }
    flush(&mut word, &mut tokens, bold, code);
    let mut lines: Vec<Vec<Piece>> = vec![Vec::new()];
    let mut x = 0.0;
    let push = |lines: &mut Vec<Vec<Piece>>, text: &str, bold: bool, code: bool| {
        let line = lines.last_mut().expect("sempre há uma linha");
        match line.last_mut() {
            Some(piece) if piece.bold == bold && piece.code == code => piece.text.push_str(text),
            _ => line.push(Piece { text: text.to_owned(), bold, code }),
        }
    };
    for (token, bold, code) in tokens {
        let style = Piece { text: String::new(), bold, code }.style(&base);
        let width = measure_text(&token, &style);
        let line_is_empty = lines.last().is_some_and(Vec::is_empty);
        if token == " " {
            if !line_is_empty {
                push(&mut lines, " ", bold, code);
                x += width;
            }
            continue;
        }
        if x + width > max_width && !line_is_empty {
            if let Some(piece) = lines.last_mut().and_then(|l| l.last_mut()) {
                piece.text = piece.text.trim_end().to_owned();
            }
            lines.push(Vec::new());
            x = 0.0;
        }
        if width <= max_width {
            push(&mut lines, &token, bold, code);
            x += width;
            continue;
        }
        // Uma palavra mais larga que a linha quebra onde acabar o espaço.
        let mut chunk = String::new();
        for ch in token.chars() {
            let candidate = format!("{chunk}{ch}");
            if !chunk.is_empty() && x + measure_text(&candidate, &style) > max_width {
                push(&mut lines, &chunk, bold, code);
                lines.push(Vec::new());
                x = 0.0;
                chunk.clear();
            }
            chunk.push(ch);
        }
        x += measure_text(&chunk, &style);
        push(&mut lines, &chunk, bold, code);
    }
    if let Some(piece) = lines.last_mut().and_then(|l| l.last_mut()) {
        piece.text = piece.text.trim_end().to_owned();
    }
    lines
}

// ---- provedor mínimo de visão de contexto ----------------------------------------------------------------------

/// Um [`IContextViewProvider`] mínimo: desenha o conteúdo do delegado logo abaixo da âncora, na camada mais
/// alta da janela, e o tira ao esconder. É o que o `je-widgets` tem enquanto o módulo `contextview` (a visão
/// de contexto de verdade, com posicionamento dentro da janela) não está integrado.
// TODO(integração): trocar pelo `ContextView` quando o módulo `contextview` entrar.
pub struct InlineContextViewProvider {
    ui: Ui,
    current: RefCell<Option<(NodeId, Rc<dyn IContextViewDelegate>)>>,
}

impl InlineContextViewProvider {
    pub fn new(ui: &Ui) -> Rc<Self> {
        Rc::new(Self { ui: ui.clone(), current: RefCell::new(None) })
    }

    /// O nó que contém o que o delegado desenhou, se há uma visão aberta.
    pub fn container(&self) -> Option<NodeId> {
        self.current.borrow().as_ref().map(|(node, _)| *node)
    }

    /// Posiciona o contêiner abaixo da âncora, com a largura e a altura do que o delegado desenhou.
    fn position(&self, node: NodeId, delegate: &dyn IContextViewDelegate) {
        let Anchor::Node(anchor) = delegate.get_anchor() else { return };
        let mut tree = self.ui.tree_mut();
        let bounds = tree.absolute_bounds(anchor);
        let content = tree.children(node).first().map(|c| tree.bounds(*c));
        let (width, height) = content.map_or((bounds.width(), 0.0), |c| (c.width(), c.height()));
        let x = if delegate.anchor_alignment() == Some(AnchorAlignment::Right) { bounds.x1 - width } else { bounds.x0 };
        tree.set_bounds(node, Rect::new(x, bounds.y1, x + width, bounds.y1 + height));
    }
}

impl IContextViewProvider for InlineContextViewProvider {
    fn show_context_view(&self, delegate: Rc<dyn IContextViewDelegate>, container: Option<NodeId>) {
        self.hide_context_view();
        let node = {
            let mut tree = self.ui.tree_mut();
            let parent = container.unwrap_or_else(|| tree.root());
            create_el(&mut tree, parent, Role::Generic, "context-view")
        };
        delegate.render(node);
        self.position(node, delegate.as_ref());
        *self.current.borrow_mut() = Some((node, delegate));
    }

    fn hide_context_view(&self) {
        if let Some((node, delegate)) = self.current.borrow_mut().take() {
            self.ui.remove(node);
            delegate.on_hide(None);
        }
    }

    fn layout(&self) {
        let current = self.current.borrow().clone();
        if let Some((node, delegate)) = current {
            delegate.layout();
            self.position(node, delegate.as_ref());
        }
    }
}

// ---- HistoryInputBox -------------------------------------------------------------------------------------------

/// O `IHistoryInputOptions`.
#[derive(Clone)]
pub struct HistoryInputBoxOptions {
    pub input: InputBoxOptions,
    /// `showHistoryHint`: acrescenta o `⇅` ao texto-guia quando há histórico.
    pub show_history_hint: Option<Rc<dyn Fn() -> bool>>,
}

/// O `HistoryInputBox`: um [`InputBox`] com `HistoryNavigator` (limite 100).
pub struct HistoryInputBox {
    base: Rc<Disposable>,
    inner: Rc<InputBox>,
    history: Rc<HistoryNavigator<String>>,
}

impl std::ops::Deref for HistoryInputBox {
    type Target = InputBox;

    fn deref(&self) -> &InputBox {
        &self.inner
    }
}

fn history_hint_suffix(in_parens: bool) -> String {
    if in_parens { " (\u{21C5} for history)".to_owned() } else { " or \u{21C5} for history".to_owned() }
}

/// O `a || b` do JavaScript sobre o valor do navegador: `a` se for verdadeiro (texto não vazio).
fn or_truthy(a: Option<String>, b: impl FnOnce() -> Option<String>) -> Option<String> {
    a.filter(Truthy::is_truthy).or_else(b)
}

impl HistoryInputBox {
    pub fn new(ui: &Ui, container: NodeId, context_view_provider: Option<Rc<dyn IContextViewProvider>>, options: HistoryInputBoxOptions) -> Rc<HistoryInputBox> {
        let base = Disposable::new();
        let history = HistoryNavigator::new(options.input.history.clone(), 100);
        let inner = InputBox::new(ui, container, context_view_provider, options.input.clone());
        let this = Rc::new(HistoryInputBox { base, inner, history });
        this.base.register(this.history.clone());
        // O `setPlaceHolder` do `HistoryInputBox` também troca o tooltip.
        let show_on_focus = options.input.show_placeholder_on_focus;
        let show_hint = options.show_history_hint.clone();
        let add_suffix = {
            let (inner, history) = (this.inner.clone(), this.history.clone());
            Rc::new(move || {
                let placeholder = inner.placeholder();
                let hinted = |in_parens| placeholder.ends_with(&history_hint_suffix(in_parens));
                if show_hint.as_ref().is_some_and(|f| f()) && !hinted(false) && !hinted(true) && !history.get_history().is_empty() {
                    let suffixed = placeholder.clone() + &history_hint_suffix(!placeholder.ends_with(')'));
                    if show_on_focus && !inner.has_focus() {
                        inner.set_logical_placeholder(&suffixed);
                    } else {
                        inner.set_place_holder(&suffixed);
                        inner.set_tooltip(&suffixed);
                    }
                }
            })
        };
        // O `MutationObserver` do `class` do `<input>`: dispara quando o campo passa a vazio ou deixa de ser.
        let was_empty = Rc::new(std::cell::Cell::new(this.inner.value().is_empty()));
        {
            let add_suffix = add_suffix.clone();
            this.base.register(this.inner.on_did_change().listen(move |value| {
                let empty = value.is_empty();
                if was_empty.replace(empty) != empty {
                    add_suffix();
                }
            }));
        }
        {
            let add_suffix = add_suffix.clone();
            this.base.register(this.inner.on_did_focus().listen(move |()| add_suffix()));
        }
        {
            let inner = this.inner.clone();
            this.base.register(this.inner.on_did_blur().listen(move |()| {
                let placeholder = inner.placeholder();
                for in_parens in [true, false] {
                    let hint = history_hint_suffix(in_parens);
                    if let Some(reverted) = placeholder.strip_suffix(hint.as_str()) {
                        if show_on_focus {
                            inner.set_logical_placeholder(reverted);
                        } else {
                            inner.set_place_holder(reverted);
                            inner.set_tooltip(reverted);
                        }
                        break;
                    }
                }
            }));
        }
        this
    }

    /// `setPlaceHolder` do `HistoryInputBox`: também troca o tooltip.
    pub fn set_place_holder(&self, place_holder: &str) {
        self.inner.set_place_holder(place_holder);
        self.inner.set_tooltip(place_holder);
    }

    /// `addToHistory`.
    pub fn add_to_history(&self, always: bool) {
        let value = self.inner.value();
        if !value.is_empty() && (always || Some(&value) != self.current_value().as_ref()) {
            self.history.add(value);
        }
    }

    /// `prependHistory`.
    pub fn prepend_history(&self, restored_history: &[String]) {
        let new_history = self.history();
        self.clear_history();
        for item in restored_history.iter().chain(new_history.iter()) {
            self.history.add(item.clone());
        }
    }

    pub fn history(&self) -> Vec<String> {
        self.history.get_history()
    }

    pub fn is_at_first_in_history(&self) -> bool {
        self.history.is_first()
    }

    pub fn is_at_last_in_history(&self) -> bool {
        self.history.is_last()
    }

    pub fn is_nowhere_in_history(&self) -> bool {
        self.history.is_nowhere()
    }

    /// `showNextValue`.
    pub fn show_next_value(&self) {
        let value = self.inner.value();
        if !self.history.has(&value) {
            self.add_to_history(false);
        }
        let mut next = self.history.next();
        if next.as_ref().is_some_and(Truthy::is_truthy) {
            next = if next.as_ref() == Some(&value) { self.history.next() } else { next };
        }
        self.inner.set_value(&next.unwrap_or_default());
    }

    /// `showPreviousValue`.
    pub fn show_previous_value(&self) {
        let value = self.inner.value();
        if !self.history.has(&value) {
            self.add_to_history(false);
        }
        let mut previous = or_truthy(self.history.previous(), || self.history.first());
        if previous.as_ref().is_some_and(Truthy::is_truthy) {
            previous = if previous.as_ref() == Some(&value) { or_truthy(self.history.previous(), || self.history.first()) } else { previous };
        }
        if let Some(previous) = previous.filter(Truthy::is_truthy) {
            self.inner.set_value(&previous);
        }
    }

    /// `clearHistory`.
    pub fn clear_history(&self) {
        self.history.clear();
    }

    /// `getCurrentValue`.
    fn current_value(&self) -> Option<String> {
        let mut current = self.history.current().filter(Truthy::is_truthy);
        if current.is_none() {
            current = self.history.last();
            self.history.next();
        }
        current
    }
}

impl IDisposable for HistoryInputBox {
    fn dispose(&self) {
        self.inner.dispose();
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "HistoryInputBox"
    }
}
