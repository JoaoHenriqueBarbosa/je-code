//! `require("url")` do bun 1.4.2: o `url.ts` em JavaScript (`node_url.js`, gerado por `scripts/gen-node-url.js`) e os
//! três bindings do `NodeURL.cpp` que ele recebe pelo `host` (`domainToAscii`, `domainToUnicode`, `idnaToASCII`).
//!
//! O bun usa o UTS #46 do ICU (`uidna_openUTS46`): no ToASCII, `CHECK_BIDI | CHECK_CONTEXTJ | NONTRANSITIONAL_TO_ASCII`
//! sem `USE_STD3_RULES`, e os erros de hífen e de tamanho de rótulo e de domínio filtrados depois. É o que o `idna`
//! faz com `AsciiDenyList::EMPTY`, `Hyphens::Allow` e `DnsLength::Ignore` (o mesmo de `url_host::domain_to_ascii`).

use idna::uts46::{AsciiDenyList, DnsLength, Hyphens, Uts46};

use crate::api::node_js_host::{module_host, run_factory};
use crate::host_function;
use crate::runtime::host_call::{pending_or, HostCall, HostResult};
use crate::runtime::js_global_object::JSGlobalObject;
use crate::runtime::js_string::js_string;
use crate::runtime::js_value::JSValue;
use crate::runtime::node_error::throw_coded_type_error;
use crate::runtime::url::{parse_with_base, wtf_units};
use crate::wtf::text::wtf_string::String as WtfString;

const SOURCE: &str = include_str!("node_url.js");

/// `icuToASCII` em modo `Default`: o domínio só ASCII e sem rótulo `xn--` só vai para minúsculas; o resto passa pelo
/// UTS #46, e qualquer erro que sobra ao filtro dá `None`.
fn icu_to_ascii(input: &[u16]) -> Option<String> {
    if input.iter().all(|&unit| unit < 0x80) {
        let lowered: String = input.iter().map(|&unit| (unit as u8).to_ascii_lowercase() as char).collect();
        if !lowered.contains("xn--") {
            return Some(lowered);
        }
    }
    let domain = String::from_utf16(input).ok()?;
    Uts46::new().to_ascii(domain.as_bytes(), AsciiDenyList::EMPTY, Hyphens::Allow, DnsLength::Ignore).ok().map(|ascii| ascii.into_owned())
}

/// `icuToUnicode`: o ToUnicode do UTS #46 sempre produz saída; os erros ficam marcados no texto e são ignorados.
fn icu_to_unicode(input: &[u16]) -> String {
    let domain = String::from_utf16_lossy(input);
    Uts46::new().to_unicode(domain.as_bytes(), AsciiDenyList::EMPTY, Hyphens::Allow).0.into_owned()
}

fn contains_units(haystack: &[u16], needle: &str) -> bool {
    let needle: Vec<u16> = needle.encode_utf16().collect();
    haystack.windows(needle.len()).any(|window| window == needle.as_slice())
}

/// `parseDomainAsHost`: o `url.domainToASCII` do node passa a entrada pelo setter `hostname` de uma URL `ws://x`; o
/// setter para no primeiro `/`, `?`, `#` ou `\`, recusa porta e `@` fora de colchetes e, entre colchetes, qualquer coisa
/// depois do `]`. `None` quando o host não se analisa ou tem rótulo punycode inválido.
fn parse_domain_as_host(domain: &[u16]) -> Option<Vec<u16>> {
    let end = domain.iter().position(|&unit| matches!(unit, 0x2f | 0x3f | 0x23 | 0x5c)).unwrap_or(domain.len());
    let host = &domain[..end];
    if host.is_empty() {
        return None;
    }
    if host[0] == u16::from(b'[') {
        if host.last() != Some(&u16::from(b']')) {
            return None;
        }
    } else if host.iter().any(|&unit| unit == u16::from(b':') || unit == u16::from(b'@')) {
        return None;
    }
    let mut input: Vec<u16> = "ws://".encode_utf16().collect();
    input.extend_from_slice(host);
    input.push(u16::from(b'/'));
    let url = parse_with_base(&input, None)?;
    if !url.is_valid() {
        return None;
    }
    let parsed = wtf_units(&url.host());
    // `hasValidPunycodeHost`.
    if contains_units(&parsed, "xn--") && icu_to_ascii(&parsed).is_none() {
        return None;
    }
    Some(parsed)
}

fn text_value(global_object: &JSGlobalObject, units: &[u16]) -> JSValue {
    JSValue::from_js_string(js_string(global_object.vm(), &WtfString::from_utf16(units)))
}

/// O argumento `domain` como texto (o node faz `${domain}`: `undefined` vira o domínio "undefined"); sem argumento,
/// `ERR_MISSING_ARGS`.
fn domain_argument(global_object: &JSGlobalObject, call: &HostCall) -> Result<Vec<u16>, crate::runtime::host_call::Thrown> {
    if call.argument_count() < 1 {
        return Err(throw_coded_type_error(global_object, "The \"domain\" argument must be specified", "ERR_MISSING_ARGS"));
    }
    Ok(wtf_units(&pending_or(global_object, call.argument(0).to_wtf_string())?))
}

/// `url.domainToASCII(domain)`: o host analisado, ou `""`.
fn domain_to_ascii_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let domain = domain_argument(global_object, call)?;
    Ok(text_value(global_object, &parse_domain_as_host(&domain).unwrap_or_default()))
}
host_function!(domain_to_ascii, domain_to_ascii_body);

/// `url.domainToUnicode(domain)`: o host analisado passado pelo ToUnicode, ou `""`.
fn domain_to_unicode_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let domain = domain_argument(global_object, call)?;
    let unicode: Vec<u16> = parse_domain_as_host(&domain).map(|host| icu_to_unicode(&host).encode_utf16().collect()).unwrap_or_default();
    Ok(text_value(global_object, &unicode))
}
host_function!(domain_to_unicode, domain_to_unicode_body);

/// `idnaToASCII(texto)`: o ToASCII do `url.parse`, sem análise de host (sem IPv4 canônico, `:` aceito); `""` na falha.
fn idna_to_ascii_body(global_object: &JSGlobalObject, call: &HostCall) -> HostResult {
    let input = wtf_units(&pending_or(global_object, call.argument(0).to_wtf_string())?);
    let ascii: Vec<u16> = icu_to_ascii(&input).map(|ascii| ascii.encode_utf16().collect()).unwrap_or_default();
    Ok(text_value(global_object, &ascii))
}
host_function!(idna_to_ascii, idna_to_ascii_body);

/// Instalador de `node:url`.
pub(crate) fn install_url_module(global_object: &JSGlobalObject) -> HostResult {
    let host = module_host(global_object, &[("domainToAscii", 1, domain_to_ascii), ("domainToUnicode", 1, domain_to_unicode), ("idnaToASCII", 1, idna_to_ascii)])?;
    Ok(run_factory(global_object, SOURCE, "url", &host))
}
