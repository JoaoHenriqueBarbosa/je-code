//! Porte de `src/vs/platform/webview/common/mimeTypes.ts`: o tipo MIME com que o conteúdo local de um webview é servido.
//!
//! Notas de tradução:
//! - `webviewMimeTypes` (um `Map` de extensão para tipo) é a lista [`WEBVIEW_MIME_TYPES`], consultada em ordem; as
//!   chaves são distintas, então a ordem não muda o resultado.
//! - `extname` do módulo `path` e `fsPath` leem a plataforma do hospedeiro ([`host_environment`] e [`host_platform`]).

use je_base::mime::{get_media_mime, mimes};
use je_base::path::PathModule;
use je_base::uri::URI;

use crate::resources::{host_environment, host_platform};

const WEBVIEW_MIME_TYPES: [(&str, &str); 13] = [
    (".svg", "image/svg+xml"),
    (".txt", mimes::TEXT),
    (".css", "text/css"),
    (".js", "application/javascript"),
    (".cjs", "application/javascript"),
    (".mjs", "application/javascript"),
    (".json", "application/json"),
    (".html", "text/html"),
    (".htm", "text/html"),
    (".xhtml", "application/xhtml+xml"),
    (".oft", "font/otf"),
    (".xml", "application/xml"),
    (".wasm", "application/wasm"),
];

/// `getWebviewContentMimeType(resource)`.
pub fn get_webview_content_mime_type(resource: &URI) -> String {
    let env = host_environment();
    let fs_path = resource.fs_path(host_platform()).to_string_lossy();
    let ext = PathModule::native(env.as_ref()).extname(&fs_path).to_lowercase();
    if let Some((_, mime)) = WEBVIEW_MIME_TYPES.iter().find(|(known, _)| *known == ext) {
        return (*mime).to_owned();
    }
    get_media_mime(&fs_path, env.as_ref()).unwrap_or(mimes::UNKNOWN).to_owned()
}
