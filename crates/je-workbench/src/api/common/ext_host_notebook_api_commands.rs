//! Porte de `ExtHostNotebookController._registerApiCommands` (`src/vs/workbench/api/common/extHostNotebook.ts`): os comandos de API
//! `vscode.executeDataToNotebook` e `vscode.executeNotebookToData`, que a extensão chama e que o host traduz para os comandos
//! internos `_executeDataToNotebook` e `_executeNotebookToData` do workbench (`mainThreadNotebook.ts`).
//!
//! Divergências:
//! - **Representação dos valores.** Os argumentos e os resultados de um comando de API cruzam como [`Dynamic`]. O `VSBuffer`, o
//!   `SerializableObjectWithBuffers<NotebookDataDto>` e o `vscode.NotebookData` (que o porte tem como a struct
//!   [`types::NotebookData`], sem forma de objeto com propriedades) viajam como objetos [`Dynamic`] com carga nativa
//!   ([`ObjectRef::with_host`]); as funções `*_to_dynamic` e `*_from_dynamic` deste módulo são o par de cada um. O `Uint8Array`
//!   do argumento `data` e do resultado `Bytes` é o [`Dynamic::TypedArray`].
//! - **`ApiCommandArgument<Uint8Array, VSBuffer>`.** O `v instanceof Uint8Array` é o `matches!` da variante `TypedArray`; o
//!   `VSBuffer.wrap(v)` copia os bytes. O resultado `dto => dto.buffer` devolve os bytes do `VSBuffer`.
//! - **Registro.** O `ExtHostNotebookController` o chama no construtor, com o `ExtHostCommands` que recebe; o
//!   `registerApiCommand` lança no original quando o id já existe, e aqui devolve o [`Result`].

use std::rc::Rc;

use je_base::buffer::VSBuffer;
use je_base::dynamic::{Dynamic, ObjectRef};
use je_base::errors::Error;

use super::ext_host_commands::{ApiCommand, ApiCommandArgument, ApiCommandResult, ExtHostCommands};
use super::ext_host_protocol::{NotebookDataDto, SerializableObjectWithBuffers};
use super::ext_host_type_converters::NotebookData as NotebookDataConverter;
use super::ext_host_types as types;

/// Um `VSBuffer` como valor [`Dynamic`].
pub fn buffer_to_dynamic(buffer: VSBuffer) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(buffer)))
}

/// O `VSBuffer` que o valor carrega (`value instanceof VSBuffer`).
pub fn buffer_from_dynamic(value: &Dynamic) -> Option<Rc<VSBuffer>> {
    match value {
        Dynamic::Object(object) => object.host::<VSBuffer>(),
        _ => None,
    }
}

/// Um `SerializableObjectWithBuffers<NotebookDataDto>` como valor [`Dynamic`].
pub fn notebook_data_dto_to_dynamic(dto: SerializableObjectWithBuffers<NotebookDataDto>) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(dto)))
}

/// O `SerializableObjectWithBuffers<NotebookDataDto>` que o valor carrega.
pub fn notebook_data_dto_from_dynamic(value: &Dynamic) -> Option<SerializableObjectWithBuffers<NotebookDataDto>> {
    match value {
        Dynamic::Object(object) => object.host::<SerializableObjectWithBuffers<NotebookDataDto>>().map(|dto| (*dto).clone()),
        _ => None,
    }
}

/// Um `vscode.NotebookData` como valor [`Dynamic`].
pub fn notebook_data_to_dynamic(data: types::NotebookData) -> Dynamic {
    Dynamic::Object(ObjectRef::with_host(Rc::new(data)))
}

/// O `vscode.NotebookData` que o valor carrega.
pub fn notebook_data_from_dynamic(value: &Dynamic) -> Option<types::NotebookData> {
    match value {
        Dynamic::Object(object) => object.host::<types::NotebookData>().map(|data| (*data).clone()),
        _ => None,
    }
}

/// `ExtHostNotebookController._registerApiCommands(extHostCommands)`.
pub fn register_api_commands(ext_host_commands: &ExtHostCommands) -> Result<(), Error> {
    let notebook_type_arg = || ApiCommandArgument::string().with(Some("notebookType"), Some("A notebook type"));

    let command_data_to_notebook = ApiCommand::new(
        "vscode.executeDataToNotebook",
        "_executeDataToNotebook",
        "Invoke notebook serializer",
        vec![
            notebook_type_arg(),
            ApiCommandArgument::new(
                "data",
                "Bytes to convert to data",
                |value| matches!(value, Dynamic::TypedArray(_)),
                |value| match value {
                    Dynamic::TypedArray(bytes) => Ok(buffer_to_dynamic(VSBuffer::wrap(bytes.to_vec()))),
                    _ => Err(Error::type_error("Expected a Uint8Array")),
                },
            ),
        ],
        ApiCommandResult::new("Notebook Data", |data, _api_args, _converter| {
            let dto = notebook_data_dto_from_dynamic(data).ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'value')"))?;
            Ok(notebook_data_to_dynamic(NotebookDataConverter::to(&dto.value)?))
        }),
    );

    let command_notebook_to_data = ApiCommand::new(
        "vscode.executeNotebookToData",
        "_executeNotebookToData",
        "Invoke notebook serializer",
        vec![
            notebook_type_arg(),
            ApiCommandArgument::new(
                "NotebookData",
                "Notebook data to convert to bytes",
                |_| true,
                |value| {
                    let data = notebook_data_from_dynamic(value).ok_or_else(|| Error::type_error("Expected a NotebookData"))?;
                    Ok(notebook_data_dto_to_dynamic(SerializableObjectWithBuffers::new(NotebookDataConverter::from(&data))))
                },
            ),
        ],
        ApiCommandResult::new("Bytes", |dto, _api_args, _converter| {
            let buffer = buffer_from_dynamic(dto).ok_or_else(|| Error::type_error("Cannot read properties of undefined (reading 'buffer')"))?;
            Ok(Dynamic::typed_array(buffer.to_vec()))
        }),
    );

    ext_host_commands.register_api_command(command_data_to_notebook)?;
    ext_host_commands.register_api_command(command_notebook_to_data)?;
    Ok(())
}
