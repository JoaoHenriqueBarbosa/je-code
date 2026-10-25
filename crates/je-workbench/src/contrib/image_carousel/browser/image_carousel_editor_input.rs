//! Porte de `src/vs/workbench/contrib/imageCarousel/browser/imageCarouselEditorInput.ts`: a entrada de editor do carrossel.
//!
//! Divergências:
//! - **`registerIcon('image-carousel-editor-label-icon', Codicon.fileMedia, ...)`.** O registro de ícones do porte não existe
//!   como módulo: o ícone do rótulo é o `ThemeIcon` de [`image_carousel_editor_icon`], com o id do original.
//! - **`startIndex`.** É `number` no original e fica `f64`.

use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

use je_base::lifecycle::IDisposable;
use je_base::network::schemas;
use je_base::themables::ThemeIcon;
use je_base::uri::{URI, UriComponents, encode_uri_component};
use je_base::JsString;

use super::image_carousel_types::IImageCarouselCollection;
use crate::common::editor::editor_input::{EditorInput, EditorInputBase, EditorInputIcon};
use crate::common::editor::{EditorInputCapabilities, EditorInputOrUntyped};

/// `imageCarouselEditorIcon`.
pub fn image_carousel_editor_icon() -> ThemeIcon {
    ThemeIcon::from_id("image-carousel-editor-label-icon")
}

/// `ImageCarouselEditorInput`.
pub struct ImageCarouselEditorInput {
    base: EditorInputBase,
    /// `collection`.
    pub collection: IImageCarouselCollection,
    /// `startIndex`.
    pub start_index: f64,
    resource: URI,
    name: RefCell<String>,
}

impl ImageCarouselEditorInput {
    /// `ImageCarouselEditorInput.ID`.
    pub const ID: &'static str = "workbench.input.imageCarousel";

    /// `constructor(collection, startIndex = 0)`.
    pub fn new(collection: IImageCarouselCollection, start_index: f64) -> Rc<Self> {
        let id_units: Vec<u16> = collection.id.encode_utf16().collect();
        let mut path = vec![u16::from(b'/')];
        path.extend(encode_uri_component(&id_units));
        let resource = URI::from(
            &UriComponents { scheme: Some(JsString::from(schemas::VSCODE_IMAGE_CAROUSEL)), path: Some(JsString::from_units(path)), ..UriComponents::default() },
            false,
        );
        let name = collection.title.clone();
        Rc::new_cyclic(|this| Self { base: EditorInputBase::new(this.clone()), collection, start_index, resource, name: RefCell::new(name) })
    }

    /// `setName(name)`.
    pub fn set_name(&self, name: &str) {
        if *self.name.borrow() != name {
            *self.name.borrow_mut() = name.to_owned();
            self.base.fire_did_change_label();
        }
    }
}

impl IDisposable for ImageCarouselEditorInput {
    fn dispose(&self) {
        self.base.dispose();
    }
}

impl EditorInput for ImageCarouselEditorInput {
    fn base(&self) -> &EditorInputBase {
        &self.base
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn editor_type_id(&self) -> String {
        Self::ID.to_owned()
    }

    fn resource(&self) -> Option<URI> {
        Some(self.resource.clone())
    }

    fn capabilities(&self) -> EditorInputCapabilities {
        EditorInputCapabilities::READONLY | EditorInputCapabilities::SINGLETON | EditorInputCapabilities::REQUIRES_MODAL
    }

    fn get_name(&self) -> String {
        self.name.borrow().clone()
    }

    fn get_icon(&self) -> Option<EditorInputIcon> {
        Some(EditorInputIcon::Theme(image_carousel_editor_icon()))
    }

    fn matches(&self, other_input: &EditorInputOrUntyped) -> bool {
        match other_input {
            EditorInputOrUntyped::Typed(other) => match other.as_any().downcast_ref::<ImageCarouselEditorInput>() {
                Some(other) => other.collection.id == self.collection.id,
                None => false,
            },
            EditorInputOrUntyped::Untyped(_) => false,
        }
    }
}
