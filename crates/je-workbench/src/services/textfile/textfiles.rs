//! Porte parcial de `src/vs/workbench/services/textfile/common/textfiles.ts`: o contrato `ITextFileService` reduzido ao
//! que as entradas de editor de texto leem dele (`save`, `saveAs` e `revert`), as opções de salvar do serviço, o
//! `ILanguageSupport` e os auxiliares de `ITextSnapshot`.
//!
//! Divergências:
//! - **Contrato reduzido.** `files`, `untitled`, `encoding`, `isDirty`, `read`, `readStream`, `write`, `create` e os
//!   métodos de codificação do serviço entram com o porte do `TextFileService` e dos modelos de arquivo de texto.
//! - **`extends`.** `ITextFileSaveOptions extends ISaveOptions` é [`TextFileSaveOptions`], que guarda as [`SaveOptions`]
//!   na base e dá `Deref` para elas; `ITextFileSaveAsOptions extends ITextFileSaveOptions` é [`TextFileSaveAsOptions`],
//!   que faz o mesmo com a [`TextFileSaveOptions`]. `{ ...options, suggestedTarget }` é a struct com o campo ajustado.
//! - **`ITextSnapshot`** é o [`TextSnapshot`] do `je-editor` (`read()` devolve `None` no fim, o `null` do original), e o
//!   texto de um pedaço é [`JsString`]; `snapshotToString` junta os pedaços sem perder unidades UTF-16.

use std::ops::Deref;

use je_base::JsString;
use je_base::promise::Promise;
use je_base::uri::URI;
use je_editor::model::TextSnapshot;
use je_instantiation::service_id;

use crate::common::editor::{RevertOptions, SaveOptions};

/// `ITextFileSaveOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextFileSaveOptions {
    /// A parte `ISaveOptions`.
    pub save: SaveOptions,

    /// Save the file with an attempt to unlock it.
    pub write_unlock: Option<bool>,

    /// Save the file with elevated privileges.
    ///
    /// Note: This may not be supported in all environments.
    pub write_elevated: Option<bool>,

    /// Allows to write to a file even if it has been modified on disk.
    pub ignore_modified_since: Option<bool>,

    /// If set, will bubble up the error to the caller instead of handling it.
    pub ignore_error_handler: Option<bool>,
}

impl Deref for TextFileSaveOptions {
    type Target = SaveOptions;

    fn deref(&self) -> &SaveOptions {
        &self.save
    }
}

impl From<SaveOptions> for TextFileSaveOptions {
    /// Um `ISaveOptions` onde se espera um `ITextFileSaveOptions` (os campos do serviço ficam `undefined`).
    fn from(save: SaveOptions) -> Self {
        Self { save, ..Self::default() }
    }
}

/// `ITextFileSaveAsOptions`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextFileSaveAsOptions {
    /// A parte `ITextFileSaveOptions`.
    pub text_file_save: TextFileSaveOptions,

    /// Optional URI of the resource the text file is saved from if known.
    pub from: Option<URI>,

    /// Optional URI to use as suggested file path to save as.
    pub suggested_target: Option<URI>,
}

impl Deref for TextFileSaveAsOptions {
    type Target = TextFileSaveOptions;

    fn deref(&self) -> &TextFileSaveOptions {
        &self.text_file_save
    }
}

/// `ITextFileService`.
pub trait ITextFileService {
    /// Saves the resource.
    ///
    /// `resource`: the resource to save. `options`: optional save options. Devolve o caminho do recurso salvo, ou `None`
    /// se foi cancelado.
    fn save(&self, resource: &URI, options: Option<&TextFileSaveOptions>) -> Promise<Option<URI>>;

    /// Saves the provided resource asking the user for a file name or using the provided one.
    ///
    /// `resource`: the resource to save as. `target_resource`: the optional target to save to. `options`: optional save
    /// options. Devolve o caminho do recurso salvo, ou `None` se foi cancelado.
    fn save_as(&self, resource: &URI, target_resource: Option<&URI>, options: Option<&TextFileSaveAsOptions>) -> Promise<Option<URI>>;

    /// Reverts the provided resource.
    ///
    /// `resource`: the resource of the file to revert. `options.force`: to force revert even when the file is not dirty.
    fn revert(&self, resource: &URI, options: Option<RevertOptions>) -> Promise<()>;
}

service_id!(pub I_TEXT_FILE_SERVICE: dyn ITextFileService = "textFileService");

/// `ILanguageSupport`.
pub trait ILanguageSupport {
    /// Sets the language id of the object.
    fn set_language_id(&self, language_id: &str, source: Option<&str>);
}

/// `snapshotToString(snapshot)`.
pub fn snapshot_to_string(snapshot: &mut dyn TextSnapshot) -> JsString {
    let mut units: Vec<u16> = Vec::new();

    while let Some(chunk) = snapshot.read() {
        units.extend_from_slice(chunk.units());
    }

    JsString::from_units(units)
}

/// `stringToSnapshot(value)`: um snapshot de um pedaço só.
pub fn string_to_snapshot(value: JsString) -> Box<dyn TextSnapshot> {
    Box::new(StringSnapshot { value: Some(value) })
}

struct StringSnapshot {
    value: Option<JsString>,
}

impl TextSnapshot for StringSnapshot {
    fn read(&mut self) -> Option<JsString> {
        self.value.take()
    }
}
