//! Porte de `src/vs/workbench/contrib/webview/common/webview.ts`: a URI que carrega um recurso dentro de um webview
//! (`asWebviewUri`) e a conta inversa da autoridade (`decodeAuthority`).
//!
//! Notas de tradução:
//! - `encodeAuthority` e `decodeAuthority` trabalham sobre as unidades UTF-16 do [`JsString`], como as expressões
//!   regulares sem a bandeira `u` do original: o `.` de `/./g` casa cada unidade, menos os terminadores de linha
//!   (`\n`, `\r`, ` ` e ` `), que passam sem codificação; `/-([0-9a-f]{4})/g` é a varredura que procura `-` e
//!   quatro dígitos hexadecimais minúsculos.
//! - `WebviewRemoteInfo.authority` (`string | undefined`) é `Option<String>`.

use je_base::js_string::JsString;
use je_base::network::schemas;
use je_base::uri::{URI, UriComponents};

/// `WebviewRemoteInfo`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WebviewRemoteInfo {
    pub is_remote: bool,
    pub authority: Option<String>,
}

/// Root from which resources in webviews are loaded.
///
/// This is hardcoded because we never expect to actually hit it. Instead these requests
/// should always go to a service worker.
pub const WEBVIEW_RESOURCE_BASE_HOST: &str = "vscode-cdn.net";

/// `webviewRootResourceAuthority`.
pub const WEBVIEW_ROOT_RESOURCE_AUTHORITY: &str = "vscode-resource.vscode-cdn.net";

/// `webviewGenericCspSource`.
pub const WEBVIEW_GENERIC_CSP_SOURCE: &str = "'self' https://*.vscode-cdn.net";

/// Construct a uri that can load resources inside a webview
///
/// We encode the resource component of the uri so that on the main thread
/// we know where to load the resource from (remote or truly local):
///
/// ```txt
/// ${scheme}+${resource-authority}.vscode-resource.vscode-cdn.net/${path}
/// ```
///
/// `resource`: Uri of the resource to load. `remote_info`: Optional information about the remote that specifies where
/// `resource` should be resolved from.
pub fn as_webview_uri(resource: &URI, remote_info: Option<&WebviewRemoteInfo>) -> URI {
    if resource.scheme() == schemas::HTTP || resource.scheme() == schemas::HTTPS {
        return resource.clone();
    }

    let mut resource = resource.clone();
    if let Some(remote_info) = remote_info
        && let Some(authority) = remote_info.authority.as_ref().filter(|authority| !authority.is_empty())
        && remote_info.is_remote
        && resource.scheme() == schemas::FILE
    {
        resource = URI::from(
            &UriComponents {
                scheme: Some(JsString::from(schemas::VSCODE_REMOTE)),
                authority: Some(JsString::from(authority.as_str())),
                path: Some(resource.path().clone()),
                ..UriComponents::default()
            },
            false,
        );
    }

    let mut authority = resource.scheme().clone();
    authority.push_units(&[u16::from(b'+')]);
    authority.push_units(encode_authority(resource.authority()).units());
    authority.push_units(&[u16::from(b'.')]);
    authority.push_units(JsString::from(WEBVIEW_ROOT_RESOURCE_AUTHORITY).units());

    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(schemas::HTTPS)),
            authority: Some(authority),
            path: Some(resource.path().clone()),
            fragment: Some(resource.fragment().clone()),
            query: Some(resource.query().clone()),
        },
        false,
    )
}

/// `encodeAuthority(authority)`: letras e dígitos ASCII ficam, o resto vira `-` e o código em quatro dígitos
/// hexadecimais.
fn encode_authority(authority: &JsString) -> JsString {
    let mut encoded: Vec<u16> = Vec::with_capacity(authority.len());
    for &unit in authority.units() {
        let is_line_terminator = matches!(unit, 0x0A | 0x0D | 0x2028 | 0x2029);
        let is_alphanumeric = u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_alphanumeric());
        if is_line_terminator || is_alphanumeric {
            encoded.push(unit);
        } else {
            encoded.extend(format!("-{unit:04x}").encode_utf16());
        }
    }
    JsString::from_units(encoded)
}

/// `decodeAuthority(authority)`.
pub fn decode_authority(authority: &JsString) -> JsString {
    let units = authority.units();
    let is_lower_hex = |unit: u16| u8::try_from(unit).is_ok_and(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    let mut decoded: Vec<u16> = Vec::with_capacity(units.len());
    let mut index = 0;
    while index < units.len() {
        if units[index] == u16::from(b'-') && index + 4 < units.len() && units[index + 1..index + 5].iter().all(|&unit| is_lower_hex(unit)) {
            let code = units[index + 1..index + 5].iter().fold(0u16, |code, &unit| {
                let digit = (unit as u8 as char).to_digit(16).unwrap_or(0) as u16;
                code * 16 + digit
            });
            decoded.push(code);
            index += 5;
        } else {
            decoded.push(units[index]);
            index += 1;
        }
    }
    JsString::from_units(decoded)
}
