//! Porte de `src/vs/platform/webContentExtractor/common/webContentExtractor.ts`: as opções e o resultado
//! da extração de conteúdo web.
//!
//! Divergências: só entram os tipos que o cache e a conversão usam ([`WebContentExtractorOptions`] e
//! [`WebContentExtractResult`]). Os identificadores de serviço (`IWebContentExtractorService`,
//! `ISharedWebContentExtractorService`), as interfaces dos serviços e as implementações nulas dependem
//! da injeção de dependência e do `VSBuffer`/`CancellationToken` do processo hospedeiro, e ficam com a
//! fatia dos serviços. `statusCode` é `u32` (o código HTTP).

use je_base::uri::URI;

/// `IWebContentExtractorOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WebContentExtractorOptions {
    /// Whether to allow cross-authority redirects on the web content.
    /// 'false' by default.
    pub follow_redirects: Option<bool>,

    /// List of trusted domain patterns for redirect validation.
    pub trusted_domains: Option<Vec<String>>,
}

/// `WebContentExtractResult`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WebContentExtractResult {
    Ok { result: String, title: Option<String> },
    Error { error: String, status_code: Option<u32>, result: Option<String>, title: Option<String> },
    Redirect { to_uri: URI },
}
