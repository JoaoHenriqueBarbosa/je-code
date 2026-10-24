//! Porte parcial de `src/vs/workbench/contrib/notebook/browser/viewModel/cellOutputTextHelper.ts`: o texto de uma saída de célula
//! (`getOutputText`, `getOutputStreamText`) e a cópia dele para a área de transferência (`copyCellOutput`, com `TEXT_BASED_MIMETYPES`).
//!
//! Divergências:
//! - `getAllOutputsText` não é portado: ele chama `outputViewModel.resolveMimeTypes(notebook, undefined)`, que depende do
//!   `INotebookService.getOutputMimeTypeInfo` e do resolvedor de renderizadores (ver `cell_output_view_model.rs`).
//! - `ICellOutputViewModel` do original (com `model` e `cellViewModel`) é o trait [`ICellOutputViewModel`], e o `ICellViewModel` que o
//!   `getOutputStreamText` lê (`outputsViewModels` e `model.outputs`) é o [`ICellOutputsOwner`]. O [`CellOutputViewModel`] do porte guarda
//!   só o `id` da célula dona (não a célula), então ele não implementa o trait: quem liga a célula à saída (o widget) implementa os dois.
//! - `indexOf(output)` é a identidade do objeto (o endereço do `Rc`).
//! - `new TextDecoder().decode(bytes)` é a decodificação UTF-8 com `U+FFFD` nos bytes inválidos e sem o BOM inicial.
//! - `JSON.parse(text)` é o `parse_str` do je-base (que aceita comentários e vírgula final): o texto só vale quando a leitura não
//!   reporta erro. Um `stack` que não seja texto é tratado como ausente (o original chamaria `trim` num valor que não é texto).
//! - `await clipboardService.writeText(text)` é a promessa devolvida; a falha vira `logService.error(...)` e a promessa resolve.
//! - `string.trim()` é o `trim` de `je_base::js_string`.

use std::rc::Rc;

use je_base::JsString;
use je_base::js_regexp::replace_all;
use je_base::js_string::trim;
use je_base::json::{Value, parse_str};
use je_base::mime::is_text_stream_mime;
use je_base::promise::Promise;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::log::{LogService, Logger};
use zyarr::RegExp;

use crate::contrib::notebook::common::notebook_common::{ICellOutput, IOutputItemDto};

/// A saída de uma célula (`ICellOutputViewModel`): o modelo e a célula dona.
pub trait ICellOutputViewModel {
    /// `model`.
    fn model(&self) -> Rc<dyn ICellOutput>;
    /// `cellViewModel`.
    fn cell_view_model(&self) -> Option<Rc<dyn ICellOutputsOwner>>;
}

/// O que o `getOutputStreamText` lê da célula dona (`ICellViewModel`).
pub trait ICellOutputsOwner {
    /// `outputsViewModels`.
    fn outputs_view_models(&self) -> Vec<Rc<dyn ICellOutputViewModel>>;
    /// `model.outputs`.
    fn model_outputs(&self) -> Vec<Rc<dyn ICellOutput>>;
}

/// `new TextDecoder().decode(bytes)`.
fn decode(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    text.strip_prefix('\u{feff}').map_or_else(|| text.to_string(), str::to_string)
}

/// `text.trim()`.
fn js_trim(text: &JsString) -> String {
    JsString::from(trim(text.units())).to_string_lossy()
}

/// `getOutputStreamText(output)`: o texto (já sem espaços nas pontas) e quantas saídas de fluxo seguidas entraram.
pub fn get_output_stream_text(output: &dyn ICellOutputViewModel) -> (String, usize) {
    let mut text = String::new();
    let cell_view_model = output.cell_view_model().expect("a saída precisa da célula dona");
    let target: *const dyn ICellOutputViewModel = output;
    let mut index = cell_view_model.outputs_view_models().iter().position(|candidate| std::ptr::addr_eq(Rc::as_ptr(candidate), target));
    let model_outputs = cell_view_model.model_outputs();
    let mut count = 0;
    // `indexOf` devolve -1 quando a saída não está na lista, e `outputs[-1]` é `undefined`: o laço do original lê isso como o fim.
    while let Some(current) = index {
        if current >= model_outputs.len() {
            break;
        }
        let next_cell_output = &model_outputs[current];
        let next_output = next_cell_output.outputs().into_iter().find(|output| is_text_stream_mime(&output.mime));
        let Some(next_output) = next_output else {
            break;
        };

        text.push_str(&decode(&next_output.data.to_vec()));
        index = Some(current + 1);
        count += 1;
    }

    (js_trim(&JsString::from(text)), count)
}

/// `getOutputText(mimeType, buffer, shortError = false)`.
pub fn get_output_text(mime_type: &str, buffer: &IOutputItemDto, short_error: bool) -> String {
    const CHAR_LIMIT: i64 = 100000;
    let mut text = JsString::from(decode(&buffer.data.slice(Some(0), Some(CHAR_LIMIT)).to_vec()));

    if buffer.data.byte_length() > CHAR_LIMIT as usize {
        let mut truncated = text.to_string_lossy();
        truncated.push_str("...(truncated)");
        text = JsString::from(truncated);
    } else if mime_type == "application/vnd.code.notebook.error" {
        let pattern: Vec<u16> = r"\\u001b\[[0-9;]*m".encode_utf16().collect();
        let flags: Vec<u16> = "gi".encode_utf16().collect();
        let regexp = RegExp::new(&pattern, &flags).expect("o padrão fixo é válido");
        text = replace_all(&regexp, text.units(), |_| JsString::new());
        let mut errors = Vec::new();
        let parsed = parse_str(&text.to_string_lossy(), &mut errors).filter(|_| errors.is_empty());
        // just use raw text quando o JSON não é válido (ou é `null`, de onde o original lança `TypeError` e cai no `catch`)
        match parsed {
            Some(Value::Null) | None => {}
            Some(error) => {
                let property = |key: &str| match &error {
                    Value::Object(object) => object.get(key).cloned(),
                    _ => None,
                };
                let stack = property("stack").and_then(|value| match value {
                    Value::String(stack) if !stack.units().is_empty() => Some(stack),
                    _ => None,
                });
                text = match stack {
                    Some(stack) if !short_error => stack,
                    _ => JsString::from(format!("{}: {}", js_to_string(property("name")), js_to_string(property("message")))),
                };
            }
        }
    }

    js_trim(&text)
}

/// A conversão de um valor JSON para texto na interpolação `${value}`.
fn js_to_string(value: Option<Value>) -> String {
    match value {
        None => "undefined".to_string(),
        Some(Value::Null) => "null".to_string(),
        Some(Value::Bool(value)) => value.to_string(),
        Some(Value::Number(number)) => {
            if number.is_nan() {
                "NaN".to_string()
            } else if number.is_finite() && number.fract() == 0.0 && number.abs() < 1e21 {
                format!("{}", number as i128)
            } else {
                number.to_string()
            }
        }
        Some(Value::String(text)) => text.to_string_lossy(),
        Some(Value::Array(_)) => String::new(),
        Some(Value::Object(_)) => "[object Object]".to_string(),
    }
}

/// `copyCellOutput(mimeType, outputViewModel, clipboardService, logService)`.
pub fn copy_cell_output(
    mime_type: Option<&str>,
    output_view_model: &dyn ICellOutputViewModel,
    clipboard_service: Rc<dyn IClipboardService>,
    log_service: Rc<dyn LogService>,
) -> Promise<()> {
    let cell_output = output_view_model.model();
    let outputs = cell_output.outputs();
    let output = match mime_type {
        Some(mime_type) if !mime_type.is_empty() && TEXT_BASED_MIMETYPES.contains(&mime_type) => outputs.iter().find(|output| output.mime == mime_type),
        _ => outputs.iter().find(|output| TEXT_BASED_MIMETYPES.contains(&output.mime.as_str())),
    };

    let Some(output) = output.filter(|output| !output.mime.is_empty()) else {
        return Promise::resolved(());
    };
    let mime_type = output.mime.as_str();

    let text = if is_text_stream_mime(mime_type) { get_output_stream_text(output_view_model).0 } else { get_output_text(mime_type, output, false) };

    clipboard_service.write_text(&text).then_result(move |result| {
        if let Err(e) = result {
            log_service.error(&format!("Failed to copy content: {e}"));
        }
        Ok(())
    })
}

/// `TEXT_BASED_MIMETYPES`.
pub const TEXT_BASED_MIMETYPES: [&str; 11] = [
    "text/latex",
    "text/html",
    "application/vnd.code.notebook.error",
    "application/vnd.code.notebook.stdout",
    "application/x.notebook.stdout",
    "application/x.notebook.stream",
    "application/vnd.code.notebook.stderr",
    "application/x.notebook.stderr",
    "text/plain",
    "text/markdown",
    "application/json",
];
