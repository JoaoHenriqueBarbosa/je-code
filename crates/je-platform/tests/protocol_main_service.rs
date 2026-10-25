//! Porte de `src/vs/platform/protocol/test/electron-main/protocolMainService.test.ts` (suíte `ProtocolMainService`), caso a caso e na
//! mesma ordem.
//!
//! Notas de tradução:
//! - O `Request` e o `Response` do `fetch` são o `GlobalRequest` e o `GlobalResponse` de `je_platform::protocol::fetch`; o
//!   `await response.text()` é `text()`, síncrono (o corpo já está em memória).
//! - O `Buffer.from('content').toString('base64')` é o `STANDARD.encode`.
//! - O terceiro caso roda no Electron de teste do VS Code (`ipcRenderer.invoke('vscode:test-remote-resource')`, tratado em
//!   `test/unit/electron/index.js`): um servidor HTTP em `127.0.0.1` registra os cabeçalhos do pedido e responde com os
//!   cabeçalhos de `getRemoteResourceResponseHeaders` e um SVG em dois pedaços; uma janela escondida carrega uma página
//!   `vscode-file://vscode-app/...` e, nela, uma `Image` com `crossOrigin = 'anonymous'` do `vscode-remote-resource://`.
//!   Sem o Chromium, o servidor e o tratador `vscode-remote-resource:` são os reais (`std::net` e `ureq`), e a imagem é o pedido
//!   que o Chromium monta para ela (o `Origin` da página, `vscode-file://vscode-app`, e o `Sec-Fetch-Mode: cors`); `loaded` é a
//!   checagem de CORS que o Chromium faz antes de entregar a imagem (`Access-Control-Allow-Origin` igual ao `Origin`, com
//!   status 200). O esperado é o literal do original.
//! - O `this.timeout(15_000)` do mocha não existe; o `ureq` já falha sozinho se o servidor não responder.

mod common;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use common::{ensure_no_disposables_are_leaked_in_test_suite, run_with_faked_timers, run_with_real_timers};
use je_base::errors::Error;
use je_base::network::{VSCODE_AUTHORITY, schemas};
use je_base::promise::Promise;
use je_base::uri::{URI, UriComponents};
use je_platform::log::{LogServiceRef, NullLogService};
use je_platform::protocol::fetch::{GlobalHeaders, GlobalRequest, GlobalResponse};
use je_platform::protocol::managed_remote_resource_protocol::create_managed_remote_resource_request_handler;
use je_platform::protocol::remote_resource_protocol::create_remote_resource_request_handler;
use je_platform::remote::electron_remote_resources::NodeRemoteResourceResponse;
use je_server::remote_resource_response::get_remote_resource_response_headers;

fn create_request(authority: &str) -> GlobalRequest {
    let uri = URI::from(
        &UriComponents {
            scheme: Some(schemas::VSCODE_MANAGED_REMOTE_RESOURCE.into()),
            authority: Some(authority.into()),
            path: Some("/resource.txt".into()),
            ..UriComponents::default()
        },
        false,
    );

    GlobalRequest::new(&uri.to_string(false).to_string_lossy()).unwrap()
}

/// `{ statusCode, data, mimeType }`.
#[derive(Debug, PartialEq, Eq)]
struct Summary {
    status_code: u16,
    data: String,
    mime_type: Option<String>,
}

fn summarize(response: &GlobalResponse) -> Summary {
    Summary { status_code: response.status(), data: response.text(), mime_type: response.headers().get("Content-Type").map(str::to_owned) }
}

#[test]
fn loads_workbench_requests_with_an_empty_referrer() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let log_service = guard.add_rc(Rc::new(NullLogService));
    let log_service: LogServiceRef = log_service;

    run_with_faked_timers(async move {
        let requested: Rc<RefCell<Vec<URI>>> = Rc::default();
        let handler = create_managed_remote_resource_request_handler(
            Rc::new({
                let requested = requested.clone();
                move |url: URI| {
                    requested.borrow_mut().push(url);
                    Promise::resolved(NodeRemoteResourceResponse { status_code: 200, body: STANDARD.encode("content"), mime_type: Some("text/plain".to_owned()) })
                }
            }),
            log_service,
        );

        let response = handler(create_request("window:7")).await.unwrap();

        assert_eq!(
            (summarize(&response), requested.borrow().iter().map(|url| url.to_string(false).to_string_lossy()).collect::<Vec<_>>()),
            (
                Summary { status_code: 200, data: "content".to_owned(), mime_type: Some("text/plain".to_owned()) },
                vec!["vscode-managed-remote-resource://window:7/resource.txt".to_owned()],
            )
        );
    });
}

#[test]
fn returns_buffer_data_for_error_responses() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let log_service = guard.add_rc(Rc::new(NullLogService));
    let log_service: LogServiceRef = log_service;

    run_with_faked_timers(async move {
        let request_count = Rc::new(Cell::new(0));
        let handler = create_managed_remote_resource_request_handler(
            Rc::new({
                let request_count = request_count.clone();
                move |_url: URI| {
                    request_count.set(request_count.get() + 1);
                    Promise::<NodeRemoteResourceResponse>::rejected(Error::new("failed"))
                }
            }),
            log_service,
        );

        let not_found = handler(create_request("invalid")).await.unwrap();
        let failed = handler(create_request("window:7")).await.unwrap();

        assert_eq!(
            (summarize(&not_found), summarize(&failed), request_count.get()),
            (
                Summary { status_code: 404, data: "Not found".to_owned(), mime_type: Some("text/plain;charset=UTF-8".to_owned()) },
                Summary { status_code: 500, data: String::new(), mime_type: None },
                1,
            )
        );
    });
}

/// O servidor HTTP do teste: atende um pedido, guarda os cabeçalhos dele (nomes em minúsculas) e responde com os cabeçalhos de
/// `getRemoteResourceResponseHeaders(origin, () => false)` e um SVG em dois pedaços.
fn start_remote_resource_server(request_headers: Arc<Mutex<HashMap<String, String>>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("O servidor de recursos remotos do teste não abriu uma porta TCP");
    let port = listener.local_addr().expect("O servidor de recursos remotos do teste não tem endereço").port();

    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else { return };
        let Ok(reader_stream) = stream.try_clone() else { return };
        let mut reader = BufReader::new(reader_stream);

        let mut line = String::new();
        let _ = reader.read_line(&mut line); // a linha do pedido
        let mut headers = HashMap::new();
        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap_or(0) == 0 || line.trim_end().is_empty() {
                break;
            }
            if let Some((name, value)) = line.trim_end().split_once(':') {
                headers.insert(name.trim().to_ascii_lowercase(), value.trim().to_owned());
            }
        }
        let origin = headers.get("origin").cloned();
        *request_headers.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = headers;

        let mut head = String::from("HTTP/1.1 200 OK\r\n");
        for (name, value) in &get_remote_resource_response_headers(origin.as_deref(), |_| false) {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        head.push_str("Content-Type: image/svg+xml\r\nTransfer-Encoding: chunked\r\nConnection: close\r\n\r\n");

        let chunk = |text: &str| format!("{:x}\r\n{text}\r\n", text.len());
        let _ = stream.write_all(head.as_bytes());
        let _ = stream.write_all(chunk("<svg xmlns=\"http://www.w3.org/2000/svg\">").as_bytes());
        let _ = stream.flush();
        std::thread::sleep(Duration::from_millis(10));
        let _ = stream.write_all(chunk("</svg>").as_bytes());
        let _ = stream.write_all(b"0\r\n\r\n");
    });

    port
}

/// O `{ loaded, requestHeaders: { origin, secFetchMode } }` que o teste devolve.
#[derive(Debug, PartialEq, Eq)]
struct RemoteResourceLoad {
    loaded: bool,
    origin: Option<String>,
    sec_fetch_mode: Option<String>,
}

#[test]
fn loads_vscode_remote_resource_with_workbench_cors_headers() {
    let guard = ensure_no_disposables_are_leaked_in_test_suite();
    let log_service = guard.add_rc(Rc::new(NullLogService));
    let log_service: LogServiceRef = log_service;

    run_with_real_timers(async move {
        let request_headers = Arc::new(Mutex::new(HashMap::new()));
        let port = start_remote_resource_server(request_headers.clone());
        let handler = create_remote_resource_request_handler(log_service);

        // A `Image` com `crossOrigin = 'anonymous'`, carregada da página `vscode-file://vscode-app/...`
        let page_origin = format!("{}://{}", schemas::VSCODE_FILE_RESOURCE, VSCODE_AUTHORITY);
        let image_request = GlobalRequest::new(&format!("{}://127.0.0.1:{port}/vscode-remote-resource", schemas::VSCODE_REMOTE_RESOURCE))
            .unwrap()
            .with_headers(GlobalHeaders::from_pairs([("Origin", page_origin.as_str()), ("Sec-Fetch-Mode", "cors")]));
        let response = handler(image_request).await.unwrap();

        // O `image.onload` só dispara se o CORS deixa a imagem passar
        let loaded = response.status() == 200 && response.headers().get("Access-Control-Allow-Origin") == Some(page_origin.as_str());
        let recorded = request_headers.lock().unwrap_or_else(std::sync::PoisonError::into_inner);

        assert_eq!(
            RemoteResourceLoad { loaded, origin: recorded.get("origin").cloned(), sec_fetch_mode: recorded.get("sec-fetch-mode").cloned() },
            RemoteResourceLoad { loaded: true, origin: Some("vscode-file://vscode-app".to_owned()), sec_fetch_mode: Some("cors".to_owned()) }
        );
    });
}
