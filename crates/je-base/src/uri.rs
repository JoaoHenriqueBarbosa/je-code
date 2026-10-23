//! Porte de `src/vs/base/common/uri.ts`: a classe `URI` (RFC 3986) com `parse`, `file`, `from`, `with`,
//! `toString`, `fsPath`, `toJSON` e `revive`.
//!
//! Os componentes são [`JsString`] (UTF-16) porque o original mede e compara em unidades UTF-16 e aceita
//! surrogates isolados (a codificação mínima de `toString(true)` os deixa passar). O `URI` do original
//! tem identidade de objeto, e `with` devolve a própria instância quando nada muda: aqui o `URI` é um
//! `Rc` barato de clonar e [`URI::ptr_eq`] reproduz o `===`. Os caches `_formatted` e `_fsPath` do
//! original ficam em `RefCell` dentro da instância e só afetam o desempenho, nunca o resultado.
//!
//! Tudo que no original lê `isWindows` recebe a [`Platform`] como argumento (a mesma injeção do
//! [`crate::path`]). O cache de `fsPath` guarda a plataforma com que foi calculado.
//!
//! Erros de validação (`_validateUri`) são exceções no original e aqui viram [`errors::throw`].

use std::any::Any;
use std::cell::RefCell;
use std::fmt;
use std::rc::Rc;

use crate::char_code as cc;
use crate::errors::{self, Error};
use crate::path::{FixedEnvironment, PathModule, Platform};
use crate::JsString;

/// `MarshalledId.Uri` (`marshallingIds.ts`).
pub const MARSHALLED_ID_URI: i32 = 1;

/// `UriComponents`: `scheme` obrigatório no tipo do original, os demais opcionais. `None` é o `undefined`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UriComponents {
    pub scheme: Option<JsString>,
    pub authority: Option<JsString>,
    pub path: Option<JsString>,
    pub query: Option<JsString>,
    pub fragment: Option<JsString>,
}

/// `UriState`: o que `toJSON` produz e `revive` consome (componentes mais o estado em cache).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UriState {
    /// `$mid`.
    pub mid: Option<i32>,
    pub external: Option<JsString>,
    pub fs_path: Option<JsString>,
    /// `_sep`: `1` quando o `fsPath` em cache foi calculado com separador do Windows.
    pub sep: Option<u8>,
    pub components: UriComponents,
}

impl From<UriComponents> for UriState {
    fn from(components: UriComponents) -> Self {
        Self { components, ..Self::default() }
    }
}

/// Uma das alterações de `with`: `undefined` (manter), `null` (limpar) ou um valor.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Field {
    #[default]
    Keep,
    Clear,
    Set(JsString),
}

impl From<&str> for Field {
    fn from(value: &str) -> Self {
        Field::Set(JsString::from(value))
    }
}

impl From<JsString> for Field {
    fn from(value: JsString) -> Self {
        Field::Set(value)
    }
}

impl Field {
    fn resolve(&self, current: &JsString) -> JsString {
        match self {
            Field::Keep => current.clone(),
            Field::Clear => JsString::new(),
            Field::Set(value) => value.clone(),
        }
    }
}

/// O argumento de `URI#with`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UriChange {
    pub scheme: Field,
    pub authority: Field,
    pub path: Field,
    pub query: Field,
    pub fragment: Field,
}

struct UriData {
    scheme: JsString,
    authority: JsString,
    path: JsString,
    query: JsString,
    fragment: JsString,
    /// `_formatted`.
    formatted: RefCell<Option<JsString>>,
    /// `_fsPath`, com a plataforma que o calculou.
    fs_path: RefCell<Option<(Platform, JsString)>>,
}

/// Uniform Resource Identifier (RFC 3986), componentes: esquema, autoridade, caminho, consulta, fragmento.
#[derive(Clone)]
pub struct URI(Rc<UriData>);

// ---- helpers sobre unidades UTF-16 -----------------------------------------------------------------

fn units(s: &str) -> Vec<u16> {
    s.encode_utf16().collect()
}

fn index_of_unit(haystack: &[u16], unit: u16, from: usize) -> Option<usize> {
    haystack.get(from..)?.iter().position(|&u| u == unit).map(|p| p + from)
}

fn is_ascii_letter(code: u16) -> bool {
    (cc::A..=cc::Z).contains(&code) || (cc::a..=cc::z).contains(&code)
}

fn is_word_unit(code: u16) -> bool {
    is_ascii_letter(code) || (cc::Digit0..=cc::Digit9).contains(&code) || code == cc::Underline
}

/// `/^\w[\w\d+.-]*$/`.
fn is_valid_scheme(scheme: &[u16]) -> bool {
    match scheme.split_first() {
        Some((&first, rest)) => is_word_unit(first) && rest.iter().all(|&u| is_scheme_rest_unit(u)),
        None => false,
    }
}

fn is_scheme_rest_unit(code: u16) -> bool {
    is_word_unit(code) || code == cc::Plus || code == cc::Period || code == cc::Dash
}

fn uri_error(message: String) -> ! {
    errors::throw(Error::new(message))
}

// ---- validação e normalização ----------------------------------------------------------------------

fn validate_uri(ret: &UriData, strict: bool) {
    // scheme, must be set
    if ret.scheme.is_empty() && strict {
        uri_error(format!(
            "[UriError]: Scheme is missing: {{scheme: \"\", authority: \"{}\", path: \"{}\", query: \"{}\", fragment: \"{}\"}}",
            ret.authority, ret.path, ret.query, ret.fragment
        ));
    }

    // scheme, https://tools.ietf.org/html/rfc3986#section-3.1
    if !ret.scheme.is_empty() && !is_valid_scheme(ret.scheme.units()) {
        // `matchAll(/[^\w\d+.-]/gu)`: o índice é em unidades UTF-16, a iteração é por ponto de código.
        let mut matches: Vec<(usize, String)> = Vec::new();
        let mut index = 0;
        for decoded in char::decode_utf16(ret.scheme.units().iter().copied()) {
            let (width, text) = match decoded {
                Ok(c) => (c.len_utf16(), c.to_string()),
                Err(_) => (1, "\u{FFFD}".to_string()),
            };
            let illegal = match decoded {
                Ok(c) => u16::try_from(c as u32).map(|u| !is_scheme_rest_unit(u)).unwrap_or(true),
                Err(_) => true,
            };
            if illegal {
                matches.push((index, text));
            }
            index += width;
        }
        let detail = match matches.first() {
            Some((at, text)) => format!(" Found '{}' at index {} ({} total)", text, at, matches.len()),
            None => String::new(),
        };
        uri_error(format!("[UriError]: Scheme contains illegal characters.{} (len:{})", detail, ret.scheme.len()));
    }

    // path, http://tools.ietf.org/html/rfc3986#section-3.3
    if !ret.path.is_empty() {
        let units = ret.path.units();
        if !ret.authority.is_empty() {
            if units[0] != cc::Slash {
                uri_error("[UriError]: If a URI contains an authority component, then the path component must either be empty or begin with a slash (\"/\") character".to_string());
            }
        } else if units.len() >= 2 && units[0] == cc::Slash && units[1] == cc::Slash {
            uri_error("[UriError]: If a URI does not contain an authority component, then the path cannot begin with two slash characters (\"//\")".to_string());
        }
    }
}

/// `_schemeFix`: um URI sem esquema e sem modo estrito cai em `file`.
fn scheme_fix(scheme: JsString, strict: bool) -> JsString {
    if scheme.is_empty() && !strict { JsString::from("file") } else { scheme }
}

/// `_referenceResolution`: para `http`, `https` e `file` o caminho vazio ou relativo ganha a barra inicial.
fn reference_resolution(scheme: &JsString, path: JsString) -> JsString {
    if scheme == "https" || scheme == "http" || scheme == "file" {
        if path.is_empty() {
            return JsString::from("/");
        } else if path.units()[0] != cc::Slash {
            let mut with_slash = vec![cc::Slash];
            with_slash.extend_from_slice(path.units());
            return JsString::from_units(with_slash);
        }
    }
    path
}

impl URI {
    fn from_parts(scheme: JsString, authority: JsString, path: JsString, query: JsString, fragment: JsString) -> URI {
        URI(Rc::new(UriData {
            scheme,
            authority,
            path,
            query,
            fragment,
            formatted: RefCell::new(None),
            fs_path: RefCell::new(None),
        }))
    }

    /// O construtor de strings de `Uri` (com `_schemeFix`, `_referenceResolution` e `_validateUri`).
    fn construct(scheme: JsString, authority: JsString, path: JsString, query: JsString, fragment: JsString, strict: bool) -> URI {
        let scheme = scheme_fix(scheme, strict);
        let path = reference_resolution(&scheme, path);
        let uri = Self::from_parts(scheme, authority, path, query, fragment);
        validate_uri(&uri.0, strict);
        uri
    }

    /// `thing instanceof URI`. O `isUri` do original também aceita objetos com a forma de um `URI`
    /// (duck typing), o que não existe em Rust: só uma instância de [`URI`] conta.
    pub fn is_uri(thing: &dyn Any) -> bool {
        thing.is::<URI>()
    }

    /// O `===` do original: a mesma instância.
    pub fn ptr_eq(a: &URI, b: &URI) -> bool {
        Rc::ptr_eq(&a.0, &b.0)
    }

    pub fn scheme(&self) -> &JsString {
        &self.0.scheme
    }

    pub fn authority(&self) -> &JsString {
        &self.0.authority
    }

    pub fn path(&self) -> &JsString {
        &self.0.path
    }

    pub fn query(&self) -> &JsString {
        &self.0.query
    }

    pub fn fragment(&self) -> &JsString {
        &self.0.fragment
    }

    /// `URI#fsPath`: o caminho do sistema de arquivos, com o separador da plataforma, UNC tratado e a
    /// letra de unidade em minúscula.
    pub fn fs_path(&self, platform: Platform) -> JsString {
        if let Some((cached_platform, cached)) = &*self.0.fs_path.borrow()
            && *cached_platform == platform
            && !cached.is_empty()
        {
            return cached.clone();
        }
        let computed = uri_to_fs_path(self, false, platform);
        *self.0.fs_path.borrow_mut() = Some((platform, computed.clone()));
        computed
    }

    /// `URI#with`. O `with(null)`/`with(undefined)` do original é `with(&UriChange::default())`.
    pub fn with(&self, change: &UriChange) -> URI {
        let scheme = change.scheme.resolve(&self.0.scheme);
        let authority = change.authority.resolve(&self.0.authority);
        let path = change.path.resolve(&self.0.path);
        let query = change.query.resolve(&self.0.query);
        let fragment = change.fragment.resolve(&self.0.fragment);

        if scheme == self.0.scheme
            && authority == self.0.authority
            && path == self.0.path
            && query == self.0.query
            && fragment == self.0.fragment
        {
            return self.clone();
        }

        Self::construct(scheme, authority, path, query, fragment, false)
    }

    /// `URI.parse`.
    pub fn parse(value: impl Into<JsString>, strict: bool) -> URI {
        let value = value.into();
        let parts = parse_parts(value.units());
        Self::construct(
            JsString::from(parts.scheme),
            percent_decode(parts.authority),
            percent_decode(parts.path),
            percent_decode(parts.query),
            percent_decode(parts.fragment),
            strict,
        )
    }

    /// `URI.file`: trata o argumento como caminho do sistema de arquivos, não como URI em texto.
    pub fn file(path: impl Into<JsString>, platform: Platform) -> URI {
        let mut path = path.into().units().to_vec();
        let mut authority: Vec<u16> = Vec::new();

        // normalize to fwd-slashes on windows, on other systems bwd-slashes are valid filename character
        if platform == Platform::Win32 {
            for unit in &mut path {
                if *unit == cc::Backslash {
                    *unit = cc::Slash;
                }
            }
        }

        // check for authority as used in UNC shares or use the path as given
        if path.len() >= 2 && path[0] == cc::Slash && path[1] == cc::Slash {
            match index_of_unit(&path, cc::Slash, 2) {
                None => {
                    authority = path[2..].to_vec();
                    path = vec![cc::Slash];
                }
                Some(idx) => {
                    authority = path[2..idx].to_vec();
                    path = path[idx..].to_vec();
                }
            }
        }

        Self::construct(
            JsString::from("file"),
            JsString::from_units(authority),
            JsString::from_units(path),
            JsString::new(),
            JsString::new(),
            false,
        )
    }

    /// `URI.from`: valida componentes não confiáveis; sem `strict`, o esquema vazio vira `file`.
    pub fn from(components: &UriComponents, strict: bool) -> URI {
        let part = |field: &Option<JsString>| field.clone().unwrap_or_default();
        Self::construct(
            part(&components.scheme),
            part(&components.authority),
            part(&components.path),
            part(&components.query),
            part(&components.fragment),
            strict,
        )
    }

    /// `URI.joinPath`.
    pub fn join_path(uri: &URI, path_fragments: &[&str], platform: Platform) -> URI {
        if uri.path().is_empty() {
            uri_error(format!("[UriError]: cannot call joinPath on URI without path: {}", uri.to_string(false)));
        }
        // `join` não consulta o diretório atual, o ambiente só fixa a plataforma do módulo.
        let env = FixedEnvironment::new(platform, "/");
        let new_path = if platform == Platform::Win32 && uri.scheme() == "file" {
            let mut segments = vec![uri_to_fs_path(uri, true, platform).to_string_lossy()];
            segments.extend(path_fragments.iter().map(|s| s.to_string()));
            let refs: Vec<&str> = segments.iter().map(String::as_str).collect();
            let joined = PathModule::win32(&env).join(&refs);
            URI::file(joined, platform).0.path.clone()
        } else {
            let mut segments = vec![uri.path().to_string_lossy()];
            segments.extend(path_fragments.iter().map(|s| s.to_string()));
            let refs: Vec<&str> = segments.iter().map(String::as_str).collect();
            JsString::from(PathModule::posix(&env).join(&refs))
        };
        uri.with(&UriChange { path: Field::Set(new_path), ..UriChange::default() })
    }

    /// `URI#toString`: a forma externa do URI, com percent-encoding (ou só o mínimo, com `skip_encoding`).
    pub fn to_string(&self, skip_encoding: bool) -> JsString {
        if skip_encoding {
            // we don't cache that
            return as_formatted(self, true);
        }
        if let Some(formatted) = &*self.0.formatted.borrow()
            && !formatted.is_empty()
        {
            return formatted.clone();
        }
        let formatted = as_formatted(self, false);
        *self.0.formatted.borrow_mut() = Some(formatted.clone());
        formatted
    }

    /// `URI#toJSON`: componentes não vazios mais o estado em cache (`external`, `fsPath`, `_sep`).
    pub fn to_json(&self) -> UriState {
        let non_empty = |value: &JsString| if value.is_empty() { None } else { Some(value.clone()) };
        let mut state = UriState { mid: Some(MARSHALLED_ID_URI), ..UriState::default() };
        if let Some((platform, fs_path)) = &*self.0.fs_path.borrow()
            && !fs_path.is_empty()
        {
            state.fs_path = Some(fs_path.clone());
            state.sep = (*platform == Platform::Win32).then_some(1);
        }
        if let Some(formatted) = &*self.0.formatted.borrow() {
            state.external = non_empty(formatted);
        }
        state.components = UriComponents {
            scheme: non_empty(&self.0.scheme),
            authority: non_empty(&self.0.authority),
            path: non_empty(&self.0.path),
            query: non_empty(&self.0.query),
            fragment: non_empty(&self.0.fragment),
        };
        state
    }

    /// `URI.revive`: reconstrói a partir de `toJSON`, sem validação. O `_sep` do dado só vale se bater
    /// com o da plataforma informada.
    pub fn revive(data: &UriState, platform: Platform) -> URI {
        let part = |field: &Option<JsString>| field.clone().unwrap_or_default();
        let c = &data.components;
        let result = Self::from_parts(part(&c.scheme), part(&c.authority), part(&c.path), part(&c.query), part(&c.fragment));
        *result.0.formatted.borrow_mut() = data.external.clone();
        let marker = (platform == Platform::Win32).then_some(1u8);
        if data.sep == marker {
            *result.0.fs_path.borrow_mut() = data.fs_path.clone().map(|fs_path| (platform, fs_path));
        }
        result
    }
}

impl PartialEq for URI {
    /// Igualdade pelos cinco componentes (o `deepStrictEqual` do original também olharia os caches).
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
            || (self.0.scheme == other.0.scheme
                && self.0.authority == other.0.authority
                && self.0.path == other.0.path
                && self.0.query == other.0.query
                && self.0.fragment == other.0.fragment)
    }
}

impl Eq for URI {}

impl fmt::Debug for URI {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "URI({})", self.to_string(false))
    }
}

/// `isUriComponents`: qualquer coisa com a forma de `UriComponents` (um [`URI`], os componentes, ou o
/// estado de `toJSON`).
pub fn is_uri_components(thing: &dyn Any) -> bool {
    if thing.is::<URI>() {
        return true;
    }
    if let Some(components) = thing.downcast_ref::<UriComponents>() {
        return components.scheme.is_some();
    }
    if let Some(state) = thing.downcast_ref::<UriState>() {
        return state.components.scheme.is_some();
    }
    false
}

// ---- análise do texto ------------------------------------------------------------------------------

struct ParsedParts<'a> {
    scheme: &'a [u16],
    authority: &'a [u16],
    path: &'a [u16],
    query: &'a [u16],
    fragment: &'a [u16],
}

/// A expressão `^(([^:/?#]+?):)?(\/\/([^/?#]*))?([^?#]*)(\?([^#]*))?(#(.*))?`, escrita à mão. O `.` do
/// fragmento não casa terminadores de linha, então o fragmento acaba no primeiro deles.
fn parse_parts(value: &[u16]) -> ParsedParts<'_> {
    let is_stop = |u: u16| u == cc::Colon || u == cc::Slash || u == cc::QuestionMark || u == cc::Hash;
    let mut rest = value;

    // scheme: `[^:/?#]+?` seguido de ':'
    let mut scheme: &[u16] = &[];
    if let Some(stop) = rest.iter().position(|&u| is_stop(u))
        && rest[stop] == cc::Colon
        && stop > 0
    {
        scheme = &rest[..stop];
        rest = &rest[stop + 1..];
    }

    // authority: `//` seguido de `[^/?#]*`
    let mut authority: &[u16] = &[];
    if rest.len() >= 2 && rest[0] == cc::Slash && rest[1] == cc::Slash {
        let end = rest[2..]
            .iter()
            .position(|&u| u == cc::Slash || u == cc::QuestionMark || u == cc::Hash)
            .map_or(rest.len(), |p| p + 2);
        authority = &rest[2..end];
        rest = &rest[end..];
    }

    // path: `[^?#]*`
    let path_end = rest.iter().position(|&u| u == cc::QuestionMark || u == cc::Hash).unwrap_or(rest.len());
    let path = &rest[..path_end];
    rest = &rest[path_end..];

    // query: `\?([^#]*)`
    let mut query: &[u16] = &[];
    if rest.first() == Some(&cc::QuestionMark) {
        let end = rest.iter().position(|&u| u == cc::Hash).unwrap_or(rest.len());
        query = &rest[1..end];
        rest = &rest[end..];
    }

    // fragment: `#(.*)`
    let mut fragment: &[u16] = &[];
    if rest.first() == Some(&cc::Hash) {
        let body = &rest[1..];
        let end = body
            .iter()
            .position(|&u| u == cc::LineFeed || u == cc::CarriageReturn || u == 0x2028 || u == 0x2029)
            .unwrap_or(body.len());
        fragment = &body[..end];
    }

    ParsedParts { scheme, authority, path, query, fragment }
}

// ---- caminho do sistema de arquivos ----------------------------------------------------------------

/// `uriToFsPath`: o `fsPath` de um URI, opcionalmente preservando a caixa da letra de unidade.
pub fn uri_to_fs_path(uri: &URI, keep_drive_letter_casing: bool, platform: Platform) -> JsString {
    let path = uri.path().units();
    let at = |i: usize| path.get(i).copied();
    let mut value: Vec<u16>;
    if !uri.authority().is_empty() && path.len() > 1 && uri.scheme() == "file" {
        // unc path: file://shares/c$/far/boo
        value = units("//");
        value.extend_from_slice(uri.authority().units());
        value.extend_from_slice(path);
    } else if at(0) == Some(cc::Slash) && at(1).is_some_and(is_ascii_letter) && at(2) == Some(cc::Colon) {
        if !keep_drive_letter_casing {
            // windows drive letter: file:///c:/far/boo
            let letter = at(1).unwrap();
            value = vec![if (cc::A..=cc::Z).contains(&letter) { letter + 32 } else { letter }];
            value.extend_from_slice(&path[2..]);
        } else {
            value = path[1..].to_vec();
        }
    } else {
        // other path
        value = path.to_vec();
    }
    if platform == Platform::Win32 {
        for unit in &mut value {
            if *unit == cc::Slash {
                *unit = cc::Backslash;
            }
        }
    }
    JsString::from_units(value)
}

// ---- codificação -----------------------------------------------------------------------------------

/// A tabela `encodeTable`: caracteres reservados (RFC 3986, 2.2) e o espaço.
fn encode_table(code: u16) -> Option<&'static str> {
    Some(match code {
        cc::Colon => "%3A", // gen-delims
        cc::Slash => "%2F",
        cc::QuestionMark => "%3F",
        cc::Hash => "%23",
        cc::OpenSquareBracket => "%5B",
        cc::CloseSquareBracket => "%5D",
        cc::AtSign => "%40",
        cc::ExclamationMark => "%21", // sub-delims
        cc::DollarSign => "%24",
        cc::Ampersand => "%26",
        cc::SingleQuote => "%27",
        cc::OpenParen => "%28",
        cc::CloseParen => "%29",
        cc::Asterisk => "%2A",
        cc::Plus => "%2B",
        cc::Comma => "%2C",
        cc::Semicolon => "%3B",
        cc::Equals => "%3D",
        cc::Space => "%20",
        _ => return None,
    })
}

fn push_ascii(out: &mut Vec<u16>, text: &str) {
    out.extend(text.bytes().map(u16::from));
}

/// `encodeURIComponent` do JavaScript: UTF-8 em percent-encoding, uppercase, preservando
/// `A-Z a-z 0-9 - _ . ! ~ * ' ( )`. Surrogate isolado lança `URIError`.
pub fn encode_uri_component(input: &[u16]) -> Vec<u16> {
    let mut out = Vec::with_capacity(input.len());
    for decoded in char::decode_utf16(input.iter().copied()) {
        let Ok(c) = decoded else {
            errors::throw(Error::named("URIError", "URI malformed"));
        };
        let keep = c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '!' | '~' | '*' | '\'' | '(' | ')');
        if keep {
            out.push(c as u16);
        } else {
            let mut buf = [0u8; 4];
            for byte in c.encode_utf8(&mut buf).bytes() {
                push_ascii(&mut out, &format!("%{byte:02X}"));
            }
        }
    }
    out
}

/// `encodeURIComponentFast`.
fn encode_uri_component_fast(component: &[u16], is_path: bool, is_authority: bool) -> Vec<u16> {
    let mut res: Option<Vec<u16>> = None;
    let mut native_encode_pos: Option<usize> = None;

    for (pos, &code) in component.iter().enumerate() {
        // unreserved characters: https://tools.ietf.org/html/rfc3986#section-2.3
        let unreserved = is_ascii_letter(code)
            || (cc::Digit0..=cc::Digit9).contains(&code)
            || code == cc::Dash
            || code == cc::Period
            || code == cc::Underline
            || code == cc::Tilde
            || (is_path && code == cc::Slash)
            || (is_authority && (code == cc::OpenSquareBracket || code == cc::CloseSquareBracket || code == cc::Colon));

        if unreserved {
            // check if we are delaying native encode
            if let Some(start) = native_encode_pos.take() {
                res.as_mut().unwrap().extend(encode_uri_component(&component[start..pos]));
            }
            // check if we write into a new string (by default we try to return the param)
            if let Some(res) = res.as_mut() {
                res.push(code);
            }
        } else {
            // encoding needed, we need to allocate a new string
            let res = res.get_or_insert_with(|| component[..pos].to_vec());

            // check with default table first
            if let Some(escaped) = encode_table(code) {
                // check if we are delaying native encode
                if let Some(start) = native_encode_pos.take() {
                    res.extend(encode_uri_component(&component[start..pos]));
                }
                // append escaped variant to result
                push_ascii(res, escaped);
            } else if native_encode_pos.is_none() {
                // use native encode only when needed
                native_encode_pos = Some(pos);
            }
        }
    }

    if let Some(start) = native_encode_pos {
        res.as_mut().unwrap().extend(encode_uri_component(&component[start..]));
    }

    res.unwrap_or_else(|| component.to_vec())
}

/// `encodeURIComponentMinimal`: só `?` e `#` (o resto fica como está, inclusive surrogates isolados).
fn encode_uri_component_minimal(path: &[u16]) -> Vec<u16> {
    let first = |unit: u16| index_of_unit(path, unit, 0);
    let (question, hash) = (first(cc::QuestionMark), first(cc::Hash));
    let pos = match (question, hash) {
        (None, other) | (other, None) => other,
        (Some(q), Some(h)) => Some(q.min(h)),
    };
    let Some(pos) = pos else {
        return path.to_vec();
    };

    let mut res = path[..pos].to_vec();
    for &code in &path[pos..] {
        match code {
            cc::Hash => push_ascii(&mut res, "%23"),
            cc::QuestionMark => push_ascii(&mut res, "%3F"),
            other => res.push(other),
        }
    }
    res
}

/// `_asFormatted`: a versão externa de um URI.
fn as_formatted(uri: &URI, skip_encoding: bool) -> JsString {
    let encoder: fn(&[u16], bool, bool) -> Vec<u16> =
        if skip_encoding { |c, _, _| encode_uri_component_minimal(c) } else { encode_uri_component_fast };

    let mut res: Vec<u16> = Vec::new();
    let scheme = uri.scheme();
    let mut authority = uri.authority().units();
    let mut path = uri.path().units().to_vec();
    let (query, fragment) = (uri.query().units(), uri.fragment().units());

    if !scheme.is_empty() {
        res.extend_from_slice(scheme.units());
        res.push(cc::Colon);
    }
    if !authority.is_empty() || scheme == "file" {
        res.push(cc::Slash);
        res.push(cc::Slash);
    }
    let lowered_authority;
    if !authority.is_empty() {
        if let Some(idx) = index_of_unit(authority, cc::AtSign, 0) {
            // <user>@<auth>
            let userinfo = &authority[..idx];
            authority = &authority[idx + 1..];
            match index_of_unit(userinfo, cc::Colon, 0) {
                None => res.extend(encoder(userinfo, false, false)),
                Some(colon) => {
                    // <user>:<pass>@<auth>
                    res.extend(encoder(&userinfo[..colon], false, false));
                    res.push(cc::Colon);
                    res.extend(encoder(&userinfo[colon + 1..], false, false));
                }
            }
            res.push(cc::AtSign);
        }
        lowered_authority = JsString::from(authority).to_lower_case();
        let authority = lowered_authority.units();
        match authority.iter().rposition(|&u| u == cc::Colon) {
            None => res.extend(encoder(authority, false, true)),
            Some(idx) => {
                // <auth>:<port>
                res.extend(encoder(&authority[..idx], false, true));
                res.extend_from_slice(&authority[idx..]);
            }
        }
    }
    if !path.is_empty() {
        // HTTP paths are case-sensitive, even when their first segment resembles a drive letter.
        let lower_scheme = scheme.to_lower_case();
        let is_http = lower_scheme == "http" || lower_scheme == "https";
        if path.len() >= 3 && path[0] == cc::Slash && path[2] == cc::Colon {
            let code = path[1];
            if (cc::A..=cc::Z).contains(&code) && !is_http {
                path[1] = code + 32; // "/c:".length === 3
            }
        } else if path.len() >= 2 && path[1] == cc::Colon {
            let code = path[0];
            if (cc::A..=cc::Z).contains(&code) && !is_http {
                path[0] = code + 32;
            }
        }
        // encode the rest of the path
        res.extend(encoder(&path, true, false));
    }
    if !query.is_empty() {
        res.push(cc::QuestionMark);
        res.extend(encoder(query, false, false));
    }
    if !fragment.is_empty() {
        res.push(cc::Hash);
        if skip_encoding {
            res.extend_from_slice(fragment);
        } else {
            res.extend(encode_uri_component_fast(fragment, false, false));
        }
    }
    JsString::from_units(res)
}

// ---- decodificação ---------------------------------------------------------------------------------

fn hex_value(unit: u16) -> Option<u8> {
    char::from_u32(u32::from(unit)).and_then(|c| c.to_digit(16)).map(|d| d as u8)
}

/// `decodeURIComponent` do JavaScript: `Err` no lugar do `URIError` (sequência `%` malformada ou UTF-8 inválido).
pub fn decode_uri_component(input: &[u16]) -> Result<Vec<u16>, Error> {
    let malformed = || Error::named("URIError", "URI malformed");
    let byte_at = |k: usize| -> Option<u8> {
        if input.get(k) != Some(&cc::PercentSign) {
            return None;
        }
        Some(hex_value(*input.get(k + 1)?)? * 16 + hex_value(*input.get(k + 2)?)?)
    };

    let mut out = Vec::with_capacity(input.len());
    let mut k = 0;
    while k < input.len() {
        if input[k] != cc::PercentSign {
            out.push(input[k]);
            k += 1;
            continue;
        }
        let first = byte_at(k).ok_or_else(malformed)?;
        if first < 0x80 {
            out.push(u16::from(first));
            k += 3;
            continue;
        }
        // O número de bytes vem dos bits altos do primeiro byte.
        let n = first.leading_ones() as usize;
        if !(2..=4).contains(&n) {
            return Err(malformed());
        }
        let mut bytes = vec![first];
        for i in 1..n {
            bytes.push(byte_at(k + 3 * i).ok_or_else(malformed)?);
        }
        let text = std::str::from_utf8(&bytes).map_err(|_| malformed())?;
        out.extend(text.encode_utf16());
        k += 3 * n;
    }
    Ok(out)
}

/// `decodeURIComponentGraceful`: onde a decodificação falha, mantém os 3 primeiros caracteres e tenta o resto.
fn decode_uri_component_graceful(input: &[u16]) -> Vec<u16> {
    match decode_uri_component(input) {
        Ok(decoded) => decoded,
        Err(_) if input.len() > 3 => {
            let mut out = input[..3].to_vec();
            out.extend(decode_uri_component_graceful(&input[3..]));
            out
        }
        Err(_) => input.to_vec(),
    }
}

/// `percentDecode`: decodifica cada trecho que casa `(%[0-9A-Za-z][0-9A-Za-z])+`.
fn percent_decode(input: &[u16]) -> JsString {
    let is_alnum = |u: u16| u < 128 && (u as u8).is_ascii_alphanumeric();
    let is_escape_at = |k: usize| {
        input.get(k) == Some(&cc::PercentSign) && input.get(k + 1).is_some_and(|&u| is_alnum(u)) && input.get(k + 2).is_some_and(|&u| is_alnum(u))
    };

    let mut out = Vec::with_capacity(input.len());
    let mut k = 0;
    while k < input.len() {
        if is_escape_at(k) {
            let start = k;
            while is_escape_at(k) {
                k += 3;
            }
            out.extend(decode_uri_component_graceful(&input[start..k]));
        } else {
            out.push(input[k]);
            k += 1;
        }
    }
    JsString::from_units(out)
}
