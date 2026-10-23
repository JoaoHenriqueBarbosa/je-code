//! Porte de `src/vs/base/common/uriIpc.ts`: a troca dos `URI` dentro de um valor que atravessa o IPC (o
//! extension host, o servidor remoto), na ida (`transformOutgoing`) e na volta (`transformIncoming`).
//!
//! O original percorre `any` e troca as propriedades no lugar. Aqui o valor é [`Marshalled`] (o JSON com
//! `URI`, `RegExp`, `Date` e `VSBuffer`), tomado e devolvido por valor. "Sem mudança" é a identidade do
//! objeto no original; aqui é [`None`] nos transformadores, e as funções devolvem o próprio valor.
//!
//! Um objeto `{ $mid: 1, ... }` é um `URI` ainda não revivido (a forma de `URI#toJSON`); um
//! [`Marshalled::Uri`] já é um `URI`.

use crate::marshalling::{Marshalled, uri_state_of, uri_state_to_object};
use crate::marshalling_ids::MarshalledId;
use crate::path::Platform;
use crate::uri::{URI, UriComponents, UriState};

/// `UriParts`: os componentes de um URI, com `scheme` sempre presente.
pub type UriParts = UriComponents;

/// `IURITransformer`. Os métodos que no original devolvem o próprio argumento quando nada muda devolvem
/// `None` aqui.
pub trait IURITransformer {
    fn transform_incoming(&self, uri: &UriComponents) -> Option<UriState>;
    fn transform_outgoing(&self, uri: &UriComponents) -> Option<UriState>;
    fn transform_outgoing_uri(&self, uri: &URI) -> URI;
    fn transform_outgoing_scheme(&self, scheme: &str) -> String;
}

/// `IRawURITransformer`: o mapa de esquemas, sem a reconstrução do `URI`.
pub trait IRawURITransformer {
    /// `None` é o `return uri` do original (o mesmo objeto).
    fn transform_incoming(&self, uri: &UriParts) -> Option<UriParts>;
    fn transform_outgoing(&self, uri: &UriParts) -> Option<UriParts>;
    fn transform_outgoing_scheme(&self, scheme: &str) -> String;
}

/// A representação de um `URI` como componentes (o que um transformador lê).
fn components_of(uri: &URI) -> UriComponents {
    UriComponents {
        scheme: Some(uri.scheme().clone()),
        authority: Some(uri.authority().clone()),
        path: Some(uri.path().clone()),
        query: Some(uri.query().clone()),
        fragment: Some(uri.fragment().clone()),
    }
}

/// `toJSON(URI.from(result))`.
fn to_json(parts: &UriParts) -> UriState {
    URI::from(parts, false).to_json()
}

/// `URITransformer`.
pub struct URITransformer<T: IRawURITransformer> {
    uri_transformer: T,
}

impl<T: IRawURITransformer> URITransformer<T> {
    pub fn new(uri_transformer: T) -> Self {
        Self { uri_transformer }
    }
}

impl<T: IRawURITransformer> IURITransformer for URITransformer<T> {
    fn transform_incoming(&self, uri: &UriComponents) -> Option<UriState> {
        self.uri_transformer.transform_incoming(uri).map(|result| to_json(&result))
    }

    fn transform_outgoing(&self, uri: &UriComponents) -> Option<UriState> {
        self.uri_transformer.transform_outgoing(uri).map(|result| to_json(&result))
    }

    fn transform_outgoing_uri(&self, uri: &URI) -> URI {
        match self.uri_transformer.transform_outgoing(&components_of(uri)) {
            Some(result) => URI::from(&result, false),
            None => uri.clone(),
        }
    }

    fn transform_outgoing_scheme(&self, scheme: &str) -> String {
        self.uri_transformer.transform_outgoing_scheme(scheme)
    }
}

/// `DefaultURITransformer`: não troca nada.
pub struct DefaultURITransformer;

impl IURITransformer for DefaultURITransformer {
    fn transform_incoming(&self, _uri: &UriComponents) -> Option<UriState> {
        None
    }

    fn transform_outgoing(&self, _uri: &UriComponents) -> Option<UriState> {
        None
    }

    fn transform_outgoing_uri(&self, uri: &URI) -> URI {
        uri.clone()
    }

    fn transform_outgoing_scheme(&self, scheme: &str) -> String {
        scheme.to_owned()
    }
}

/// O limite de profundidade do percurso do original.
const MAX_DEPTH: usize = 200;

/// `!obj`: `null`, `0`, `NaN`, `''` e `false`.
fn is_falsy(obj: &Marshalled) -> bool {
    match obj {
        Marshalled::Undefined | Marshalled::Null => true,
        Marshalled::Bool(b) => !b,
        Marshalled::Number(n) => *n == 0.0 || n.is_nan(),
        Marshalled::String(s) => s.is_empty(),
        _ => false,
    }
}

fn walk(obj: Marshalled, depth: usize, visit: &dyn Fn(Marshalled, usize) -> Marshalled) -> Marshalled {
    match obj {
        Marshalled::Array(items) => Marshalled::Array(items.into_iter().map(|item| visit(item, depth + 1)).collect()),
        Marshalled::Object(entries) => Marshalled::Object(entries.into_iter().map(|(key, value)| (key, visit(value, depth + 1))).collect()),
        other => other,
    }
}

fn transform_outgoing_at(obj: Marshalled, transformer: &dyn IURITransformer, depth: usize) -> Marshalled {
    if is_falsy(&obj) || depth > MAX_DEPTH {
        return obj;
    }
    if let Marshalled::Uri(uri) = &obj {
        return match transformer.transform_outgoing(&components_of(uri)) {
            Some(state) => uri_state_to_object(&state),
            None => obj,
        };
    }
    walk(obj, depth, &|child, depth| transform_outgoing_at(child, transformer, depth))
}

/// `transformOutgoingURIs`.
pub fn transform_outgoing_uris(obj: Marshalled, transformer: &dyn IURITransformer) -> Marshalled {
    transform_outgoing_at(obj, transformer, 0)
}

fn is_uri_object(obj: &Marshalled) -> bool {
    matches!(obj.key("$mid"), Some(Marshalled::Number(mid)) if *mid == f64::from(MarshalledId::Uri as i32))
}

fn transform_incoming_at(obj: Marshalled, transformer: &dyn IURITransformer, revive: Option<Platform>, depth: usize) -> Marshalled {
    if is_falsy(&obj) || depth > MAX_DEPTH {
        return obj;
    }
    if is_uri_object(&obj) {
        let state = uri_state_of(&obj);
        let transformed = transformer.transform_incoming(&state.components);
        return match (revive, transformed) {
            (Some(platform), Some(state)) => Marshalled::Uri(URI::revive(&state, platform)),
            (Some(platform), None) => Marshalled::Uri(URI::revive(&state, platform)),
            (None, Some(state)) => uri_state_to_object(&state),
            (None, None) => obj,
        };
    }
    if matches!(obj, Marshalled::Buffer(_)) {
        return obj;
    }
    walk(obj, depth, &|child, depth| transform_incoming_at(child, transformer, revive, depth))
}

/// `transformIncomingURIs`.
pub fn transform_incoming_uris(obj: Marshalled, transformer: &dyn IURITransformer) -> Marshalled {
    transform_incoming_at(obj, transformer, None, 0)
}

/// `transformAndReviveIncomingURIs`.
pub fn transform_and_revive_incoming_uris(obj: Marshalled, transformer: &dyn IURITransformer, platform: Platform) -> Marshalled {
    transform_incoming_at(obj, transformer, Some(platform), 0)
}
