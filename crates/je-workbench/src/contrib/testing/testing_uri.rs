//! Porte de `src/vs/workbench/contrib/testing/common/testingUri.ts`: as URIs `vscode-test-data:` que apontam
//! para a saída de uma tarefa, a saída de um teste e as mensagens (texto, saída atual e saída esperada) de um
//! resultado.
//!
//! Notas de tradução:
//! - `ParsedTestUri` do original é uma união discriminada por `type`; aqui é um `enum` cujas variantes são os
//!   cinco `TestUriType`, e [`ParsedTestUri::uri_type`] devolve o discriminante do original. As três variantes
//!   de mensagem têm os mesmos campos (`IResultTestUri` mais `messageIndex`).
//! - `taskIndex` e `messageIndex` são `number` no original (`Number(...)` sobre o segmento do caminho, que dá
//!   `NaN` quando o segmento falta), então são `f64`, como o `instance_id` de `terminal_uri.rs`. `Number(text)`
//!   é [`string_to_number`] e `String(number)` do `join` é [`number_to_js_string`].
//! - `resultId` e `testExtId` são [`JsString`] (UTF-16), como o `string` do original.
//! - O `default: assertNever(parsed, 'Invalid test uri')` de `buildTestUri` não tem equivalente: o `match` sobre o
//!   `enum` é exaustivo.
//! - `const enum TestUriParts` são as constantes `RESULTS`, `ALL_OUTPUT`, `MESSAGES`, `TEXT`, `ACTUAL_OUTPUT` e
//!   `EXPECTED_OUTPUT`.

use je_base::JsString;
use je_base::dynamic::number_to_js_string;
use je_base::uri::{URI, UriComponents};
use je_platform::js::string_to_number;

pub const TEST_DATA_SCHEME: &str = "vscode-test-data";

/// `TestUriType`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TestUriType {
    /// All console output for a task
    TaskOutput,
    /// All console output for a test in a task
    TestOutput,
    /// Specific message in a test
    ResultMessage,
    /// Specific actual output message in a test
    ResultActualOutput,
    /// Specific expected output message in a test
    ResultExpectedOutput,
}

/// `ParsedTestUri`.
#[derive(Clone, Debug, PartialEq)]
pub enum ParsedTestUri {
    /// `IAllOutputReference`.
    TaskOutput { result_id: JsString, task_index: f64 },
    /// `ITestOutputReference`.
    TestOutput { result_id: JsString, task_index: f64, test_ext_id: JsString },
    /// `IResultTestMessageReference`.
    ResultMessage { result_id: JsString, task_index: f64, test_ext_id: JsString, message_index: f64 },
    /// `ITestDiffOutputReference` com `TestUriType.ResultActualOutput`.
    ResultActualOutput { result_id: JsString, task_index: f64, test_ext_id: JsString, message_index: f64 },
    /// `ITestDiffOutputReference` com `TestUriType.ResultExpectedOutput`.
    ResultExpectedOutput { result_id: JsString, task_index: f64, test_ext_id: JsString, message_index: f64 },
}

impl ParsedTestUri {
    /// O campo `type` do original.
    pub fn uri_type(&self) -> TestUriType {
        match self {
            Self::TaskOutput { .. } => TestUriType::TaskOutput,
            Self::TestOutput { .. } => TestUriType::TestOutput,
            Self::ResultMessage { .. } => TestUriType::ResultMessage,
            Self::ResultActualOutput { .. } => TestUriType::ResultActualOutput,
            Self::ResultExpectedOutput { .. } => TestUriType::ResultExpectedOutput,
        }
    }
}

/// `TestUriParts`.
const RESULTS: &str = "results";
const ALL_OUTPUT: &str = "output";
const MESSAGES: &str = "message";
const TEXT: &str = "TestFailureMessage";
const ACTUAL_OUTPUT: &str = "ActualOutput";
const EXPECTED_OUTPUT: &str = "ExpectedOutput";

fn is_part(part: Option<&&[u16]>, expected: &str) -> bool {
    part.is_some_and(|units| units.iter().copied().eq(expected.encode_utf16()))
}

/// `Number(request[index])`: `Number(undefined)` é `NaN`.
fn number_at(request: &[&[u16]], index: usize) -> f64 {
    request.get(index).map_or(f64::NAN, |part| string_to_number(part))
}

pub fn parse_test_uri(uri: &URI) -> Option<ParsedTestUri> {
    let kind = uri.authority();
    // `const [resultId, ...request] = uri.path.slice(1).split('/')`: `split` devolve ao menos um segmento.
    let mut segments = uri.path().units().get(1..).unwrap_or_default().split(|&unit| unit == u16::from(b'/'));
    let result_id = JsString::from(segments.next().unwrap_or_default());
    let request: Vec<&[u16]> = segments.collect();

    if is_part(request.first(), MESSAGES) {
        let task_index = number_at(&request, 1);
        let test_ext_id = uri.query().clone();
        let index = number_at(&request, 2);
        let part = request.get(3);
        if *kind == RESULTS {
            if is_part(part, TEXT) {
                return Some(ParsedTestUri::ResultMessage { result_id, task_index, test_ext_id, message_index: index });
            }
            if is_part(part, ACTUAL_OUTPUT) {
                return Some(ParsedTestUri::ResultActualOutput { result_id, task_index, test_ext_id, message_index: index });
            }
            if is_part(part, EXPECTED_OUTPUT) {
                return Some(ParsedTestUri::ResultExpectedOutput { result_id, task_index, test_ext_id, message_index: index });
            }
            // `case TestUriParts.Messages:` sem corpo: sai do `switch` sem retornar.
        }
    }

    if is_part(request.first(), ALL_OUTPUT) {
        let test_ext_id = uri.query().clone();
        let task_index = number_at(&request, 1);
        return Some(if test_ext_id.is_empty() {
            ParsedTestUri::TaskOutput { result_id, task_index }
        } else {
            ParsedTestUri::TestOutput { result_id, task_index, test_ext_id }
        });
    }

    None
}

/// `[...].join('/')` sobre os segmentos do caminho.
fn join_path(segments: &[JsString]) -> JsString {
    let mut path = JsString::new();
    for (index, segment) in segments.iter().enumerate() {
        if index > 0 {
            path.push_unit(u16::from(b'/'));
        }
        path.push_units(segment.units());
    }
    path
}

fn test_data_uri(path: JsString, query: Option<&JsString>) -> URI {
    URI::from(
        &UriComponents {
            scheme: Some(JsString::from(TEST_DATA_SCHEME)),
            authority: Some(JsString::from(RESULTS)),
            path: Some(path),
            query: query.cloned(),
            ..UriComponents::default()
        },
        false,
    )
}

/// O caminho de uma saída (`/<resultId>/output/<taskIndex>`), com ou sem `testExtId` na consulta.
fn output_uri(result_id: &JsString, task_index: f64, test_ext_id: Option<&JsString>) -> URI {
    let path = join_path(&[JsString::new(), result_id.clone(), JsString::from(ALL_OUTPUT), JsString::from(number_to_js_string(task_index))]);
    test_data_uri(path, test_ext_id)
}

/// `msgRef`: o caminho de uma mensagem (`/<resultId>/message/<taskIndex>/<messageIndex>/<part>`), com o
/// `testExtId` na consulta.
fn message_uri(result_id: &JsString, task_index: f64, test_ext_id: &JsString, message_index: f64, part: &str) -> URI {
    let path = join_path(&[
        JsString::new(),
        result_id.clone(),
        JsString::from(MESSAGES),
        JsString::from(number_to_js_string(task_index)),
        JsString::from(number_to_js_string(message_index)),
        JsString::from(part),
    ]);
    test_data_uri(path, Some(test_ext_id))
}

pub fn build_test_uri(parsed: &ParsedTestUri) -> URI {
    match parsed {
        ParsedTestUri::TaskOutput { result_id, task_index } => output_uri(result_id, *task_index, None),
        ParsedTestUri::ResultActualOutput { result_id, task_index, test_ext_id, message_index } => {
            message_uri(result_id, *task_index, test_ext_id, *message_index, ACTUAL_OUTPUT)
        }
        ParsedTestUri::ResultExpectedOutput { result_id, task_index, test_ext_id, message_index } => {
            message_uri(result_id, *task_index, test_ext_id, *message_index, EXPECTED_OUTPUT)
        }
        ParsedTestUri::ResultMessage { result_id, task_index, test_ext_id, message_index } => {
            message_uri(result_id, *task_index, test_ext_id, *message_index, TEXT)
        }
        ParsedTestUri::TestOutput { result_id, task_index, test_ext_id } => output_uri(result_id, *task_index, Some(test_ext_id)),
    }
}
