//! Porte de `src/vs/platform/configuration/common/configurationVariables.ts`: a leitura de variáveis
//! `${nome:argumento}` dentro de valores de configuração. Os deslocamentos são em unidades UTF-16.

use je_base::JsString;

const DOLLAR: u16 = b'$' as u16;
const OPEN: u16 = b'{' as u16;
const CLOSE: u16 = b'}' as u16;
const COLON: u16 = b':' as u16;

/// `IConfigurationVariable`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigurationVariable {
    /// `${nome:arg}`.
    pub id: JsString,
    /// O `nome:arg` de `${nome:arg}`.
    pub inner: JsString,
    /// O `nome` de `${nome:arg}`.
    pub name: JsString,
    /// O `arg` de `${nome:arg}`.
    pub arg: Option<JsString>,
}

/// O resultado de [`parse_configuration_variable`]: a variável e o deslocamento da `}` que a fecha.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedConfigurationVariable {
    pub replacement: ConfigurationVariable,
    pub end: usize,
}

/// `parseConfigurationVariable`: lê a variável que começa em `start`, com chaves aninhadas.
pub fn parse_configuration_variable(value: &JsString, start: usize) -> Option<ParsedConfigurationVariable> {
    let units = value.units();
    if units.get(start) != Some(&DOLLAR) || units.get(start + 1) != Some(&OPEN) {
        return None;
    }

    let mut end = start + 2;
    let mut brace_count = 1usize;
    while end < units.len() {
        if units[end] == OPEN {
            brace_count += 1;
        } else if units[end] == CLOSE {
            brace_count -= 1;
            if brace_count == 0 {
                break;
            }
        }
        end += 1;
    }

    if brace_count != 0 {
        return None;
    }

    let id = JsString::from(&units[start..=end]);
    let inner_units = &units[start + 2..end];
    let inner = JsString::from(inner_units);
    let replacement = match inner_units.iter().position(|&u| u == COLON) {
        None => ConfigurationVariable { id, name: inner.clone(), inner, arg: None },
        Some(colon) => ConfigurationVariable { id, name: JsString::from(&inner_units[..colon]), arg: Some(JsString::from(&inner_units[colon + 1..])), inner },
    };
    Some(ParsedConfigurationVariable { replacement, end })
}

/// `hasConfigurationVariable`: varre de trás para a frente atrás de uma variável com a chave de
/// fechamento correspondente, em tempo linear e espaço constante.
pub fn has_configuration_variable(value: &JsString) -> bool {
    let units = value.units();
    let mut unmatched_closing_braces = 0usize;
    for offset in (0..units.len()).rev() {
        if units[offset] == CLOSE {
            unmatched_closing_braces += 1;
        } else if units[offset] == OPEN && unmatched_closing_braces > 0 {
            unmatched_closing_braces -= 1;
            if offset > 0 && units[offset - 1] == DOLLAR {
                return true;
            }
        }
    }
    false
}
