//! Porte de `src/vs/platform/protocol/electron-main/managedRemoteResourceProtocol.ts`: o tratador do protocolo
//! `vscode-managed-remote-resource:`, que carrega um recurso de um sistema de arquivos remoto por uma janela.
//!
//! Decisões de tradução:
//! - **`Buffer.from(response.body, 'base64')`** é [`decode_node_base64`]: o decodificador do Node ignora o que não é do alfabeto
//!   (e aceita o alfabeto de URL), não falha.
//! - **O `try/catch`** cobre o que rejeita a promessa de `requestRemoteResource`, o que ele lança antes de devolvê-la e o que
//!   o construtor do `Response` recusa (um `statusCode` fora de 200 a 599, por exemplo).

use base64::Engine;
use base64::alphabet::STANDARD;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use je_base::errors::{self, Error};
use je_base::promise::Promise;
use je_base::uri::URI;
use std::rc::Rc;

use super::fetch::{GlobalHeaders, GlobalRequest, GlobalResponse, ProtocolHandler, ResponseBody};
use super::protocol::RequestRemoteResource;
use crate::log::{LogArg, LogMessage, LogServiceRef, Logger};
use crate::remote::electron_remote_resources::NodeRemoteResourceResponse;

/// O decodificador de base64 do `Buffer.from(text, 'base64')` do Node.
pub fn decode_node_base64(text: &str) -> Vec<u8> {
    let mut symbols: String = text
        .chars()
        .take_while(|c| *c != '=')
        .filter_map(|c| match c {
            '-' => Some('+'),
            '_' => Some('/'),
            c if c.is_ascii_alphanumeric() || c == '+' || c == '/' => Some(c),
            _ => None,
        })
        .collect();
    if symbols.len() % 4 == 1 {
        symbols.pop(); // um símbolo sozinho não completa nenhum byte
    }

    let engine = GeneralPurpose::new(&STANDARD, GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent).with_decode_allow_trailing_bits(true));
    engine.decode(symbols).unwrap_or_default()
}

fn not_found() -> Result<GlobalResponse, Error> {
    GlobalResponse::new(ResponseBody::Text("Not found".to_owned()), 404, None)
}

fn to_response(response: &NodeRemoteResourceResponse) -> Result<GlobalResponse, Error> {
    let headers = response.mime_type.as_deref().filter(|mime_type| !mime_type.is_empty()).map(|mime_type| GlobalHeaders::from_pairs([("Content-Type", mime_type)]));

    GlobalResponse::new(ResponseBody::Bytes(decode_node_base64(&response.body)), response.status_code, headers)
}

/// `createManagedRemoteResourceRequestHandler(requestRemoteResource, logService)`.
pub fn create_managed_remote_resource_request_handler(request_remote_resource: RequestRemoteResource, log_service: LogServiceRef) -> ProtocolHandler {
    Rc::new(move |request: GlobalRequest| {
        let url = URI::parse(request.url(), false);
        if !url.authority().to_string_lossy().starts_with("window:") {
            return match not_found() {
                Ok(response) => Promise::resolved(response),
                Err(error) => Promise::rejected(error),
            };
        }

        let response = match errors::catch(|| request_remote_resource(url)) {
            Ok(response) => response,
            Err(error) => Promise::rejected(error),
        };

        let log_service = log_service.clone();
        response.then_result(move |result| {
            match result.and_then(|response| to_response(&response)) {
                Ok(response) => Ok(response),
                Err(error) => {
                    log_service.warn_args(&LogMessage::from("error dispatching remote resource call"), &[LogArg::from(&error)]);
                    GlobalResponse::new(ResponseBody::Null, 500, None)
                }
            }
        })
    })
}
