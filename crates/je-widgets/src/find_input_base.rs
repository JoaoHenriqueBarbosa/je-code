//! O que o `FindInput` e o `ReplaceInput` (`findInput.ts`, `replaceInput.ts`, `findInput.css`) têm em comum,
//! escrito uma vez: o `div.monaco-findInput` com o [`HistoryInputBox`] e o `div.controls` com os botões,
//! mais [`FindInputStyles`] e [`default_find_input_styles`].
//!
//! Como o DOM e o CSS viram [`je_ui`]:
//!
//! - `.monaco-findInput` é `position: relative` e bloco: o nó preenche a largura do pai
//!   (`set_fill`), e a altura é a do campo ([`FindInputBase::relayout`] a copia depois de cada
//!   `layout()` do campo). O `ReplaceInput.width` troca o preenchimento por uma largura fixa.
//! - `.monaco-findInput > .controls` é `position: absolute; top: 3px; right: 2px` e contém os botões em
//!   `float: left`. Como o absoluto cria um contexto de formatação de bloco, ele contém os floats:
//!   `n * 22px` de largura (cada [`Toggle`] reserva `2px` de margem e `20px` de caixa) por `20px` de
//!   altura. O `right: 2px` é [`Tree::set_right`](je_ui::Tree::set_right) e cada botão é posicionado
//!   com [`Toggle::set_position`]. `display: none` é `visible = false`.
//! - `.vs .monaco-findInput.disabled` (`#E1E1E1`) e `.vs-dark ...` (`#333`) viram
//!   [`FindInputStyles::disabled_background`]; o tema de alto contraste não tem a regra.
//! - `.monaco-findInput.highlight-0/1 .controls`: a classe alterna a cada `highlightFindOptions` como no
//!   original, mas a animação de 100ms (`animation`) não existe na árvore, então o realce em si não
//!   aparece (divergência de `transition`/`animation` do brief).
//!
//! Fica de fora, com o `InputBox` (que ainda não tem a `ActionBar` à direita): as opções `actions` e
//! `actionViewItemProvider`, e `setActions` e `actionsWidth` do `FindInput`; `flexibleHeight`,
//! `flexibleWidth` e `flexibleMaxHeight` (o `<textarea>` do campo multilinha); a sessão de IME
//! (`compositionstart` e `compositionend`, `isImeSessionInProgress`), que o `je-ui` não modela; e a opção
//! `width`, que o construtor do original lê mas não usa.
// TODO(integração): `actions`, `actionViewItemProvider` e `setActions` do FindInput quando o InputBox ganhar a ActionBar.

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event as BaseEvent};
use je_base::history::IHistory;
use je_base::lifecycle::{Disposable, DisposableStore, IDisposable, MutableDisposable};
use je_render::Rgba;
use je_render::kurbo::Rect;
use je_theme::{ColorTheme, ThemeType};
use je_ui::{EventKind, NodeId, PointerData, Role, Ui};

use crate::common::registered_emitter;
use crate::context_view_api::IContextViewProvider;
use crate::dom::{StandardKeyboardEvent, create_el};
use crate::find_input_toggles::navigate_toggles;
use crate::input_box::{HistoryInputBox, HistoryInputBoxOptions, InputBoxOptions, InputBoxStyles, InputValidationOptions, InputValidator, Message, default_input_box_styles};
use crate::text::UI_FONT_SIZE;
use crate::toggle::{Toggle, ToggleStyles, default_toggle_styles};

/// O `top: 3px` dos `.controls`.
const CONTROLS_TOP: f64 = 3.0;
/// O `right: 2px` dos `.controls`.
const CONTROLS_RIGHT: f64 = 2.0;
/// A altura dos `.controls`: a de um botão (`20px`).
const CONTROLS_HEIGHT: f64 = 20.0;
/// A margem esquerda de cada botão.
const TOGGLE_MARGIN: f64 = 2.0;

/// Os estilos de um campo de busca: o `inputBoxStyles` e o `toggleStyles` que o original recebe nas
/// opções, mais a cor do fundo desabilitado que o CSS escolhe pelo tema.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FindInputStyles {
    pub input_box_styles: InputBoxStyles,
    pub toggle_styles: ToggleStyles,
    /// `.monaco-findInput.disabled { background-color }`.
    pub disabled_background: Option<Rgba>,
}

/// `{ inputBoxStyles: defaultInputBoxStyles, toggleStyles: defaultToggleStyles }` de
/// `defaultFindWidgetStyles`, com o fundo desabilitado do `findInput.css` (`#E1E1E1` no tema claro, `#333`
/// no escuro, nenhum nos de alto contraste).
pub fn default_find_input_styles(theme: &ColorTheme) -> FindInputStyles {
    let disabled_background = match theme.theme_type {
        ThemeType::Light => Some(Rgba::opaque(0xE1, 0xE1, 0xE1)),
        ThemeType::Dark => Some(Rgba::opaque(0x33, 0x33, 0x33)),
        ThemeType::HighContrastDark | ThemeType::HighContrastLight => None,
    };
    FindInputStyles { input_box_styles: default_input_box_styles(theme), toggle_styles: default_toggle_styles(theme), disabled_background }
}

/// As opções do campo que os dois widgets repassam ao `HistoryInputBox`.
pub(crate) struct InputParts {
    pub placeholder: String,
    pub label: String,
    pub validation: Option<InputValidator>,
    pub history: Option<Rc<dyn IHistory<String>>>,
    pub show_history_hint: Option<Rc<dyn Fn() -> bool>>,
    pub input_box_styles: InputBoxStyles,
    pub hide_hover_on_value_change: bool,
    pub caret_blink: bool,
}

impl InputParts {
    fn into_options(self) -> HistoryInputBoxOptions {
        let mut input = InputBoxOptions::new(self.input_box_styles);
        input.placeholder = Some(self.placeholder);
        input.aria_label = Some(self.label);
        input.validation_options = Some(InputValidationOptions { validation: self.validation });
        input.history = self.history;
        input.hide_hover_on_value_change = self.hide_hover_on_value_change;
        // `.monaco-findInput .monaco-inputbox { font-size: 13px }`.
        input.font_size = UI_FONT_SIZE;
        input.caret_blink = self.caret_blink;
        HistoryInputBoxOptions { input, show_history_hint: self.show_history_hint }
    }
}

/// O `domNode`, o `inputBox` e os botões de um campo de busca (ver a documentação do módulo).
pub struct FindInputBase {
    base: Rc<Disposable>,
    weak_self: Weak<FindInputBase>,
    ui: Ui,
    dom_node: NodeId,
    controls: NodeId,
    /// `public readonly inputBox`.
    pub input_box: Rc<HistoryInputBox>,
    /// Os botões que o widget cria (`caseSensitive`, `wholeWords`, `regex`, ou o `preserveCase`), na ordem
    /// do `controls`.
    common_toggles: Vec<Rc<Toggle>>,
    additional_toggles: RefCell<Vec<Rc<Toggle>>>,
    additional_disposables: Rc<MutableDisposable<DisposableStore>>,
    fix_focus_on_option_click_enabled: Cell<bool>,
    last_highlight_find_options: Cell<u8>,
    disabled_background: Option<Rgba>,
    on_did_option_change: Rc<Emitter<bool>>,
    on_mouse_down: Rc<Emitter<PointerData>>,
}

impl FindInputBase {
    /// O `constructor` comum: cria o `div.monaco-findInput` (como último filho de `parent`, se houver), o
    /// campo, o `div.controls` com os `common_toggles` e liga os ouvintes. `controls_visible` é o
    /// `controls.style.display`.
    pub(crate) fn new(ui: &Ui, parent: Option<NodeId>, context_view_provider: Option<Rc<dyn IContextViewProvider>>, input: InputParts, common_toggles: Vec<Rc<Toggle>>, controls_visible: bool, disabled_background: Option<Rgba>) -> Rc<Self> {
        let base = Disposable::new();
        let dom_node = {
            let mut tree = ui.tree_mut();
            let dom_node = match parent {
                Some(parent) => create_el(&mut tree, parent, Role::Generic, "monaco-findInput"),
                None => {
                    let node = tree.create_detached(Role::Generic);
                    tree.set_class_name(node, "monaco-findInput");
                    node
                }
            };
            tree.set_fill(dom_node, true, false);
            dom_node
        };
        let input_box = HistoryInputBox::new(ui, dom_node, context_view_provider, input.into_options());
        let controls = {
            let mut tree = ui.tree_mut();
            let controls = create_el(&mut tree, dom_node, Role::Generic, "controls");
            tree.style_mut(controls).visible = controls_visible;
            for toggle in &common_toggles {
                tree.append_child(controls, toggle.dom_node());
            }
            controls
        };
        let additional_disposables = MutableDisposable::new();
        base.register(additional_disposables.clone());
        let this = Rc::new_cyclic(|weak| FindInputBase {
            weak_self: weak.clone(),
            ui: ui.clone(),
            dom_node,
            controls,
            input_box,
            common_toggles,
            additional_toggles: RefCell::new(Vec::new()),
            additional_disposables,
            fix_focus_on_option_click_enabled: Cell::new(true),
            last_highlight_find_options: Cell::new(0),
            disabled_background,
            on_did_option_change: registered_emitter(&base),
            on_mouse_down: registered_emitter(&base),
            base,
        });
        for toggle in &this.common_toggles {
            this.base.register(this.on_toggle_change(toggle, true));
        }
        // Arrow-Key support to navigate between options
        if !this.common_toggles.is_empty() {
            let weak = this.weak_self.clone();
            let ui = this.ui.clone();
            this.base.register(ui.clone().listen(dom_node, EventKind::KeyDown, false, move |e| {
                if let Some(this) = weak.upgrade() {
                    navigate_toggles(&ui, e, || this.toggle_dom_nodes(), || this.input_box.focus());
                }
            }));
        }
        {
            let emitter = this.on_mouse_down.clone();
            this.base.register(ui.listen(this.input_box.input_element(), EventKind::PointerDown, false, move |e| {
                if let Some(pointer) = e.pointer {
                    emitter.fire(pointer);
                }
            }));
        }
        this.layout_controls();
        this
    }

    /// Liga o `onChange` do botão: avisa `onDidOptionChange`, devolve o foco ao campo se o clique veio do
    /// mouse e, nos botões comuns, revalida.
    fn on_toggle_change(&self, toggle: &Rc<Toggle>, validate: bool) -> je_base::lifecycle::DisposableRef {
        let weak = self.weak_self.clone();
        toggle.on_change().listen(move |via_keyboard| {
            let Some(this) = weak.upgrade() else { return };
            this.on_did_option_change.fire(*via_keyboard);
            if !*via_keyboard && this.fix_focus_on_option_click_enabled.get() {
                this.input_box.focus();
            }
            if validate {
                this.input_box.validate();
            }
        })
    }

    /// Todos os botões, na ordem do `controls`: os comuns e depois os adicionais.
    fn toggles(&self) -> Vec<Rc<Toggle>> {
        self.common_toggles.iter().cloned().chain(self.additional_toggles.borrow().iter().cloned()).collect()
    }

    /// `getToggleDomNodes`.
    pub fn toggle_dom_nodes(&self) -> Vec<NodeId> {
        self.toggles().iter().map(|toggle| toggle.dom_node()).collect()
    }

    /// A largura que os botões comuns reservam à direita do texto (o `togglesWidth` do original, sem os
    /// adicionais quando `with_additional` é falso).
    pub fn toggles_width(&self, with_additional: bool) -> f64 {
        let common: f64 = self.common_toggles.iter().map(|t| t.width()).sum();
        common + if with_additional { self.additional_toggles.borrow().iter().map(|t| t.width()).sum() } else { 0.0 }
    }

    /// Posiciona os botões e o `.controls` (`float: left` dentro do `position: absolute`).
    fn layout_controls(&self) {
        let toggles = self.toggles();
        let mut x = 0.0;
        for toggle in &toggles {
            toggle.set_position(x + TOGGLE_MARGIN, 0.0);
            x += toggle.width();
        }
        let mut tree = self.ui.tree_mut();
        tree.set_bounds(self.controls, Rect::new(0.0, CONTROLS_TOP, x, CONTROLS_TOP + CONTROLS_HEIGHT));
        tree.set_right(self.controls, Some(CONTROLS_RIGHT));
    }

    /// `layout()` do campo com o `paddingRight` dado, e a altura do `domNode` (que no CSS é a do conteúdo).
    pub fn relayout(&self, padding_right: f64) {
        self.input_box.set_padding_right(padding_right);
        let height = self.input_box.height();
        let mut tree = self.ui.tree_mut();
        let bounds = tree.bounds(self.dom_node);
        tree.set_bounds(self.dom_node, Rect::new(bounds.x0, bounds.y0, bounds.x1, bounds.y0 + height));
    }

    /// `domNode.style.width = px` (o `ReplaceInput.width`): deixa de preencher o pai.
    pub(crate) fn set_fixed_width(&self, width: f64) {
        let mut tree = self.ui.tree_mut();
        let bounds = tree.bounds(self.dom_node);
        tree.set_fill(self.dom_node, false, false);
        tree.set_bounds(self.dom_node, Rect::new(bounds.x0, bounds.y0, bounds.x0 + width, bounds.y1));
    }

    /// `setAdditionalToggles`: troca os botões depois dos comuns (descartando os antigos). Quem chama
    /// refaz o `paddingRight` com [`FindInputBase::relayout`].
    pub fn set_additional_toggles(&self, toggles: Vec<Rc<Toggle>>) {
        let old = std::mem::take(&mut *self.additional_toggles.borrow_mut());
        {
            let mut tree = self.ui.tree_mut();
            for toggle in &old {
                tree.detach(toggle.dom_node());
            }
        }
        let store = DisposableStore::new();
        self.additional_disposables.set(Some(store.clone()));
        for toggle in &toggles {
            store.add_rc(toggle.clone());
            self.ui.tree_mut().append_child(self.controls, toggle.dom_node());
            store.add(self.on_toggle_change(toggle, false));
        }
        if !toggles.is_empty() {
            self.ui.tree_mut().style_mut(self.controls).visible = true;
        }
        *self.additional_toggles.borrow_mut() = toggles;
        self.layout_controls();
    }

    // ---- API pública comum -------------------------------------------------------------------------------

    pub fn dom_node(&self) -> NodeId {
        self.dom_node
    }

    /// O `div.controls`.
    pub fn controls(&self) -> NodeId {
        self.controls
    }

    /// `onDidOptionChange`: dispara com `true` quando a troca veio do teclado.
    pub fn on_did_option_change(&self) -> BaseEvent<bool> {
        self.on_did_option_change.event()
    }

    pub fn on_key_down(&self) -> BaseEvent<StandardKeyboardEvent> {
        self.input_box.on_key_down()
    }

    pub fn on_key_up(&self) -> BaseEvent<StandardKeyboardEvent> {
        self.input_box.on_key_up()
    }

    /// `onMouseDown`: o botão do ponteiro apertado no campo.
    pub fn on_mouse_down(&self) -> BaseEvent<PointerData> {
        self.on_mouse_down.event()
    }

    /// O fundo do `.disabled` depende do estado dos botões comuns (o campo desabilitado deixa todos
    /// desabilitados), então segue o `enable`/`disable`.
    fn apply_disabled_background(&self, disabled: bool) {
        self.ui.tree_mut().style_mut(self.dom_node).background = if disabled { self.disabled_background } else { None };
    }

    pub fn enable(&self) {
        self.ui.tree_mut().remove_class(self.dom_node, "disabled");
        self.apply_disabled_background(false);
        self.input_box.enable();
        self.toggles().iter().for_each(|toggle| toggle.enable());
    }

    pub fn disable(&self) {
        self.ui.tree_mut().add_class(self.dom_node, "disabled");
        self.apply_disabled_background(true);
        self.input_box.disable();
        self.toggles().iter().for_each(|toggle| toggle.disable());
    }

    pub fn set_enabled(&self, enabled: bool) {
        if enabled {
            self.enable();
        } else {
            self.disable();
        }
    }

    pub fn set_focus_input_on_option_click(&self, value: bool) {
        self.fix_focus_on_option_click_enabled.set(value);
    }

    /// `clear`: tira a mensagem, esvazia o valor e foca o campo.
    pub fn clear(&self) {
        self.clear_message();
        self.set_value("");
        self.focus();
    }

    pub fn get_value(&self) -> String {
        self.input_box.value()
    }

    pub fn set_value(&self, value: &str) {
        if self.input_box.value() != value {
            self.input_box.set_value(value);
        }
    }

    /// `onSearchSubmit`: põe o valor no histórico.
    pub fn on_search_submit(&self) {
        self.input_box.add_to_history(false);
    }

    pub fn select(&self) {
        self.input_box.select(None);
    }

    pub fn focus(&self) {
        self.input_box.focus();
    }

    /// `highlightFindOptions`: alterna `highlight-0` e `highlight-1` no `domNode`.
    pub fn highlight_find_options(&self) {
        let last = self.last_highlight_find_options.get();
        let next = 1 - last;
        self.last_highlight_find_options.set(next);
        let mut tree = self.ui.tree_mut();
        tree.remove_class(self.dom_node, &format!("highlight-{last}"));
        tree.add_class(self.dom_node, &format!("highlight-{next}"));
    }

    pub fn validate(&self) {
        self.input_box.validate();
    }

    pub fn show_message(&self, message: Message) {
        self.input_box.show_message(message, false);
    }

    /// `clearMessage` (e o `clearValidation`, que faz o mesmo).
    pub fn clear_message(&self) {
        self.input_box.hide_message();
    }
}

impl IDisposable for FindInputBase {
    fn dispose(&self) {
        for toggle in &self.common_toggles {
            toggle.dispose();
        }
        self.input_box.dispose();
        self.base.dispose();
        self.ui.remove(self.dom_node);
    }

    fn constructor_name(&self) -> &'static str {
        "FindInputBase"
    }
}
