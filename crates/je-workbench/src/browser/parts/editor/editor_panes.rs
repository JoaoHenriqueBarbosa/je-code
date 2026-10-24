//! Porte lógico de `src/vs/workbench/browser/parts/editor/editorPanes.ts`: o [`EditorPanes`] de um grupo de editor, sem DOM.
//!
//! O "pane" de editor (a área que mostra o editor) é substituído por um registro lógico do editor visível: o
//! [`EditorPane`] guarda a entrada atribuída (`input`), as opções e o identificador do painel registrado para a entrada, e o
//! [`EditorPanes`] escolhe o painel, o reaproveita quando o descritor é o mesmo, atribui a entrada (`doSetInput`) e esconde o
//! painel ativo quando o editor fecha. Nada é criado, anexado nem pintado.
//!
//! Divergências:
//! - **Registro de painéis.** `Registry.as<IEditorPaneRegistry>(EditorExtensions.EditorPane)` é o [`EditorPaneRegistry`] da
//!   thread ([`editor_pane_registry`]). As classes de entrada (`SyncDescriptor<EditorInput>`) são [`TypeId`]s do tipo concreto
//!   da entrada: só vale a comparação exata do construtor (`editor.constructor === inputClass`); a segunda busca por
//!   `instanceof` e pela cadeia de protótipos não existe sem herança. O desempate entre vários descritores é o
//!   `EditorInput::prefers_editor_pane`.
//! - **`setInput` do painel.** O painel lógico se comporta como o `TestEditor` do original: atribui a entrada na hora
//!   (`this._input = input`) e espera `input.resolve()`; um `resolve` que falha rejeita o `setInput`. O painel de erro
//!   ([`ERROR_EDITOR_ID`], o `ErrorPlaceholderEditor`) não resolve a entrada.
//! - **Seleção (`IEditorPaneWithSelection`).** O painel criado por [`EditorPaneDescriptor::new_with_selection`] (o
//!   `TestTextFileEditor` dos testes) tem `onDidChangeSelection` e `getSelection`: as opções com que a entrada foi atribuída
//!   ficam como `ITextEditorOptions` ([`TextEditorOptions`], com a seleção), `getSelection` devolve a [`TextEditorPaneSelection`]
//!   dessa seleção, `setSelection` ([`EditorPane::set_selection`]) troca as opções e avisa a mudança, e a mudança de conteúdo do
//!   modelo de texto da entrada avisa `EDIT` (o `onDidChangeModelContent` do controle de `AbstractTextEditor`, que o painel
//!   lógico não tem).
//! - **Sem foco, sem janela, sem confiança.** `pane.focus()`, `hostService.moveTop`, `getActiveElement`, o
//!   `IWorkbenchLayoutService.isRestored` (só afeta o atraso do progresso), o `IWorkspaceTrustManagementService` (o painel de
//!   "confiança necessária" nunca é escolhido: o espaço de trabalho é de confiança), `setVisible`, `layout`,
//!   `setBoundarySashes`, o `LongRunningOperation` com barra de progresso e o `onDidChangeSizeConstraints` não existem. Do
//!   `LongRunningOperation` fica o que decide o fluxo: cada `start` invalida a operação anterior (`isCurrent`).
//! - **Erro ao abrir.** `doShowError` registra o erro no `ILogService` (se houver) e abre o painel de erro com a mesma
//!   entrada; o diálogo modal (`doShowErrorDialog`, só para `source: USER`) depende do `IDialogService.prompt` com ações de
//!   erro e não é portado: o erro vai sempre para o painel de erro.

use std::any::{Any, TypeId};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::{Rc, Weak};

use je_base::async_::async_fn;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{DisposableId, DisposableRef, DisposableStore, IDisposable, id_of, to_disposable};
use je_base::promise::Promise;
use je_instantiation::InstantiationService;
use je_platform::log::I_LOG_SERVICE;
use je_platform::opener::editor_types::{EditorOptions, TextEditorSelection};
use je_platform::registry::registry;
use je_editor::selection::Selection;

use crate::browser::parts::editor::text_editor::TextEditorPaneSelection;
use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::{
    EditorExtensions, EditorInputOrUntyped, EditorInputRef, EditorOpenContext, EditorPaneSelectionChangeEvent, EditorPaneSelectionChangeReason, GroupIdentifier,
    IEditorDescriptor, IEditorPane, IEditorPaneSelection, TextEditorOptions, WillInstantiateEditorPaneEvent,
};
use crate::services::textfile::text_file_editor_model::TextFileEditorModel;
use crate::services::textfile::textfiles::ITextFileEditorModel;

/// `ErrorPlaceholderEditor.ID`: o painel que mostra o erro de uma entrada que não abriu.
pub const ERROR_EDITOR_ID: &str = "workbench.editors.errorEditor";

/// `EditorPaneDescriptor`: o identificador e o nome do painel registrado.
pub struct EditorPaneDescriptor {
    id: String,
    name: String,
    /// Se o painel criado tem seleção (`IEditorPaneWithSelection`): o do editor de texto, que o histórico acompanha.
    with_selection: bool,
}

thread_local! {
    /// `EditorPaneDescriptor.instantiatedEditorPanes`: estático no original, aqui um por thread.
    static INSTANTIATED_EDITOR_PANES: RefCell<HashSet<String>> = RefCell::new(HashSet::new());
    /// `EditorPaneDescriptor._onWillInstantiateEditorPane`.
    static ON_WILL_INSTANTIATE_EDITOR_PANE: Rc<Emitter<WillInstantiateEditorPaneEvent>> = Emitter::new();
}

impl EditorPaneDescriptor {
    /// `EditorPaneDescriptor.create(ctor, id, name)`.
    pub fn new(id: &str, name: &str) -> Rc<Self> {
        Rc::new(Self { id: id.to_string(), name: name.to_string(), with_selection: false })
    }

    /// Como [`EditorPaneDescriptor::new`], para um painel com seleção (`IEditorPaneWithSelection`, como o `TextFileEditor`): o
    /// painel lógico guarda a seleção nas opções com que a entrada foi atribuída e avisa as mudanças dela
    /// ([`EditorPane::set_selection`], o `setSelection` do `TestTextFileEditor`).
    pub fn new_with_selection(id: &str, name: &str) -> Rc<Self> {
        Rc::new(Self { id: id.to_string(), name: name.to_string(), with_selection: true })
    }

    /// `EditorPaneDescriptor.didInstantiateEditorPane(typeId)`.
    pub fn did_instantiate_editor_pane(type_id: &str) -> bool {
        INSTANTIATED_EDITOR_PANES.with(|instantiated| instantiated.borrow().contains(type_id))
    }

    /// `EditorPaneDescriptor.onWillInstantiateEditorPane`.
    pub fn on_will_instantiate_editor_pane() -> Event<WillInstantiateEditorPaneEvent> {
        ON_WILL_INSTANTIATE_EDITOR_PANE.with(|emitter| emitter.event())
    }

    /// `instantiate(instantiationService, group)`: avisa que o painel vai nascer e o marca como instanciado depois de criado.
    fn instantiate(&self, group_id: GroupIdentifier) -> Rc<EditorPane> {
        ON_WILL_INSTANTIATE_EDITOR_PANE.with(|emitter| emitter.fire(WillInstantiateEditorPaneEvent { editor_type_id: self.id.clone() }));

        let pane = EditorPane::new(&self.id, group_id, self.with_selection);
        INSTANTIATED_EDITOR_PANES.with(|instantiated| instantiated.borrow_mut().insert(self.id.clone()));

        pane
    }

    /// `describes(editorPane)`: `editorPane.getId() === this.typeId`.
    pub fn describes(&self, editor_pane: &EditorPane) -> bool {
        editor_pane.id == self.id
    }
}

impl IEditorDescriptor for EditorPaneDescriptor {
    fn editor_type_id(&self) -> String {
        self.id.clone()
    }

    fn name(&self) -> String {
        self.name.clone()
    }
}

/// `EditorPaneRegistry`: os painéis registrados e as classes de entrada de cada um.
pub struct EditorPaneRegistry {
    this: Weak<EditorPaneRegistry>,
    editor_panes: RefCell<Vec<(Rc<EditorPaneDescriptor>, Vec<TypeId>)>>,
}

impl EditorPaneRegistry {
    pub fn new() -> Rc<Self> {
        Rc::new_cyclic(|this| Self { this: this.clone(), editor_panes: RefCell::new(Vec::new()) })
    }

    /// `registerEditorPane(editorPaneDescriptor, inputDescriptors)`: as classes de entrada são os [`TypeId`]s dos tipos
    /// concretos. O disposable remove o registro.
    pub fn register_editor_pane(&self, editor_pane_descriptor: Rc<EditorPaneDescriptor>, input_types: Vec<TypeId>) -> DisposableRef {
        self.editor_panes.borrow_mut().push((editor_pane_descriptor.clone(), input_types));

        let registry = self.this.clone();
        to_disposable(move || {
            if let Some(registry) = registry.upgrade() {
                registry.editor_panes.borrow_mut().retain(|(descriptor, _)| !Rc::ptr_eq(descriptor, &editor_pane_descriptor));
            }
        })
    }

    /// `getEditorPane(editor)`.
    pub fn get_editor_pane(&self, editor: &EditorInputRef) -> Option<Rc<EditorPaneDescriptor>> {
        let descriptors = self.find_editor_pane_descriptors(editor);
        match descriptors.len() {
            0 => None,
            1 => descriptors.into_iter().next(),
            _ => {
                let candidates: Vec<Rc<dyn IEditorDescriptor>> = descriptors.iter().map(|descriptor| descriptor.clone() as Rc<dyn IEditorDescriptor>).collect();
                let preferred = editor.prefers_editor_pane(&candidates)?;

                descriptors.into_iter().find(|descriptor| descriptor.editor_type_id() == preferred.editor_type_id())
            }
        }
    }

    /// `findEditorPaneDescriptors(editor)`: direct check on the type of the input.
    fn find_editor_pane_descriptors(&self, editor: &EditorInputRef) -> Vec<Rc<EditorPaneDescriptor>> {
        let input_type = editor.as_any().type_id();

        self.editor_panes.borrow().iter().filter(|(_, input_types)| input_types.contains(&input_type)).map(|(descriptor, _)| descriptor.clone()).collect()
    }
}

/// `Registry.as<IEditorPaneRegistry>(EditorExtensions.EditorPane)`: o registro de painéis de editor da thread.
pub fn editor_pane_registry() -> Rc<EditorPaneRegistry> {
    let registry = registry();
    if !registry.knows(EditorExtensions::EDITOR_PANE) {
        registry.add(EditorExtensions::EDITOR_PANE, EditorPaneRegistry::new());
    }

    registry.as_::<EditorPaneRegistry>(EditorExtensions::EDITOR_PANE).expect("o registro de painéis de editor foi adicionado acima")
}

/// O painel de editor lógico (o `EditorPane` do original): o editor visível de um grupo.
pub struct EditorPane {
    id: String,
    group_id: GroupIdentifier,
    input: RefCell<Option<EditorInputRef>>,
    /// `options`: as opções com que a entrada foi atribuída, como `ITextEditorOptions` (a seleção do editor de texto fica
    /// nelas, de onde o painel com seleção a lê).
    options: RefCell<Option<TextEditorOptions>>,
    on_did_change_control: Rc<Emitter<()>>,
    /// `onDidChangeSelection`: só existe nos painéis criados por [`EditorPaneDescriptor::new_with_selection`].
    on_did_change_selection: Option<Rc<Emitter<EditorPaneSelectionChangeEvent>>>,
    /// A assinatura do conteúdo do modelo de texto (só no painel com seleção).
    model_listener: RefCell<Option<DisposableRef>>,
}

impl EditorPane {
    fn new(id: &str, group_id: GroupIdentifier, with_selection: bool) -> Rc<Self> {
        Rc::new(Self {
            id: id.to_string(),
            group_id,
            input: RefCell::new(None),
            options: RefCell::new(None),
            on_did_change_control: Emitter::new(),
            on_did_change_selection: with_selection.then(Emitter::new),
            model_listener: RefCell::new(None),
        })
    }

    /// `options`: as opções com que a entrada foi atribuída.
    pub fn options(&self) -> Option<EditorOptions> {
        self.options.borrow().as_ref().map(|options| options.base.clone())
    }

    /// `options` como `ITextEditorOptions`.
    pub fn text_options(&self) -> Option<TextEditorOptions> {
        self.options.borrow().clone()
    }

    /// `setSelection(selection, reason)` do `TestTextFileEditor`: as opções passam a ser só a seleção (ou a nenhuma) e a
    /// mudança é avisada com o motivo. Só vale para o painel com seleção ([`EditorPaneDescriptor::new_with_selection`]).
    pub fn set_selection(&self, selection: Option<Selection>, reason: EditorPaneSelectionChangeReason) {
        let emitter = self.on_did_change_selection.as_ref().expect("o painel não tem seleção");

        *self.options.borrow_mut() = selection.map(|selection| TextEditorOptions {
            selection: Some(TextEditorSelection {
                start_line_number: f64::from(selection.start_line_number),
                start_column: f64::from(selection.start_column),
                end_line_number: Some(f64::from(selection.end_line_number)),
                end_column: Some(f64::from(selection.end_column)),
            }),
            ..TextEditorOptions::default()
        });

        emitter.fire(EditorPaneSelectionChangeEvent { reason });
    }

    /// `setInput(input, options, context, token)`: a entrada vale já; a promessa assenta quando o modelo foi resolvido. O painel
    /// com seleção passa a avisar `EDIT` a cada mudança de conteúdo do modelo de texto (o `onDidChangeModelContent` do controle
    /// de `AbstractTextEditor`).
    fn set_input(self: &Rc<Self>, editor: &EditorInputRef, options: Option<TextEditorOptions>) -> Promise<()> {
        self.unbind_model();

        *self.input.borrow_mut() = Some(editor.clone());
        *self.options.borrow_mut() = options;

        if self.id == ERROR_EDITOR_ID {
            return Promise::resolved(());
        }

        let weak = Rc::downgrade(self);
        editor.resolve().then(move |model| {
            if let Some(pane) = weak.upgrade() {
                pane.bind_model(model);
            }
        })
    }

    /// Liga o aviso de `EDIT` ao conteúdo do modelo de texto resolvido (só no painel com seleção).
    fn bind_model(self: &Rc<Self>, model: Option<Rc<dyn IEditorModel>>) {
        if self.on_did_change_selection.is_none() {
            return;
        }

        let Some(text_model) = model.as_ref().and_then(|model| model.as_any().downcast_ref::<TextFileEditorModel>()).and_then(|model| ITextFileEditorModel::text_editor_model(model)) else {
            return;
        };

        let weak = Rc::downgrade(self);
        let listener = text_model.on_did_change_content().listen(move |_| {
            if let Some(pane) = weak.upgrade()
                && let Some(emitter) = &pane.on_did_change_selection
            {
                emitter.fire(EditorPaneSelectionChangeEvent { reason: EditorPaneSelectionChangeReason::Edit });
            }
        });
        *self.model_listener.borrow_mut() = Some(listener);
    }

    fn unbind_model(&self) {
        if let Some(listener) = self.model_listener.borrow_mut().take() {
            listener.dispose();
        }
    }

    /// `setOptions(options)`.
    fn set_options(&self, options: Option<TextEditorOptions>) {
        *self.options.borrow_mut() = options;
    }

    /// `clearInput()`.
    fn clear_input(&self) {
        self.unbind_model();

        *self.input.borrow_mut() = None;
        *self.options.borrow_mut() = None;
    }

    fn key(self: &Rc<Self>) -> DisposableId {
        id_of(self)
    }
}

impl IEditorPane for EditorPane {
    fn input(&self) -> Option<EditorInputRef> {
        self.input.borrow().clone()
    }

    fn group_id(&self) -> GroupIdentifier {
        self.group_id
    }

    fn get_view_state(&self) -> Option<Dynamic> {
        None
    }

    fn on_did_change_control(&self) -> Event<()> {
        self.on_did_change_control.event()
    }

    fn get_id(&self) -> String {
        self.id.clone()
    }

    fn options(&self) -> Option<EditorOptions> {
        EditorPane::options(self)
    }

    fn on_did_change_selection(&self) -> Option<Event<EditorPaneSelectionChangeEvent>> {
        self.on_did_change_selection.as_ref().map(|emitter| emitter.event())
    }

    /// `getSelection()` do `TestTextFileEditor`: a seleção das opções atribuídas, como `TextEditorPaneSelection`.
    fn get_selection(&self) -> Option<Rc<dyn IEditorPaneSelection>> {
        self.on_did_change_selection.as_ref()?;

        let text_selection = self.options.borrow().as_ref()?.selection?;
        let end_line_number = text_selection.end_line_number.unwrap_or(text_selection.start_line_number);
        let end_column = text_selection.end_column.unwrap_or(text_selection.start_column);

        Some(Rc::new(TextEditorPaneSelection::new(Selection::new(
            text_selection.start_line_number as i32,
            text_selection.start_column as i32,
            end_line_number as i32,
            end_column as i32,
        ))))
    }

    fn text_options(&self) -> Option<TextEditorOptions> {
        EditorPane::text_options(self)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl IDisposable for EditorPane {
    fn dispose(&self) {
        self.unbind_model();
        self.on_did_change_control.dispose();
        if let Some(emitter) = &self.on_did_change_selection {
            emitter.dispose();
        }
    }
}

/// `IOpenEditorResult`.
#[derive(Clone)]
pub struct OpenEditorResult {
    pub pane: Option<Rc<EditorPane>>,
    /// Whether the editor pane changed its input (`false` quando o painel já mostrava a mesma entrada).
    pub changed: bool,
    /// Whether the operation was cancelled by another operation opened right after.
    pub cancelled: bool,
    /// The error the editor failed to open with, if any.
    pub error: Option<Error>,
}

/// O resultado de `doSetInput`.
#[derive(Clone)]
struct SetInputResult {
    changed: bool,
    cancelled: bool,
}

/// `EditorPanes`.
pub struct EditorPanes {
    this: Weak<EditorPanes>,
    group_id: GroupIdentifier,
    instantiation_service: Rc<InstantiationService>,
    registry: Rc<EditorPaneRegistry>,
    active_editor_pane: RefCell<Option<Rc<EditorPane>>>,
    editor_panes: RefCell<Vec<Rc<EditorPane>>>,
    map_editor_pane_to_pending_set_input: RefCell<HashMap<DisposableId, Promise<()>>>,
    /// `editorOperation.currentOperationId`.
    current_operation_id: Cell<u64>,
    disposables: Rc<DisposableStore>,
}

impl EditorPanes {
    pub fn new(group_id: GroupIdentifier, instantiation_service: Rc<InstantiationService>) -> Rc<Self> {
        Rc::new_cyclic(|this| Self {
            this: this.clone(),
            group_id,
            instantiation_service,
            registry: editor_pane_registry(),
            active_editor_pane: RefCell::new(None),
            editor_panes: RefCell::new(Vec::new()),
            map_editor_pane_to_pending_set_input: RefCell::new(HashMap::new()),
            current_operation_id: Cell::new(0),
            disposables: DisposableStore::new(),
        })
    }

    /// `activeEditorPane`.
    pub fn active_editor_pane(&self) -> Option<Rc<EditorPane>> {
        self.active_editor_pane.borrow().clone()
    }

    fn rc(&self) -> Rc<Self> {
        self.this.upgrade().expect("os painéis estão vivos enquanto o grupo os usa")
    }

    /// `openEditor(editor, options, internalOptions, context)`.
    pub fn open_editor(&self, editor: &EditorInputRef, options: Option<EditorOptions>, context: EditorOpenContext) -> Promise<OpenEditorResult> {
        self.open_editor_with_text_options(editor, options.map(TextEditorOptions::from), context)
    }

    /// `openEditor(editor, options, internalOptions, context)` com as opções como `ITextEditorOptions` (a seleção do editor de
    /// texto chega ao painel).
    pub fn open_editor_with_text_options(&self, editor: &EditorInputRef, options: Option<TextEditorOptions>, context: EditorOpenContext) -> Promise<OpenEditorResult> {
        let (this, editor) = (self.rc(), editor.clone());

        async_fn(async move {
            let outcome = match this.get_editor_pane_descriptor(&editor) {
                Ok(descriptor) => this.do_open_editor(descriptor, &editor, options.clone(), context).await,
                Err(error) => Err(error),
            };

            match outcome {
                Ok(result) => Ok(result),
                Err(error) => {
                    // First check if caller instructed us to ignore error handling
                    if options.as_ref().is_some_and(|options| options.ignore_error == Some(true)) {
                        return Ok(OpenEditorResult { pane: None, changed: false, cancelled: false, error: Some(error) });
                    }

                    // In case of an error when opening an editor, we still want to show an editor in the desired location to
                    // preserve the user intent and view state (e.g. when restoring).
                    this.do_show_error(error, &editor, options, context).await
                }
            }
        })
    }

    fn do_show_error(&self, error: Error, editor: &EditorInputRef, options: Option<TextEditorOptions>, context: EditorOpenContext) -> Promise<OpenEditorResult> {
        let (this, editor) = (self.rc(), editor.clone());

        async_fn(async move {
            // Always log the error to figure out what is going on
            if let Some(log_service) = this.instantiation_service.invoke_function(|accessor| accessor.try_get(I_LOG_SERVICE)) {
                log_service.error(&error.to_string());
            }

            let result = this.do_open_editor(EditorPaneDescriptor::new(ERROR_EDITOR_ID, "Error Editor"), &editor, options, context).await?;

            Ok(OpenEditorResult { error: Some(error), ..result })
        })
    }

    fn do_open_editor(&self, descriptor: Rc<EditorPaneDescriptor>, editor: &EditorInputRef, options: Option<TextEditorOptions>, context: EditorOpenContext) -> Promise<OpenEditorResult> {
        let (this, editor) = (self.rc(), editor.clone());

        async_fn(async move {
            // Editor pane
            let pane = this.do_show_editor_pane(&descriptor);

            // Apply input to pane
            let SetInputResult { changed, cancelled } = this.do_set_input(&pane, &editor, options, context).await?;

            Ok(OpenEditorResult { pane: Some(pane), changed, cancelled, error: None })
        })
    }

    /// `getEditorPaneDescriptor(editor)`: `assertReturnsDefined(this.editorPanesRegistry.getEditorPane(editor))`.
    fn get_editor_pane_descriptor(&self, editor: &EditorInputRef) -> Result<Rc<EditorPaneDescriptor>, Error> {
        self.registry.get_editor_pane(editor).ok_or_else(|| Error::new("Assertion Failed: argument is undefined or null"))
    }

    /// `doShowEditorPane(descriptor)`.
    fn do_show_editor_pane(&self, descriptor: &EditorPaneDescriptor) -> Rc<EditorPane> {
        // Return early if the currently active editor pane can handle the input
        if let Some(active) = self.active_editor_pane()
            && descriptor.describes(&active)
        {
            return active;
        }

        // Hide active one first
        self.do_hide_active_editor_pane();

        // Create editor pane (return early if already instantiated)
        let existing = self.editor_panes.borrow().iter().find(|editor_pane| descriptor.describes(editor_pane)).cloned();
        let editor_pane = existing.unwrap_or_else(|| {
            let editor_pane = self.disposables.add_rc(descriptor.instantiate(self.group_id));
            self.editor_panes.borrow_mut().push(editor_pane.clone());

            editor_pane
        });

        // Set editor as active
        *self.active_editor_pane.borrow_mut() = Some(editor_pane.clone());

        editor_pane
    }

    /// `doSetInput(editorPane, editor, options, context)`.
    fn do_set_input(&self, editor_pane: &Rc<EditorPane>, editor: &EditorInputRef, options: Option<TextEditorOptions>, _context: EditorOpenContext) -> Promise<SetInputResult> {
        let (this, editor_pane, editor) = (self.rc(), editor_pane.clone(), editor.clone());

        async_fn(async move {
            let candidate = EditorInputOrUntyped::Typed(editor.clone());
            let pane_input_matches = |editor_pane: &EditorPane| editor_pane.input().is_some_and(|input| input.matches(&candidate));

            // If the input did not change, return early and only apply the options unless the options instruct us to force
            // open it even if it is the same
            let mut input_matches = pane_input_matches(&editor_pane);
            if input_matches && options.as_ref().is_none_or(|options| options.force_reload != Some(true)) {
                // We have to await a pending `setInput()` call for this pane before we can call into `setOptions()`,
                // otherwise we risk calling when the input is not yet fully applied.
                let pending = this.map_editor_pane_to_pending_set_input.borrow().get(&editor_pane.key()).cloned();
                if let Some(pending) = pending {
                    pending.await?;
                }

                // At this point, the input might have changed, so we check again
                input_matches = pane_input_matches(&editor_pane);
                if input_matches {
                    editor_pane.set_options(options);
                }

                return Ok(SetInputResult { changed: false, cancelled: !input_matches });
            }

            // Start a new editor input operation to support cancellation. Any new operation that is started will cancel the
            // previous one.
            let operation = this.current_operation_id.get() + 1;
            this.current_operation_id.set(operation);
            let is_current = || this.current_operation_id.get() == operation;

            let mut cancelled = false;
            let mut failure: Option<Error> = None;

            // Clear the current input before setting new input. This ensures that a slow loading input will not be visible
            // for the duration of the new input to load (https://github.com/microsoft/vscode/issues/34697)
            editor_pane.clear_input();

            // Set the input to the editor pane and keep track of it
            let pending_set_input = editor_pane.set_input(&editor, options);
            this.map_editor_pane_to_pending_set_input.borrow_mut().insert(editor_pane.key(), pending_set_input.clone());
            match pending_set_input.await {
                Ok(()) => {
                    if !is_current() {
                        cancelled = true;
                    }
                }
                Err(error) => {
                    if !is_current() {
                        cancelled = true;
                    } else {
                        failure = Some(error);
                    }
                }
            }

            if is_current() {
                this.map_editor_pane_to_pending_set_input.borrow_mut().remove(&editor_pane.key());
            }

            match failure {
                Some(error) => Err(error),
                None => Ok(SetInputResult { changed: !input_matches, cancelled }),
            }
        })
    }

    /// `doHideActiveEditorPane()`.
    fn do_hide_active_editor_pane(&self) {
        let Some(active) = self.active_editor_pane() else {
            return;
        };

        // Indicate to editor pane before removing the editor to give a chance to persist certain state
        active.clear_input();

        // Clear any pending setInput promise
        self.map_editor_pane_to_pending_set_input.borrow_mut().remove(&active.key());

        // Clear active editor pane
        *self.active_editor_pane.borrow_mut() = None;
    }

    /// `closeEditor(editor)`.
    pub fn close_editor(&self, editor: &EditorInputRef) {
        let matches_active = self
            .active_editor_pane()
            .and_then(|active| active.input())
            .is_some_and(|input| editor.matches(&EditorInputOrUntyped::Typed(input)));

        if matches_active {
            self.do_hide_active_editor_pane();
        }
    }
}

impl IDisposable for EditorPanes {
    fn dispose(&self) {
        self.disposables.dispose();
    }
}
