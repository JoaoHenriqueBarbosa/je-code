//! Porte parcial do namespace `COI` de `src/vs/base/common/network.ts`: os cabeçalhos de isolamento de origem que o
//! `ProtocolMainService` acrescenta às respostas (`CoopAndCoep` e `getHeadersFromQuery`).
//!
//! Fora do porte: `addSearchParam`, que só roda no renderizador (lê `globalThis.crossOriginIsolated`) e monta a consulta que
//! `get_headers_from_query` lê. O parser de URL é o WHATWG do crate `url`, o mesmo do `new URL(url).searchParams` do original.

/// O parâmetro de consulta que pede os cabeçalhos (`vscode-coi`).
const COI_SEARCH_PARAM_NAME: &str = "vscode-coi";

/// Os cabeçalhos de cada valor de `vscode-coi` (`coiHeaders`).
fn coi_headers(value: &str) -> Option<&'static [(&'static str, &'static str)]> {
    match value {
        "1" => Some(&[("Cross-Origin-Opener-Policy", "same-origin")]),
        "2" => Some(&[("Cross-Origin-Embedder-Policy", "require-corp")]),
        "3" => Some(COOP_AND_COEP),
        _ => None,
    }
}

/// `COI.CoopAndCoep`.
pub const COOP_AND_COEP: &[(&str, &str)] = &[("Cross-Origin-Opener-Policy", "same-origin"), ("Cross-Origin-Embedder-Policy", "require-corp")];

/// `COI.getHeadersFromQuery(url)`: extrai os cabeçalhos pedidos pelo `vscode-coi` da URL. Uma URL que o parser não lê não tem
/// consulta, então não pede nada (no original o `new URL` lança; os chamadores só passam a `request.url`, já validada).
pub fn get_headers_from_query(url: &str) -> Option<&'static [(&'static str, &'static str)]> {
    let url = ::url::Url::parse(url).ok()?;
    let value = url.query_pairs().find(|(name, _)| name == COI_SEARCH_PARAM_NAME).map(|(_, value)| value.into_owned())?;
    if value.is_empty() {
        return None;
    }

    coi_headers(&value)
}
