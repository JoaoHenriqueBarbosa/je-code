//! Porte de `src/vs/platform/remoteTunnel/common/tunnelMachineStatus.ts`: o parser das linhas de status
//! que a CLI do túnel emite.
//!
//! Divergências:
//! - A união discriminada é o enum [`TunnelMachineStatus`]; os campos opcionais (`undefined`) são `Option`.
//! - `JSON.parse` é o `je_base::json::parse` sem comentários e sem vírgula final, descartando o resultado se
//!   houver qualquer erro de sintaxe, que é o que o `JSON.parse` aceita. As strings saem como `String`
//!   (conversão com perda só para surrogates isolados, que a CLI não emite).
//! - `value.tunnelId !== undefined && !isString(...)` vira `Option<&Value>`: `null` conta como presente e
//!   não é string, então invalida a linha, como no original.

use je_base::json::{self, Object, ParseOptions, Value};
use je_base::js_string::starts_with_at;
use je_base::strings::remove_ansi_escape_codes;

pub const TUNNEL_MACHINE_STATUS_PREFIX: &str = "__VSCODE_CLI_STATUS__";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IConnectedTunnelMachineStatus {
    pub tunnel_name: String,
    pub tunnel_id: Option<String>,
    pub is_attached: bool,
    pub link: Option<String>,
    pub domain: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITokenErrorTunnelMachineStatus {
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TunnelMachineStatus {
    Connected(IConnectedTunnelMachineStatus),
    TokenError(ITokenErrorTunnelMachineStatus),
}

impl TunnelMachineStatus {
    /// O campo `type` da união do original.
    pub fn type_(&self) -> &'static str {
        match self {
            TunnelMachineStatus::Connected(_) => "connected",
            TunnelMachineStatus::TokenError(_) => "tokenError",
        }
    }
}

/// Parses a machine-status line emitted by the tunnel CLI.
pub fn parse_tunnel_machine_status(message: &str) -> Option<TunnelMachineStatus> {
    let message_units: Vec<u16> = message.encode_utf16().collect();
    let cleaned_message = remove_ansi_escape_codes(&message_units);
    let prefix: Vec<u16> = TUNNEL_MACHINE_STATUS_PREFIX.encode_utf16().collect();
    if !starts_with_at(cleaned_message.units(), &prefix, 0) {
        return None;
    }

    let mut errors = Vec::new();
    let options = ParseOptions { disallow_comments: true, allow_trailing_comma: false, allow_empty_content: false };
    let value = json::parse(&cleaned_message.units()[prefix.len()..], &mut errors, options)?;
    if !errors.is_empty() {
        return None;
    }

    let Value::Object(value) = value else {
        return None;
    };
    let Some(type_) = string_field(&value, "type") else {
        return None;
    };

    if type_ == "connected" {
        let tunnel_name = string_field(&value, "tunnelName")?;
        let Some(Value::Bool(is_attached)) = value.get("isAttached") else {
            return None;
        };
        let tunnel_id = optional_string_field(&value, "tunnelId")?;
        let link = optional_string_field(&value, "link")?;
        let domain = optional_string_field(&value, "domain")?;
        if link.is_none() != domain.is_none() {
            return None;
        }
        return Some(TunnelMachineStatus::Connected(IConnectedTunnelMachineStatus {
            tunnel_name,
            tunnel_id,
            is_attached: *is_attached,
            link,
            domain,
        }));
    }

    if type_ == "tokenError" {
        let message = string_field(&value, "message")?;
        return Some(TunnelMachineStatus::TokenError(ITokenErrorTunnelMachineStatus { message }));
    }

    None
}

/// O campo quando existe e é string (`isString(value.key)`).
fn string_field(value: &Object, key: &str) -> Option<String> {
    match value.get(key) {
        Some(Value::String(text)) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `Some(None)` se o campo é `undefined`, `Some(Some(texto))` se é string, `None` (linha inválida) se existe
/// e não é string.
fn optional_string_field(value: &Object, key: &str) -> Option<Option<String>> {
    match value.get(key) {
        None => Some(None),
        Some(Value::String(text)) => Some(Some(text.to_string_lossy())),
        Some(_) => None,
    }
}
