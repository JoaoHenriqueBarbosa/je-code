//! Porte de `src/vs/server/node/serverConnectionToken.ts`: o token que toda requisição ao servidor precisa trazer
//! (`--connection-token`, `--connection-token-file`, `--without-connection-token`, ou um gerado e guardado em
//! `<user-data-dir>/token`).
//!
//! Divergências:
//! - `ServerConnectionToken | ServerConnectionTokenParseError` é um `Result` dentro da promessa
//!   ([`ParsedServerConnectionToken`]), e a união `None | Mandatory` é o enum [`ServerConnectionToken`].
//! - `validate(connectionToken: unknown)` recebe `Option<&str>`: só uma string igual ao valor valida, e o `unknown` do
//!   original que não é string (inclusive a lista de valores repetidos) é `None`.
//! - `req: Pick<http.IncomingMessage, 'headers'>` é o mapa de cabeçalhos com os nomes em minúsculas, como o Node os entrega;
//!   só o `cookie` é lido.
//! - `fs.promises.readFile` é a leitura do `std::fs` dentro de `async_fn`, e o `Promises.writeFile` é o `pfs::write_file`.

use std::collections::HashMap;

use je_base::async_::async_fn;
use je_base::network::{CONNECTION_TOKEN_COOKIE_NAME, CONNECTION_TOKEN_QUERY_NAME};
use je_base::oauth::UrlSearchParams;
use je_base::promise::Promise;
use je_base::uuid::generate_uuid;
use je_platform::files::pfs::{self, WriteFileOptions};

use crate::cookie;
use crate::server_environment_service::ServerParsedArgs;

/// `/^[0-9A-Za-z_-]+$/.test(token)`.
fn is_valid_connection_token(token: &str) -> bool {
    !token.is_empty() && token.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// `ServerConnectionTokenType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerConnectionTokenType {
    None,
    Optional, // TODO: Remove this soon
    Mandatory,
}

/// `NoneServerConnectionToken`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NoneServerConnectionToken;

impl NoneServerConnectionToken {
    pub const TYPE: ServerConnectionTokenType = ServerConnectionTokenType::None;

    pub fn validate(&self, _connection_token: Option<&str>) -> bool {
        true
    }
}

/// `MandatoryServerConnectionToken`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MandatoryServerConnectionToken {
    pub value: String,
}

impl MandatoryServerConnectionToken {
    pub const TYPE: ServerConnectionTokenType = ServerConnectionTokenType::Mandatory;

    pub fn new(value: impl Into<String>) -> Self {
        Self { value: value.into() }
    }

    pub fn validate(&self, connection_token: Option<&str>) -> bool {
        connection_token == Some(self.value.as_str())
    }
}

/// `ServerConnectionToken`: `NoneServerConnectionToken | MandatoryServerConnectionToken`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ServerConnectionToken {
    None(NoneServerConnectionToken),
    Mandatory(MandatoryServerConnectionToken),
}

impl ServerConnectionToken {
    /// `type`.
    pub fn token_type(&self) -> ServerConnectionTokenType {
        match self {
            ServerConnectionToken::None(_) => NoneServerConnectionToken::TYPE,
            ServerConnectionToken::Mandatory(_) => MandatoryServerConnectionToken::TYPE,
        }
    }

    pub fn validate(&self, connection_token: Option<&str>) -> bool {
        match self {
            ServerConnectionToken::None(token) => token.validate(connection_token),
            ServerConnectionToken::Mandatory(token) => token.validate(connection_token),
        }
    }
}

/// `ServerConnectionTokenParseError`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerConnectionTokenParseError {
    pub message: String,
}

impl ServerConnectionTokenParseError {
    pub fn new(message: impl Into<String>) -> Self {
        Self { message: message.into() }
    }
}

/// `ServerConnectionToken | ServerConnectionTokenParseError`.
pub type ParsedServerConnectionToken = Result<ServerConnectionToken, ServerConnectionTokenParseError>;

/// `replace(/\r?\n$/, '')`: tira uma quebra de linha (LF ou CRLF) do fim.
fn strip_trailing_newline(text: &str) -> &str {
    match text.strip_suffix('\n') {
        Some(rest) => rest.strip_suffix('\r').unwrap_or(rest),
        None => text,
    }
}

/// `parseServerConnectionToken(args, defaultValue)`.
pub fn parse_server_connection_token(args: &ServerParsedArgs, default_value: impl FnOnce() -> Promise<String> + 'static) -> Promise<ParsedServerConnectionToken> {
    let without_connection_token = args.get_bool("without-connection-token");
    let connection_token = args.get_string("connection-token");
    let connection_token_file = args.get_string("connection-token-file");

    async_fn(async move {
        if without_connection_token {
            if connection_token.is_some() || connection_token_file.is_some() {
                return Ok(Err(ServerConnectionTokenParseError::new(
                    "Please do not use the argument '--connection-token' or '--connection-token-file' at the same time as '--without-connection-token'.",
                )));
            }
            return Ok(Ok(ServerConnectionToken::None(NoneServerConnectionToken)));
        }

        if let Some(connection_token_file) = connection_token_file {
            if connection_token.is_some() {
                return Ok(Err(ServerConnectionTokenParseError::new(
                    "Please do not use the argument '--connection-token' at the same time as '--connection-token-file'.",
                )));
            }

            let raw_connection_token = match std::fs::read_to_string(&connection_token_file) {
                Ok(contents) => strip_trailing_newline(&contents).to_owned(),
                Err(_) => {
                    return Ok(Err(ServerConnectionTokenParseError::new(format!("Unable to read the connection token file at '{connection_token_file}'."))));
                }
            };

            if !is_valid_connection_token(&raw_connection_token) {
                return Ok(Err(ServerConnectionTokenParseError::new(format!(
                    "The connection token defined in '{connection_token_file} does not adhere to the characters 0-9, a-z, A-Z, _, or -."
                ))));
            }

            return Ok(Ok(ServerConnectionToken::Mandatory(MandatoryServerConnectionToken::new(raw_connection_token))));
        }

        if let Some(connection_token) = connection_token {
            if !is_valid_connection_token(&connection_token) {
                return Ok(Err(ServerConnectionTokenParseError::new(format!(
                    "The connection token '{connection_token} does not adhere to the characters 0-9, a-z, A-Z or -."
                ))));
            }

            return Ok(Ok(ServerConnectionToken::Mandatory(MandatoryServerConnectionToken::new(connection_token))));
        }

        Ok(Ok(ServerConnectionToken::Mandatory(MandatoryServerConnectionToken::new(default_value().await?))))
    })
}

/// `determineServerConnectionToken(args)`.
pub fn determine_server_connection_token(args: &ServerParsedArgs) -> Promise<ParsedServerConnectionToken> {
    let user_data_dir = args.get_string("user-data-dir").filter(|dir| !dir.is_empty());
    let read_or_generate_connection_token = move || {
        async_fn(async move {
            let Some(user_data_dir) = user_data_dir else {
                // No place to store it!
                return Ok(generate_uuid());
            };
            let storage_location = je_base::path::PathModule::native(&je_base::path::ProcessEnvironment).join(&[&user_data_dir, "token"]);

            // First try to find a connection token
            if let Ok(file_contents) = std::fs::read(&storage_location) {
                let connection_token = strip_trailing_newline(&String::from_utf8_lossy(&file_contents)).to_owned();
                if is_valid_connection_token(&connection_token) {
                    return Ok(connection_token);
                }
            }

            // No connection token found, generate one
            let connection_token = generate_uuid();

            // Try to store it
            let _ = pfs::write_file(&storage_location, connection_token.clone().into_bytes(), Some(WriteFileOptions { mode: Some(0o600), flag: None })).await;

            Ok(connection_token)
        })
    };
    parse_server_connection_token(args, read_or_generate_connection_token)
}

/// `requestHasValidConnectionToken(connectionToken, req, searchParams)`.
pub fn request_has_valid_connection_token(connection_token: &ServerConnectionToken, headers: &HashMap<String, String>, search_params: &UrlSearchParams) -> bool {
    // First check if there is a valid query parameter
    let query_tokens = search_params.get_all(CONNECTION_TOKEN_QUERY_NAME);
    if connection_token.validate(if query_tokens.len() > 1 { None } else { query_tokens.first().copied() }) {
        return true;
    }

    // Otherwise, check if there is a valid cookie
    let cookies = cookie::parse(headers.get("cookie").map_or("", String::as_str));
    connection_token.validate(cookie::get(&cookies, CONNECTION_TOKEN_COOKIE_NAME))
}
