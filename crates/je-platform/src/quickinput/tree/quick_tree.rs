//! Porte de `src/vs/platform/quickinput/browser/tree/quickTree.ts`: o `QuickTree`, o controlador de um quick input com uma
//! árvore de itens marcáveis.
//!
//! Notas de tradução:
//! - **Herança.** Como `QuickPick`, `InputBox` e `QuickWidget` (ver [`crate::quickinput::quick_input_browser`]), o `QuickTree` guarda
//!   um [`QuickInputBase`] e implementa [`QuickInputUpdate`]; o `show()` é `show_impl`, que termina em [`QuickInputBase::show`].
//! - **Observáveis.** `value`, `ariaLabel`, `placeholder`, `matchOnDescription`, `matchOnLabel`, `sortByLabel`, `activeItems` e
//!   `itemTree` são `observableValue`, lidos por `autorun`s que só rodam com o quick input visível. Os arrays do JavaScript
//!   comparam por identidade, então `activeItems` e `itemTree` notificam a cada atribuição.
//! - **Itens por valor.** Os itens que o `QuickTree` entrega (`activeItems`, `checkedLeafItems`, eventos) são cópias do modelo da
//!   árvore, identificadas pelo `id` (ver [`crate::quickinput::tree::quick_input_tree_controller`]). `itemTree` guarda o que o chamador
//!   passou, sem as mudanças de `checked` que a árvore faz nas cópias dela.
//! - **`setTimeout0`** é um `disposable_timeout` de 0 ms preso ao descarte do controlador.

use std::rc::{Rc, Weak};

use je_base::async_::disposable_timeout;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::observable::base::{IObservable, IReaderWithStore, Observable, SettableExt};
use je_base::observable::debug_name::DebugNameOptions;
use je_base::observable::observables::observable_value::{ObservableValue, observable_value};
use je_base::observable::reactions::autorun::autorun;
use je_base::severity::Severity;

use crate::quickinput::quick_input::{
    IQuickInput, IQuickInputHideEvent, IQuickTree, IQuickTreeItem, IQuickTreeItemButtonEvent, QuickInputAnchor, QuickInputAnchorPosition,
    QuickInputButtonRef, QuickInputHideReason, QuickInputType, QuickPickFocus,
};
use crate::quickinput::quick_input_browser::{IQuickInputUi, QuickInputBase, QuickInputUpdate, Visibilities, bind_core, impl_quick_input};

/// `observableValue<readonly T[]>`: o array do JavaScript compara por identidade, então toda atribuição notifica.
fn items_observable(name: &str) -> Rc<ObservableValue<Vec<IQuickTreeItem>, ()>> {
    ObservableValue::new(DebugNameOptions::named(name).into(), Vec::new(), Rc::new(|_, _| false))
}

pub struct QuickTree {
    core: Rc<QuickInputBase>,
    this: Weak<QuickTree>,
    value: Rc<ObservableValue<String, ()>>,
    aria_label: Rc<ObservableValue<Option<String>, ()>>,
    placeholder: Rc<ObservableValue<Option<String>, ()>>,
    match_on_description: Rc<ObservableValue<bool, ()>>,
    match_on_label: Rc<ObservableValue<bool, ()>>,
    sort_by_label: Rc<ObservableValue<bool, ()>>,
    active_items: Rc<ObservableValue<Vec<IQuickTreeItem>, ()>>,
    item_tree: Rc<ObservableValue<Vec<IQuickTreeItem>, ()>>,
    on_did_change_value: Event<String>,
    on_did_change_active: Event<Vec<IQuickTreeItem>>,
    on_did_change_checked_leaf_items: Rc<Emitter<Vec<IQuickTreeItem>>>,
    on_did_change_checkbox_state: Rc<Emitter<IQuickTreeItem>>,
    on_did_accept_emitter: Rc<Emitter<()>>,
    on_did_accept: Event<()>,
    timeout_store: Rc<DisposableStore>,
}

impl QuickTree {
    /// `QuickTree.DEFAULT_ARIA_LABEL`.
    fn default_aria_label() -> String {
        localize("quickInputBox.ariaLabel", "Type to narrow down results.", &[])
    }

    pub fn new(ui: Rc<dyn IQuickInputUi>) -> Rc<Self> {
        let core = QuickInputBase::new(ui.clone(), QuickInputType::QuickTree);
        let store = DisposableStore::new();
        core.register(store.clone());
        let timeout_store = DisposableStore::new();
        core.register(timeout_store.clone());

        let value = observable_value("value", String::new());
        let active_items = items_observable("activeItems");
        let on_did_change_checked_leaf_items = Emitter::new();
        let on_did_change_checkbox_state = Emitter::new();
        let on_did_accept_emitter = Emitter::new();
        core.register(on_did_change_checked_leaf_items.clone());
        core.register(on_did_change_checkbox_state.clone());
        core.register(on_did_accept_emitter.clone());

        let value_observable: IObservable<String> = value.clone();
        let active_observable: IObservable<Vec<IQuickTreeItem>> = active_items.clone();
        let this = Rc::new_cyclic(|this| Self {
            core: core.clone(),
            this: this.clone(),
            on_did_change_value: Event::from_observable(value_observable, Some(&store)),
            on_did_change_active: Event::from_observable(active_observable, Some(&store)),
            value,
            aria_label: observable_value("ariaLabel", None),
            placeholder: observable_value("placeholder", None),
            match_on_description: observable_value("matchOnDescription", false),
            match_on_label: observable_value("matchOnLabel", true),
            sort_by_label: observable_value("sortByLabel", true),
            active_items,
            item_tree: items_observable("itemTree"),
            on_did_accept: je_base::event::any(vec![ui.on_did_accept(), on_did_accept_emitter.event()]),
            on_did_change_checked_leaf_items,
            on_did_change_checkbox_state,
            on_did_accept_emitter,
            timeout_store,
        });
        let this = bind_core(this, &core);
        this.register_autoruns();

        let tree = ui.tree();
        let weak = this.this.clone();
        core.register(tree.on_did_change_checked_leaf_items().listen({
            let weak = weak.clone();
            move |items| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_checked_leaf_items.fire(items.clone());
                }
            }
        }));
        core.register(tree.on_did_change_checkbox_state().listen({
            let weak = weak.clone();
            move |event| {
                if let Some(this) = weak.upgrade() {
                    this.on_did_change_checkbox_state.fire(event.item.clone());
                }
            }
        }));
        // Sync active items with tree focus changes
        core.register(tree.on_did_change_focus().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.active_items.set(this.core.ui().tree().get_active_items(), None);
            }
        }));
        this
    }

    /// `registerVisibleAutorun(fn)`: roda `f` quando o quick input está visível, e de novo quando o que `f` leu muda.
    fn register_visible_autorun(&self, f: impl Fn(&dyn IReaderWithStore) + 'static) {
        let visible: IObservable<bool> = self.core.visible_observable().clone();
        self.core.register(autorun(move |reader| {
            if visible.read(reader) {
                f(reader);
            }
        }));
    }

    /// `_registerAutoruns()`.
    fn register_autoruns(&self) {
        let ui = self.core.ui().clone();
        let core = Rc::downgrade(&self.core);

        let (value, input_ui) = (self.value.clone(), ui.clone());
        self.register_visible_autorun(move |reader| {
            let value = value.read(reader);
            input_ui.input_box().set_value(&value);
            input_ui.tree().filter(&value);
        });

        let (aria_label, placeholder, label_ui) = (self.aria_label.clone(), self.placeholder.clone(), ui.clone());
        self.register_visible_autorun(move |reader| {
            let mut aria_label = aria_label.read(reader).filter(|label| !label.is_empty());
            if aria_label.is_none() {
                let mut label = placeholder.get().filter(|placeholder| !placeholder.is_empty()).unwrap_or_else(Self::default_aria_label);
                // If we have a title, include it in the aria label.
                if let Some(title) = core.upgrade().and_then(|core| core.title()).filter(|title| !title.is_empty()) {
                    label.push_str(&format!(" - {title}"));
                }
                aria_label = Some(label);
            }
            let aria_label = aria_label.unwrap_or_default();
            if label_ui.list().aria_label().as_deref() != Some(aria_label.as_str()) {
                label_ui.list().set_aria_label(Some(&aria_label));
            }
            if label_ui.input_box().aria_label().as_deref() != Some(aria_label.as_str()) {
                label_ui.input_box().set_aria_label(&aria_label);
            }
        });

        let (placeholder, placeholder_ui) = (self.placeholder.clone(), ui.clone());
        self.register_visible_autorun(move |reader| {
            let placeholder = placeholder.read(reader);
            let input_box = placeholder_ui.input_box();
            if Some(input_box.placeholder()) != placeholder {
                input_box.set_placeholder(&placeholder.unwrap_or_default());
            }
        });

        let (match_on_label, match_on_description, filter_ui) = (self.match_on_label.clone(), self.match_on_description.clone(), ui.clone());
        self.register_visible_autorun(move |reader| {
            let match_on_label = match_on_label.read(reader);
            let match_on_description = match_on_description.read(reader);
            filter_ui.tree().update_filter_options(Some(match_on_label), Some(match_on_description));
        });

        let (sort_by_label, sort_ui) = (self.sort_by_label.clone(), ui.clone());
        self.register_visible_autorun(move |reader| {
            let sort_by_label = sort_by_label.read(reader);
            sort_ui.tree().set_sort_by_label(sort_by_label);
        });

        let item_tree = self.item_tree.clone();
        self.register_visible_autorun(move |reader| {
            let item_tree = item_tree.read(reader);
            ui.tree().set_tree_data(&item_tree);
        });
    }

    /// Os `Visibilities` de `show()` e `update()`.
    fn visibilities(&self) -> Visibilities {
        let title = self.core.title().is_some_and(|title| !title.is_empty());
        let step = self.core.step().is_some_and(|step| step != 0);
        Visibilities {
            title: Some(title || step || self.core.title_buttons_len() > 0),
            description: Some(self.core.description().is_some_and(|description| !description.is_empty())),
            check_all: Some(true),
            check_box: Some(true),
            input_box: Some(true),
            progress_bar: Some(true),
            visible_count: Some(true),
            count: Some(true),
            ok: Some(true),
            tree: Some(true),
            message: Some(self.core.validation_message().is_some_and(|message| !message.is_empty())),
            ..Default::default()
        }
    }

    /// `if (this.ui.checkAll.checked !== checkAllState) this.ui.checkAll.checked = checkAllState`.
    fn sync_check_all(ui: &dyn IQuickInputUi) {
        let check_all_state = ui.tree().get_check_all_state();
        if ui.check_all_state() != check_all_state {
            ui.set_check_all_state(check_all_state);
        }
    }

    fn show_impl(&self) {
        let ui = self.core.ui().clone();
        if !self.core.is_visible() {
            ui.set_visibilities(self.visibilities());
            let store = self.core.visible_disposables();
            let value = self.value.clone();
            store.add(ui.input_box().on_did_change().listen(move |input| {
                value.set(input.clone(), None);
            }));
            let check_all_ui = ui.clone();
            store.add(ui.tree().on_did_change_checkbox_state().listen(move |_| Self::sync_check_all(&*check_all_ui)));
            let tree_ui = ui.clone();
            store.add(ui.on_did_change_check_all().listen(move |()| {
                let checked = tree_ui.check_all_state();
                tree_ui.tree().check_all(checked);
            }));
            let count_ui = ui.clone();
            store.add(ui.tree().on_did_change_checked_leaf_items().listen(move |items| count_ui.set_count(items.len())));
        }
        self.core.show();

        // Initial state
        // Without this setTimeout, the screen reader will not read out the final count of checked items correctly.
        // ref https://github.com/microsoft/vscode/issues/258617
        let count_ui = ui.clone();
        disposable_timeout(move || count_ui.set_count(count_ui.tree().get_checked_leaf_items().len()), 0, Some(self.timeout_store.clone()));
        Self::sync_check_all(&*ui);
    }
}

impl QuickInputUpdate for QuickTree {
    fn update(&self) {
        if !self.core.is_visible() {
            return;
        }
        self.core.ui().set_visibilities(self.visibilities());
        self.core.base_update();
    }
}

impl_quick_input!(QuickTree);

impl IQuickTree for QuickTree {
    fn on_did_change_value(&self) -> Event<String> {
        self.on_did_change_value.clone()
    }

    fn on_did_accept(&self) -> Event<()> {
        self.on_did_accept.clone()
    }

    fn on_did_change_active(&self) -> Event<Vec<IQuickTreeItem>> {
        self.on_did_change_active.clone()
    }

    fn on_did_change_checked_leaf_items(&self) -> Event<Vec<IQuickTreeItem>> {
        self.on_did_change_checked_leaf_items.event()
    }

    fn on_did_change_checkbox_state(&self) -> Event<IQuickTreeItem> {
        self.on_did_change_checkbox_state.event()
    }

    fn on_did_trigger_item_button(&self) -> Event<IQuickTreeItemButtonEvent> {
        self.core.ui().tree().on_did_trigger_button()
    }

    fn value(&self) -> String {
        self.value.get()
    }

    fn set_value(&self, value: String) {
        self.value.set(value, None);
    }

    fn aria_label(&self) -> Option<String> {
        self.aria_label.get()
    }

    fn set_aria_label(&self, aria_label: Option<String>) {
        self.aria_label.set(aria_label, None);
    }

    fn placeholder(&self) -> Option<String> {
        self.placeholder.get()
    }

    fn set_placeholder(&self, placeholder: Option<String>) {
        self.placeholder.set(placeholder, None);
    }

    fn match_on_description(&self) -> bool {
        self.match_on_description.get()
    }

    fn set_match_on_description(&self, match_on_description: bool) {
        self.match_on_description.set(match_on_description, None);
    }

    fn match_on_label(&self) -> bool {
        self.match_on_label.get()
    }

    fn set_match_on_label(&self, match_on_label: bool) {
        self.match_on_label.set(match_on_label, None);
    }

    fn sort_by_label(&self) -> bool {
        self.sort_by_label.get()
    }

    fn set_sort_by_label(&self, sort_by_label: bool) {
        self.sort_by_label.set(sort_by_label, None);
    }

    fn active_items(&self) -> Vec<IQuickTreeItem> {
        self.active_items.get()
    }

    fn set_active_items(&self, active_items: Vec<IQuickTreeItem>) {
        self.active_items.set(active_items, None);
    }

    fn validation_message(&self) -> Option<String> {
        self.core.validation_message()
    }

    fn set_validation_message(&self, validation_message: Option<String>) {
        self.core.set_validation_message(validation_message);
    }

    fn severity(&self) -> Severity {
        self.core.severity()
    }

    fn set_severity(&self, severity: Severity) {
        self.core.set_severity(severity);
    }

    fn item_tree(&self) -> Vec<IQuickTreeItem> {
        self.item_tree.get()
    }

    fn checked_leaf_items(&self) -> Vec<IQuickTreeItem> {
        self.core.ui().tree().get_checked_leaf_items()
    }

    fn get_parent(&self, element: &IQuickTreeItem) -> Option<IQuickTreeItem> {
        self.core.ui().tree().get_parent_element(element)
    }

    fn set_item_tree(&self, item_tree: Vec<IQuickTreeItem>) {
        self.item_tree.set(item_tree, None);
    }

    fn expand(&self, element: &IQuickTreeItem) {
        self.core.ui().tree().expand(element);
    }

    fn collapse(&self, element: &IQuickTreeItem) {
        self.core.ui().tree().collapse(element);
    }

    fn is_collapsed(&self, element: &IQuickTreeItem) -> bool {
        self.core.ui().tree().is_collapsed(element)
    }

    fn focus_on_input(&self) {
        self.core.ui().input_box().set_focus();
    }

    fn reveal(&self, element: &IQuickTreeItem) {
        self.core.ui().tree().reveal(element);
    }

    fn focus(&self, focus: QuickPickFocus) {
        let tree = self.core.ui().tree();
        tree.focus(focus);
        // To allow things like space to check/uncheck items
        tree.dom_focus();
    }

    /// Programmatically accepts an item. Used internally for keyboard navigation.
    fn accept(&self, _in_background: Option<bool>) {
        self.on_did_accept_emitter.fire(());
    }
}
