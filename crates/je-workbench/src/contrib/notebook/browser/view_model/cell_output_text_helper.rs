//! Porte de `src/vs/workbench/contrib/notebook/browser/viewModel/cellOutputTextHelper.ts`: o texto de uma saída de célula
//! (`getOutputText`, `getOutputStreamText`), o texto de todas as saídas de uma célula (`getAllOutputsText`) e a cópia dele para a
//! área de transferência (`copyCellOutput`, com `TEXT_BASED_MIMETYPES`).
//!
//! Divergências:
//! - `ICellOutputViewModel` do original (com `model`, `cellViewModel` e `resolveMimeTypes`) é o trait [`ICellOutputViewModel`], e o
//!   `ICellViewModel` que o `getOutputStreamText` lê (`outputsViewModels` e `model.outputs`) é o [`ICellOutputsOwner`]. O
//!   [`CellOutputViewModel`] do porte guarda só o `id` da célula dona (não a célula), então ele não implementa o trait: quem liga a
//!   célula à saída (o widget) implementa os dois. O `resolveMimeTypes` tem corpo padrão que lança `TypeError` (o objeto literal
//!   `{ model } as ICellOutputViewModel` do original não o tem).
//! - `getAllOutputsText(notebook, viewCell, shortErrors)` recebe também o `INotebookService` (o `CellOutputViewModel` do original o
//!   guardava; ver `cell_output_view_model.rs`) e faz a ligação de cada saída com a célula num [`BoundCellOutputs`] criado na chamada.
//!   O `viewCell.outputsViewModels` de uma célula de markup é `undefined` no original, e ler `length` lança `TypeError`.
//! - `indexOf(output)` é a identidade do objeto (o endereço do `Rc`).
//! - `new TextDecoder().decode(bytes)` é a decodificação UTF-8 com `U+FFFD` nos bytes inválidos e sem o BOM inicial.
//! - `JSON.parse(text)` é o `parse_str` do je-base (que aceita comentários e vírgula final): o texto só vale quando a leitura não
//!   reporta erro. Um `stack` que não seja texto é tratado como ausente (o original chamaria `trim` num valor que não é texto).
//! - `await clipboardService.writeText(text)` é a promessa devolvida; a falha vira `logService.error(...)` e a promessa resolve.
//! - `string.trim()` é o `trim` de `je_base::js_string`.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::JsString;
use je_base::errors::{self, Error};
use je_base::js_regexp::replace_all;
use je_base::js_string::trim;
use je_base::json::{Value, parse_str};
use je_base::mime::is_text_stream_mime;
use je_base::promise::Promise;
use je_editor::services::clipboard_service::IClipboardService;
use je_platform::log::{LogService, Logger};
use zyarr::RegExp;

use crate::contrib::notebook::browser::view_model::cell_output_view_model::CellOutputViewModel;
use crate::contrib::notebook::browser::view_model::notebook_view_model_impl::CellViewModel;
use crate::contrib::notebook::common::model::notebook_text_model::NotebookTextModel;
use crate::contrib::notebook::common::notebook_common::{ICell, ICellOutput, IOrderedMimeType, IOutputItemDto};
use crate::contrib::notebook::common::notebook_service::INotebookService;

/// A saída de uma célula (`ICellOutputViewModel`): o modelo, a célula dona e a escolha do tipo de mídia.
pub trait ICellOutputViewModel {
    /// `model`.
    fn model(&self) -> Rc<dyn ICellOutput>;
    /// `cellViewModel`.
    fn cell_view_model(&self) -> Option<Rc<dyn ICellOutputsOwner>>;
    /// `resolveMimeTypes(textModel, kernelProvides)`: os tipos de mídia na ordem de preferência e o índice do escolhido.
    fn resolve_mime_types(&self, _text_model: &NotebookTextModel, _kernel_provides: Option<&[String]>) -> (Vec<IOrderedMimeType>, usize) {
        errors::throw(Error::type_error("resolveMimeTypes is not a function"))
    }
}

/// O que o `getOutputStreamText` lê da célula dona (`ICellViewModel`).
pub trait ICellOutputsOwner {
    /// `outputsViewModels`.
    fn outputs_view_models(&self) -> Vec<Rc<dyn ICellOutputViewModel>>;
    /// `model.outputs`.
    fn model_outputs(&self) -> Vec<Rc<dyn ICellOutput>>;
}

/// Uma saída de célula de código ligada à célula dona e ao serviço de notebooks (o `CellOutputViewModel` do original).
struct BoundOutput {
    view_model: Rc<CellOutputViewModel>,
    notebook_service: Rc<dyn INotebookService>,
    owner: RefCell<Weak<BoundCellOutputs>>,
}

impl ICellOutputViewModel for BoundOutput {
    fn model(&self) -> Rc<dyn ICellOutput> {
        self.view_model.model()
    }

    fn cell_view_model(&self) -> Option<Rc<dyn ICellOutputsOwner>> {
        self.owner.borrow().upgrade().map(|owner| owner as Rc<dyn ICellOutputsOwner>)
    }

    fn resolve_mime_types(&self, text_model: &NotebookTextModel, kernel_provides: Option<&[String]>) -> (Vec<IOrderedMimeType>, usize) {
        self.view_model.resolve_mime_types(self.notebook_service.as_ref(), text_model, kernel_provides)
    }
}

/// As saídas de uma célula de código ligadas à célula (o `ICellViewModel` que o `getOutputStreamText` lê). As saídas são criadas uma
/// vez, para que a identidade do `Rc` valha como o `indexOf` do original.
pub struct BoundCellOutputs {
    outputs: RefCell<Vec<Rc<BoundOutput>>>,
    model_outputs: Vec<Rc<dyn ICellOutput>>,
}

impl BoundCellOutputs {
    pub fn new(cell: &CellViewModel, notebook_service: &Rc<dyn INotebookService>) -> Rc<Self> {
        let Some(code_cell) = cell.as_code() else {
            errors::throw(Error::type_error("Cannot read properties of undefined (reading 'length')"))
        };
        let owner = Rc::new(Self { outputs: RefCell::new(Vec::new()), model_outputs: code_cell.model.outputs() });
        let bound: Vec<Rc<BoundOutput>> = code_cell
            .outputs_view_models()
            .into_iter()
            .map(|view_model| Rc::new(BoundOutput { view_model, notebook_service: notebook_service.clone(), owner: RefCell::new(Rc::downgrade(&owner)) }))
            .collect();
        *owner.outputs.borrow_mut() = bound;
        owner
    }
}

impl ICellOutputsOwner for BoundCellOutputs {
    fn outputs_view_models(&self) -> Vec<Rc<dyn ICellOutputViewModel>> {
        self.outputs.borrow().iter().map(|output| output.clone() as Rc<dyn ICellOutputViewModel>).collect()
    }

    fn model_outputs(&self) -> Vec<Rc<dyn ICellOutput>> {
        self.model_outputs.clone()
    }
}

/// `getAllOutputsText(notebook, viewCell, shortErrors = false)`.
pub fn get_all_outputs_text(notebook_service: &Rc<dyn INotebookService>, notebook: &NotebookTextModel, view_cell: &CellViewModel, short_errors: bool) -> String {
    let owner = BoundCellOutputs::new(view_cell, notebook_service);
    let output_view_models = owner.outputs_view_models();
    let model_outputs = owner.model_outputs();
    let mut output_text: Vec<String> = Vec::new();
    let mut i = 0;
    while i < output_view_models.len() {
        let output_view_model = &output_view_models[i];
        let Some(output_text_model) = model_outputs.get(i) else {
            errors::throw(Error::type_error("Cannot read properties of undefined (reading 'outputs')"))
        };
        let (mime_types, pick) = output_view_model.resolve_mime_types(notebook, None);
        let Some(picked) = mime_types.get(pick) else {
            errors::throw(Error::type_error("Cannot read properties of undefined (reading 'mimeType')"))
        };
        let mime_type = picked.mime_type.clone();
        let outputs = output_text_model.outputs();
        let mut buffer = outputs.iter().find(|output| output.mime == mime_type);

        if buffer.is_none() || mime_type.starts_with("image") {
            buffer = outputs.iter().find(|output| !output.mime.starts_with("image"));
        }

        let Some(buffer) = buffer else {
            i += 1;
            continue;
        };

        let text = if is_text_stream_mime(&mime_type) {
            let (stream, count) = get_output_stream_text(output_view_model.as_ref());
            if count > 1 {
                i += count - 1;
            }
            stream
        } else {
            get_output_text(&mime_type, buffer, short_errors)
        };

        output_text.push(text);
        i += 1;
    }

    if output_text.len() > 1 {
        let total = output_text.len();
        output_text.iter().enumerate().map(|(i, output)| format!("Cell output {} of {}\n{}", i + 1, total, output)).collect::<Vec<_>>().join("\n")
    } else {
        output_text.into_iter().next().unwrap_or_default()
    }
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
