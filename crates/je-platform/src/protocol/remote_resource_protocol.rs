//! Porte de `src/vs/platform/protocol/electron-main/remoteResourceProtocol.ts`: o tratador do protocolo
//! `vscode-remote-resource:`, que busca o recurso no servidor remoto trocando o esquema por `http:`.
//!
//! O `net.fetch(..., { bypassCustomProtocolHandlers: true })` é [`net_fetch`], que só fala `http:` (ver `fetch.rs`).

use std::rc::Rc;

use je_base::network::schemas;
use je_base::promise::Promise;

use super::fetch::{GlobalRequest, GlobalResponse, ProtocolHandler, net_fetch};
use crate::log::{LogArg, LogMessage, LogServiceRef, Logger};

/// `createRemoteResourceRequestHandler(logService)`.
pub fn create_remote_resource_request_handler(log_service: LogServiceRef) -> ProtocolHandler {
    Rc::new(move |request: GlobalRequest| {
        // `String#replace` troca só a primeira ocorrência.
        let url = request.url().replacen(&format!("{}:", schemas::VSCODE_REMOTE_RESOURCE), &format!("{}:", schemas::HTTP), 1);

        let response = match GlobalRequest::new(&url) {
            Ok(forwarded) => net_fetch(forwarded.with_method(request.method()).with_headers(request.headers().clone()).with_body(request.body().map(<[u8]>::to_vec))),
            Err(error) => Promise::rejected(error),
        };

        let log_service = log_service.clone();
        response.then_result(move |result| match result {
            Ok(response) => Ok(response),
            Err(error) => {
                log_service.warn_args(&LogMessage::from("error loading remote resource"), &[LogArg::from(&error)]);
                Ok(GlobalResponse::error())
            }
        })
    })
}
