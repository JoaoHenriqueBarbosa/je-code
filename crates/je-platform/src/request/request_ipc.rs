//! Porte de `src/vs/platform/request/common/requestIpc.ts`: o canal de IPC do serviço de requisição (o lado do
//! servidor, [`RequestChannel`], e o cliente, [`RequestChannelClient`]).
//!
//! O resultado de `request` viaja como `[{ headers, statusCode }, VSBuffer, timings?]`. As opções de
//! requisição são convertidas de e para [`Marshalled`] campo a campo (as chaves são as do original).

use std::rc::Rc;

use je_base::JsString;
use je_base::buffer::{buffer_to_stream, stream_to_buffer};
use je_base::cancellation::CancellationToken;
use je_base::errors::Error;
use je_base::event::Event;
use je_base::marshalling::Marshalled;
use je_base::parts::ipc::{IChannel, IServerChannel};
use je_base::parts::request::{HeaderValue, IHeaders, IRequestContext, IRequestOptions, IRequestResponse, IRequestTimings};
use je_base::promise::Promise;

use super::request::{AuthInfo, Credentials, IRequestService, RequestCompleteEvent};

fn key(name: &str) -> JsString {
    JsString::from(name)
}

fn object_get<'a>(value: &'a Marshalled, name: &str) -> Option<&'a Marshalled> {
    match value {
        Marshalled::Object(entries) => entries.iter().find(|(k, _)| *k == name).map(|(_, v)| v),
        _ => None,
    }
}

fn string_of(value: Option<&Marshalled>) -> Option<String> {
    match value {
        Some(Marshalled::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn number_of(value: Option<&Marshalled>) -> Option<f64> {
    match value {
        Some(Marshalled::Number(n)) => Some(*n),
        _ => None,
    }
}

fn bool_of(value: Option<&Marshalled>) -> Option<bool> {
    match value {
        Some(Marshalled::Bool(b)) => Some(*b),
        _ => None,
    }
}

fn headers_to_marshalled(headers: &IHeaders) -> Marshalled {
    Marshalled::Object(
        headers
            .iter()
            .map(|(name, value)| {
                let value = match value {
                    HeaderValue::One(text) => Marshalled::String(JsString::from(text.as_str())),
                    HeaderValue::Many(items) => Marshalled::Array(items.iter().map(|i| Marshalled::String(JsString::from(i.as_str()))).collect()),
                };
                (key(name), value)
            })
            .collect(),
    )
}

fn headers_from_marshalled(value: Option<&Marshalled>) -> Option<IHeaders> {
    let Some(Marshalled::Object(entries)) = value else { return None };
    let mut headers = IHeaders::new();
    for (name, value) in entries {
        let value = match value {
            Marshalled::String(text) => HeaderValue::One(text.to_string_lossy()),
            Marshalled::Array(items) => HeaderValue::Many(items.iter().filter_map(|i| string_of(Some(i))).collect()),
            _ => continue,
        };
        headers.insert(name.to_string_lossy(), value);
    }
    Some(headers)
}

/// `IRequestOptions` como o objeto que atravessa o canal (só os campos presentes).
pub fn request_options_to_marshalled(options: &IRequestOptions) -> Marshalled {
    let mut entries: Vec<(JsString, Marshalled)> = Vec::new();
    let mut put = |name: &str, value: Marshalled| entries.push((key(name), value));
    let text = |value: &str| Marshalled::String(JsString::from(value));
    if let Some(v) = &options.r#type {
        put("type", text(v));
    }
    if let Some(v) = &options.url {
        put("url", text(v));
    }
    if let Some(v) = &options.user {
        put("user", text(v));
    }
    if let Some(v) = &options.password {
        put("password", text(v));
    }
    if let Some(v) = &options.headers {
        put("headers", headers_to_marshalled(v));
    }
    if let Some(v) = options.timeout {
        put("timeout", Marshalled::Number(v as f64));
    }
    if let Some(v) = &options.data {
        put("data", text(v));
    }
    if let Some(v) = options.follow_redirects {
        put("followRedirects", Marshalled::Number(f64::from(v)));
    }
    if let Some(v) = &options.proxy_authorization {
        put("proxyAuthorization", text(v));
    }
    if let Some(v) = options.disable_cache {
        put("disableCache", Marshalled::Bool(v));
    }
    if let Some(v) = options.log_url {
        put("logUrl", Marshalled::Bool(v));
    }
    put("callSite", text(&options.call_site));
    if let Some(v) = &options.diagnostic_id {
        put("diagnosticId", text(v));
    }
    Marshalled::Object(entries)
}

/// O inverso de [`request_options_to_marshalled`].
pub fn request_options_from_marshalled(value: &Marshalled) -> IRequestOptions {
    IRequestOptions {
        r#type: string_of(object_get(value, "type")),
        url: string_of(object_get(value, "url")),
        user: string_of(object_get(value, "user")),
        password: string_of(object_get(value, "password")),
        headers: headers_from_marshalled(object_get(value, "headers")),
        timeout: number_of(object_get(value, "timeout")).map(|n| n as u64),
        data: string_of(object_get(value, "data")),
        follow_redirects: number_of(object_get(value, "followRedirects")).map(|n| n as u32),
        proxy_authorization: string_of(object_get(value, "proxyAuthorization")),
        disable_cache: bool_of(object_get(value, "disableCache")),
        log_url: bool_of(object_get(value, "logUrl")),
        call_site: string_of(object_get(value, "callSite")).unwrap_or_default(),
        diagnostic_id: string_of(object_get(value, "diagnosticId")),
    }
}

fn timings_to_marshalled(timings: &IRequestTimings) -> Marshalled {
    Marshalled::Object(vec![
        (key("responseHeadersMs"), Marshalled::Number(timings.response_headers_ms)),
        (key("responseBodyMs"), Marshalled::Number(timings.response_body_ms)),
        (key("decodedBodyBytes"), Marshalled::Number(timings.decoded_body_bytes as f64)),
    ])
}

fn timings_from_marshalled(value: Option<&Marshalled>) -> Option<IRequestTimings> {
    let value = value?;
    Some(IRequestTimings {
        response_headers_ms: number_of(object_get(value, "responseHeadersMs"))?,
        response_body_ms: number_of(object_get(value, "responseBodyMs"))?,
        decoded_body_bytes: number_of(object_get(value, "decodedBodyBytes"))? as usize,
    })
}

fn first_arg(arg: &Marshalled) -> Marshalled {
    match arg {
        Marshalled::Array(items) => items.first().cloned().unwrap_or(Marshalled::Undefined),
        _ => Marshalled::Undefined,
    }
}

fn string_arg(arg: &Marshalled) -> String {
    string_of(Some(&first_arg(arg))).unwrap_or_default()
}

fn optional_string(value: Option<String>) -> Marshalled {
    value.map_or(Marshalled::Undefined, |v| Marshalled::String(JsString::from(v.as_str())))
}

/// `RequestChannel`.
pub struct RequestChannel {
    service: Rc<dyn IRequestService>,
}

impl RequestChannel {
    pub fn new(service: Rc<dyn IRequestService>) -> Self {
        Self { service }
    }
}

impl IServerChannel for RequestChannel {
    fn listen(&self, _ctx: &Marshalled, _event: &str, _arg: Marshalled) -> Event<Marshalled> {
        je_base::errors::throw(Error::new("Invalid listen"))
    }

    fn call(&self, _ctx: &Marshalled, command: &str, arg: Marshalled, token: CancellationToken) -> Promise<Marshalled> {
        match command {
            "request" => {
                let options = request_options_from_marshalled(&first_arg(&arg));
                self.service.request(options, token).then_promise(|context| {
                    let IRequestContext { res, stream, timings } = context;
                    stream_to_buffer(&*stream).then(move |buffer| {
                        let mut head = vec![(key("headers"), headers_to_marshalled(&res.headers))];
                        if let Some(code) = res.status_code {
                            head.push((key("statusCode"), Marshalled::Number(f64::from(code))));
                        }
                        Marshalled::Array(vec![
                            Marshalled::Object(head),
                            Marshalled::Buffer(buffer),
                            timings.as_ref().map_or(Marshalled::Undefined, timings_to_marshalled),
                        ])
                    })
                })
            }
            "resolveProxy" => self.service.resolve_proxy(&string_arg(&arg)).then(optional_string),
            "lookupAuthorization" => {
                let info = first_arg(&arg);
                let auth_info = AuthInfo {
                    is_proxy: bool_of(object_get(&info, "isProxy")).unwrap_or(false),
                    scheme: string_of(object_get(&info, "scheme")).unwrap_or_default(),
                    host: string_of(object_get(&info, "host")).unwrap_or_default(),
                    port: number_of(object_get(&info, "port")).unwrap_or(0.0) as u16,
                    realm: string_of(object_get(&info, "realm")).unwrap_or_default(),
                    attempt: number_of(object_get(&info, "attempt")).unwrap_or(0.0) as u32,
                };
                self.service.lookup_authorization(auth_info).then(|credentials| match credentials {
                    Some(c) => Marshalled::Object(vec![
                        (key("username"), Marshalled::String(JsString::from(c.username.as_str()))),
                        (key("password"), Marshalled::String(JsString::from(c.password.as_str()))),
                    ]),
                    None => Marshalled::Undefined,
                })
            }
            "lookupKerberosAuthorization" => self.service.lookup_kerberos_authorization(&string_arg(&arg)).then(optional_string),
            "loadCertificates" => {
                self.service.load_certificates().then(|list| Marshalled::Array(list.iter().map(|c| Marshalled::String(JsString::from(c.as_str()))).collect()))
            }
            _ => Promise::rejected(Error::new("Invalid call")),
        }
    }
}

/// `RequestChannelClient`.
pub struct RequestChannelClient {
    channel: Rc<dyn IChannel>,
}

impl RequestChannelClient {
    pub fn new(channel: Rc<dyn IChannel>) -> Self {
        Self { channel }
    }

    fn call(&self, command: &str, args: Marshalled, token: CancellationToken) -> Promise<Marshalled> {
        self.channel.call(command, args, token)
    }
}

impl IRequestService for RequestChannelClient {
    /// `Event.None`: o evento não atravessa o canal.
    fn on_did_complete_request(&self) -> Event<RequestCompleteEvent> {
        Event::none()
    }

    fn request(&self, options: IRequestOptions, token: CancellationToken) -> Promise<IRequestContext> {
        self.call("request", Marshalled::Array(vec![request_options_to_marshalled(&options)]), token).then_result(|result| {
            let Marshalled::Array(items) = result? else { return Err(Error::new("Invalid request response")) };
            let [head, body, timings] = <[Marshalled; 3]>::try_from(items).map_err(|_| Error::new("Invalid request response"))?;
            let Marshalled::Buffer(buffer) = body else { return Err(Error::new("Invalid request response")) };
            Ok(IRequestContext {
                res: IRequestResponse {
                    headers: headers_from_marshalled(object_get(&head, "headers")).unwrap_or_default(),
                    status_code: number_of(object_get(&head, "statusCode")).map(|n| n as u16),
                },
                stream: buffer_to_stream(buffer),
                timings: timings_from_marshalled(Some(&timings)),
            })
        })
    }

    fn resolve_proxy(&self, url: &str) -> Promise<Option<String>> {
        self.call("resolveProxy", Marshalled::Array(vec![Marshalled::String(JsString::from(url))]), CancellationToken::None).then(|value| string_of(Some(&value)))
    }

    fn lookup_authorization(&self, auth_info: AuthInfo) -> Promise<Option<Credentials>> {
        let info = Marshalled::Object(vec![
            (key("isProxy"), Marshalled::Bool(auth_info.is_proxy)),
            (key("scheme"), Marshalled::String(JsString::from(auth_info.scheme.as_str()))),
            (key("host"), Marshalled::String(JsString::from(auth_info.host.as_str()))),
            (key("port"), Marshalled::Number(f64::from(auth_info.port))),
            (key("realm"), Marshalled::String(JsString::from(auth_info.realm.as_str()))),
            (key("attempt"), Marshalled::Number(f64::from(auth_info.attempt))),
        ]);
        self.call("lookupAuthorization", Marshalled::Array(vec![info]), CancellationToken::None).then(|value| {
            Some(Credentials { username: string_of(object_get(&value, "username"))?, password: string_of(object_get(&value, "password"))? })
        })
    }

    fn lookup_kerberos_authorization(&self, url: &str) -> Promise<Option<String>> {
        self.call("lookupKerberosAuthorization", Marshalled::Array(vec![Marshalled::String(JsString::from(url))]), CancellationToken::None)
            .then(|value| string_of(Some(&value)))
    }

    fn load_certificates(&self) -> Promise<Vec<String>> {
        self.call("loadCertificates", Marshalled::Undefined, CancellationToken::None).then(|value| match value {
            Marshalled::Array(items) => items.iter().filter_map(|i| string_of(Some(i))).collect(),
            _ => Vec::new(),
        })
    }
}
