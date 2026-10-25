//! Porte de `src/vs/editor/contrib/gotoSymbol/browser/peek/referencesController.ts`: o `ReferencesController` (a
//! contribuição que abre, troca e fecha o peek de referências), a chave de contexto `referenceSearchVisible` e os comandos e
//! atalhos do peek (`togglePeekWidgetFocus`, `goToNextReference`, `goToPreviousReference`, `closeReferenceSearch`,
//! `revealReference`, `openReferenceToSide` e `openReference`).
//!
//! Representação:
//! - `abstract class ReferencesController` é a struct [`ReferencesController`], com o `_defaultTreeKeyboardSupport` como
//!   parâmetro de [`register_references_controller`] (o `WorkbenchReferencesController` passa `true` e o
//!   `StandaloneReferencesController` passa `false`). O editor é um `Weak` (o editor possui a contribuição).
//! - `ReferencesController.get(editor)` é [`ReferencesController::get`]. O `toggleWidget` é o método do contrato
//!   [`IReferencesController`] de `go_to_commands`, que as ações de navegação chamam; o serviço
//!   [`ReferencesControllerLookup`] (o `ReferencesController.get(editor)` que elas fazem) é registrado por
//!   [`register_references_controller`] no id `I_REFERENCES_CONTROLLER_LOOKUP` de `go_to_commands`.
//! - `async goToNextOrPreviousReference` e o `then` do `_gotoReference` são [`Promise`]s; o que o original roda antes do
//!   primeiro `await` roda dentro da chamada.
//! - `@IContextKeyService`, `@ICodeEditorService`, `@INotificationService`, `@IStorageService`, `@IConfigurationService` e
//!   `@IInstantiationService` do construtor chegam em [`ReferencesControllerServices`] (o mesmo expediente de
//!   `CommonFindControllerServices`). O `createInstance(ReferenceWidget, editor, defaultTreeKeyboardSupport, data)` é o
//!   [`IReferenceWidgetFactory`].
//! - A chave `peekViewLayout` guarda o `LayoutData` como texto JSON. `LayoutData.fromJSON(raw)` e o `JSON.stringify` do
//!   `layoutData` são do widget (que é o dono do `LayoutData`): a fábrica recebe o texto lido e o widget devolve o texto a
//!   guardar em [`IReferenceWidget::layout_data_json`].
//! - `lastFocusedList?.getFocus()[0] instanceof OneReference` dos comandos de teclado é o primeiro elemento em foco da
//!   lista (`IListWidget::get_focused_element`) com `downcast` para `Rc<OneReference>`; a árvore do widget registra os
//!   elementos como `Rc<OneReference>`.
//! - `ref: Location` de `openReference` e `_gotoReference` é [`Location`]; um `OneReference` vira `{ uri, range }`.
//! - `CommandsRegistry.registerCommandAlias`, `registerCommand`, `registerCommandAndKeybindingRule` e
//!   `registerKeybindingRule` do carregamento do módulo são [`register_references_controller`], idempotente por thread.
//!
//! # Contrato do `ReferenceWidget` (`referencesWidget.ts`, a fatia seguinte)
//!
//! O controlador cria o widget pela [`IReferenceWidgetFactory`] e fala com ele pela [`IReferenceWidget`]. A fatia do widget
//! implementa os dois e entrega a fábrica em [`ReferencesControllerServices::widget_factory`]:
//! - **A fábrica** (`createInstance(ReferenceWidget, editor, defaultTreeKeyboardSupport, data)`): recebe o editor, o
//!   `defaultTreeKeyboardSupport` e o texto JSON do `LayoutData` (`'{}'` quando nada foi guardado), e devolve o widget já
//!   criado (o construtor do original chama `this.create()`, aplica o tema e registra o widget na `IPeekViewService` com
//!   `addExclusiveWidget(editor, this)`).
//! - **O widget** é um [`PeekViewWidget`](crate::peek_view::PeekViewWidget) por dentro (`setTitle`, `setMetaTitle`,
//!   `show`/`hide`/`position` do `ZoneWidget` e `onDidClose`). `show(range)` é o `show(where)` do `ReferenceWidget`, que usa
//!   `layoutData.heightInLines || 18`. `dispose` marca `isClosing` antes de qualquer outra coisa (o controlador depende
//!   disso para não recursar no `onDidClose`). `layout_data_json()` é o `JSON.stringify(layoutData)`.
//! - **Os eventos** `onDidSelectReference` carregam o [`SelectionEvent`] (`kind`, `source` e `element: Location`), também
//!   definido aqui porque o controlador é quem o consome.
//! - `setModel`, `setSelection` e `revealReference` devolvem [`Promise<()>`](Promise) (o `Promise<unknown>` do original).

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, create_cancelable_promise};
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::{self, Error};
use je_base::event::Event;
use je_base::key_codes::{KeyCode, KeyMod, key_chord};
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::nls::localize;
use je_base::promise::{CancelablePromise, Promise};
use je_instantiation::{Injectable, InstantiationType, ServicesAccessor, register_singleton, service_impl};
use je_platform::commands::{Command as RegisteredCommand, CommandHandler, commands_registry};
use je_platform::configuration::service::IConfigurationService;
use je_platform::contextkey::contextkeys::INPUT_FOCUSED_CONTEXT_KEY;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression, IContextKey, IContextKeyService, RawContextKey};
use je_platform::keybinding::keybindings_registry::{
    CommandAndKeybindingRule, KeybindingRule, KeybindingWeight, Keybindings, PlatformKeybindings, keybindings_registry,
};
use je_platform::list::list_service::{
    I_LIST_SERVICE, workbench_list_focus_context_key, workbench_tree_element_can_collapse, workbench_tree_element_can_expand,
};
use je_platform::notification::{INotificationService, NotificationMessage};
use je_platform::opener::editor_types::EditorOptions;
use je_platform::storage::{StorageScope, StorageService, StorageTarget};

use crate::code_editor_widget::{CodeEditorWidget, EditorContributionInstantiation, ICodeEditor, IEditorContribution};
use crate::config::editor_options::registry::PEEK_WIDGET_DEFAULT_FOCUS;
use crate::core_commands::{editor_text_focus, text_editor_selection_source};
use crate::editor_extensions::register_editor_contribution;
use crate::embedded_code_editor_widget::get_outer_editor;
use crate::goto_symbol::go_to_commands::{I_REFERENCES_CONTROLLER_LOOKUP, IReferencesController, IReferencesControllerLookup};
use crate::goto_symbol::references_model::{OneReference, ReferencesModel};
use crate::language_features_service::Location;
use crate::opener_service::{ICodeEditorService, TextEditorOptions, TextResourceEditorInput, range_to_text_editor_selection};
use crate::peek_view::in_peek_editor;
use crate::position::Position;
use crate::range::Range;

thread_local! {
    /// `ctxReferenceSearchVisible`.
    static CTX_REFERENCE_SEARCH_VISIBLE: RawContextKey = RawContextKey::new(
        "referenceSearchVisible",
        false,
        localize("referenceSearchVisible", "Whether reference peek is visible, like 'Peek References' or 'Peek Definition'", &[]),
    );
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

/// `ctxReferenceSearchVisible`.
pub fn ctx_reference_search_visible() -> RawContextKey {
    CTX_REFERENCE_SEARCH_VISIBLE.with(Clone::clone)
}

/// A chave do `LayoutData` no armazenamento (`storageKey`).
const PEEK_VIEW_LAYOUT_KEY: &str = "peekViewLayout";

// #region Contrato do ReferenceWidget

/// `SelectionEvent.kind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionKind {
    Goto,
    Show,
    Side,
    Open,
}

/// `SelectionEvent.source`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SelectionSource {
    Editor,
    Tree,
    Title,
}

/// `SelectionEvent`.
#[derive(Clone, Debug)]
pub struct SelectionEvent {
    pub kind: SelectionKind,
    pub source: SelectionSource,
    pub element: Option<Location>,
}

/// O que o controlador pede ao `ReferenceWidget` (ver "Contrato do `ReferenceWidget`" no cabeçalho).
pub trait IReferenceWidget: IDisposable {
    /// `setTitle(primaryHeading, secondaryHeading?)`.
    fn set_title(&self, primary_heading: &str, secondary_heading: Option<&str>);
    /// `setMetaTitle(value)`.
    fn set_meta_title(&self, value: &str);
    /// `show(where)`.
    fn show(&self, range: Range);
    /// `hide()`.
    fn hide(&self);
    /// `position`.
    fn position(&self) -> Option<Position>;
    /// `onDidClose`.
    fn on_did_close(&self) -> Event<()>;
    /// `onDidSelectReference`.
    fn on_did_select_reference(&self) -> Event<SelectionEvent>;
    /// `JSON.stringify(layoutData)`.
    fn layout_data_json(&self) -> String;
    /// `isClosing`.
    fn is_closing(&self) -> bool;
    /// `setModel(newModel)`.
    fn set_model(&self, model: Option<Rc<ReferencesModel>>) -> Promise<()>;
    /// `setSelection(selection)`.
    fn set_selection(&self, selection: &Rc<OneReference>) -> Promise<()>;
    /// `focusOnReferenceTree()`.
    fn focus_on_reference_tree(&self);
    /// `focusOnPreviewEditor()`.
    fn focus_on_preview_editor(&self);
    /// `isPreviewEditorFocused()`.
    fn is_preview_editor_focused(&self) -> bool;
    /// `revealReference(reference)`.
    fn reveal_reference(&self, reference: &Rc<OneReference>) -> Promise<()>;
}

/// `createInstance(ReferenceWidget, editor, defaultTreeKeyboardSupport, data)`.
pub trait IReferenceWidgetFactory {
    /// `layout_data` é o texto JSON guardado de `LayoutData` (`'{}'` quando não há nada).
    fn create(&self, editor: &Rc<CodeEditorWidget>, default_tree_keyboard_support: bool, layout_data: &str) -> Rc<dyn IReferenceWidget>;
}

// #endregion

/// Os serviços que o construtor de `ReferencesController` recebe por injeção.
#[derive(Clone)]
pub struct ReferencesControllerServices {
    pub context_key_service: Rc<dyn IContextKeyService>,
    pub editor_service: Rc<dyn ICodeEditorService>,
    pub notification_service: Rc<dyn INotificationService>,
    pub storage_service: Rc<dyn StorageService>,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub widget_factory: Rc<dyn IReferenceWidgetFactory>,
}

/// `{ uri, range }` de uma referência (um `OneReference` usado como `Location`).
fn location_of(reference: &OneReference) -> Location {
    Location { uri: reference.uri().clone(), range: reference.range() }
}

/// `{ resource: uri, options: { selection, selectionSource: TextEditorSelectionSource.JUMP, pinned } }`.
fn jump_input(reference: &Location, selection: &Range, pinned: bool) -> TextResourceEditorInput {
    TextResourceEditorInput {
        resource: reference.uri.clone(),
        options: Some(TextEditorOptions {
            editor: EditorOptions { pinned: Some(pinned), ..EditorOptions::default() },
            selection: Some(range_to_text_editor_selection(selection)),
            selection_reveal_type: None,
            selection_source: Some(text_editor_selection_source::JUMP.to_owned()),
        }),
    }
}

/// `ReferencesController`.
pub struct ReferencesController {
    me: Weak<ReferencesController>,
    default_tree_keyboard_support: bool,
    editor: Weak<CodeEditorWidget>,
    services: ReferencesControllerServices,
    disposables: Rc<DisposableStore>,
    widget: RefCell<Option<Rc<dyn IReferenceWidget>>>,
    model: RefCell<Option<Rc<ReferencesModel>>>,
    peek_mode: Cell<Option<bool>>,
    request_id_pool: Cell<u32>,
    ignore_model_change_event: Cell<bool>,
    reference_search_visible: Rc<dyn IContextKey>,
}

impl ReferencesController {
    pub const ID: &'static str = "editor.contrib.referencesController";

    /// `ReferencesController.get(editor)`.
    pub fn get(editor: &Rc<CodeEditorWidget>) -> Option<Rc<ReferencesController>> {
        editor.get_contribution_as::<ReferencesController>(Self::ID)
    }

    pub fn new(default_tree_keyboard_support: bool, editor: &Rc<CodeEditorWidget>, services: ReferencesControllerServices) -> Rc<Self> {
        let reference_search_visible = ctx_reference_search_visible().bind_to(&*services.context_key_service);
        Rc::new_cyclic(|me| Self {
            me: me.clone(),
            default_tree_keyboard_support,
            editor: Rc::downgrade(editor),
            services,
            disposables: DisposableStore::new(),
            widget: RefCell::new(None),
            model: RefCell::new(None),
            peek_mode: Cell::new(None),
            request_id_pool: Cell::new(0),
            ignore_model_change_event: Cell::new(false),
            reference_search_visible,
        })
    }

    fn editor(&self) -> Option<Rc<CodeEditorWidget>> {
        self.editor.upgrade()
    }

    fn widget(&self) -> Option<Rc<dyn IReferenceWidget>> {
        self.widget.borrow().clone()
    }

    fn model(&self) -> Option<Rc<ReferencesModel>> {
        self.model.borrow().clone()
    }

    /// O `then` de `modelPromise` em `toggleWidget`: o resultado chegou e ainda é o pedido corrente.
    fn on_model_resolved(&self, model: Rc<ReferencesModel>, request_id: u32, range: Range) -> Promise<()> {
        // still current request? widget still open?
        let widget = self.widget();
        let Some(widget) = widget.filter(|_| request_id == self.request_id_pool.get()) else {
            model.dispose();
            return Promise::resolved(());
        };

        let previous = self.model.borrow_mut().replace(Rc::clone(&model));
        if let Some(previous) = previous {
            previous.dispose();
        }

        // show widget
        let weak = self.me.clone();
        widget.set_model(Some(model)).then_promise(move |_| match weak.upgrade() {
            Some(controller) => controller.on_model_set(range),
            None => Promise::resolved(()),
        })
    }

    /// O `then` de `this._widget.setModel(this._model)`.
    fn on_model_set(&self, range: Range) -> Promise<()> {
        let (Some(widget), Some(model), Some(editor)) = (self.widget(), self.model(), self.editor()) else {
            return Promise::resolved(());
        };
        // might have been closed
        let Some(editor_model) = editor.get_model() else {
            return Promise::resolved(());
        };

        // set title
        if model.is_empty() {
            widget.set_meta_title("");
        } else {
            widget.set_meta_title(&localize("metaTitle.N", "{0} ({1})", &[model.title(), model.references().len().to_string().as_str()]));
        }

        // set 'best' selection
        let position = Position::new(range.start_line_number, range.start_column);
        let Some(selection) = model.nearest_reference(&editor_model.uri(), &position) else {
            return Promise::resolved(());
        };
        let weak = self.me.clone();
        widget.set_selection(&selection).then(move |_| {
            let Some(controller) = weak.upgrade() else {
                return;
            };
            if let (Some(widget), Some(editor)) = (controller.widget(), controller.editor())
                && editor.get_option(PEEK_WIDGET_DEFAULT_FOCUS).as_str() == "editor"
            {
                widget.focus_on_preview_editor();
            }
        })
    }

    /// `changeFocusBetweenPreviewAndReferences()`.
    pub fn change_focus_between_preview_and_references(&self) {
        // can be called while still resolving...
        let Some(widget) = self.widget() else {
            return;
        };
        if widget.is_preview_editor_focused() {
            widget.focus_on_reference_tree();
        } else {
            widget.focus_on_preview_editor();
        }
    }

    /// `async goToNextOrPreviousReference(fwd)`.
    pub fn go_to_next_or_previous_reference(&self, fwd: bool) -> Promise<()> {
        // can be called while still resolving...
        let (Some(editor), Some(model), Some(widget)) = (self.editor(), self.model(), self.widget()) else {
            return Promise::resolved(());
        };
        let (Some(editor_model), Some(current_position)) = (editor.get_model(), widget.position()) else {
            return Promise::resolved(());
        };
        let Some(source) = model.nearest_reference(&editor_model.uri(), &current_position) else {
            return Promise::resolved(());
        };
        let target = model.next_or_previous_reference(&source, fwd);
        let editor_focus = editor.has_text_focus();
        let preview_editor_focus = widget.is_preview_editor_focused();

        let weak = self.me.clone();
        async_fn(async move {
            widget.set_selection(&target).await?;
            let Some(controller) = weak.upgrade() else {
                return Ok(());
            };
            controller.goto_reference(&location_of(&target), false).await?;
            if editor_focus {
                editor.focus();
            } else if preview_editor_focus && let Some(widget) = controller.widget() {
                widget.focus_on_preview_editor();
            }
            Ok::<(), Error>(())
        })
    }

    /// `async revealReference(reference)`.
    pub fn reveal_reference(&self, reference: &Rc<OneReference>) -> Promise<()> {
        // can be called while still resolving...
        match (self.editor().is_some_and(|editor| editor.has_model()), self.model(), self.widget()) {
            (true, Some(_), Some(widget)) => widget.reveal_reference(reference),
            _ => Promise::resolved(()),
        }
    }

    /// `closeWidget(focusEditor = true)`.
    pub fn close_widget(&self, focus_editor: bool) {
        // Os dois ficam atribuídos durante o `dispose`: o `onDidClose` do widget lê `_widget`.
        let widget = self.widget();
        if let Some(widget) = widget {
            widget.dispose();
        }
        let model = self.model();
        if let Some(model) = model {
            model.dispose();
        }
        self.reference_search_visible.reset();
        self.disposables.clear();
        *self.widget.borrow_mut() = None;
        *self.model.borrow_mut() = None;
        if focus_editor && let Some(editor) = self.editor() {
            editor.focus();
        }
        // Cancel pending requests
        self.request_id_pool.set(self.request_id_pool.get() + 1);
    }

    /// `_gotoReference(ref, pinned)`.
    fn goto_reference(&self, reference: &Location, pinned: bool) -> Promise<()> {
        let widget = self.widget();
        if let Some(widget) = widget {
            widget.hide();
        }

        let Some(editor) = self.editor() else {
            return Promise::resolved(());
        };
        self.ignore_model_change_event.set(true);
        let range = reference.range.collapse_to_start();

        let weak = self.me.clone();
        let opened_from = Rc::clone(&editor);
        self.services.editor_service.open_code_editor(jump_input(reference, &range, pinned), Some(editor), None).then_result(move |result| {
            let Some(controller) = weak.upgrade() else {
                return Ok(());
            };
            controller.ignore_model_change_event.set(false);
            match result {
                Err(error) => errors::on_unexpected_error(&error),
                Ok(opened_editor) => controller.on_goto_opened(&opened_from, opened_editor, range),
            }
            Ok(())
        })
    }

    /// O `then` de `openCodeEditor` em `_gotoReference`.
    fn on_goto_opened(&self, editor: &Rc<CodeEditorWidget>, opened_editor: Option<Rc<CodeEditorWidget>>, range: Range) {
        let (Some(opened_editor), Some(widget)) = (opened_editor, self.widget()) else {
            // something went wrong...
            self.close_widget(true);
            return;
        };

        if Rc::ptr_eq(editor, &opened_editor) {
            widget.show(range);
            widget.focus_on_reference_tree();
        } else {
            // we opened a different editor instance which means a different controller instance.
            // therefore we stop with this controller and continue with the other
            let other = ReferencesController::get(&opened_editor);
            let model = self.model().map(|model| model.clone_model());
            let peek_mode = self.peek_mode.get().unwrap_or(false);

            self.close_widget(true);
            opened_editor.focus();

            if let (Some(other), Some(model)) = (other, model) {
                other.toggle_widget(range, create_cancelable_promise(move |_| Promise::resolved(model)), peek_mode);
            }
        }
    }

    /// `openReference(ref, sideBySide, pinned)`.
    pub fn open_reference(&self, reference: &Location, side_by_side: bool, pinned: bool) {
        // clear stage
        if !side_by_side {
            self.close_widget(true);
        }

        let _ = self.services.editor_service.open_code_editor(jump_input(reference, &reference.range, pinned), self.editor(), Some(side_by_side));
    }
}

impl IReferencesController for ReferencesController {
    /// `toggleWidget(range, modelPromise, peekMode)`.
    fn toggle_widget(&self, range: Range, model_promise: CancelablePromise<Rc<ReferencesModel>>, peek_mode: bool) {
        let Some(editor) = self.editor() else {
            return;
        };

        // close current widget and return early is position didn't change
        let widget_position = self.widget().and_then(|widget| widget.position());
        self.close_widget(true);
        if widget_position.is_some_and(|position| range.contains_position(&position)) {
            return;
        }

        self.peek_mode.set(Some(peek_mode));
        self.reference_search_visible.set(Dynamic::Bool(true));

        // close the widget on model/mode changes
        let weak = self.me.clone();
        self.disposables.add(editor.on_did_change_model_language().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(controller) = weak.upgrade() {
                    controller.close_widget(true);
                }
            }
        }));
        self.disposables.add(editor.on_did_change_model().listen({
            let weak = weak.clone();
            move |_| {
                if let Some(controller) = weak.upgrade()
                    && !controller.ignore_model_change_event.get()
                {
                    controller.close_widget(true);
                }
            }
        }));

        let layout_data = self.services.storage_service.get(PEEK_VIEW_LAYOUT_KEY, StorageScope::Profile, Some("{}")).unwrap_or_else(|| "{}".to_owned());
        let widget = self.services.widget_factory.create(&editor, self.default_tree_keyboard_support, &layout_data);
        *self.widget.borrow_mut() = Some(Rc::clone(&widget));
        widget.set_title(&localize("labelLoading", "Loading...", &[]), None);
        widget.show(range);

        self.disposables.add(widget.on_did_close().listen({
            let (weak, model_promise) = (weak.clone(), model_promise.clone());
            move |_| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                model_promise.cancel();
                let current = controller.widget();
                match current {
                    Some(widget) => {
                        controller.services.storage_service.store(
                            PEEK_VIEW_LAYOUT_KEY,
                            widget.layout_data_json().as_str().into(),
                            StorageScope::Profile,
                            StorageTarget::Machine,
                        );
                        if !widget.is_closing() {
                            // to prevent calling this too many times, check whether it was already closing.
                            controller.close_widget(true);
                        }
                        *controller.widget.borrow_mut() = None;
                    }
                    None => controller.close_widget(true),
                }
            }
        }));

        self.disposables.add(widget.on_did_select_reference().listen({
            let weak = weak.clone();
            move |event: &SelectionEvent| {
                let Some(controller) = weak.upgrade() else {
                    return;
                };
                let Some(element) = event.element.as_ref() else {
                    return;
                };
                match event.kind {
                    SelectionKind::Open => {
                        let stable_peek = controller.services.configuration_service.get_value(Some("editor.stablePeek"), None).is_truthy();
                        if event.source != SelectionSource::Editor || !stable_peek {
                            // when stable peek is configured we don't close
                            // the peek window on selecting the editor
                            controller.open_reference(element, false, false);
                        }
                    }
                    SelectionKind::Side => controller.open_reference(element, true, false),
                    SelectionKind::Goto => {
                        if peek_mode {
                            let _ = controller.goto_reference(element, true);
                        } else {
                            controller.open_reference(element, false, true);
                        }
                    }
                    SelectionKind::Show => {}
                }
            }
        }));

        let request_id = self.request_id_pool.get() + 1;
        self.request_id_pool.set(request_id);

        let _ = model_promise.then_result_promise(move |result| match (result, weak.upgrade()) {
            (Ok(model), Some(controller)) => controller.on_model_resolved(model, request_id, range),
            (Ok(model), None) => {
                model.dispose();
                Promise::resolved(())
            }
            (Err(error), controller) => {
                if let Some(controller) = controller {
                    controller.services.notification_service.error(vec![NotificationMessage::Error(error)]);
                }
                Promise::resolved(())
            }
        });
    }
}

impl IDisposable for ReferencesController {
    fn dispose(&self) {
        self.reference_search_visible.reset();
        self.disposables.dispose();
        let widget = self.widget.borrow_mut().take();
        if let Some(widget) = widget {
            widget.dispose();
        }
        let model = self.model.borrow_mut().take();
        if let Some(model) = model {
            model.dispose();
        }
    }
}

impl IEditorContribution for ReferencesController {}

/// O serviço que dá o controlador de cada editor às ações de navegação (o `ReferencesController.get(editor)` delas).
pub struct ReferencesControllerLookup;

impl Injectable for ReferencesControllerLookup {
    type Args = ();
    type Deps = ();

    fn create(_: (), _: ()) -> Rc<Self> {
        Rc::new(Self)
    }
}

service_impl!(ReferencesControllerLookup => dyn IReferencesControllerLookup);

impl IReferencesControllerLookup for ReferencesControllerLookup {
    fn get(&self, editor: &Rc<CodeEditorWidget>) -> Option<Rc<dyn IReferencesController>> {
        ReferencesController::get(editor).map(|controller| controller as Rc<dyn IReferencesController>)
    }
}

// #region Comandos e atalhos

/// `withController(accessor, fn)`.
fn with_controller(accessor: &ServicesAccessor, run: impl FnOnce(&Rc<ReferencesController>)) {
    let Some(outer_editor) = get_outer_editor(accessor) else {
        return;
    };
    if let Some(controller) = ReferencesController::get(&outer_editor) {
        run(&controller);
    }
}

/// `{ primary }` e `{ primary, secondary }` sem plataforma.
fn keybindings(primary: u32, secondary: Vec<u32>) -> Keybindings {
    Keybindings { primary: Some(primary), secondary: (!secondary.is_empty()).then_some(secondary), ..Keybindings::default() }
}

fn keybinding_rule(id: &str, weight: i64, keybindings: Keybindings, when: Option<ContextKeyExpression>) -> KeybindingRule {
    KeybindingRule { keybindings, id: id.to_owned(), weight, args: Dynamic::Undefined, when }
}

/// `KeybindingsRegistry.registerCommandAndKeybindingRule({ id, weight, primary, secondary, when, handler })`.
fn register_command_and_keybinding(id: &str, weight: i64, keybindings: Keybindings, when: Option<ContextKeyExpression>, handler: CommandHandler) {
    keybindings_registry().register_command_and_keybinding_rule(CommandAndKeybindingRule {
        rule: keybinding_rule(id, weight, keybindings, when),
        handler,
        metadata: None,
    });
}

/// O handler `accessor => withController(accessor, controller => ...)` que não lê argumentos.
fn controller_handler(run: fn(&Rc<ReferencesController>)) -> CommandHandler {
    Rc::new(move |accessor: &ServicesAccessor, _args: &[Dynamic]| {
        with_controller(accessor, run);
        Dynamic::Undefined
    })
}

/// O handler dos atalhos que agem no `OneReference` em foco da lista (`listService.lastFocusedList?.getFocus()`).
fn focused_reference_handler(run: fn(&Rc<ReferencesController>, &Rc<OneReference>)) -> CommandHandler {
    Rc::new(move |accessor: &ServicesAccessor, _args: &[Dynamic]| {
        let focused = accessor.get(I_LIST_SERVICE).last_focused_list().and_then(|list| list.get_focused_element());
        if let Some(reference) = focused.and_then(|element| element.downcast::<OneReference>().ok()) {
            with_controller(accessor, |controller| run(controller, &reference));
        }
        Dynamic::Undefined
    })
}

/// `ContextKeyExpr.or(ctxReferenceSearchVisible, PeekContext.inPeekEditor)`.
fn visible_or_in_peek_editor() -> Option<ContextKeyExpression> {
    ContextKeyExpr::or([Some(ctx_reference_search_visible().expr()), Some(in_peek_editor().expr())])
}

/// `ctxReferenceSearchVisible && WorkbenchListFocusContextKey && !WorkbenchTreeElementCanCollapse && !WorkbenchTreeElementCanExpand`.
fn visible_and_leaf_focused() -> Option<ContextKeyExpression> {
    ContextKeyExpr::and([
        Some(ctx_reference_search_visible().expr()),
        Some(workbench_list_focus_context_key()),
        Some(workbench_tree_element_can_collapse().to_negated()),
        Some(workbench_tree_element_can_expand().to_negated()),
    ])
}

/// `KeybindingsRegistry` e `CommandsRegistry` do fim do arquivo.
fn register_commands_and_keybindings() {
    let key_k_chord = |second: u32| key_chord(KeyMod::CTRL_CMD | KeyCode::KeyK.bits(), second);

    register_command_and_keybinding(
        "togglePeekWidgetFocus",
        KeybindingWeight::EDITOR_CONTRIB,
        keybindings(key_k_chord(KeyCode::F2.bits()), Vec::new()),
        visible_or_in_peek_editor(),
        controller_handler(|controller| controller.change_focus_between_preview_and_references()),
    );

    register_command_and_keybinding(
        "goToNextReference",
        KeybindingWeight::EDITOR_CONTRIB - 10,
        keybindings(KeyCode::F4.bits(), vec![KeyCode::F12.bits()]),
        visible_or_in_peek_editor(),
        controller_handler(|controller| {
            let _ = controller.go_to_next_or_previous_reference(true);
        }),
    );

    register_command_and_keybinding(
        "goToPreviousReference",
        KeybindingWeight::EDITOR_CONTRIB - 10,
        keybindings(KeyMod::SHIFT | KeyCode::F4.bits(), vec![KeyMod::SHIFT | KeyCode::F12.bits()]),
        visible_or_in_peek_editor(),
        controller_handler(|controller| {
            let _ = controller.go_to_next_or_previous_reference(false);
        }),
    );

    // commands that aren't needed anymore because there is now ContextKeyExpr.OR
    commands_registry().register_command_alias("goToNextReferenceFromEmbeddedEditor", "goToNextReference");
    commands_registry().register_command_alias("goToPreviousReferenceFromEmbeddedEditor", "goToPreviousReference");

    // close
    commands_registry().register_command_alias("closeReferenceSearchEditor", "closeReferenceSearch");
    commands_registry().register_command(RegisteredCommand {
        id: "closeReferenceSearch".to_owned(),
        handler: controller_handler(|controller| controller.close_widget(true)),
        metadata: None,
    });
    let not_stable_peek = || ContextKeyExpr::not("config.editor.stablePeek");
    let close_keys = || keybindings(KeyCode::Escape.bits(), vec![KeyMod::SHIFT | KeyCode::Escape.bits()]);
    keybindings_registry().register_keybinding_rule(keybinding_rule(
        "closeReferenceSearch",
        KeybindingWeight::EDITOR_CONTRIB - 101,
        close_keys(),
        ContextKeyExpr::and([Some(in_peek_editor().expr()), Some(not_stable_peek())]),
    ));
    keybindings_registry().register_keybinding_rule(keybinding_rule(
        "closeReferenceSearch",
        KeybindingWeight::WORKBENCH_CONTRIB + 50,
        close_keys(),
        ContextKeyExpr::and([
            Some(ctx_reference_search_visible().expr()),
            Some(not_stable_peek()),
            ContextKeyExpr::or([Some(editor_text_focus()), Some(ContextKeyExpr::not(INPUT_FOCUSED_CONTEXT_KEY))]),
        ]),
    ));

    register_command_and_keybinding(
        "revealReference",
        KeybindingWeight::WORKBENCH_CONTRIB,
        Keybindings {
            primary: Some(KeyCode::Enter.bits()),
            mac: Some(PlatformKeybindings { primary: Some(KeyCode::Enter.bits()), secondary: Some(vec![KeyMod::CTRL_CMD | KeyCode::DownArrow.bits()]) }),
            ..Keybindings::default()
        },
        visible_and_leaf_focused(),
        focused_reference_handler(|controller, reference| {
            let _ = controller.reveal_reference(reference);
        }),
    );

    register_command_and_keybinding(
        "openReferenceToSide",
        KeybindingWeight::EDITOR_CONTRIB,
        Keybindings {
            primary: Some(KeyMod::CTRL_CMD | KeyCode::Enter.bits()),
            mac: Some(PlatformKeybindings { primary: Some(KeyMod::WIN_CTRL | KeyCode::Enter.bits()), secondary: None }),
            ..Keybindings::default()
        },
        visible_and_leaf_focused(),
        focused_reference_handler(|controller, reference| controller.open_reference(&location_of(reference), true, true)),
    );

    commands_registry().register_command(RegisteredCommand {
        id: "openReference".to_owned(),
        handler: focused_reference_handler(|controller, reference| controller.open_reference(&location_of(reference), false, true)),
        metadata: None,
    });
}

// #endregion

/// `registerEditorContribution(ReferencesController.ID, ..., Lazy)` das subclasses concretas (`WorkbenchReferencesController`
/// e `StandaloneReferencesController`), mais o serviço de busca do controlador e os comandos e atalhos do módulo (ver o topo
/// do módulo). `services` entrega os serviços de cada editor.
pub fn register_references_controller(
    default_tree_keyboard_support: bool,
    services: Rc<dyn Fn(&Rc<CodeEditorWidget>) -> ReferencesControllerServices>,
) {
    if REGISTERED.replace(true) {
        return;
    }
    register_editor_contribution(
        ReferencesController::ID,
        Rc::new(move |editor: &Rc<CodeEditorWidget>| -> Rc<dyn IEditorContribution> {
            ReferencesController::new(default_tree_keyboard_support, editor, services(editor))
        }),
        EditorContributionInstantiation::Lazy,
    );
    register_singleton::<ReferencesControllerLookup>(I_REFERENCES_CONTROLLER_LOOKUP, InstantiationType::Eager);
    register_commands_and_keybindings();
}
