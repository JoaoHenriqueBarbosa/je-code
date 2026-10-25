//! O pedaço de HTTP/1.1 que o proxy de túnel precisa (o que o `http.Server` e o `http.request` do Node fazem por baixo
//! do `tunnelProxy.ts`): cabeçalho de requisição e de resposta, enquadramento do corpo (`Content-Length`, `chunked` e
//! até o fim da conexão) e a serialização.

use std::io;

use super::tunnel_socket::Duplex;

/// O `maxHeaderSize` do Node (16 KiB).
const MAX_HEAD_BYTES: usize = 16 * 1024;
/// O tamanho de cada leitura do corpo.
const BODY_CHUNK: usize = 16 * 1024;

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn unexpected_eof() -> io::Error {
    io::Error::from(io::ErrorKind::UnexpectedEof)
}

/// Os cabeçalhos de uma mensagem, na ordem em que chegaram.
#[derive(Clone, Debug, Default)]
pub struct Headers(Vec<(String, String)>);

/// Os cabeçalhos em que o Node descarta as repetições (fica o primeiro valor).
const DISCARD_DUPLICATES: [&str; 20] = [
    "age",
    "authorization",
    "content-length",
    "content-type",
    "etag",
    "expires",
    "from",
    "host",
    "if-modified-since",
    "if-unmodified-since",
    "last-modified",
    "location",
    "max-forwards",
    "proxy-authorization",
    "referer",
    "retry-after",
    "server",
    "user-agent",
    "content-disposition",
    "content-encoding",
];

impl Headers {
    /// O `IncomingMessage.headers` do Node: nomes em minúsculas e as repetições unidas (`, `; `; ` no `cookie`;
    /// `set-cookie` fica um por linha; o primeiro valor vence nos cabeçalhos de valor único).
    fn from_wire(raw: Vec<(String, String)>) -> Self {
        let mut headers: Vec<(String, String)> = Vec::new();
        for (name, value) in raw {
            match headers.iter().position(|(existing, _)| *existing == name) {
                None => headers.push((name, value)),
                Some(_) if name == "set-cookie" => headers.push((name, value)),
                Some(index) if name == "cookie" => {
                    headers[index].1.push_str("; ");
                    headers[index].1.push_str(&value);
                }
                Some(_) if DISCARD_DUPLICATES.contains(&name.as_str()) => {}
                Some(index) => {
                    headers[index].1.push_str(", ");
                    headers[index].1.push_str(&value);
                }
            }
        }
        Self(headers)
    }

    pub fn push(&mut self, name: &str, value: impl Into<String>) {
        self.0.push((name.to_owned(), value.into()));
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.0.iter().find(|(existing, _)| existing.eq_ignore_ascii_case(name)).map(|(_, value)| value.as_str())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.get(name).is_some()
    }

    pub fn remove(&mut self, name: &str) {
        self.0.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
    }

    pub fn set(&mut self, name: &str, value: impl Into<String>) {
        self.remove(name);
        self.push(name, value);
    }

    /// Os itens de um cabeçalho em lista (`a, b`), em minúsculas e sem os vazios.
    pub fn tokens(&self, name: &str) -> Vec<String> {
        self.get(name).map(|value| value.split(',').map(|token| token.trim().to_ascii_lowercase()).filter(|token| !token.is_empty()).collect()).unwrap_or_default()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.0.iter().map(|(name, value)| (name.as_str(), value.as_str()))
    }
}

/// A linha de requisição e os cabeçalhos.
#[derive(Clone, Debug)]
pub struct RequestHead {
    pub method: String,
    pub target: String,
    pub minor_version: u8,
    pub headers: Headers,
}

impl RequestHead {
    /// A conexão continua depois desta requisição? (`shouldKeepAlive` do Node.)
    pub fn keep_alive(&self) -> bool {
        let tokens = self.headers.tokens("connection");
        if self.minor_version >= 1 { !tokens.iter().any(|token| token == "close") } else { tokens.iter().any(|token| token == "keep-alive") }
    }
}

/// A linha de status e os cabeçalhos.
#[derive(Clone, Debug)]
pub struct ResponseHead {
    pub status: u16,
    pub minor_version: u8,
    pub headers: Headers,
}

fn is_token_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&byte)
}

fn parse_header_lines<'a>(lines: impl Iterator<Item = &'a str>) -> Option<Headers> {
    let mut raw = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':')?;
        if name.is_empty() || !name.bytes().all(is_token_byte) {
            return None;
        }
        raw.push((name.to_ascii_lowercase(), value.trim().to_owned()));
    }
    Some(Headers::from_wire(raw))
}

fn parse_version(version: &str) -> Option<u8> {
    match version {
        "HTTP/1.1" => Some(1),
        "HTTP/1.0" => Some(0),
        _ => None,
    }
}

/// O cabeçalho de requisição (incluindo a linha em branco final); `None` se malformado.
pub fn parse_request_head(head: &[u8]) -> Option<RequestHead> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let mut parts = lines.next()?.split(' ');
    let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
    if parts.next().is_some() || method.is_empty() || target.is_empty() || !method.bytes().all(is_token_byte) {
        return None;
    }
    Some(RequestHead { method: method.to_owned(), target: target.to_owned(), minor_version: parse_version(version)?, headers: parse_header_lines(lines)? })
}

/// O cabeçalho de resposta (incluindo a linha em branco final); `None` se malformado.
pub fn parse_response_head(head: &[u8]) -> Option<ResponseHead> {
    let text = String::from_utf8_lossy(head);
    let mut lines = text.split("\r\n");
    let mut parts = lines.next()?.splitn(3, ' ');
    let (version, status) = (parts.next()?, parts.next()?);
    let status: u16 = status.parse().ok().filter(|status| (100..1000).contains(status))?;
    Some(ResponseHead { status, minor_version: parse_version(version)?, headers: parse_header_lines(lines)? })
}

/// Onde termina o cabeçalho (depois do `\r\n\r\n`).
fn find_head_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n").map(|position| position + 4)
}

/// Como o corpo de uma mensagem é delimitado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyFraming {
    None,
    Length(u64),
    Chunked,
    UntilClose,
}

/// O enquadramento do corpo de uma requisição; `Err` se o `Content-Length` é inválido.
pub fn request_framing(headers: &Headers) -> Result<BodyFraming, ()> {
    if headers.tokens("transfer-encoding").iter().any(|token| token == "chunked") {
        return Ok(BodyFraming::Chunked);
    }
    match headers.get("content-length") {
        None => Ok(BodyFraming::None),
        Some(value) => match value.trim().parse::<u64>() {
            Ok(0) => Ok(BodyFraming::None),
            Ok(length) => Ok(BodyFraming::Length(length)),
            Err(_) => Err(()),
        },
    }
}

/// O enquadramento do corpo de uma resposta a `method`.
pub fn response_framing(method: &str, status: u16, headers: &Headers) -> BodyFraming {
    if method == "HEAD" || (100..200).contains(&status) || status == 204 || status == 304 {
        return BodyFraming::None;
    }
    if headers.tokens("transfer-encoding").iter().any(|token| token == "chunked") {
        return BodyFraming::Chunked;
    }
    match headers.get("content-length").and_then(|value| value.trim().parse::<u64>().ok()) {
        Some(0) => BodyFraming::None,
        Some(length) => BodyFraming::Length(length),
        None => BodyFraming::UntilClose,
    }
}

/// Lê mensagens de um [`Duplex`], guardando o que sobra do último pedaço lido.
pub struct WireReader<'a> {
    stream: &'a dyn Duplex,
    buffer: Vec<u8>,
}

impl<'a> WireReader<'a> {
    pub fn new(stream: &'a dyn Duplex) -> Self {
        Self { stream, buffer: Vec::new() }
    }

    /// Os bytes lidos a mais (o `head` do evento `connect` do Node).
    pub fn into_leftover(self) -> Vec<u8> {
        self.buffer
    }

    /// Não sobrou nada lido a mais?
    pub fn is_drained(&self) -> bool {
        self.buffer.is_empty()
    }

    /// Lê até o fim do cabeçalho. `Ok(None)` se a conexão fechou antes do primeiro byte. `after_read` roda depois de cada
    /// leitura que trouxe bytes.
    pub fn read_head(&mut self, after_read: &mut dyn FnMut()) -> io::Result<Option<Vec<u8>>> {
        loop {
            if let Some(end) = find_head_end(&self.buffer) {
                return Ok(Some(self.buffer.drain(..end).collect()));
            }
            if self.buffer.len() > MAX_HEAD_BYTES {
                return Err(invalid_data("HTTP head too large"));
            }
            let mut chunk = [0u8; 8192];
            let count = self.stream.read(&mut chunk)?;
            if count == 0 {
                return if self.buffer.is_empty() { Ok(None) } else { Err(unexpected_eof()) };
            }
            self.buffer.extend_from_slice(&chunk[..count]);
            after_read();
        }
    }

    /// Lê o cabeçalho de uma resposta, pulando as respostas informativas (`1xx`).
    pub fn read_response_head(&mut self) -> io::Result<ResponseHead> {
        loop {
            let head = self.read_head(&mut || {})?.ok_or_else(unexpected_eof)?;
            let response = parse_response_head(&head).ok_or_else(|| invalid_data("malformed HTTP response head"))?;
            if !(100..200).contains(&response.status) {
                return Ok(response);
            }
        }
    }

    fn fill(&mut self) -> io::Result<()> {
        let mut chunk = [0u8; 8192];
        let count = self.stream.read(&mut chunk)?;
        if count == 0 {
            return Err(unexpected_eof());
        }
        self.buffer.extend_from_slice(&chunk[..count]);
        Ok(())
    }

    /// Até `max` bytes; vazio no fim do fluxo.
    fn read_some(&mut self, max: usize) -> io::Result<Vec<u8>> {
        if !self.buffer.is_empty() {
            let count = self.buffer.len().min(max);
            return Ok(self.buffer.drain(..count).collect());
        }
        let mut chunk = vec![0u8; max.min(BODY_CHUNK)];
        let count = self.stream.read(&mut chunk)?;
        chunk.truncate(count);
        Ok(chunk)
    }

    /// Uma linha terminada em `\r\n` (sem o terminador).
    fn read_line(&mut self) -> io::Result<Vec<u8>> {
        loop {
            if let Some(position) = self.buffer.windows(2).position(|window| window == b"\r\n") {
                let mut line: Vec<u8> = self.buffer.drain(..position + 2).collect();
                line.truncate(position);
                return Ok(line);
            }
            if self.buffer.len() > MAX_HEAD_BYTES {
                return Err(invalid_data("HTTP line too long"));
            }
            self.fill()?;
        }
    }

    /// Entrega o corpo, já sem o enquadramento, aos pedaços.
    pub fn copy_body(&mut self, framing: BodyFraming, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>) -> io::Result<()> {
        match framing {
            BodyFraming::None => Ok(()),
            BodyFraming::Length(length) => self.copy_exact(length, sink),
            BodyFraming::UntilClose => loop {
                let data = self.read_some(BODY_CHUNK)?;
                if data.is_empty() {
                    return Ok(());
                }
                sink(&data)?;
            },
            BodyFraming::Chunked => loop {
                let line = self.read_line()?;
                let size_text = String::from_utf8_lossy(&line);
                let size_text = size_text.split(';').next().unwrap_or("").trim().to_owned();
                let size = u64::from_str_radix(&size_text, 16).map_err(|_| invalid_data("invalid chunk size"))?;
                if size == 0 {
                    // Trailers até a linha em branco.
                    while !self.read_line()?.is_empty() {}
                    return Ok(());
                }
                self.copy_exact(size, sink)?;
                if !self.read_line()?.is_empty() {
                    return Err(invalid_data("invalid chunk terminator"));
                }
            },
        }
    }

    fn copy_exact(&mut self, length: u64, sink: &mut dyn FnMut(&[u8]) -> io::Result<()>) -> io::Result<()> {
        let mut remaining = length;
        while remaining > 0 {
            let wanted = usize::try_from(remaining).unwrap_or(usize::MAX).min(BODY_CHUNK);
            let data = self.read_some(wanted)?;
            if data.is_empty() {
                return Err(unexpected_eof());
            }
            remaining -= data.len() as u64;
            sink(&data)?;
        }
        Ok(())
    }
}

/// Como o corpo sai.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BodyEncoding {
    Identity,
    Chunked,
}

/// Escreve um corpo num [`Duplex`] com a codificação escolhida.
pub struct BodyWriter<'a> {
    stream: &'a dyn Duplex,
    encoding: BodyEncoding,
}

impl<'a> BodyWriter<'a> {
    pub fn new(stream: &'a dyn Duplex, encoding: BodyEncoding) -> Self {
        Self { stream, encoding }
    }

    pub fn write(&mut self, data: &[u8]) -> io::Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        match self.encoding {
            BodyEncoding::Identity => self.stream.write_all(data),
            BodyEncoding::Chunked => self.stream.write_all(&[format!("{:x}\r\n", data.len()).into_bytes(), data.to_vec(), b"\r\n".to_vec()].concat()),
        }
    }

    /// O último pedaço (`0\r\n\r\n`) do corpo `chunked`.
    pub fn finish(&mut self) -> io::Result<()> {
        match self.encoding {
            BodyEncoding::Identity => Ok(()),
            BodyEncoding::Chunked => self.stream.write_all(b"0\r\n\r\n"),
        }
    }
}

fn serialize_headers(first_line: String, headers: &Headers) -> Vec<u8> {
    let mut out = first_line;
    out.push_str("\r\n");
    for (name, value) in headers.iter() {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    out.into_bytes()
}

pub fn serialize_request_head(method: &str, path: &str, headers: &Headers) -> Vec<u8> {
    serialize_headers(format!("{method} {path} HTTP/1.1"), headers)
}

pub fn serialize_response_head(status: u16, headers: &Headers) -> Vec<u8> {
    serialize_headers(format!("HTTP/1.1 {status} {}", status_text(status)), headers)
}

/// O `Date` do Node (`Tue, 03 Mar 2026 12:00:00 GMT`).
pub fn http_date() -> String {
    jiff::Timestamp::now().strftime("%a, %d %b %Y %H:%M:%S GMT").to_string()
}

/// O `http.STATUS_CODES` do Node (`unknown` fora da tabela).
pub fn status_text(status: u16) -> &'static str {
    match status {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        208 => "Already Reported",
        226 => "IM Used",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        305 => "Use Proxy",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Payload Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a Teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Entity",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        506 => "Variant Also Negotiates",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        509 => "Bandwidth Limit Exceeded",
        510 => "Not Extended",
        511 => "Network Authentication Required",
        _ => "unknown",
    }
}
