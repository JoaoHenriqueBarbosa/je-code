//! Porte de `src/vs/workbench/contrib/customEditor/common/customTextEditorModel.ts`: o `CustomTextEditorModel`, o modelo de
//! um editor personalizado de texto, que se apoia no modelo de texto e no modelo de arquivo de texto do recurso.
//!
//! Divergências:
//! - **Injeção.** `@ITextFileService`, `@ILabelService` e `@IExtensionService` são [`CustomTextEditorModelServices`], e
//!   `create(instantiationService, viewType, resource)` recebe também o `ITextModelService` (o `invokeFunction` do
//!   original o lê do acessor).
//! - **Herança.** `extends Disposable` é um [`Disposable`] guardado em `base`; o modelo é um `Rc` (nasce em `Rc::new_cyclic`).
//!   `implements ICustomEditorModel` é o [`ICustomEditorModel`], e o `instanceof CustomTextEditorModel` é o `downcast_ref` de
//!   [`ICustomEditorModel::as_any`].
//! - **`textEditorModel`** é o modelo de texto do modelo resolvido (que o contrato do original garante definido).
//! - `onWillStop` veta a parada com a promessa já resolvida em `true` e a razão (`localize` é o texto padrão).
//! - `isEqual(this.resource, e.resource)` é o `ExtUri::is_equal`, e `textFileService.save/saveAs` recebem as opções do
//!   editor como as do serviço de arquivos de texto (`ITextFileSaveOptions` estende `ISaveOptions`).

use std::any::Any;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::nls::localize;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::text_model::TextModel;
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::resources::ext_uri;

use super::custom_editor::ICustomEditorModel;
use crate::common::editor::{RevertOptions, SaveOptions};
use crate::services::extensions::extensions::IExtensionService;
use crate::services::files_configuration::files_configuration_service::Readonly;
use crate::services::textfile::textfiles::{ITextFileEditorModel, ITextFileService, TextFileEditorModelState, TextFileSaveAsOptions, TextFileSaveOptions};
use crate::services::textmodel_resolver::resolver_service::{ITextModelService, TextModelReference};
use crate::services::working_copy::working_copy::IWorkingCopy;

/// Os serviços que o construtor do `CustomTextEditorModel` injeta.
#[derive(Clone)]
pub struct CustomTextEditorModelServices {
    pub text_file_service: Rc<dyn ITextFileService>,
    pub label_service: Rc<dyn ILabelService>,
    pub extension_service: Rc<dyn IExtensionService>,
}

/// `CustomTextEditorModel`.
pub struct CustomTextEditorModel {
    base: Rc<Disposable>,

    view_type: String,
    resource: URI,
    model: TextModelReference,

    text_file_model: Option<Rc<dyn ITextFileEditorModel>>,

    on_did_change_orphaned: Rc<Emitter<()>>,
    on_did_change_readonly: Rc<Emitter<()>>,
    on_did_change_dirty: Rc<Emitter<()>>,
    on_did_change_content: Rc<Emitter<()>>,

    text_file_service: Rc<dyn ITextFileService>,
    label_service: Rc<dyn ILabelService>,
}

impl CustomTextEditorModel {
    /// `create(instantiationService, viewType, resource)`.
    pub fn create(
        text_model_resolver_service: Rc<dyn ITextModelService>,
        services: CustomTextEditorModelServices,
        view_type: &str,
        resource: &URI,
    ) -> Promise<Rc<CustomTextEditorModel>> {
        let view_type = view_type.to_owned();
        let resource = resource.clone();
        async_fn(async move {
            let model = text_model_resolver_service.create_model_reference(&resource).await?;
            Ok(Self::new(&view_type, resource, model, services))
        })
    }

    /// `constructor(viewType, resource, model, @ITextFileService, @ILabelService, @IExtensionService)`.
    pub fn new(view_type: &str, resource: URI, model: TextModelReference, services: CustomTextEditorModelServices) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_orphaned = Emitter::new();
        let on_did_change_readonly = Emitter::new();
        let on_did_change_dirty = Emitter::new();
        let on_did_change_content = Emitter::new();
        base.register(on_did_change_orphaned.clone());
        base.register(on_did_change_readonly.clone());
        base.register(on_did_change_dirty.clone());
        base.register(on_did_change_content.clone());

        let CustomTextEditorModelServices { text_file_service, label_service, extension_service } = services;

        base.register(Rc::new(model.clone()));

        let text_file_model = text_file_service.files().get(&resource);
        if let Some(text_file_model) = &text_file_model {
            let emitter = on_did_change_orphaned.clone();
            base.register(text_file_model.on_did_change_orphaned().listen(move |_| emitter.fire(())));
            let emitter = on_did_change_readonly.clone();
            base.register(text_file_model.on_did_change_readonly().listen(move |_| emitter.fire(())));
        }

        {
            let (resource, on_did_change_dirty, on_did_change_content) = (resource.clone(), on_did_change_dirty.clone(), on_did_change_content.clone());
            base.register(text_file_service.files().on_did_change_dirty().listen(move |e| {
                if ext_uri().is_equal(Some(&resource), Some(&IWorkingCopy::resource(e.as_ref())), false) {
                    on_did_change_dirty.fire(());
                    on_did_change_content.fire(());
                }
            }));
        }

        let this = Rc::new(Self {
            base,
            view_type: view_type.to_owned(),
            resource,
            model,
            text_file_model,
            on_did_change_orphaned,
            on_did_change_readonly,
            on_did_change_dirty,
            on_did_change_content,
            text_file_service,
            label_service,
        });

        let name = Rc::downgrade(&this);
        this.base.register(extension_service.on_will_stop().listen(move |e| {
            let name = name.upgrade().map(|model| model.name()).unwrap_or_default();
            e.veto(
                Promise::resolved(true),
                &localize("vetoExtHostRestart", "An extension provided text editor for '{0}' is still open that would close otherwise.", &[name.as_str()]),
            );
        }));

        this
    }

    /// `resource`.
    pub fn resource(&self) -> &URI {
        &self.resource
    }

    /// `textEditorModel`.
    pub fn text_editor_model(&self) -> Rc<TextModel> {
        self.model.object.text_editor_model().expect("o modelo de texto do editor personalizado está resolvido")
    }

    /// `name`.
    pub fn name(&self) -> String {
        PathModule::native(&ProcessEnvironment).basename(&self.label_service.get_uri_label(&self.resource, &UriLabelOptions::default()), None)
    }

    /// `onDidChangeContent`.
    pub fn on_did_change_content(&self) -> Event<()> {
        self.on_did_change_content.event()
    }
}

impl IDisposable for CustomTextEditorModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl ICustomEditorModel for CustomTextEditorModel {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn view_type(&self) -> String {
        self.view_type.clone()
    }

    fn resource(&self) -> URI {
        self.resource.clone()
    }

    fn backup_id(&self) -> Option<String> {
        None
    }

    fn can_hot_exit(&self) -> bool {
        true // ensured via backups from text file models
    }

    fn is_readonly(&self) -> Readonly {
        self.model.object.is_readonly()
    }

    fn on_did_change_readonly(&self) -> Event<()> {
        self.on_did_change_readonly.event()
    }

    fn is_orphaned(&self) -> bool {
        self.text_file_model.as_ref().is_some_and(|model| model.has_state(TextFileEditorModelState::Orphan))
    }

    fn on_did_change_orphaned(&self) -> Event<()> {
        self.on_did_change_orphaned.event()
    }

    fn is_dirty(&self) -> bool {
        self.text_file_service.is_dirty(&self.resource)
    }

    fn on_did_change_dirty(&self) -> Event<()> {
        self.on_did_change_dirty.event()
    }

    fn revert(&self, options: Option<RevertOptions>) -> Promise<()> {
        self.text_file_service.revert(&self.resource, options)
    }

    fn save_custom_editor(&self, options: Option<SaveOptions>) -> Promise<Option<URI>> {
        self.text_file_service.save(&self.resource, options.map(|base| TextFileSaveOptions { base, ..TextFileSaveOptions::default() }))
    }

    fn save_custom_editor_as(&self, resource: &URI, target_resource: &URI, options: Option<SaveOptions>) -> Promise<bool> {
        self.text_file_service.save_as(resource, Some(target_resource), options.map(TextFileSaveAsOptions::from_save_options)).then(|saved| saved.is_some())
    }
}
