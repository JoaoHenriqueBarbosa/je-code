//! Porte de `src/vs/platform/quickinput/browser/tree/quickInputTreeRenderer.ts`: o `QuickInputTreeRenderer` e o
//! `QuickInputCheckboxStateHandler`.
//!
//! Notas de tradução:
//! - **DOM.** O `renderTemplate` monta `TriStateCheckbox`, `IconLabel` e `ToolBar` em elementos; aqui o que o `renderElement`
//!   decide vira o [`QuickTreeEntryView`] (o estado do checkbox, a classe e a cor do ícone, o rótulo com os realces e as ações),
//!   que o desenho nativo lê. Os elementos do template (`.quick-input-tree-entry`, `.quick-input-tree-label`,
//!   `.quick-input-tree-rows`, `.quick-input-tree-row`, `.quick-input-tree-icon`, `.quick-input-tree-entry-action-bar` e a
//!   classe `has-actions`) são dele.
//! - **Tema.** `isDark(this._themeService.getColorTheme().type)` é o fecho `is_dark`; o `asCSSUrl(URI.revive(icon))` fica para
//!   quem desenha, que recebe o [`URI`] em [`QuickTreeEntryView::icon_url`].
//! - **Foco.** `dom.isAncestorOfActiveElement(templateData.entry)` é o argumento `entry_has_focus` de
//!   [`QuickInputTreeRenderer::dispose_element`].

use std::rc::Rc;

use je_base::actions::ActionRef;
use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::filters::Match;
use je_base::js_string::JsString;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::promise::MaybePromise;
use je_base::strings::escape;
use je_base::uri::URI;

use super::quick_input_tree::QuickTreeFilterData;
use crate::action_widget::action_list::as_css_variable;
use crate::quickinput::quick_input::{IQuickTreeCheckboxEvent, IQuickTreeItem, IQuickTreeItemButtonEvent, QuickTreeItemChecked};
use crate::quickinput::quick_input_utils::{OnTrigger, quick_input_buttons_to_action_arrays};

/// O `{ markdown: { value, supportThemeIcons }, markdownNotSupportedFallback }` de `descriptionTitle`.
#[derive(Clone, Debug, PartialEq)]
pub struct QuickTreeDescriptionTitle {
    pub markdown: JsString,
    pub support_theme_icons: bool,
    pub markdown_not_supported_fallback: JsString,
}

/// O que o `renderElement` escreve no template de uma linha.
#[derive(Clone)]
pub struct QuickTreeEntryView {
    /// `checkbox.domNode.style.display = 'none'` (item não pickable).
    pub checkbox_hidden: bool,
    pub checkbox_checked: QuickTreeItemChecked,
    /// `checkbox.disable()`.
    pub checkbox_disabled: bool,
    /// `templateData.icon.className`.
    pub icon_class_name: String,
    /// `style.backgroundImage` do ícone (o URI do tema atual), ou `None` (`''`).
    pub icon_url: Option<URI>,
    /// `style.color` do ícone (`''` sem cor).
    pub icon_color: String,
    pub label: JsString,
    pub description: Option<JsString>,
    pub label_matches: Option<Vec<Match>>,
    pub description_matches: Option<Vec<Match>>,
    pub extra_classes: Option<Vec<String>>,
    pub italic: Option<bool>,
    pub strikethrough: Option<bool>,
    /// `labelEscapeNewLines: true`.
    pub label_escape_new_lines: bool,
    pub description_title: Option<QuickTreeDescriptionTitle>,
    pub primary_actions: Vec<ActionRef>,
    pub secondary_actions: Vec<ActionRef>,
    /// A classe `has-actions` do `entry`.
    pub has_actions: bool,
}

pub struct QuickInputCheckboxStateHandler {
    base: Rc<Disposable>,
    on_did_change_checkbox_state: Rc<Emitter<IQuickTreeCheckboxEvent>>,
}

impl QuickInputCheckboxStateHandler {
    pub fn new() -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_checkbox_state = Emitter::new();
        base.register(on_did_change_checkbox_state.clone());
        Rc::new(Self { base, on_did_change_checkbox_state })
    }

    pub fn on_did_change_checkbox_state(&self) -> Event<IQuickTreeCheckboxEvent> {
        self.on_did_change_checkbox_state.event()
    }

    pub fn set_checkbox_state(&self, node: &IQuickTreeItem, checked: QuickTreeItemChecked) {
        self.on_did_change_checkbox_state.fire(IQuickTreeCheckboxEvent { item: node.clone(), checked });
    }
}

impl IDisposable for QuickInputCheckboxStateHandler {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "QuickInputCheckboxStateHandler"
    }
}

pub struct QuickInputTreeRenderer {
    base: Rc<Disposable>,
    is_dark: Rc<dyn Fn() -> bool>,
    button_triggered_emitter: Rc<Emitter<IQuickTreeItemButtonEvent>>,
    on_did_dispose_focused_element: Rc<Emitter<()>>,
}

impl QuickInputTreeRenderer {
    pub const ID: &'static str = "quickInputTreeElement";

    pub fn new(is_dark: Rc<dyn Fn() -> bool>, button_triggered_emitter: Rc<Emitter<IQuickTreeItemButtonEvent>>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_dispose_focused_element = Emitter::new();
        base.register(on_did_dispose_focused_element.clone());
        Rc::new(Self { base, is_dark, button_triggered_emitter, on_did_dispose_focused_element })
    }

    pub fn template_id(&self) -> &'static str {
        Self::ID
    }

    /// This event is emitted when the renderer disposes an element that has focus.
    /// This allows the list to re-focus itself and prevent focus from being lost
    /// (potentially causing quickinput to dismiss itself) when an element is
    /// removed while focused.
    pub fn on_did_dispose_focused_element(&self) -> Event<()> {
        self.on_did_dispose_focused_element.event()
    }

    /// `renderElement(node, ...)`: o `quick_tree_item` é `node.element` e `filter_data` é `node.filterData`.
    pub fn render_element(&self, quick_tree_item: &IQuickTreeItem, filter_data: Option<&QuickTreeFilterData>) -> QuickTreeEntryView {
        let base = &quick_tree_item.base;

        // Icon
        let (icon_class_name, icon_url) = if let Some(icon_path) = &base.icon_path {
            let icon = if (self.is_dark)() { icon_path.dark.clone() } else { icon_path.light.clone().unwrap_or_else(|| icon_path.dark.clone()) };
            ("quick-input-tree-icon".to_owned(), Some(icon))
        } else {
            (base.icon_class.as_ref().filter(|class| !class.is_empty()).map_or_else(String::new, |class| format!("quick-input-tree-icon {class}")), None)
        };
        let icon_color = base.icon_color.as_ref().map_or_else(String::new, |color| as_css_variable(&color.id));

        let (label_matches, description_matches) = filter_data.map_or((None, None), |data| (data.label_highlights.clone(), data.description_highlights.clone()));

        // Label and Description
        // NOTE: If we bring back quick tool tips, we need to check that here like we do in the QuickInputListRenderer
        let description = base.description.clone().filter(|description| !description.is_empty());
        let description_title = description.as_ref().map(|description| QuickTreeDescriptionTitle {
            markdown: escape(description.units()),
            support_theme_icons: true,
            markdown_not_supported_fallback: description.clone(),
        });

        // Action Bar
        let (primary_actions, secondary_actions) = match base.buttons.as_ref().filter(|buttons| !buttons.is_empty()) {
            Some(buttons) => {
                let (emitter, item) = (self.button_triggered_emitter.clone(), quick_tree_item.clone());
                let on_trigger: OnTrigger = Rc::new(move |button| {
                    emitter.fire(IQuickTreeItemButtonEvent { item: item.clone(), button: button.clone() });
                    MaybePromise::Value(Dynamic::Undefined)
                });
                let arrays = quick_input_buttons_to_action_arrays(buttons, "quick-input-tree", on_trigger);
                (arrays.primary, arrays.secondary)
            }
            None => (Vec::new(), Vec::new()),
        };

        QuickTreeEntryView {
            checkbox_hidden: quick_tree_item.pickable == Some(false),
            checkbox_checked: quick_tree_item.checked.unwrap_or(QuickTreeItemChecked::Bool(false)),
            checkbox_disabled: quick_tree_item.pickable != Some(false) && base.disabled == Some(true),
            icon_class_name,
            icon_url,
            icon_color,
            label: base.label.clone(),
            description: base.description.clone(),
            label_matches,
            description_matches,
            extra_classes: base.icon_classes.clone(),
            italic: base.italic,
            strikethrough: base.strikethrough,
            label_escape_new_lines: true,
            description_title,
            has_actions: !primary_actions.is_empty() || !secondary_actions.is_empty(),
            primary_actions,
            secondary_actions,
        }
    }

    /// `disposeElement(...)`: `entry_has_focus` é `dom.isAncestorOfActiveElement(templateData.entry)`.
    pub fn dispose_element(&self, entry_has_focus: bool) {
        if entry_has_focus {
            self.on_did_dispose_focused_element.fire(());
        }
    }
}

impl IDisposable for QuickInputTreeRenderer {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "QuickInputTreeRenderer"
    }
}
