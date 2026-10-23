//! Porte de `src/vs/base/common/mime.ts`: tipos MIME conhecidos por extensão e a normalização de
//! um tipo MIME (`media/subtype;parâmetros`).
//!
//! O `extname` do original é o do módulo `path` da plataforma, que aqui vem do [`Environment`].
//! Os mapas de extensão para MIME são listas na ordem de declaração do original, que é a ordem em que
//! `getExtensionForMimeType` os percorre.

use crate::js_string::JsString;
use crate::path::{Environment, PathModule};
use zyarr::RegExp;

/// `Mimes`.
pub mod mimes {
    pub const TEXT: &str = "text/plain";
    pub const BINARY: &str = "application/octet-stream";
    pub const UNKNOWN: &str = "application/unknown";
    pub const MARKDOWN: &str = "text/markdown";
    pub const LATEX: &str = "text/latex";
    pub const URI_LIST: &str = "text/uri-list";
    pub const HTML: &str = "text/html";
}

type ExtMap = &'static [(&'static str, &'static [&'static str])];

const MAP_EXT_TO_TEXT_MIMES: ExtMap = &[
    (".css", &["text/css"]),
    (".csv", &["text/csv"]),
    (".htm", &["text/html"]),
    (".html", &["text/html"]),
    (".ics", &["text/calendar"]),
    (".js", &["text/javascript"]),
    (".mjs", &["text/javascript"]),
    (".txt", &["text/plain"]),
    (".xml", &["text/xml"]),
];

// Known media mimes that we can handle
const MAP_EXT_TO_MEDIA_MIMES: ExtMap = &[
    (".aac", &["audio/x-aac"]),
    (".avi", &["video/x-msvideo"]),
    (".avif", &["image/avif"]),
    (".bmp", &["image/bmp"]),
    (".flv", &["video/x-flv"]),
    (".gif", &["image/gif"]),
    (".ico", &["image/x-icon"]),
    (".jpe", &["image/jpg", "image/jpeg"]),
    (".jpeg", &["image/jpg", "image/jpeg"]),
    (".jpg", &["image/jpg", "image/jpeg"]),
    (".m1v", &["video/mpeg"]),
    (".m2a", &["audio/mpeg"]),
    (".m2v", &["video/mpeg"]),
    (".m3a", &["audio/mpeg"]),
    (".mid", &["audio/midi"]),
    (".midi", &["audio/midi"]),
    (".mk3d", &["video/x-matroska"]),
    (".mks", &["video/x-matroska"]),
    (".mkv", &["video/x-matroska"]),
    (".mov", &["video/quicktime"]),
    (".movie", &["video/x-sgi-movie"]),
    (".mp2", &["audio/mpeg"]),
    (".mp2a", &["audio/mpeg"]),
    (".mp3", &["audio/mpeg"]),
    (".mp4", &["video/mp4"]),
    (".mp4a", &["audio/mp4"]),
    (".mp4v", &["video/mp4"]),
    (".mpe", &["video/mpeg"]),
    (".mpeg", &["video/mpeg"]),
    (".mpg", &["video/mpeg"]),
    (".mpg4", &["video/mp4"]),
    (".mpga", &["audio/mpeg"]),
    (".oga", &["audio/ogg"]),
    (".ogg", &["audio/ogg"]),
    (".opus", &["audio/opus"]),
    (".ogv", &["video/ogg"]),
    (".png", &["image/png"]),
    (".psd", &["image/vnd.adobe.photoshop"]),
    (".qt", &["video/quicktime"]),
    (".spx", &["audio/ogg"]),
    (".svg", &["image/svg+xml"]),
    (".tga", &["image/x-tga"]),
    (".tif", &["image/tiff"]),
    (".tiff", &["image/tiff"]),
    (".wav", &["audio/x-wav"]),
    (".webm", &["video/webm"]),
    (".webp", &["image/webp"]),
    (".wma", &["audio/x-ms-wma"]),
    (".wmv", &["video/x-ms-wmv"]),
    (".woff", &["application/font-woff"]),
];

/// `map[ext.toLowerCase()]`; um valor com vários tipos (`.jpg`) devolve o primeiro, como o original.
fn lookup(map: ExtMap, path: &str, env: &dyn Environment) -> Option<&'static str> {
    let ext = JsString::from(PathModule::native(env).extname(path)).to_lower_case().to_string_lossy();
    map.iter().find(|(extension, _)| *extension == ext).map(|(_, mimes)| mimes[0])
}

pub fn get_media_or_text_mime(path: &str, env: &dyn Environment) -> Option<&'static str> {
    lookup(MAP_EXT_TO_TEXT_MIMES, path, env).or_else(|| get_media_mime(path, env))
}

pub fn get_media_mime(path: &str, env: &dyn Environment) -> Option<&'static str> {
    lookup(MAP_EXT_TO_MEDIA_MIMES, path, env)
}

/// A primeira extensão (na ordem de declaração) cujo tipo MIME é `mime_type`.
pub fn get_extension_for_mime_type(mime_type: &str) -> Option<&'static str> {
    [MAP_EXT_TO_TEXT_MIMES, MAP_EXT_TO_MEDIA_MIMES]
        .into_iter()
        .flatten()
        .find(|(_, mimes)| mimes.contains(&mime_type))
        .map(|(extension, _)| *extension)
}

thread_local! {
    static SIMPLE_PATTERN: RegExp = RegExp::from_str(r"^(.+)\/(.+?)(;.+)?$", "").expect("regex do MIME válido");
}

/// `normalizeMimeType(mimeType, strict)`: `None` quando `strict` e o texto não tem a forma
/// `media/subtype`. Media e subtype são sempre minúsculos (RFC 2045, 5.1), o parâmetro não.
pub fn normalize_mime_type_strict(mime_type: &str) -> Option<String> {
    let units: Vec<u16> = mime_type.encode_utf16().collect();
    let found = SIMPLE_PATTERN.with(|pattern| pattern.exec(&units, 0))?;
    let group = |index: usize| found.captures.get(index).and_then(Option::as_ref).map_or_else(JsString::new, |range| JsString::from(&units[range.clone()]));
    Some(format!("{}/{}{}", group(1).to_lower_case(), group(2).to_lower_case(), group(3)))
}

/// `normalizeMimeType(mimeType)`: um texto que não é `media/subtype` volta como veio.
pub fn normalize_mime_type(mime_type: &str) -> String {
    normalize_mime_type_strict(mime_type).unwrap_or_else(|| mime_type.to_string())
}

/// Whether the provided mime type is a text stream like `stdout`, `stderr`.
pub fn is_text_stream_mime(mime_type: &str) -> bool {
    ["application/vnd.code.notebook.stdout", "application/vnd.code.notebook.stderr"].contains(&mime_type)
}
