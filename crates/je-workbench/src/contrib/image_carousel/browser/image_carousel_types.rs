//! Porte de `src/vs/workbench/contrib/imageCarousel/browser/imageCarouselTypes.ts`: a imagem, a seção e a coleção do
//! carrossel.

use je_base::buffer::VSBuffer;
use je_base::uri::URI;

/// `ICarouselImage`.
#[derive(Clone)]
pub struct ICarouselImage {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    /// In-memory image data. Omit when the image can be loaded lazily from `uri`.
    pub data: Option<VSBuffer>,
    /// Original resource, preferred for downloads when `data` is a resized preview.
    pub uri: Option<URI>,
    pub source: Option<String>,
    pub caption: Option<String>,
}

/// `ICarouselSection`.
#[derive(Clone)]
pub struct ICarouselSection {
    pub title: String,
    pub images: Vec<ICarouselImage>,
}

/// `IImageCarouselCollection`.
#[derive(Clone)]
pub struct IImageCarouselCollection {
    pub id: String,
    pub title: String,
    pub sections: Vec<ICarouselSection>,
}

/// `isVideoMimeType`.
pub fn is_video_mime_type(mime_type: &str) -> bool {
    mime_type.starts_with("video/")
}
