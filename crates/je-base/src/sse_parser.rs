//! Porte de `src/vs/base/common/sseParser.ts`: o analisador de fluxos Server-Sent Events, conforme a
//! especificação do HTML (<https://html.spec.whatwg.org/multipage/server-sent-events.html#event-stream-interpretation>).
//!
//! Decisões de tradução:
//! - O texto do fluxo é UTF-8 decodificado como o `TextDecoder('utf-8')` do original (substituição de
//!   sequências inválidas por U+FFFD e remoção de um BOM no começo de cada linha, porque o `decode` sem
//!   `stream` do original reinicia o decodificador a cada linha), então os campos são `String`: o resultado
//!   é o mesmo que o de `JsString`, já que um `String` do Rust nunca tem surrogate isolado e a decodificação
//!   UTF-8 também não produz.
//! - Os pedaços de uma linha ainda incompleta ficam num único `Vec<u8>`: decodificar os pedaços em
//!   sequência com `stream: true` e o último sem equivale a decodificar a concatenação de uma vez.
//! - `retry` é `f64` (um `number`), `parseInt` de uma sequência de dígitos ASCII.
//! - O campo `type` do evento é `r#type`.

/// Um evento despachado por um fluxo SSE.
#[derive(Clone, Debug, PartialEq)]
pub struct ISSEEvent {
    /// O tipo do evento. Se não foi especificado, é `message`.
    pub r#type: String,
    /// Os dados do evento.
    pub data: String,
    /// O último ID de evento, usado para reconexão.
    pub id: Option<String>,
    /// O tempo de reconexão em milissegundos.
    pub retry: Option<f64>,
}

/// A função chamada quando um evento é despachado.
pub type SSEEventHandler = Box<dyn FnMut(ISSEEvent)>;

const CR: u8 = 13;
const LF: u8 = 10;

/// `new TextDecoder('utf-8').decode(bytes)`: sequência inválida vira U+FFFD e um BOM inicial é removido.
fn decode_utf8(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    match text.strip_prefix('\u{FEFF}') {
        Some(rest) => rest.to_owned(),
        None => text.into_owned(),
    }
}

/// O analisador de fluxos Server-Sent Events.
pub struct SSEParser {
    data_buffer: String,
    event_type_buffer: String,
    current_event_id: Option<String>,
    last_event_id_buffer: Option<String>,
    reconnection_time: Option<f64>,
    buffer: Vec<u8>,
    ended_on_cr: bool,
    on_event_handler: SSEEventHandler,
}

impl SSEParser {
    /// Cria um analisador; `on_event` é chamado a cada evento despachado.
    pub fn new(on_event: impl FnMut(ISSEEvent) + 'static) -> Self {
        Self {
            data_buffer: String::new(),
            event_type_buffer: String::new(),
            current_event_id: None,
            last_event_id_buffer: None,
            reconnection_time: None,
            buffer: Vec::new(),
            ended_on_cr: false,
            on_event_handler: Box::new(on_event),
        }
    }

    /// O último ID de evento recebido por este analisador.
    pub fn get_last_event_id(&self) -> Option<&str> {
        self.last_event_id_buffer.as_deref()
    }

    /// O tempo de reconexão em milissegundos, se o servidor especificou um.
    pub fn get_reconnection_time(&self) -> Option<f64> {
        self.reconnection_time
    }

    /// Alimenta o analisador com um pedaço do fluxo, em bytes UTF-8.
    pub fn feed(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }

        let mut offset = 0;

        // If the data stream was bifurcated between a CR and LF, avoid processing the CR as an extra newline
        if self.ended_on_cr && chunk[0] == LF {
            offset += 1;
        }
        self.ended_on_cr = false;

        // Process complete lines from the buffer
        while offset < chunk.len() {
            let Some(relative) = chunk[offset..].iter().position(|&b| b == CR || b == LF) else {
                break;
            };
            let index = offset + relative;

            self.buffer.extend_from_slice(&chunk[offset..index]);
            let line = decode_utf8(&self.buffer);
            self.process_line(&line);

            self.buffer.clear();
            offset = index + if chunk[index] == CR && chunk.get(index + 1) == Some(&LF) { 2 } else { 1 };
        }

        if offset < chunk.len() {
            self.buffer.extend_from_slice(&chunk[offset..]);
        } else {
            self.ended_on_cr = chunk[chunk.len() - 1] == CR;
        }
    }

    /// Processa uma linha do fluxo.
    fn process_line(&mut self, line: &str) {
        if line.is_empty() {
            self.dispatch_event();
            return;
        }

        if line.starts_with(':') {
            return;
        }

        // Parse the field name and value
        match line.find(':') {
            // Line with no colon - the entire line is the field name, value is empty
            None => self.process_field(line, ""),
            // Line with a colon - split into field name and value; if value starts with a space, remove it
            Some(colon_index) => {
                let value = &line[colon_index + 1..];
                self.process_field(&line[..colon_index], value.strip_prefix(' ').unwrap_or(value));
            }
        }
    }

    /// Processa um campo com o nome e o valor dados.
    fn process_field(&mut self, field: &str, value: &str) {
        match field {
            "event" => self.event_type_buffer = value.to_owned(),

            // Append the value to the data buffer, followed by a newline
            "data" => {
                self.data_buffer.push_str(value);
                self.data_buffer.push('\n');
            }

            // If the field value doesn't contain NULL, set the last event ID buffer
            "id" => {
                if value.contains('\0') {
                    self.current_event_id = None;
                } else {
                    self.current_event_id = Some(value.to_owned());
                    self.last_event_id_buffer = Some(value.to_owned());
                }
            }

            // If the field value consists only of ASCII digits, set the reconnection time
            "retry" if !value.is_empty() && value.bytes().all(|b| b.is_ascii_digit()) => self.reconnection_time = value.parse::<f64>().ok(),

            // Ignore any other fields
            _ => {}
        }
    }

    /// Despacha o evento conforme o estado dos buffers.
    fn dispatch_event(&mut self) {
        // If the data buffer is empty, reset the buffers and return
        if self.data_buffer.is_empty() {
            self.event_type_buffer.clear();
            return;
        }

        // If the data buffer's last character is a newline, remove it
        if self.data_buffer.ends_with('\n') {
            self.data_buffer.pop();
        }

        // Create and dispatch the event; the optional fields are added if they exist
        let event = ISSEEvent {
            r#type: if self.event_type_buffer.is_empty() { "message".to_owned() } else { self.event_type_buffer.clone() },
            data: self.data_buffer.clone(),
            id: self.current_event_id.clone(),
            retry: self.reconnection_time,
        };

        (self.on_event_handler)(event);

        // Reset the data and event type buffers
        self.reset();
    }

    /// Reinicia o estado do analisador.
    pub fn reset(&mut self) {
        self.data_buffer.clear();
        self.event_type_buffer.clear();
        self.current_event_id = None;
        // Note: lastEventIdBuffer is not reset as it's used for reconnection
    }
}
