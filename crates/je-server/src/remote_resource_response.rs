//! Porte de `src/vs/server/node/remoteResourceResponse.ts`: os cabeçalhos da resposta de um recurso remoto.
//!
//! Divergência: `Record<string, string>` é um `IndexMap`, que mantém a ordem de inserção como o objeto do JavaScript.

use indexmap::IndexMap;
use je_base::network::{VSCODE_AUTHORITY, schemas};

fn native_workbench_origin() -> String {
    format!("{}://{}", schemas::VSCODE_FILE_RESOURCE, VSCODE_AUTHORITY)
}

/// `getRemoteResourceResponseHeaders(requestOrigin, isAllowedWebEndpointOrigin)`.
pub fn get_remote_resource_response_headers(request_origin: Option<&str>, is_allowed_web_endpoint_origin: impl Fn(&str) -> bool) -> IndexMap<String, String> {
    let mut headers: IndexMap<String, String> = IndexMap::new();
    headers.insert("Content-Security-Policy".to_owned(), "default-src 'none'; sandbox".to_owned());
    headers.insert("X-Content-Type-Options".to_owned(), "nosniff".to_owned());
    headers.insert("Vary".to_owned(), "Origin".to_owned());

    if let Some(request_origin) = request_origin.filter(|origin| !origin.is_empty())
        && (request_origin == native_workbench_origin() || is_allowed_web_endpoint_origin(request_origin))
    {
        headers.insert("Access-Control-Allow-Origin".to_owned(), request_origin.to_owned());
    }

    headers
}
