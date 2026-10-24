//! Porte parcial de `src/vs/platform/terminal/common/xterm/shellIntegrationAddon.ts`: os identificadores de OSC da
//! integração de shell (`ShellIntegrationOscPs`) e as funções puras de serialização e de leitura das sequências
//! (`deserializeVSCodeOscMessage`, `serializeVSCodeOscMessage`, `parseKeyValueAssignment`, `parseMarkSequence`).
//!
//! A classe `ShellIntegrationAddon` (os tratadores de OSC registrados no `parser` do xterm, `_doHandle*`,
//! `_createOrGetCommandDetection`, `_createOrGetCwdDetection`) depende do emulador de terminal (Fase 6: o `Terminal` com
//! `parser.registerOscHandler`, o buffer e os marcadores) e das capacidades `CommandDetectionCapability`,
//! `CwdDetectionCapability` e `BufferMarkCapability`, ainda não portadas. Ela entra com o emulador.
//!
//! Divergências:
//! - As mensagens são `&str`/`String`: as funções só mexem em caracteres ASCII (`\`, `;` e controles até `0x20`) e em
//!   dígitos hexadecimais, e o `String.fromCharCode(parseInt(hex, 16))` de `0x00` a `0xff` é o `char` de mesmo código.
//! - `{ id?: string; hidden?: boolean }` é [`MarkSequence`] (`id` opcional, `hidden` sempre definido, como o retorno do
//!   original), e `{ key, value: string | undefined }` é [`KeyValueAssignment`].
//! - `(string | undefined)[]` é uma fatia de `Option<&str>`.

/// `ShellIntegrationOscPs`: o `Ps` de cada família de sequências OSC.
pub mod shell_integration_osc_ps {
    pub const FINAL_TERM: i32 = 133;
    pub const VS_CODE: i32 = 633;
    pub const I_TERM: i32 = 1337;
    pub const SET_CWD: i32 = 7;
    pub const SET_WINDOWS_FRIENDLY_CWD: i32 = 9;
}

/// O resultado de [`parse_key_value_assignment`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeyValueAssignment {
    pub key: String,
    pub value: Option<String>,
}

/// O resultado de [`parse_mark_sequence`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarkSequence {
    pub id: Option<String>,
    pub hidden: bool,
}

/// `deserializeVSCodeOscMessage`: desfaz o escape de `serializeVSCodeOscMessage`. Uma barra invertida seguida de outra
/// barra vira uma barra, e uma barra seguida de `x` e dois dígitos hexadecimais (em qualquer caixa) vira o caractere.
pub fn deserialize_vscode_osc_message(message: &str) -> String {
    let chars: Vec<char> = message.chars().collect();
    let mut result = String::with_capacity(message.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == '\\' {
            // Backslash ('\') followed by an escape operator: either another '\', or 'x' and two hex chars.
            match chars.get(index + 1) {
                Some('\\') => {
                    result.push('\\');
                    index += 2;
                    continue;
                }
                Some('x') => {
                    if let (Some(high), Some(low)) = (chars.get(index + 2).and_then(|c| c.to_digit(16)), chars.get(index + 3).and_then(|c| c.to_digit(16))) {
                        result.push(char::from_u32(high * 16 + low).expect("um byte é um ponto de código válido"));
                        index += 4;
                        continue;
                    }
                }
                _ => {}
            }
        }
        result.push(chars[index]);
        index += 1;
    }
    result
}

/// `serializeVSCodeOscMessage`: escapa a barra invertida (`\\`), o ponto e vírgula e os caracteres até `0x20` (`\xAB`).
pub fn serialize_vscode_osc_message(message: &str) -> String {
    let mut result = String::with_capacity(message.len());
    for character in message.chars() {
        // Match backslash ('\'), semicolon (';'), or characters 0x20 and below
        if character == '\\' {
            result.push_str("\\\\");
        } else if character == ';' || u32::from(character) <= 0x20 {
            result.push_str(&format!("\\x{:02x}", u32::from(character)));
        } else {
            result.push(character);
        }
    }
    result
}

/// `parseKeyValueAssignment`: separa `chave=valor` no primeiro `=`; sem `=`, o valor é `None`.
pub fn parse_key_value_assignment(message: &str) -> KeyValueAssignment {
    match message.find('=') {
        None => KeyValueAssignment { key: message.to_owned(), value: None },
        Some(separator_index) => {
            KeyValueAssignment { key: message[..separator_index].to_owned(), value: Some(message[separator_index + 1..].to_owned()) }
        }
    }
}

/// `parseMarkSequence`: lê as propriedades `Id=<id>` e `Hidden` de uma sequência `SetMark`.
pub fn parse_mark_sequence(sequence: &[Option<&str>]) -> MarkSequence {
    let mut id = None;
    let mut hidden = false;
    for property in sequence {
        // Sanity check, this shouldn't happen in practice
        let Some(property) = property else { continue };
        if *property == "Hidden" {
            hidden = true;
        }
        if let Some(rest) = property.strip_prefix("Id=") {
            id = Some(rest.to_owned());
        }
    }
    MarkSequence { id, hidden }
}
