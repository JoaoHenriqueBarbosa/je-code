//! Porte de `src/vs/platform/url` do VS Code: o serviço de URL do aplicativo (`vscode://`), o casamento de URL
//! com glob e os domínios confiáveis. O `URL` do JavaScript é a crate `url` (Rust puro, com `idna`).

pub mod trusted_domains;
pub mod url_glob;
pub mod url_service;
pub mod whatwg;

pub use trusted_domains::*;
pub use url_glob::{UrlInput, normalize_url_path_separators, test_url_matches_glob, test_url_matches_glob_js};
pub use url_service::*;
