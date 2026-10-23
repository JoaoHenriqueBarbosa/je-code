//! Porte de `src/vs/base/common/uriTransformer.ts`: o transformador de URI do servidor remoto.
//!
//! ```text
//! --------------------------------
//! |    UI SIDE    |  AGENT SIDE  |
//! |---------------|--------------|
//! | vscode-remote | file         |
//! | file          | vscode-local |
//! --------------------------------
//! ```

use crate::js_string::JsString;
use crate::uri_ipc::{IRawURITransformer, IURITransformer, URITransformer, UriParts};

struct RawURITransformer {
    remote_authority: String,
}

fn parts(scheme: &str, authority: Option<JsString>, uri: &UriParts) -> UriParts {
    UriParts {
        scheme: Some(JsString::from(scheme)),
        authority,
        path: uri.path.clone(),
        query: uri.query.clone(),
        fragment: uri.fragment.clone(),
    }
}

impl IRawURITransformer for RawURITransformer {
    fn transform_incoming(&self, uri: &UriParts) -> Option<UriParts> {
        match uri.scheme.as_ref()?.to_string_lossy().as_str() {
            "vscode-remote" => Some(parts("file", None, uri)),
            "file" => Some(parts("vscode-local", None, uri)),
            _ => None,
        }
    }

    fn transform_outgoing(&self, uri: &UriParts) -> Option<UriParts> {
        match uri.scheme.as_ref()?.to_string_lossy().as_str() {
            "file" => Some(parts("vscode-remote", Some(JsString::from(self.remote_authority.as_str())), uri)),
            "vscode-local" => Some(parts("file", None, uri)),
            _ => None,
        }
    }

    fn transform_outgoing_scheme(&self, scheme: &str) -> String {
        match scheme {
            "file" => "vscode-remote".to_owned(),
            "vscode-local" => "file".to_owned(),
            other => other.to_owned(),
        }
    }
}

/// `createURITransformer`.
pub fn create_uri_transformer(remote_authority: &str) -> impl IURITransformer {
    URITransformer::new(RawURITransformer { remote_authority: remote_authority.to_owned() })
}
