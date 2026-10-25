//! Porte do `ResourceNavigator` e do `TreeResourceNavigator` de `src/vs/platform/list/browser/listService.ts` (e do
//! `IOpenEvent` e do `IResourceNavigatorOptions`): o que dá o `onDidOpen` às árvores do workbench (a seleção por teclado,
//! o clique simples, o duplo clique), sem a árvore, o contexto e os estilos do `WorkbenchTreeInternals`.
//!
//! Notas de tradução:
//! - **A origem dos eventos.** O original lê `widget.onDidChangeSelection`, `onPointer` e `onMouseDblClick` e o
//!   `getSelection()[0]` da própria árvore; aqui o [`ResourceNavigator`] recebe esses quatro (os eventos da árvore e a leitura
//!   da seleção), então serve às árvores de dados, assíncronas e de objetos sem depender de nenhuma.
//! - **`SelectionKeyboardEvent`.** O `preserveFocus` e o `pinned` que o `getSelectionKeyboardEvent` pendura no evento de teclado
//!   não existem no [`BrowserEvent`] da lista, então a seleção por teclado sempre cai no padrão do original (`preserveFocus`
//!   verdadeiro e `pinned` falso).
//! - **Duplo clique sobre o twistie.** O `target.classList.contains('monaco-tl-twistie')` (e o `offsetX < 16` do rótulo de
//!   pasta, "copiado do AbstractTree") é o `target` do [`TreeMouseEvent`], que a árvore já calcula com essa mesma regra.

use std::cell::Cell;
use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::configuration::service::IConfigurationService;
use je_platform::opener::editor_types::EditorOptions;
use je_ui::PointerButton;
use je_widgets::list::list_types::BrowserEvent;
use je_widgets::tree::tree_types::{TreeEvent, TreeMouseEvent, TreeMouseEventTarget};

/// `openModeSettingKey`.
pub const OPEN_MODE_SETTING_KEY: &str = "workbench.list.openMode";

/// `IOpenEvent<T>`.
#[derive(Clone)]
pub struct IOpenEvent<T> {
    pub editor_options: EditorOptions,
    pub side_by_side: bool,
    pub element: T,
    pub browser_event: Option<BrowserEvent>,
}

/// `IResourceNavigatorOptions`.
#[derive(Clone, Default)]
pub struct IResourceNavigatorOptions {
    pub configuration_service: Option<Rc<dyn IConfigurationService>>,
    pub open_on_single_click: Option<bool>,
}

/// `getValue(openModeSettingKey) !== 'doubleClick'`.
fn opens_on_single_click(service: &dyn IConfigurationService) -> bool {
    !matches!(service.get_value(Some(OPEN_MODE_SETTING_KEY), None), Dynamic::String(mode) if mode.to_string_lossy() == "doubleClick")
}

/// `ResourceNavigator<T>` (e o `TreeResourceNavigator`, que só muda o `getSelectedElement`).
pub struct ResourceNavigator<T: Clone + 'static> {
    base: Rc<Disposable>,
    on_did_open: Rc<Emitter<IOpenEvent<T>>>,
}

impl<T: Clone + 'static> ResourceNavigator<T> {
    pub fn new(
        on_did_change_selection: Event<TreeEvent<T>>,
        on_pointer: Event<TreeMouseEvent<T>>,
        on_mouse_dbl_click: Event<TreeMouseEvent<T>>,
        get_selected_element: impl Fn() -> Option<T> + 'static,
        options: Option<IResourceNavigatorOptions>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_open = Emitter::new();
        base.register(on_did_open.clone());

        let open_on_single_click = Rc::new(Cell::new(true));
        let options = options.unwrap_or_default();
        match (options.open_on_single_click, &options.configuration_service) {
            (None, Some(service)) => {
                open_on_single_click.set(opens_on_single_click(service.as_ref()));
                let (flag, service_for_change) = (open_on_single_click.clone(), service.clone());
                base.register(service.on_did_change_configuration().listen(move |e| {
                    if e.affects_configuration(OPEN_MODE_SETTING_KEY, None) {
                        flag.set(opens_on_single_click(service_for_change.as_ref()));
                    }
                }));
            }
            (single_click, _) => open_on_single_click.set(single_click.unwrap_or(true)),
        }

        let emitter = on_did_open.clone();
        base.register(on_did_change_selection.listen(move |event: &TreeEvent<T>| {
            if !event.browser_event.is_some_and(|browser_event| browser_event.is_keyboard) {
                return;
            }
            // onSelectionFromKeyboard
            if event.elements.len() != 1 {
                return;
            }

            let preserve_focus = true;
            let pinned = !preserve_focus;
            let side_by_side = false;

            Self::open(&emitter, get_selected_element(), preserve_focus, pinned, side_by_side, event.browser_event);
        }));

        let (emitter, flag) = (on_did_open.clone(), open_on_single_click);
        base.register(on_pointer.listen(move |e: &TreeMouseEvent<T>| {
            // onPointer
            if !flag.get() {
                return;
            }

            let is_double_click = e.browser_event.click_count == 2;

            if is_double_click {
                return;
            }

            let is_middle_click = e.browser_event.button == PointerButton::Auxiliary;
            let preserve_focus = true;
            let pinned = is_middle_click;
            let modifiers = e.browser_event.modifiers;
            let side_by_side = modifiers.ctrl || modifiers.meta || modifiers.alt;

            Self::open(&emitter, e.element.clone(), preserve_focus, pinned, side_by_side, Some(e.browser_event));
        }));

        let emitter = on_did_open.clone();
        base.register(on_mouse_dbl_click.listen(move |e: &TreeMouseEvent<T>| {
            // onMouseDblClick: copied from AbstractTree
            if e.target == TreeMouseEventTarget::Twistie {
                return;
            }

            let preserve_focus = false;
            let pinned = true;
            let modifiers = e.browser_event.modifiers;
            let side_by_side = modifiers.ctrl || modifiers.meta || modifiers.alt;

            Self::open(&emitter, e.element.clone(), preserve_focus, pinned, side_by_side, Some(e.browser_event));
        }));

        Rc::new(Self { base, on_did_open })
    }

    /// `onDidOpen`.
    pub fn on_did_open(&self) -> Event<IOpenEvent<T>> {
        self.on_did_open.event()
    }

    /// `_open`.
    fn open(emitter: &Emitter<IOpenEvent<T>>, element: Option<T>, preserve_focus: bool, pinned: bool, side_by_side: bool, browser_event: Option<BrowserEvent>) {
        let Some(element) = element else {
            return;
        };

        emitter.fire(IOpenEvent {
            editor_options: EditorOptions { preserve_focus: Some(preserve_focus), pinned: Some(pinned), reveal_if_visible: Some(true), ..EditorOptions::default() },
            side_by_side,
            element,
            browser_event,
        });
    }
}

impl<T: Clone + 'static> IDisposable for ResourceNavigator<T> {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ResourceNavigator"
    }
}
