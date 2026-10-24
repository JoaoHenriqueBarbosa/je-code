//! Porte de `src/vs/workbench/contrib/files/browser/editors/textFileEditorTracker.ts`: o [`TextFileEditorTracker`], que garante
//! que todo arquivo de texto e todo arquivo sem título sujo esteja aberto num editor (e que o salvamento automático depois de
//! um atraso curto tenha deixado de cuidar dele), e que recarrega os arquivos visíveis e limpos quando a janela ganha o foco.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`], na ordem do original. O
//!   argumento estático é o [`TextFileEditorTrackerHooks`] opcional (a subclasse de teste do original sobrescreve
//!   `getDirtyTextFileTrackerDelay`; sem hooks vale o [`DefaultTextFileEditorTrackerHooks`], 800 ms). A classe herda de
//!   `Disposable`, aqui um `Disposable` guardado no campo `base`. `IWorkbenchContribution` ainda não foi portado: a classe
//!   traz só o `ID`.
//! - **`ICodeEditorService`.** O `listCodeEditors()` que o original lê é o do [`CodeEditorRegistry`] (o `ICodeEditorService` do
//!   porte em `opener_service.rs` é o recorte do `EditorOpener` e não o expõe), o mesmo que `MainThreadDocumentsAndEditors`
//!   recebe.
//! - **`RunOnceWorker`.** O trabalhador é criado dentro do `Rc::new_cyclic`, com o `Weak` da classe no executor (a
//!   propriedade inicializada com `this` do original).
//! - **`distinct`.** É `je_base::arrays::distinct` com a chave `resource.toString()`.
//! - **`openEditors`.** O `editorService.openEditors(resources.map(...))` com entradas sem tipo é `open_editors_in` com a forma
//!   [`FindGroupEditor::Untyped`], sem grupo preferido e sem opções; a promessa é solta sem aguardar, como o original.
//! - **`findEditor(model)`.** O modelo (de arquivo ou sem título) é visto como `IWorkingCopy`.

use std::rc::{Rc, Weak};

use je_base::arrays::distinct;
use je_base::async_::RunOnceWorker;
use je_base::event::Event;
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::schemas;
use je_base::uri::URI;
use je_editor::code_editor_widget::ICodeEditor;
use je_editor::services::abstract_code_editor_service::CodeEditorRegistry;
use je_instantiation::Injectable;
use je_platform::opener::editor_types::EditorOptions;

use crate::common::editor::editor_model::IEditorModel;
use crate::common::editor::{DEFAULT_EDITOR_ASSOCIATION_ID, TextEditorOptions, UntypedEditorInput};
use crate::contrib::files::common_files::FILE_EDITOR_INPUT_ID;
use crate::services::editor::editor_group_finder::FindGroupEditor;
use crate::services::editor::editor_service::{IEditorService, ResourceEditorInputIdentifier};
use crate::services::files_configuration::files_configuration_service::{IFilesConfigurationService, ResourceOrEditor};
use crate::services::lifecycle::lifecycle::ILifecycleService;
use crate::services::textfile::textfiles::{
    ITextFileEditorModel, ITextFileService, TextFileEditorModelResolveOrCreateOptions, TextFileEditorModelState, TextFileReloadOptions,
};
use crate::services::untitled::untitled_text_editor_input::UntitledTextEditorInput;
use crate::services::host::host::IHostService;
use crate::services::working_copy::working_copy::IWorkingCopy;
use crate::services::working_copy::working_copy_editor_service::IWorkingCopyEditorService;

/// Os membros que a subclasse de `TextFileEditorTracker` sobrescreve.
pub trait TextFileEditorTrackerHooks {
    /// `protected getDirtyTextFileTrackerDelay()`: o padrão é o corpo da classe base (encapsulado num método para os testes
    /// o sobrescreverem).
    fn get_dirty_text_file_tracker_delay(&self) -> u64 {
        800
    }
}

/// Os hooks sem sobrescrita: o corpo da classe base.
pub struct DefaultTextFileEditorTrackerHooks;

impl TextFileEditorTrackerHooks for DefaultTextFileEditorTrackerHooks {}

/// `resource.toString()`.
fn uri_key(resource: &URI) -> String {
    resource.to_string(false).to_string_lossy()
}

/// `TextFileEditorTracker`.
pub struct TextFileEditorTracker {
    this: Weak<TextFileEditorTracker>,
    base: Rc<Disposable>,
    ensure_dirty_files_are_opened_worker: Rc<RunOnceWorker<URI>>,

    editor_service: Rc<dyn IEditorService>,
    text_file_service: Rc<dyn ITextFileService>,
    lifecycle_service: Rc<dyn ILifecycleService>,
    host_service: Rc<dyn IHostService>,
    code_editor_service: Rc<CodeEditorRegistry>,
    files_configuration_service: Rc<dyn IFilesConfigurationService>,
    working_copy_editor_service: Rc<dyn IWorkingCopyEditorService>,
}

impl Injectable for TextFileEditorTracker {
    type Args = Option<Rc<dyn TextFileEditorTrackerHooks>>;
    type Deps = (
        Rc<dyn IEditorService>,
        Rc<dyn ITextFileService>,
        Rc<dyn ILifecycleService>,
        Rc<dyn IHostService>,
        Rc<CodeEditorRegistry>,
        Rc<dyn IFilesConfigurationService>,
        Rc<dyn IWorkingCopyEditorService>,
    );

    fn create(
        hooks: Self::Args,
        (editor_service, text_file_service, lifecycle_service, host_service, code_editor_service, files_configuration_service, working_copy_editor_service): Self::Deps,
    ) -> Rc<Self> {
        let hooks: Rc<dyn TextFileEditorTrackerHooks> = hooks.unwrap_or_else(|| Rc::new(DefaultTextFileEditorTrackerHooks));
        let base = Disposable::new();

        let this = Rc::new_cyclic(|this: &Weak<TextFileEditorTracker>| {
            let weak = this.clone();
            let ensure_dirty_files_are_opened_worker = RunOnceWorker::new(
                move |units: Vec<URI>| {
                    if let Some(this) = weak.upgrade() {
                        this.ensure_dirty_text_files_are_opened(units);
                    }
                },
                hooks.get_dirty_text_file_tracker_delay(),
            );
            base.register(ensure_dirty_files_are_opened_worker.clone());

            Self {
                this: this.clone(),
                base,
                ensure_dirty_files_are_opened_worker,
                editor_service,
                text_file_service,
                lifecycle_service,
                host_service,
                code_editor_service,
                files_configuration_service,
                working_copy_editor_service,
            }
        });

        this.register_listeners();

        this
    }
}

impl IDisposable for TextFileEditorTracker {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl TextFileEditorTracker {
    /// `TextFileEditorTracker.ID`.
    pub const ID: &'static str = "workbench.contrib.textFileEditorTracker";

    /// `this._register(event(listener))`, com o `this` seguro por `Weak` para o ouvinte não manter a classe viva.
    fn listen<T: 'static>(&self, event: Event<T>, handler: impl Fn(&TextFileEditorTracker, &T) + 'static) {
        let weak = self.this.clone();
        self.base.register(event.listen(move |value| {
            if let Some(this) = weak.upgrade() {
                handler(&this, value);
            }
        }));
    }

    fn register_listeners(&self) {
        // Ensure dirty text file and untitled models are always opened as editors
        self.listen(self.text_file_service.files().on_did_change_dirty(), |this, model| this.ensure_dirty_files_are_opened_worker.work(IWorkingCopy::resource(&**model)));
        self.listen(self.text_file_service.files().on_did_save_error(), |this, model| this.ensure_dirty_files_are_opened_worker.work(IWorkingCopy::resource(&**model)));
        self.listen(self.text_file_service.untitled().on_did_change_dirty(), |this, model| this.ensure_dirty_files_are_opened_worker.work(IWorkingCopy::resource(&**model)));

        // Update visible text file editors when focus is gained
        self.listen(self.host_service.on_did_change_focus(), |this, has_focus| {
            if *has_focus {
                this.reload_visible_text_file_editors();
            }
        });

        // Lifecycle
        self.listen(self.lifecycle_service.on_did_shutdown(), |this, _| this.dispose());
    }

    //#region Text File: Ensure every dirty text and untitled file is opened in an editor

    fn ensure_dirty_text_files_are_opened(&self, resources: Vec<URI>) {
        let to_open: Vec<URI> = resources.into_iter().filter(|resource| self.is_dirty_text_file_to_open(resource)).collect();

        self.do_ensure_dirty_text_files_are_opened(distinct(&to_open, uri_key));
    }

    /// O predicado do `filter` de `ensureDirtyTextFilesAreOpened`.
    fn is_dirty_text_file_to_open(&self, resource: &URI) -> bool {
        if !self.text_file_service.is_dirty(resource) {
            return false; // resource must be dirty
        }

        let file_model = self.text_file_service.files().get(resource);
        if file_model.as_ref().is_some_and(|model| model.has_state(TextFileEditorModelState::PendingSave)) {
            return false; // resource must not be pending to save
        }

        let is_untitled = *resource.scheme() == schemas::UNTITLED;
        if !is_untitled
            && !file_model.as_ref().is_some_and(|model| model.has_state(TextFileEditorModelState::Error))
            && self.files_configuration_service.has_short_auto_save_delay(Some(&ResourceOrEditor::Resource(resource.clone())))
        {
            // leave models auto saved after short delay unless
            // the save resulted in an error and not for untitled
            // that are not auto-saved anyway
            return false;
        }

        let type_id = if is_untitled { UntitledTextEditorInput::ID } else { FILE_EDITOR_INPUT_ID };
        if self.editor_service.is_opened(&ResourceEditorInputIdentifier {
            type_id: type_id.to_string(),
            editor_id: Some(DEFAULT_EDITOR_ASSOCIATION_ID.to_string()),
            resource: resource.clone(),
        }) {
            return false; // model must not be opened already as file (fast check via editor type)
        }

        let model: Option<Rc<dyn IWorkingCopy>> = file_model
            .map(|model| -> Rc<dyn IWorkingCopy> { model })
            .or_else(|| self.text_file_service.untitled().get(resource).map(|model| -> Rc<dyn IWorkingCopy> { model }));
        if let Some(model) = model
            && self.working_copy_editor_service.find_editor(&*model).is_some()
        {
            return false; // model must not be opened already as file (slower check via working copy)
        }

        true
    }

    fn do_ensure_dirty_text_files_are_opened(&self, resources: Vec<URI>) {
        if resources.is_empty() {
            return;
        }

        let options = TextEditorOptions::from(EditorOptions {
            inactive: Some(true),
            pinned: Some(true),
            preserve_focus: Some(true),
            ..EditorOptions::default()
        });
        let editors = resources
            .into_iter()
            .map(|resource| FindGroupEditor::Untyped(UntypedEditorInput { resource: Some(resource), options: Some(options.clone()), ..UntypedEditorInput::default() }))
            .collect();

        drop(self.editor_service.open_editors_in(editors, None, None));
    }

    //#endregion

    //#region Window Focus Change: Update visible code editors when focus is gained that have a known text file model

    fn reload_visible_text_file_editors(&self) {
        // the window got focus and we use this as a hint that files might have been changed outside
        // of this window. since file events can be unreliable, we queue a load for models that
        // are visible in any editor. since this is a fast operation in the case nothing has changed,
        // we tolerate the additional work.
        let files = self.text_file_service.files();
        let models: Vec<Rc<dyn ITextFileEditorModel>> = self
            .code_editor_service
            .list_code_editors()
            .iter()
            .filter_map(|code_editor| {
                let resource = ICodeEditor::get_model(&**code_editor)?.uri();

                let model = files.get(&resource)?;
                if IWorkingCopy::is_dirty(&*model) || !IEditorModel::is_resolved(&*model) {
                    return None;
                }

                Some(model)
            })
            .collect();

        for model in distinct(&models, |model| uri_key(&IWorkingCopy::resource(&**model))) {
            drop(files.resolve(
                &IWorkingCopy::resource(&*model),
                Some(TextFileEditorModelResolveOrCreateOptions { reload: Some(TextFileReloadOptions { async_: true }), ..TextFileEditorModelResolveOrCreateOptions::default() }),
            ));
        }
    }

    //#endregion
}
