//! Porte parcial de `src/vs/workbench/services/authentication/common/authentication.ts`: só o tipo
//! [`AllowedExtension`], que o `AuthenticationAccessService` lê e grava.
//!
//! Divergências:
//! - **O que falta de `authentication.ts`** (nada disso é usado pelo serviço de acesso e entra quando o
//!   `AuthenticationService` for portado): `INTERNAL_AUTH_PROVIDER_PREFIX`, `AuthenticationSessionAccount`,
//!   `AuthenticationSession`, `AuthenticationSessionsChangeEvent`, `AuthenticationProviderInformation`,
//!   `IAuthenticationCreateSessionOptions`, `IAuthenticationProviderSessionOptions`,
//!   `IAuthenticationConstraint`, `IAuthenticationWwwAuthenticateRequest`,
//!   `IAuthenticationGetSessionsOptions`, `IAuthenticationService`, `IAuthenticationExtensionsService`,
//!   `IAuthenticationProvider`, `IAuthenticationProviderCreateSessionOptions`,
//!   `isAuthenticationSession`, `isAuthenticationWwwAuthenticateRequest` e as funções auxiliares de
//!   `www-authenticate` que o arquivo carrega.
//! - **JSON.** No original o objeto lido do armazenamento (`JSON.parse`) é guardado como veio, com qualquer
//!   propriedade a mais. Aqui só os cinco campos declarados sobrevivem ([`AllowedExtension::from_json`]),
//!   e [`AllowedExtension::to_json`] escreve na ordem `id`, `name`, `allowed`, `lastUsed`, `trusted`, sem os
//!   campos ausentes (o `JSON.stringify` omite `undefined`). Uma entrada que não é objeto ou não tem `id`
//!   textual é descartada (no original ela ficaria na lista, sem nunca casar com uma extensão); um `name`
//!   ausente vira texto vazio, que é falso como o `undefined` nas conferências do serviço.

use je_base::JsString;
use je_base::json::{Object, Value};

/// `AllowedExtension`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AllowedExtension {
    pub id: String,
    pub name: String,
    /// If true or undefined, the extension is allowed to use the account. If false, the extension is not
    /// allowed to use the account. (`undefined` não deveria ser um valor válido, mas é, por enquanto.)
    pub allowed: Option<bool>,
    pub last_used: Option<f64>,
    /// If true, this comes from the product.json.
    pub trusted: Option<bool>,
}

impl AllowedExtension {
    /// O objeto do `JSON.parse` como [`AllowedExtension`]; `None` se não é objeto ou não tem `id` textual.
    pub fn from_json(value: &Value) -> Option<Self> {
        let Value::Object(object) = value else { return None };
        let Some(Value::String(id)) = object.get("id") else { return None };
        Some(Self {
            id: id.to_string_lossy(),
            name: match object.get("name") {
                Some(Value::String(name)) => name.to_string_lossy(),
                _ => String::new(),
            },
            allowed: match object.get("allowed") {
                Some(Value::Bool(allowed)) => Some(*allowed),
                _ => None,
            },
            last_used: match object.get("lastUsed") {
                Some(Value::Number(last_used)) => Some(*last_used),
                _ => None,
            },
            trusted: match object.get("trusted") {
                Some(Value::Bool(trusted)) => Some(*trusted),
                _ => None,
            },
        })
    }

    /// O objeto que o `JSON.stringify` escreveria.
    pub fn to_json(&self) -> Value {
        let mut object = Object::new();
        object.insert(JsString::from("id"), Value::String(JsString::from(self.id.as_str())));
        object.insert(JsString::from("name"), Value::String(JsString::from(self.name.as_str())));
        if let Some(allowed) = self.allowed {
            object.insert(JsString::from("allowed"), Value::Bool(allowed));
        }
        if let Some(last_used) = self.last_used {
            object.insert(JsString::from("lastUsed"), Value::Number(last_used));
        }
        if let Some(trusted) = self.trusted {
            object.insert(JsString::from("trusted"), Value::Bool(trusted));
        }
        Value::Object(object)
    }
}
