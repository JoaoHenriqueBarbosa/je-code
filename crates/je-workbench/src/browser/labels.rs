//! Porte de `src/vs/workbench/browser/labels.ts`: o rótulo de recurso ([`IResourceLabel`]) sobre o [`IconLabel`] de
//! `je-widgets`, com o ícone de arquivo do tema, as decorações de arquivo e o ajuste às mudanças de idioma, de
//! configuração, de modelo, de pastas do workspace, de formatador e de nome de arquivo sem título.
//!
//! Notas de tradução:
//! - **`ResourceLabelWidget extends IconLabel`** é composição: o widget guarda o `Rc<IconLabel>` e repassa `setLabel`.
//! - **`IResourceLabel`** (o objeto de closures que só expõe um handle para fora) é o trait [`IResourceLabel`] com a
//!   struct [`ResourceLabelHandle`]; `dispose` do handle tira o widget da lista dos [`ResourceLabels`].
//! - **`IResourceLabelOptions extends IIconLabelValueOptions`** guarda o `IIconLabelValueOptions` no campo `icon_label`;
//!   `IFileLabelOptions extends IResourceLabelOptions` guarda o `IResourceLabelOptions` em `resource`. O widget guarda
//!   sempre o [`IFileLabelOptions`], porque o original guarda o mesmo objeto que `setFile` recebeu (e
//!   `notifyWorkspaceFoldersChange` o devolve a `setFile` com `hideLabel`, `hidePath` e `range` intactos).
//! - **`icon?: ThemeIcon | URI`** é o `IconClassesIcon` de `getIconClasses`. `hasIconChanged` compara por valor (o
//!   original compara a identidade do objeto), o que só deixa de redesenhar quando o ícone é igual de verdade.
//! - **Injeção.** Os dez parâmetros decorados dos construtores são a struct [`ResourceLabelsServices`]. O `IThemeService`
//!   daqui só traz o tema de cores, então o estilo do [`IconLabel`] (que o original lê da folha de estilo) vem de
//!   `icon_label_styles`, uma função refeita a cada troca de tema.
//! - **`IModelService.onModelLanguageChanged`** entrou no trait `IModelService` (com padrão "nunca dispara").
//! - **`outputUriData.notebook` ausente** (o `return` de `setResource`): o `extractCellOutputDetails` do porte sempre
//!   devolve o notebook, então o ramo não existe.
//! - **`ResourceLabel extends ResourceLabels`** é a struct [`ResourceLabel`], que guarda o [`ResourceLabels`] e o único
//!   rótulo criado por ele.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::event::{Emitter, Event};
use je_base::labels::normalize_drive_letter;
use je_base::lifecycle::{Disposable, IDisposable, MutableDisposable};
use je_base::network::schemas;
use je_base::nls::localize;
use je_base::platform::Platform;
use je_base::themables::ThemeIcon;
use je_base::uri::{Field, URI, UriChange};
use je_editor::get_icon_classes::{IconClassesIcon, get_icon_classes};
use je_editor::language_service::ILanguageService;
use je_editor::range::Range;
use je_editor::services::theme_service::IThemeService;
use je_editor::services_model::IModelService;
use je_editor::text_model::TextModel;
use je_platform::configuration::service::IConfigurationService;
use je_platform::files::files::{FILES_ASSOCIATIONS_CONFIG, FileKind};
use je_platform::label::label::{ILabelService, UriLabelOptions};
use je_platform::resources::ext_uri;
use je_platform::workspace::IWorkspaceContextService;
use je_ui::{NodeId, Ui};
use je_widgets::hover_api::{HoverContent, ManagedHoverContent};
use je_widgets::icon_label::{IIconLabelCreationOptions, IIconLabelValueOptions, IconLabel, IconLabelStyles, IconLabelText, IconLabelTitle, IconPath};

use crate::services::decorations::decorations::{
    DECORATION_BADGE_CLASS, DECORATION_LABEL_COLOR_CLASS, DECORATION_TEXT_BADGE_CLASS, IDecoration, IDecorationsService, IResourceDecorationChangeEvent,
};
use crate::services::notebook::common::notebook_document_service::{INotebookDocumentService, extract_cell_output_details};
use crate::services::textfile::textfiles::ITextFileService;
use crate::services::untitled::untitled_text_editor_model::IUntitledTextEditorModel;
use crate::services::untitled::untitled_text_editor_service::IUntitledTextEditorService;
use crate::services::working_copy::working_copy::IWorkingCopy;

/// `URI | { primary?: URI; secondary?: URI }`: o recurso de um [`IResourceLabelProps`].
#[derive(Clone, Debug)]
pub enum ResourceLabelResource {
    Uri(URI),
    /// Um editor lado a lado (primário e secundário).
    Pair { primary: Option<URI>, secondary: Option<URI> },
}

/// `IResourceLabelProps`.
#[derive(Clone, Debug, Default)]
pub struct IResourceLabelProps {
    pub resource: Option<ResourceLabelResource>,
    pub name: Option<IconLabelText>,
    pub range: Option<Range>,
    pub description: Option<String>,
}

/// `toResource(props)`.
fn to_resource(props: Option<&IResourceLabelProps>) -> Option<URI> {
    match props?.resource.as_ref()? {
        ResourceLabelResource::Uri(uri) => Some(uri.clone()),
        ResourceLabelResource::Pair { primary, .. } => primary.clone(),
    }
}

/// `{ colors: boolean; badges: boolean }` de `fileDecorations`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FileDecorations {
    pub colors: bool,
    pub badges: bool,
}

/// `IResourceLabelOptions`.
#[derive(Clone, Default)]
pub struct IResourceLabelOptions {
    /// A parte `IIconLabelValueOptions` das opções.
    pub icon_label: IIconLabelValueOptions,
    /// A dica do tipo de arquivo do recurso.
    pub file_kind: Option<FileKind>,
    /// As decorações de arquivo do rótulo.
    pub file_decorations: Option<FileDecorations>,
    /// Usa o rótulo dado como está (não o troca, por exemplo, em arquivos sem título).
    pub force_label: bool,
    /// Um prefixo do nome do rótulo.
    pub name_prefix: Option<String>,
    /// Um sufixo do nome do rótulo.
    pub name_suffix: Option<String>,
    /// Usa o ícone dado em vez de derivar o ícone do recurso.
    pub icon: Option<IconClassesIcon>,
}

/// `IFileLabelOptions`.
#[derive(Clone, Default)]
pub struct IFileLabelOptions {
    pub resource: IResourceLabelOptions,
    pub hide_label: bool,
    pub hide_path: bool,
    pub range: Option<Range>,
}

/// `IResourceLabel`.
pub trait IResourceLabel: IDisposable {
    fn element(&self) -> NodeId;

    fn on_did_render(&self) -> Event<()>;

    /// A forma mais genérica de aplicar um rótulo, com a informação crua.
    fn set_label(&self, label: IconLabelText, description: Option<&str>, options: Option<&IIconLabelValueOptions>);

    /// Aplica um rótulo passando o recurso junto. Para arquivos, prefira [`IResourceLabel::set_file`].
    fn set_resource(&self, label: IResourceLabelProps, options: Option<IResourceLabelOptions>);

    /// Desenha o rótulo de arquivo de um recurso.
    fn set_file(&self, resource: &URI, options: Option<IFileLabelOptions>);

    /// Deixa o rótulo vazio.
    fn clear(&self);
}

/// `IResourceLabelsContainer`.
#[derive(Clone)]
pub struct IResourceLabelsContainer {
    pub on_did_change_visibility: Event<bool>,
}

/// `DEFAULT_LABELS_CONTAINER`: a visibilidade nunca muda.
pub fn default_labels_container() -> IResourceLabelsContainer {
    IResourceLabelsContainer { on_did_change_visibility: Event::none() }
}

/// Os serviços que o `ResourceLabels` e o `ResourceLabelWidget` recebem por injeção (ver as notas do módulo).
#[derive(Clone)]
pub struct ResourceLabelsServices {
    pub ui: Ui,
    pub configuration_service: Rc<dyn IConfigurationService>,
    pub model_service: Rc<dyn IModelService>,
    pub workspace_service: Rc<dyn IWorkspaceContextService>,
    pub language_service: Rc<dyn ILanguageService>,
    pub decorations_service: Rc<dyn IDecorationsService>,
    pub theme_service: Rc<dyn IThemeService>,
    pub label_service: Rc<dyn ILabelService>,
    pub text_file_service: Rc<dyn ITextFileService>,
    pub notebook_document_service: Rc<dyn INotebookDocumentService>,
    /// O estilo do [`IconLabel`] para o tema de cores ativo.
    pub icon_label_styles: Rc<dyn Fn() -> IconLabelStyles>,
}

/// `isEqual(a, b)` de `resources.ts`.
fn is_equal(a: &URI, b: &URI) -> bool {
    ext_uri().is_equal(Some(a), Some(b), false)
}

/// `a.toString() !== b.toString()`.
fn uri_text(uri: &URI) -> String {
    uri.to_string(false).to_string_lossy()
}

/// `scheme === Schemas.x`.
fn has_scheme(resource: Option<&URI>, scheme: &str) -> bool {
    resource.is_some_and(|resource| resource.scheme().to_string_lossy() == scheme)
}

/// `this.options?.icon !== newOptions?.icon`, por valor.
fn icons_equal(a: Option<&IconClassesIcon>, b: Option<&IconClassesIcon>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(IconClassesIcon::ThemeIcon(a)), Some(IconClassesIcon::ThemeIcon(b))) => ThemeIcon::is_equal(a, b),
        (Some(IconClassesIcon::Uri(a)), Some(IconClassesIcon::Uri(b))) => uri_text(a) == uri_text(b),
        _ => false,
    }
}

/// `!title` ou um objeto de título sem `markdownNotSupportedFallback`: o título que o recurso pode preencher.
fn title_needs_path_label(title: Option<&IconLabelTitle>) -> bool {
    match title {
        None => true,
        Some(IconLabelTitle::Text(text)) => text.is_empty(),
        Some(IconLabelTitle::Markdown(ManagedHoverContent::Markdown { markdown_not_supported_fallback, .. })) => markdown_not_supported_fallback.as_deref().is_none_or(str::is_empty),
        Some(IconLabelTitle::Markdown(_)) => true,
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Redraw {
    Basic,
    Full,
}

/// `ResourceLabels`.
pub struct ResourceLabels {
    this: Weak<ResourceLabels>,
    base: Rc<Disposable>,
    services: Rc<ResourceLabelsServices>,
    on_did_change_decorations: Rc<Emitter<()>>,
    widgets: RefCell<Vec<Rc<ResourceLabelWidget>>>,
    labels: RefCell<Vec<Rc<ResourceLabelHandle>>>,
}

impl ResourceLabels {
    pub fn new(container: IResourceLabelsContainer, services: Rc<ResourceLabelsServices>) -> Rc<Self> {
        let base = Disposable::new();
        let on_did_change_decorations = Emitter::new();
        base.register(on_did_change_decorations.clone());
        let labels = Rc::new_cyclic(|this| Self { this: this.clone(), base, services, on_did_change_decorations, widgets: RefCell::new(Vec::new()), labels: RefCell::new(Vec::new()) });
        labels.register_listeners(&container);
        labels
    }

    /// `onDidChangeDecorations`.
    pub fn on_did_change_decorations(&self) -> Event<()> {
        self.on_did_change_decorations.event()
    }

    /// Chama `action` em cada widget (uma cópia da lista, porque a ação pode mexer nela).
    fn for_each_widget(this: &Weak<ResourceLabels>, action: impl Fn(&ResourceLabelWidget)) {
        let Some(labels) = this.upgrade() else { return };
        let widgets = labels.widgets.borrow().clone();
        for widget in &widgets {
            action(widget);
        }
    }

    fn register_listeners(&self, container: &IResourceLabelsContainer) {
        let services = &self.services;
        let register = |listener| {
            self.base.register(listener);
        };

        // notify when visibility changes
        let this = self.this.clone();
        register(container.on_did_change_visibility.listen(move |visible: &bool| {
            let visible = *visible;
            Self::for_each_widget(&this, |widget| widget.notify_visibility_changed(visible));
        }));

        // notify when extensions are registered with potentially new languages
        let this = self.this.clone();
        register(services.language_service.on_did_change().listen(move |_| Self::for_each_widget(&this, ResourceLabelWidget::notify_extensions_registered)));

        // notify when model language changes
        let this = self.this.clone();
        register(services.model_service.on_model_language_changed().listen(move |e| {
            Self::for_each_widget(&this, |widget| widget.notify_model_language_changed(&e.model));
        }));

        // notify when model is added
        let this = self.this.clone();
        register(services.model_service.on_model_added().listen(move |model: &Rc<TextModel>| {
            Self::for_each_widget(&this, |widget| widget.notify_model_added(model));
        }));

        // notify when workspace folders changes
        let this = self.this.clone();
        register(services.workspace_service.on_did_change_workspace_folders().listen(move |_| Self::for_each_widget(&this, ResourceLabelWidget::notify_workspace_folders_change)));

        // notify when file decoration changes
        let this = self.this.clone();
        register(services.decorations_service.on_did_change_decorations().listen(move |e: &Rc<dyn IResourceDecorationChangeEvent>| {
            let Some(labels) = this.upgrade() else { return };
            let mut notify_did_change_decorations = false;
            let widgets = labels.widgets.borrow().clone();
            for widget in &widgets {
                if widget.notify_file_decorations_changes(e.as_ref()) {
                    notify_did_change_decorations = true;
                }
            }

            if notify_did_change_decorations {
                labels.on_did_change_decorations.fire(());
            }
        }));

        // notify when theme changes
        let this = self.this.clone();
        register(services.theme_service.on_did_color_theme_change().listen(move |_| Self::for_each_widget(&this, ResourceLabelWidget::notify_theme_change)));

        // notify when files.associations changes
        let this = self.this.clone();
        register(services.configuration_service.on_did_change_configuration().listen(move |e| {
            if e.affects_configuration(FILES_ASSOCIATIONS_CONFIG, None) {
                Self::for_each_widget(&this, ResourceLabelWidget::notify_file_associations_change);
            }
        }));

        // notify when label formatters change
        let this = self.this.clone();
        register(services.label_service.on_did_change_formatters().listen(move |e| {
            Self::for_each_widget(&this, |widget| widget.notify_formatters_change(&e.scheme));
        }));

        // notify when untitled labels change
        let this = self.this.clone();
        register(services.text_file_service.untitled().on_did_change_label().listen(move |model: &Rc<dyn IUntitledTextEditorModel>| {
            let resource = model.resource();
            Self::for_each_widget(&this, |widget| widget.notify_untitled_label_change(&resource));
        }));
    }

    /// `get(index)`.
    pub fn get(&self, index: usize) -> Option<Rc<dyn IResourceLabel>> {
        self.labels.borrow().get(index).map(|label| label.clone() as Rc<dyn IResourceLabel>)
    }

    /// `create(container, options)`.
    pub fn create(&self, container: NodeId, options: Option<IIconLabelCreationOptions>) -> Rc<dyn IResourceLabel> {
        let widget = ResourceLabelWidget::new(self.services.clone(), container, options);

        // Only expose a handle to the outside
        let label = Rc::new(ResourceLabelHandle { element: widget.icon_label.element(), widget: widget.clone(), labels: self.this.clone() });

        // Store
        self.labels.borrow_mut().push(label.clone());
        self.widgets.borrow_mut().push(widget);

        label
    }

    fn dispose_widget(&self, widget: &Rc<ResourceLabelWidget>) {
        let index = self.widgets.borrow().iter().position(|candidate| Rc::ptr_eq(candidate, widget));
        if let Some(index) = index {
            self.widgets.borrow_mut().remove(index);
            self.labels.borrow_mut().remove(index);
        }

        widget.dispose();
    }

    /// `clear()`.
    pub fn clear(&self) {
        let widgets = std::mem::take(&mut *self.widgets.borrow_mut());
        for widget in &widgets {
            widget.dispose();
        }
        self.labels.borrow_mut().clear();
    }
}

impl IDisposable for ResourceLabels {
    fn dispose(&self) {
        self.base.dispose();

        self.clear();
    }

    fn constructor_name(&self) -> &'static str {
        "ResourceLabels"
    }
}

/// O handle que o `ResourceLabels.create` entrega: só expõe a interface [`IResourceLabel`].
pub struct ResourceLabelHandle {
    element: NodeId,
    widget: Rc<ResourceLabelWidget>,
    labels: Weak<ResourceLabels>,
}

impl IResourceLabel for ResourceLabelHandle {
    fn element(&self) -> NodeId {
        self.element
    }

    fn on_did_render(&self) -> Event<()> {
        self.widget.on_did_render.event()
    }

    fn set_label(&self, label: IconLabelText, description: Option<&str>, options: Option<&IIconLabelValueOptions>) {
        self.widget.icon_label.set_label(label, description, options);
    }

    fn set_resource(&self, label: IResourceLabelProps, options: Option<IResourceLabelOptions>) {
        self.widget.set_resource(label, options.map(|resource| IFileLabelOptions { resource, ..IFileLabelOptions::default() }).unwrap_or_default());
    }

    fn set_file(&self, resource: &URI, options: Option<IFileLabelOptions>) {
        self.widget.set_file(resource, options);
    }

    fn clear(&self) {
        self.widget.clear();
    }
}

impl IDisposable for ResourceLabelHandle {
    fn dispose(&self) {
        match self.labels.upgrade() {
            Some(labels) => labels.dispose_widget(&self.widget),
            None => self.widget.dispose(),
        }
    }

    fn constructor_name(&self) -> &'static str {
        "ResourceLabel"
    }
}

/// `ResourceLabel`: um rótulo só, para o widget que não precisa de mais de um (para vários, use [`ResourceLabels`]).
pub struct ResourceLabel {
    base: Rc<Disposable>,
    labels: Rc<ResourceLabels>,
    label: Rc<dyn IResourceLabel>,
}

impl ResourceLabel {
    pub fn new(container: NodeId, options: Option<IIconLabelCreationOptions>, services: Rc<ResourceLabelsServices>) -> Rc<Self> {
        let labels = ResourceLabels::new(default_labels_container(), services);
        let base = Disposable::new();
        base.register(labels.clone());
        let label = labels.create(container, options);
        base.register(label.clone());
        Rc::new(Self { base, labels, label })
    }

    /// `element`: o rótulo.
    pub fn element(&self) -> Rc<dyn IResourceLabel> {
        self.label.clone()
    }

    /// Os [`ResourceLabels`] por baixo (para `onDidChangeDecorations`).
    pub fn labels(&self) -> &Rc<ResourceLabels> {
        &self.labels
    }
}

impl IDisposable for ResourceLabel {
    fn dispose(&self) {
        self.base.dispose();
    }

    fn constructor_name(&self) -> &'static str {
        "ResourceLabel"
    }
}

/// O estado mutável do [`ResourceLabelWidget`].
#[derive(Default)]
struct WidgetState {
    label: Option<IResourceLabelProps>,
    options: Option<IFileLabelOptions>,

    computed_icon_classes: Option<Vec<String>>,
    computed_language_id: Option<String>,
    computed_path_label: Option<String>,
    computed_workspace_folder_label: Option<String>,

    needs_redraw: Option<Redraw>,
    is_hidden: bool,
}

/// `ResourceLabelWidget`.
struct ResourceLabelWidget {
    services: Rc<ResourceLabelsServices>,
    icon_label: Rc<IconLabel>,
    on_did_render: Rc<Emitter<()>>,
    decoration: Rc<MutableDisposable<dyn IDecoration>>,
    state: RefCell<WidgetState>,
}

impl ResourceLabelWidget {
    fn new(services: Rc<ResourceLabelsServices>, container: NodeId, options: Option<IIconLabelCreationOptions>) -> Rc<Self> {
        let icon_label = IconLabel::new(&services.ui, container, options, (services.icon_label_styles)());
        Rc::new(Self { services, icon_label, on_did_render: Emitter::new(), decoration: MutableDisposable::new(), state: RefCell::new(WidgetState::default()) })
    }

    fn notify_visibility_changed(&self, visible: bool) {
        let redraw = {
            let mut state = self.state.borrow_mut();
            if visible != state.is_hidden {
                return;
            }
            state.is_hidden = !visible;
            if visible { state.needs_redraw } else { None }
        };

        if let Some(redraw) = redraw {
            self.render(redraw == Redraw::Full, redraw == Redraw::Full);

            self.state.borrow_mut().needs_redraw = None;
        }
    }

    fn notify_model_language_changed(&self, model: &TextModel) {
        self.handle_model_event(model);
    }

    fn notify_model_added(&self, model: &TextModel) {
        self.handle_model_event(model);
    }

    fn handle_model_event(&self, model: &TextModel) {
        let resource = to_resource(self.state.borrow().label.as_ref());
        let Some(resource) = resource else {
            return; // only update if resource exists
        };

        if is_equal(&model.uri(), &resource) {
            let language_id = model.get_language_id();
            let changed = self.state.borrow().computed_language_id.as_deref() != Some(language_id.as_str());
            if changed {
                self.state.borrow_mut().computed_language_id = Some(language_id);
                self.render(true, false); // update if the language id of the model has changed from our last known state
            }
        }
    }

    fn notify_file_decorations_changes(&self, e: &dyn IResourceDecorationChangeEvent) -> bool {
        let resource = {
            let state = self.state.borrow();
            let Some(options) = &state.options else { return false };
            let Some(resource) = to_resource(state.label.as_ref()) else { return false };
            if options.resource.file_decorations.is_none() {
                return false;
            }
            resource
        };

        if e.affects_resource(&resource) {
            return self.render(false, true);
        }

        false
    }

    fn notify_extensions_registered(&self) {
        self.render(true, false);
    }

    fn notify_theme_change(&self) {
        self.icon_label.set_styles((self.services.icon_label_styles)());
        self.render(false, false);
    }

    fn notify_file_associations_change(&self) {
        self.render(true, false);
    }

    fn notify_formatters_change(&self, scheme: &str) {
        let resource = to_resource(self.state.borrow().label.as_ref());
        if has_scheme(resource.as_ref(), scheme) {
            self.render(false, false);
        }
    }

    fn notify_untitled_label_change(&self, resource: &URI) {
        let current = to_resource(self.state.borrow().label.as_ref());
        if current.is_some_and(|current| is_equal(resource, &current)) {
            self.render(false, false);
        }
    }

    fn notify_workspace_folders_change(&self) {
        let (resource, options) = {
            let state = self.state.borrow();
            let Some(computed) = &state.computed_workspace_folder_label else { return };
            let Some(resource) = to_resource(state.label.as_ref()) else { return };
            if state.label.as_ref().and_then(|label| label.name.as_ref()) != Some(&IconLabelText::Single(computed.clone())) {
                return;
            }
            (resource, state.options.clone())
        };

        self.set_file(&resource, options);
    }

    fn set_file(&self, resource: &URI, options: Option<IFileLabelOptions>) {
        let options = options.unwrap_or_default();
        let mut name: Option<String> = None;
        if !options.hide_label {
            if options.resource.file_kind == Some(FileKind::RootFolder)
                && let Some(workspace_folder) = self.services.workspace_service.get_workspace_folder(resource)
            {
                name = Some(workspace_folder.name.clone());
                self.state.borrow_mut().computed_workspace_folder_label = name.clone();
            }

            if name.as_deref().is_none_or(str::is_empty) {
                name = Some(normalize_drive_letter(&ext_uri().basename_or_authority(resource), Platform::detect().is_windows));
            }
        }

        let mut description: Option<String> = None;
        if !options.hide_path {
            let description_candidate = self.services.label_service.get_uri_label(&ext_uri().dirname(resource), &UriLabelOptions { relative: Some(true), ..UriLabelOptions::default() });
            if !description_candidate.is_empty() && description_candidate != "." {
                // omit description if its not significant: a relative path
                // of '.' just indicates that there is no parent to the path
                // https://github.com/microsoft/vscode/issues/208692
                description = Some(description_candidate);
            }
        }

        let label = IResourceLabelProps { resource: Some(ResourceLabelResource::Uri(resource.clone())), name: name.map(IconLabelText::Single), description, range: options.range };
        self.set_resource(label, options);
    }

    fn set_resource(&self, mut label: IResourceLabelProps, mut options: IFileLabelOptions) {
        let services = &self.services;
        let resource = to_resource(Some(&label));
        let is_side_by_side_editor = matches!(label.resource, Some(ResourceLabelResource::Pair { .. }));
        let applies = !options.resource.force_label && !is_side_by_side_editor;

        if applies && has_scheme(resource.as_ref(), schemas::UNTITLED) {
            // Untitled labels are very dynamic because they may change
            // whenever the content changes (unless a path is associated).
            // As such we always ask the actual editor for it's name and
            // description to get latest in case name/description are
            // provided. If they are not provided from the label we got
            // we assume that the client does not want to display them
            // and as such do not override.
            //
            // We do not touch the label if it represents a primary-secondary
            // because in that case we expect it to carry a proper label
            // and description.
            let untitled_model = resource.as_ref().and_then(|resource| services.text_file_service.untitled().get(resource));
            if let Some(untitled_model) = untitled_model
                && !untitled_model.has_associated_file_path()
            {
                if matches!(label.name, Some(IconLabelText::Single(_))) {
                    label.name = Some(IconLabelText::Single(untitled_model.name()));
                }

                if label.description.is_some() {
                    let untitled_description = untitled_model.resource().path().to_string_lossy();
                    if label.name != Some(IconLabelText::Single(untitled_description.clone())) {
                        label.description = Some(untitled_description);
                    } else {
                        label.description = None;
                    }
                }

                let untitled_title = untitled_model.resource().path().to_string_lossy();
                let untitled_name = untitled_model.name();
                options.resource.icon_label.title = Some(IconLabelTitle::Text(if untitled_name != untitled_title { format!("{untitled_name} • {untitled_title}") } else { untitled_title }));
            }
        }

        if applies && has_scheme(resource.as_ref(), schemas::VSCODE_NOTEBOOK_CELL) {
            // Notebook cells are embeded in a notebook document
            // As such we always ask the actual notebook document
            // for its position in the document.
            let notebook_document = resource.as_ref().and_then(|resource| services.notebook_document_service.get_notebook(resource));
            let cell_index = notebook_document.as_ref().and_then(|document| resource.as_ref().and_then(|resource| document.get_cell_index(resource)));
            if let (Some(IconLabelText::Single(name)), Some(_), Some(cell_index)) = (&label.name, &notebook_document, cell_index) {
                let cell = (cell_index + 1).to_string();
                options.resource.icon_label.title = Some(IconLabelTitle::Text(localize("notebookCellLabel", "{0} • Cell {1}", &[name, &cell])));
                label.name = Some(IconLabelText::Single(localize("notebookCellLabel", "{0} • Cell {1}", &[name, &cell])));
            }
        }

        if applies && has_scheme(resource.as_ref(), schemas::VSCODE_NOTEBOOK_CELL_OUTPUT) {
            let notebook_document = resource.as_ref().and_then(|resource| services.notebook_document_service.get_notebook(resource));
            let output_uri_data = resource.as_ref().and_then(extract_cell_output_details);
            if let Some(output_uri_data) = output_uri_data
                && let Some(cell_fragment) = output_uri_data.cell_fragment.as_deref().filter(|fragment| !fragment.is_empty())
            {
                let cell_uri = output_uri_data.notebook.with(&UriChange { scheme: Field::from(schemas::VSCODE_NOTEBOOK_CELL), fragment: Field::from(cell_fragment), ..UriChange::default() });
                let cell_index = notebook_document.as_ref().and_then(|document| document.get_cell_index(&cell_uri));
                let output_index = output_uri_data.output_index;

                if let (Some(cell_index), Some(output_index), Some(IconLabelText::Single(name))) = (cell_index, output_index, &label.name) {
                    let (cell, output) = ((cell_index + 1).to_string(), (output_index + 1).to_string());
                    label.name = Some(IconLabelText::Single(localize("notebookCellOutputLabel", "{0} • Cell {1} • Output {2}", &[name, &cell, &output])));
                } else if let (Some(cell_index), Some(IconLabelText::Single(name))) = (cell_index, &label.name) {
                    let cell = (cell_index + 1).to_string();
                    label.name = Some(IconLabelText::Single(localize("notebookCellOutputLabelSimple", "{0} • Cell {1} • Output", &[name, &cell])));
                }
            }
        }

        if let Some(prefix) = options.resource.name_prefix.as_deref().filter(|prefix| !prefix.is_empty()) {
            match &mut label.name {
                Some(IconLabelText::Single(name)) => *name = format!("{prefix}{name}"),
                Some(IconLabelText::Multiple(names)) if !names.is_empty() => names[0] = format!("{prefix}{}", names[0]),
                _ => {}
            }
        }

        if let Some(suffix) = options.resource.name_suffix.as_deref().filter(|suffix| !suffix.is_empty()) {
            match &mut label.name {
                Some(IconLabelText::Single(name)) => name.push_str(suffix),
                Some(IconLabelText::Multiple(names)) if !names.is_empty() => {
                    if let Some(last) = names.last_mut() {
                        last.push_str(suffix);
                    }
                }
                _ => {}
            }
        }

        let (has_resource_changed, has_file_kind_changed, has_icon_changed) = {
            let state = self.state.borrow();
            let old_resource = to_resource(state.label.as_ref());
            let has_resource_changed = match (&resource, &old_resource) {
                (Some(new), Some(old)) => uri_text(new) != uri_text(old),
                (None, None) => false,
                _ => true,
            };
            let old_options = state.options.as_ref().map(|options| &options.resource);
            (
                has_resource_changed,
                options.resource.file_kind != old_options.and_then(|old| old.file_kind),
                !icons_equal(old_options.and_then(|old| old.icon.as_ref()), options.resource.icon.as_ref()),
            )
        };
        let has_path_label_changed = has_resource_changed
            || resource.as_ref().is_some_and(|resource| self.state.borrow().computed_path_label.as_deref() != Some(services.label_service.get_uri_label(resource, &UriLabelOptions::default()).as_str()));

        {
            let mut state = self.state.borrow_mut();
            state.label = Some(label);
            state.options = Some(options);

            if has_resource_changed {
                state.computed_language_id = None; // reset computed language since resource changed
            }

            if has_path_label_changed {
                state.computed_path_label = None; // reset path label due to resource/path-label change
            }
        }

        self.render(has_resource_changed || has_file_kind_changed || has_icon_changed, has_resource_changed || has_file_kind_changed);
    }

    fn clear(&self) {
        {
            let mut state = self.state.borrow_mut();
            state.label = None;
            state.options = None;
            state.computed_language_id = None;
            state.computed_icon_classes = None;
            state.computed_path_label = None;
        }

        self.icon_label.set_label(IconLabelText::Single(String::new()), None, None);
    }

    fn render(&self, update_icon: bool, update_decoration: bool) -> bool {
        let services = &self.services;
        let (label, options, mut path_label, mut icon_classes) = {
            let mut state = self.state.borrow_mut();
            if state.is_hidden {
                if state.needs_redraw != Some(Redraw::Full) {
                    state.needs_redraw = Some(if update_icon || update_decoration { Redraw::Full } else { Redraw::Basic });
                }

                return false;
            }

            if update_icon {
                state.computed_icon_classes = None;
            }

            let Some(label) = state.label.clone() else { return false };
            (label, state.options.clone(), state.computed_path_label.clone(), state.computed_icon_classes.clone())
        };
        let resource_options = options.as_ref().map(|options| &options.resource);
        let user_options = resource_options.map(|options| &options.icon_label);

        let mut icon_label_options = IIconLabelValueOptions {
            title: Some(IconLabelTitle::Text(String::new())),
            bold: user_options.is_some_and(|options| options.bold),
            italic: user_options.is_some_and(|options| options.italic),
            strikethrough: user_options.is_some_and(|options| options.strikethrough),
            matches: user_options.and_then(|options| options.matches.clone()),
            description_matches: user_options.and_then(|options| options.description_matches.clone()),
            suffix: user_options.and_then(|options| options.suffix.clone()),
            extra_classes: Vec::new(),
            separator: user_options.and_then(|options| options.separator.clone()),
            dom_id: user_options.and_then(|options| options.dom_id.clone()),
            disabled_command: user_options.is_some_and(|options| options.disabled_command),
            label_escape_new_lines: user_options.is_some_and(|options| options.label_escape_new_lines),
            description_title: user_options.and_then(|options| options.description_title.clone()),
            support_icons: user_options.and_then(|options| options.support_icons),
            ..IIconLabelValueOptions::default()
        };

        let resource = to_resource(Some(&label));

        if let Some(title) = user_options.and_then(|options| options.title.clone()) {
            icon_label_options.title = Some(title);
        }

        if let Some(resource) = &resource
            && resource.scheme().to_string_lossy() != schemas::DATA /* do not accidentally inline Data URIs */
            && title_needs_path_label(user_options.and_then(|options| options.title.as_ref()))
        {
            if path_label.as_deref().is_none_or(str::is_empty) {
                path_label = Some(services.label_service.get_uri_label(resource, &UriLabelOptions::default()));
            }
            let computed = path_label.clone().unwrap_or_default();

            match &mut icon_label_options.title {
                Some(IconLabelTitle::Markdown(ManagedHoverContent::Markdown { markdown_not_supported_fallback, .. })) if markdown_not_supported_fallback.as_deref().is_none_or(str::is_empty) => {
                    *markdown_not_supported_fallback = Some(computed);
                }
                other => *other = Some(IconLabelTitle::Text(computed)),
            }
        }

        if let Some(resource_options) = resource_options
            && !resource_options.icon_label.hide_icon
        {
            if icon_classes.is_none() {
                icon_classes = Some(get_icon_classes(services.model_service.as_ref(), services.language_service.as_ref(), resource.as_ref(), resource_options.file_kind, resource_options.icon.as_ref()));
            }

            if let Some(IconClassesIcon::Uri(icon)) = &resource_options.icon {
                icon_label_options.icon_path = Some(IconPath::Uri(icon.clone()));
            }

            icon_label_options.extra_classes = icon_classes.clone().unwrap_or_default();
        }

        if let Some(resource_options) = resource_options {
            icon_label_options.extra_classes.extend(resource_options.icon_label.extra_classes.iter().cloned());
        }

        if let (Some(resource_options), Some(resource)) = (resource_options, &resource)
            && let Some(file_decorations) = resource_options.file_decorations
        {
            if update_decoration {
                self.decoration.set(services.decorations_service.get_decoration(resource, resource_options.file_kind != Some(FileKind::File)));
            }

            if let Some(decoration) = self.decoration.value() {
                let tooltip = decoration.tooltip();
                if !tooltip.is_empty() {
                    match &icon_label_options.title {
                        Some(IconLabelTitle::Text(title)) => icon_label_options.title = Some(IconLabelTitle::Text(format!("{title} • {tooltip}"))),
                        Some(IconLabelTitle::Markdown(ManagedHoverContent::Markdown { markdown: Some(HoverContent::Text(markdown)), .. })) => {
                            let title = format!("{markdown} • {tooltip}");
                            icon_label_options.title = Some(IconLabelTitle::Markdown(ManagedHoverContent::Markdown {
                                markdown: Some(HoverContent::Text(title.clone())),
                                markdown_not_supported_fallback: Some(title),
                            }));
                        }
                        _ => {}
                    }
                }

                if decoration.strikethrough() {
                    icon_label_options.strikethrough = true;
                }

                if file_decorations.colors {
                    icon_label_options.extra_classes.push(DECORATION_LABEL_COLOR_CLASS.to_owned());
                    icon_label_options.extra_classes.push(decoration.label_class_name());
                }

                if file_decorations.badges {
                    icon_label_options.extra_classes.push(DECORATION_BADGE_CLASS.to_owned());
                    if decoration.is_text_badge() {
                        icon_label_options.extra_classes.push(DECORATION_TEXT_BADGE_CLASS.to_owned());
                    }
                    icon_label_options.extra_classes.push(decoration.badge_class_name());
                    icon_label_options.extra_classes.push(decoration.icon_class_name());
                }
            }
        }

        if let Some(range) = &label.range {
            icon_label_options.suffix = Some(if range.start_line_number != range.end_line_number {
                format!(":{}-{}", range.start_line_number, range.end_line_number)
            } else {
                format!(":{}", range.start_line_number)
            });
        }

        {
            let mut state = self.state.borrow_mut();
            state.computed_path_label = path_label;
            state.computed_icon_classes = icon_classes;
        }

        self.icon_label.set_label(label.name.clone().unwrap_or_else(|| IconLabelText::Single(String::new())), label.description.as_deref(), Some(&icon_label_options));

        self.on_did_render.fire(());

        true
    }
}

impl IDisposable for ResourceLabelWidget {
    fn dispose(&self) {
        self.icon_label.dispose();
        self.decoration.dispose();
        self.on_did_render.dispose();

        *self.state.borrow_mut() = WidgetState::default();
    }

    fn constructor_name(&self) -> &'static str {
        "ResourceLabelWidget"
    }
}
