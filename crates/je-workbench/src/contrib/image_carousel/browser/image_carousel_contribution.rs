//! Porte de `src/vs/workbench/contrib/imageCarousel/browser/imageCarousel.contribution.ts`: a configuração
//! `imageCarousel.*`, o serializador da entrada de editor e as ações `workbench.action.chat.openImageInCarousel` e
//! `workbench.action.openImagesInCarousel` ("Open in Images Preview", com o item de menu do explorador).
//!
//! Divergências:
//! - **Registro explícito.** O corpo do módulo roda no `import`; aqui é [`register_image_carousel_contribution`], que roda uma
//!   vez por thread (os registros de comando e de configuração são da thread).
//! - **Fora do recorte.** `./media/imageCarousel.css` e o `EditorPaneDescriptor` do `ImageCarouselEditor` (o editor é uma
//!   árvore DOM, e a UI do porte é nativa, com o painel próprio quando o widget existir) ficam de fora.
//! - **`run` assíncrono.** O `Action2::run` do porte não devolve valor: o corpo da ação do explorador vive em
//!   [`open_images_in_carousel_from_explorer`], que devolve a promessa do `run` original, e o `run` a descarta, como o
//!   `registerAction2` do porte faz com todas as ações. Os argumentos chegam como [`Dynamic`]: `isCollectionArgs` e
//!   `isSingleImageArgs` são [`collection_from_args`] e [`single_image_from_args`] (o `Uint8Array` é o `TypedArray`).
//! - **`editorService.openEditor(input, { pinned: true })`.** O `IEditorService` do porte só abre entrada tipada por
//!   `open_editors`, com as mesmas opções.
//! - **`ExplorerFolderContext`** é [`explorer_folder_context`], e `ResourceContextKey.Extension.key` é
//!   `resource_extension_context().key()`.
//! - **`/regex/i`.** É o [`ContextRegExp`] compartilhado entre a expressão do menu e [`is_media_resource`].

use std::cell::Cell;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::collator::locale_compare;
use je_base::dynamic::Dynamic;
use je_base::errors::Error;
use je_base::lifecycle::DisposableRef;
use je_base::map::ResourceSet;
use je_base::mime::get_media_mime;
use je_base::nls::localize;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_instantiation::{InstantiationService, ServicesAccessor};
use je_platform::action::{CommandAction, CommandActionTitle, Title, localize2};
use je_platform::actions::menu_ids::explorer_context;
use je_platform::actions::{Action2, Action2MenuItem, Action2Options, register_action2};
use je_platform::configuration::registry::{ConfigurationNode, ConfigurationNodeData, PropertySchema, configuration_registry};
use je_platform::contextkey::contextkey::ContextRegExp;
use je_platform::contextkey::{ContextKeyExpr, ContextKeyExpression};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::notification::{I_NOTIFICATION_SERVICE, NotificationMessage};
use je_platform::opener::editor_types::EditorOptions;
use je_platform::resources::{ext_uri, host_environment};
use je_platform::workspace::I_WORKSPACE_CONTEXT_SERVICE;

use super::image_carousel_editor_input::ImageCarouselEditorInput;
use super::image_carousel_types::{ICarouselImage, ICarouselSection, IImageCarouselCollection};
use crate::api::common::ext_host_values::ApiValue;
use crate::common::contextkeys::resource_extension_context;
use crate::common::editor::editor_factory_registry::{IEditorSerializer, editor_factory_registry};
use crate::common::editor::{EditorInputRef, EditorInputWithOptions, TextEditorOptions};
use crate::contrib::files::browser_files::I_EXPLORER_SERVICE;
use crate::contrib::files::common_files::explorer_folder_context;
use crate::services::editor::editor_service::{I_EDITOR_SERVICE, IEditorService};

/// `workbench.action.chat.openImageInCarousel`.
pub const OPEN_IMAGE_IN_CAROUSEL_ACTION_ID: &str = "workbench.action.chat.openImageInCarousel";

/// `workbench.action.openImagesInCarousel`.
pub const OPEN_IMAGES_IN_CAROUSEL_FROM_EXPLORER_ACTION_ID: &str = "workbench.action.openImagesInCarousel";

/// `MEDIA_EXTENSION_REGEX`: supported media (image + video) extensions for the carousel explorer context menu.
const MEDIA_EXTENSION_PATTERN: &str = r"^\.(png|jpg|jpeg|jpe|gif|webp|svg|bmp|ico|mp4|webm|mov)$";

thread_local! {
    static MEDIA_EXTENSION_REGEX: ContextRegExp = ContextRegExp::new(MEDIA_EXTENSION_PATTERN, "i").expect("regex de mídia válida");
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

// --- Configuration ---

fn boolean_property(description_key: &str, description: &str, markdown: bool, experimental: bool) -> PropertySchema {
    let text = Dynamic::string(&localize(description_key, description, &[]));
    let mut entries = vec![("type", Dynamic::string("boolean")), ("default", Dynamic::Bool(true)), (if markdown { "markdownDescription" } else { "description" }, text)];
    if experimental {
        entries.push(("tags", Dynamic::array([Dynamic::string("experimental")])));
    }
    PropertySchema::from_entries(entries)
}

fn register_image_carousel_configuration() {
    let node = ConfigurationNodeData {
        id: Some("imageCarousel".to_owned()),
        title: Some(localize("imageCarouselConfigurationTitle", "Images Preview", &[])),
        type_: Some(Dynamic::string("object")),
        ..Default::default()
    }
    .with_properties([
        (
            "imageCarousel.explorerContextMenu.enabled",
            boolean_property(
                "imageCarousel.explorerContextMenu.enabled",
                "Controls whether the **Open in Images Preview** option appears in the Explorer context menu.",
                true,
                true,
            ),
        ),
        (
            "imageCarousel.chat.enabled",
            boolean_property(
                "imageCarousel.chat.enabled",
                "Controls whether clicking an image attachment in chat opens the Images Preview viewer.",
                false,
                false,
            ),
        ),
    ]);
    configuration_registry().register_configuration(ConfigurationNode::new(node));
}

// --- Serializer ---

/// `ImageCarouselEditorInputSerializer`.
struct ImageCarouselEditorInputSerializer;

impl IEditorSerializer for ImageCarouselEditorInputSerializer {
    fn can_serialize(&self, _editor: &EditorInputRef) -> bool {
        false
    }

    fn serialize(&self, _editor: &EditorInputRef) -> Option<String> {
        None
    }

    fn deserialize(&self, _instantiation_service: &InstantiationService, _serialized_editor: &str) -> Option<EditorInputRef> {
        None
    }
}

// --- Args Types ---

fn string_field(object: &Dynamic, key: &str) -> Option<String> {
    match object.get_str(key) {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn parse_image(image: &Dynamic) -> Option<ICarouselImage> {
    if !matches!(image, Dynamic::Object(_)) {
        return None;
    }
    let data = match image.get_str("data") {
        Dynamic::TypedArray(bytes) => Some(VSBuffer::wrap(bytes.to_vec())),
        _ => None,
    };
    let uri = match image.get_str("uri") {
        Dynamic::Undefined => None,
        value => <URI as ApiValue>::from_dynamic(&value).ok(),
    };
    Some(ICarouselImage {
        id: string_field(image, "id")?,
        name: string_field(image, "name")?,
        mime_type: string_field(image, "mimeType")?,
        data,
        uri,
        source: string_field(image, "source"),
        caption: string_field(image, "caption"),
    })
}

fn parse_section(section: &Dynamic) -> Option<ICarouselSection> {
    if !matches!(section, Dynamic::Object(_)) {
        return None;
    }
    let Dynamic::Array(images) = section.get_str("images") else { return None };
    Some(ICarouselSection { title: string_field(section, "title")?, images: images.to_vec().iter().map(parse_image).collect::<Option<Vec<_>>>()? })
}

/// `isCollectionArgs(args)`: um objeto com `collection` (objeto) e `startIndex` (número); `None` quando os argumentos não têm essa
/// forma ou a coleção não tem os campos de `IImageCarouselCollection`.
pub fn collection_from_args(args: Option<&Dynamic>) -> Option<(IImageCarouselCollection, f64)> {
    let args = args.filter(|args| matches!(args, Dynamic::Object(_)))?;
    let collection = args.get_str("collection");
    let Dynamic::Number(start_index) = args.get_str("startIndex") else { return None };
    if !matches!(collection, Dynamic::Object(_)) {
        return None;
    }
    let Dynamic::Array(sections) = collection.get_str("sections") else { return None };
    Some((
        IImageCarouselCollection {
            id: string_field(&collection, "id")?,
            title: string_field(&collection, "title")?,
            sections: sections.to_vec().iter().map(parse_section).collect::<Option<Vec<_>>>()?,
        },
        start_index,
    ))
}

/// `isSingleImageArgs(args)` e a coleção de uma imagem que a ação monta com ela.
pub fn single_image_from_args(args: Option<&Dynamic>) -> Option<IImageCarouselCollection> {
    let args = args.filter(|args| matches!(args, Dynamic::Object(_)))?;
    let (name, mime_type) = (string_field(args, "name")?, string_field(args, "mimeType")?);
    let Dynamic::TypedArray(data) = args.get_str("data") else { return None };
    Some(IImageCarouselCollection {
        id: generate_uuid(),
        title: string_field(args, "title").unwrap_or_else(|| localize("imageCarousel.title", "Images Preview", &[])),
        sections: vec![ICarouselSection {
            title: String::new(),
            images: vec![ICarouselImage { id: generate_uuid(), name, mime_type, data: Some(VSBuffer::wrap(data.to_vec())), uri: None, source: None, caption: None }],
        }],
    })
}

// --- Actions ---

fn localized_title(key: &str, message: &str) -> Title {
    let localized = localize2(key, message);
    Title::Localized(CommandActionTitle { value: localized.value, original: localized.original, mnemonic_title: None })
}

/// `editorService.openEditor(input, { pinned: true })`.
fn open_pinned(editor_service: &dyn IEditorService, input: Rc<ImageCarouselEditorInput>) -> Promise<()> {
    let options = TextEditorOptions { base: EditorOptions { pinned: Some(true), ..Default::default() }, ..Default::default() };
    editor_service.open_editors(vec![EditorInputWithOptions { editor: input as EditorInputRef, options: Some(options) }]).then(|_| ())
}

/// `OpenImageInCarouselAction`.
struct OpenImageInCarouselAction;

impl Action2 for OpenImageInCarouselAction {
    fn desc(&self) -> Action2Options {
        Action2Options {
            command: CommandAction::new(OPEN_IMAGE_IN_CAROUSEL_ACTION_ID, localized_title("openImageInCarousel", "Open in Images Preview")),
            menu: Vec::new(),
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, args: &[Dynamic]) {
        let editor_service = accessor.get(I_EDITOR_SERVICE);
        let args = args.first();
        let (collection, start_index) = match collection_from_args(args) {
            Some(parsed) => parsed,
            None => match single_image_from_args(args) {
                Some(collection) => (collection, 0.0),
                None => return,
            },
        };
        let _ = open_pinned(&*editor_service, ImageCarouselEditorInput::new(collection, start_index));
    }
}

// --- Explorer Context Menu Integration ---

/// `isMediaResource(uri)`.
pub fn is_media_resource(uri: &URI) -> bool {
    let extension: Vec<u16> = ext_uri().extname(uri).encode_utf16().collect();
    MEDIA_EXTENSION_REGEX.with(|regex| regex.test(&extension))
}

/// `collectImageFilesFromFolder(fileService, folderUri)`.
async fn collect_image_files_from_folder(file_service: &Rc<dyn IFileService>, folder_uri: &URI) -> Result<Vec<URI>, Error> {
    let stat = file_service.resolve(folder_uri, None).await?;
    let mut image_uris: Vec<URI> = stat.children.unwrap_or_default().into_iter().filter(|child| child.is_file && is_media_resource(&child.resource)).map(|child| child.resource).collect();
    let uri = ext_uri();
    image_uris.sort_by(|a, b| locale_compare(&uri.basename(a, None), &uri.basename(b, None)));
    Ok(image_uris)
}

/// `createImageEntries(uris)`.
fn create_image_entries(uris: &[URI]) -> Vec<ICarouselImage> {
    let env = host_environment();
    let uri = ext_uri();
    uris.iter()
        .map(|resource| ICarouselImage {
            id: generate_uuid(),
            name: uri.basename(resource, None),
            mime_type: get_media_mime(&resource.path().to_string_lossy(), env.as_ref()).unwrap_or("image/png").to_owned(),
            data: None,
            uri: Some(resource.clone()),
            source: None,
            caption: None,
        })
        .collect()
}

/// A seleção do explorador, ou a pasta do argumento ou da raiz do workspace, em imagens e a imagem inicial: o bloco `try` do
/// `run` original.
async fn collect_selected_images(
    file_service: Rc<dyn IFileService>,
    context: Vec<Rc<crate::contrib::files::explorer_model::ExplorerItem>>,
    resource: Option<URI>,
    workspace_root: Option<URI>,
) -> Result<(Vec<URI>, Option<URI>), Error> {
    let mut image_uris: Vec<URI> = Vec::new();
    let mut start_uri: Option<URI> = None;

    if context.is_empty() {
        // Empty-space right-click: the explorer passes the workspace root as the resource argument. Fall back to the first
        // workspace folder when no resource is available.
        if let Some(folder_uri) = resource.or(workspace_root) {
            image_uris = collect_image_files_from_folder(&file_service, &folder_uri).await?;
        }
    } else {
        let has_single_image_file = context.len() == 1 && !context[0].is_directory() && is_media_resource(&context[0].resource());

        if has_single_image_file {
            // Single image: show all sibling images in the same folder with the selected image focused
            start_uri = Some(context[0].resource());
            let parent_uri = ext_uri().dirname(&context[0].resource());
            image_uris = collect_image_files_from_folder(&file_service, &parent_uri).await?;
        } else {
            // Multiple items or a folder: collect images from selection, deduplicating in case a folder and its children are
            // both selected
            let mut seen = ResourceSet::new(None);
            for item in &context {
                if item.is_directory() {
                    for uri in collect_image_files_from_folder(&file_service, &item.resource()).await? {
                        if !seen.has(&uri) {
                            seen.add(uri.clone());
                            image_uris.push(uri);
                        }
                    }
                } else if is_media_resource(&item.resource()) && !seen.has(&item.resource()) {
                    seen.add(item.resource());
                    image_uris.push(item.resource());
                    if start_uri.is_none() {
                        start_uri = Some(item.resource());
                    }
                }
            }
        }
    }
    Ok((image_uris, start_uri))
}

/// O corpo de `OpenImagesInCarouselFromExplorerAction.run(accessor, resource?)`.
pub fn open_images_in_carousel_from_explorer(accessor: &ServicesAccessor, resource: Option<&Dynamic>) -> Promise<()> {
    let explorer_service = accessor.get(I_EXPLORER_SERVICE);
    let file_service = accessor.get(I_FILE_SERVICE);
    let editor_service = accessor.get(I_EDITOR_SERVICE);
    let notification_service = accessor.get(I_NOTIFICATION_SERVICE);
    let context_service = accessor.get(I_WORKSPACE_CONTEXT_SERVICE);

    let context = explorer_service.get_context(true, None);
    let resource = resource.and_then(|resource| <URI as ApiValue>::from_dynamic(resource).ok());
    let workspace_root = context_service.get_workspace().folders().first().map(|folder| folder.uri.clone());

    async_fn(async move {
        let (image_uris, start_uri) = match collect_selected_images(file_service, context, resource, workspace_root).await {
            Ok(collected) => collected,
            Err(_) => {
                notification_service.error(vec![NotificationMessage::from(localize("folderReadError", "Could not read folder contents.", &[]))]);
                return Ok(());
            }
        };

        if image_uris.is_empty() {
            notification_service.info(vec![NotificationMessage::from(localize("noImagesFound", "No images found in this folder.", &[]))]);
            return Ok(());
        }

        let images = create_image_entries(&image_uris);

        let mut start_index = 0.0;
        if let Some(start_uri) = start_uri {
            let start = start_uri.to_string(false);
            if let Some(index) = images.iter().position(|image| image.uri.as_ref().is_some_and(|uri| uri.to_string(false) == start)) {
                start_index = index as f64;
            }
        }

        let collection = IImageCarouselCollection {
            id: generate_uuid(),
            title: localize("imageCarousel.explorerTitle", "Images Preview", &[]),
            sections: vec![ICarouselSection { title: String::new(), images }],
        };

        open_pinned(&*editor_service, ImageCarouselEditorInput::new(collection, start_index)).await
    })
}

/// A expressão `when` do item de menu do explorador.
fn explorer_menu_when() -> Option<ContextKeyExpression> {
    ContextKeyExpr::and([
        Some(ContextKeyExpr::has("config.imageCarousel.explorerContextMenu.enabled")),
        ContextKeyExpr::or([
            Some(explorer_folder_context().expr()),
            Some(ContextKeyExpr::regex(resource_extension_context().key(), MEDIA_EXTENSION_REGEX.with(Clone::clone))),
        ]),
    ])
}

/// `OpenImagesInCarouselFromExplorerAction`.
struct OpenImagesInCarouselFromExplorerAction;

impl Action2 for OpenImagesInCarouselFromExplorerAction {
    fn desc(&self) -> Action2Options {
        Action2Options {
            command: CommandAction::new(OPEN_IMAGES_IN_CAROUSEL_FROM_EXPLORER_ACTION_ID, localized_title("openImagesInCarousel", "Open in Images Preview")),
            menu: vec![Action2MenuItem {
                id: explorer_context(),
                null_precondition: false,
                alt: None,
                when: explorer_menu_when(),
                group: Some("navigation".to_owned()),
                order: Some(25.0),
                is_hidden_by_default: None,
            }],
            keybinding: Vec::new(),
            f1: false,
        }
    }

    fn run(&self, accessor: &ServicesAccessor, args: &[Dynamic]) {
        let _ = open_images_in_carousel_from_explorer(accessor, args.first());
    }
}

/// O que o módulo do original faz ao ser importado: registra a configuração, o serializador e as duas ações.
pub fn register_image_carousel_contribution() {
    if REGISTERED.with(|registered| registered.replace(true)) {
        return;
    }
    register_image_carousel_configuration();
    let serializer: Rc<dyn Fn(&InstantiationService) -> Rc<dyn IEditorSerializer>> = Rc::new(|_| Rc::new(ImageCarouselEditorInputSerializer));
    // Os registros do módulo valem pela vida do processo: os `DisposableRef` não são descartados.
    let _: DisposableRef = editor_factory_registry().register_editor_serializer_with(ImageCarouselEditorInput::ID, serializer);
    let _: DisposableRef = register_action2(Rc::new(OpenImageInCarouselAction));
    let _: DisposableRef = register_action2(Rc::new(OpenImagesInCarouselFromExplorerAction));
}
