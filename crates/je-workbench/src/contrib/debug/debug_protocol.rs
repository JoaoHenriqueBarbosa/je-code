//! Porte do que o workbench usa de `src/vs/workbench/contrib/debug/common/debugProtocol.d.ts` (o `namespace DebugProtocol`,
//! gerado do esquema JSON do protocolo): as mensagens-base (`ProtocolMessage`, `Request`, `Response`, `Event`) e a
//! `Source` com a `Checksum`.
//!
//! Notas de tradução:
//! - `ProtocolMessage` é uma união no original (`type: 'request' | 'response' | 'event' | string`, com `seq`); aqui é o enum
//!   [`ProtocolMessage`] cuja variante carrega o `type` e os campos da mensagem, e [`ProtocolMessage::Other`] guarda uma
//!   mensagem de `type` desconhecido (o `| string` do original), que o `AbstractDebugAdapter` ignora ao despachar.
//! - Os campos `any` (`arguments`, `body`, `adapterData`) são [`serde_json::Value`]; os opcionais (`?`) são `Option`.
//! - `sourceReference` e `seq` são inteiros no protocolo e viram `i64`.

use serde_json::Value;

/// `ProtocolMessage`: base de requisições, respostas e eventos. O `type` é a variante e o `seq` é o campo de cada uma.
#[derive(Clone, Debug, PartialEq)]
pub enum ProtocolMessage {
    Request(Request),
    Response(Response),
    Event(Event),
    /// Uma mensagem de `type` fora de `request`, `response` e `event`.
    Other(OtherMessage),
}

impl ProtocolMessage {
    /// `seq`: o número de sequência da mensagem.
    pub fn seq(&self) -> i64 {
        match self {
            ProtocolMessage::Request(message) => message.seq,
            ProtocolMessage::Response(message) => message.seq,
            ProtocolMessage::Event(message) => message.seq,
            ProtocolMessage::Other(message) => message.seq,
        }
    }

    /// `message.seq = seq`.
    pub fn set_seq(&mut self, seq: i64) {
        match self {
            ProtocolMessage::Request(message) => message.seq = seq,
            ProtocolMessage::Response(message) => message.seq = seq,
            ProtocolMessage::Event(message) => message.seq = seq,
            ProtocolMessage::Other(message) => message.seq = seq,
        }
    }

    /// `type`: `request`, `response`, `event` ou o texto da mensagem desconhecida.
    pub fn message_type(&self) -> &str {
        match self {
            ProtocolMessage::Request(_) => "request",
            ProtocolMessage::Response(_) => "response",
            ProtocolMessage::Event(_) => "event",
            ProtocolMessage::Other(message) => &message.message_type,
        }
    }

    /// O objeto JSON que o `JSON.stringify(message)` do original serializa (campos ausentes não aparecem).
    pub fn to_json(&self) -> Value {
        let mut object = serde_json::Map::new();
        object.insert("seq".into(), Value::from(self.seq()));
        object.insert("type".into(), Value::from(self.message_type()));
        match self {
            ProtocolMessage::Request(message) => {
                object.insert("command".into(), Value::from(message.command.clone()));
                if let Some(arguments) = &message.arguments {
                    object.insert("arguments".into(), arguments.clone());
                }
            }
            ProtocolMessage::Response(message) => {
                object.insert("request_seq".into(), Value::from(message.request_seq));
                object.insert("success".into(), Value::from(message.success));
                object.insert("command".into(), Value::from(message.command.clone()));
                if let Some(text) = &message.message {
                    object.insert("message".into(), Value::from(text.clone()));
                }
                if let Some(body) = &message.body {
                    object.insert("body".into(), body.clone());
                }
            }
            ProtocolMessage::Event(message) => {
                object.insert("event".into(), Value::from(message.event.clone()));
                if let Some(body) = &message.body {
                    object.insert("body".into(), body.clone());
                }
            }
            ProtocolMessage::Other(_) => {}
        }
        Value::Object(object)
    }

    /// `<DebugProtocol.ProtocolMessage>JSON.parse(text)`: a mensagem a partir do objeto JSON recebido do adaptador.
    pub fn from_json(value: &Value) -> ProtocolMessage {
        let seq = value.get("seq").and_then(Value::as_i64).unwrap_or(0);
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        match value.get("type").and_then(Value::as_str) {
            Some("request") => ProtocolMessage::Request(Request {
                seq,
                command: text("command").unwrap_or_default(),
                arguments: value.get("arguments").cloned(),
            }),
            Some("response") => ProtocolMessage::Response(Response {
                seq,
                request_seq: value.get("request_seq").and_then(Value::as_i64).unwrap_or(0),
                success: value.get("success").and_then(Value::as_bool).unwrap_or(false),
                command: text("command").unwrap_or_default(),
                message: text("message"),
                body: value.get("body").cloned(),
            }),
            Some("event") => ProtocolMessage::Event(Event { seq, event: text("event").unwrap_or_default(), body: value.get("body").cloned() }),
            other => ProtocolMessage::Other(OtherMessage { seq, message_type: other.unwrap_or_default().to_owned() }),
        }
    }
}

/// Uma `ProtocolMessage` de `type` desconhecido.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OtherMessage {
    pub seq: i64,
    pub message_type: String,
}

/// `Request`: a requisição que o cliente ou o adaptador inicia.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Request {
    pub seq: i64,
    /// The command to execute.
    pub command: String,
    /// Object containing arguments for the command.
    pub arguments: Option<Value>,
}

/// `Event`: o evento que o adaptador inicia.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Event {
    pub seq: i64,
    /// Type of event.
    pub event: String,
    /// Event-specific information.
    pub body: Option<Value>,
}

/// `Response`: a resposta de uma requisição.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Response {
    pub seq: i64,
    /// Sequence number of the corresponding request.
    pub request_seq: i64,
    /// Outcome of the request.
    pub success: bool,
    /// The command requested.
    pub command: String,
    /// Contains the raw error in short form if `success` is false.
    pub message: Option<String>,
    /// Contains request result if success is true and error details if success is false.
    pub body: Option<Value>,
}

/// `Source['presentationHint']`: a dica de como apresentar a fonte na UI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourcePresentationHint {
    Normal,
    Emphasize,
    Deemphasize,
}

/// `ChecksumAlgorithm`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChecksumAlgorithm {
    Md5,
    Sha1,
    Sha256,
    Timestamp,
}

/// `Checksum`: a soma de verificação de um item, pelo algoritmo dado.
#[derive(Clone, Debug, PartialEq)]
pub struct Checksum {
    /// The algorithm used to calculate this checksum.
    pub algorithm: ChecksumAlgorithm,
    /// Value of the checksum, encoded as a hexadecimal value.
    pub checksum: String,
}

/// `Source`: o descritor de um código-fonte.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Source {
    /// The short name of the source.
    pub name: Option<String>,
    /// The path of the source to be shown in the UI.
    pub path: Option<String>,
    /// If the value > 0 the contents of the source must be retrieved through the `source` request.
    pub source_reference: Option<i64>,
    /// A hint for how to present the source in the UI.
    pub presentation_hint: Option<SourcePresentationHint>,
    /// The origin of this source.
    pub origin: Option<String>,
    /// A list of sources that are related to this source.
    pub sources: Option<Vec<Source>>,
    /// Additional data that a debug adapter might want to loop through the client.
    pub adapter_data: Option<Value>,
    /// The checksums associated with this file.
    pub checksums: Option<Vec<Checksum>>,
}

/// `DataBreakpointAccessType`: `'read' | 'write' | 'readWrite'`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DataBreakpointAccessType {
    Read,
    Write,
    ReadWrite,
}

/// `Variable`: uma variável ou expressão. O `presentationHint` (`VariablePresentationHint`) é o objeto JSON cru, porque
/// nenhum chamador do porte olha os campos dele.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Variable {
    /// The variable's name.
    pub name: String,
    /// The variable's value.
    pub value: String,
    /// The type of the variable's value.
    pub r#type: Option<String>,
    /// Properties of a variable that can be used to determine how to render the variable in the UI.
    pub presentation_hint: Option<Value>,
    /// The evaluatable name of this variable which can be passed to the `evaluate` request to fetch the variable's value.
    pub evaluate_name: Option<String>,
    /// If `variablesReference` is > 0, the variable is structured and its children can be retrieved by passing
    /// `variablesReference` to the `variables` request.
    pub variables_reference: i64,
    /// The number of named child variables.
    pub named_variables: Option<i64>,
    /// The number of indexed child variables.
    pub indexed_variables: Option<i64>,
    /// A memory reference associated with this variable.
    pub memory_reference: Option<String>,
    /// A reference that allows the client to request the location where the variable is declared.
    pub declaration_location_reference: Option<i64>,
    /// A reference that allows the client to request the location where the variable's value is declared.
    pub value_location_reference: Option<i64>,
}
