//! Porte parcial de `src/vs/workbench/contrib/files/browser/fileActions.ts`: a numeração de cópias (`incrementFileName`),
//! o recortar, copiar e colar do explorador (`cutFileHandler`, `copyFileHandler`, `pasteFileHandler`) e o que eles
//! chamam (`findValidPasteFileTarget`, `askForOverwrite`, `getFilesToPaste`).
//!
//! As ações e comandos (`GlobalCompareResourcesAction`, `ToggleAutoSaveAction`, `BaseSaveAllAction`, `FocusFilesExplorer`,
//! `openExplorerAndCreate`, `renameHandler`, `deleteFileHandler`, `downloadFileHandler`...) entram com
//! os serviços que eles leem (vistas, palavras-chave de contexto, `IWorkingCopyFileService`...). O `validateFileName`
//! (e o `trimLongName` e o `getWellFormedFileName` que ele chama) já está aqui.
//!
//! Notas de tradução:
//! - **`ServicesAccessor`.** Os handlers recebem o [`ServicesAccessor`] e leem os serviços de forma síncrona, antes do
//!   primeiro `await`, como o original (o acessor só vale durante a invocação); o corpo `async` é o [`async_fn`], que roda
//!   até o primeiro `await` na hora, e devolve a promessa que o `await instantiationService.invokeFunction(...)` espera.
//! - **`IHostService`.** O `pasteFileHandler` o lê só para repassar a `getFilesToPaste`, que não o usa; o porte não o lê.
//! - **`FileList` e `File`.** O `FileList` do navegador é [`FileList`] (uma lista de [`NativeFile`]), e o `File` é o
//!   [`NativeFile`]: `name`, `getPathForFile(file)` ([`NativeFile::path`]) e `arrayBuffer()`.
//! - **`let pasteShouldMove`.** Estado do módulo: um `thread_local` (o mesmo escopo que o resto do porte usa).
//! - **`'simple' | 'smart' | 'disabled'`.** São o [`IncrementalNamingSetting`] (a configuração) e o [`IncrementalNaming`]
//!   (o que `incrementFileName` aceita).
//! - **Expressões regulares.** O `incrementFileName` usa o mesmo motor de expressões regulares do JavaScript (`zyarr`),
//!   sobre unidades UTF-16, e o `String#replace` com função (sem a flag `g`) troca só o primeiro casamento.
//! - **`throw` em `async`.** Vira o `Err` do `Result` do corpo (a promessa rejeitada); o `try/catch/finally` do
//!   `pasteFileHandler` é o `Result` do corpo da tentativa seguido da notificação de erro e do bloco final.
//! - **`undefined` desreferenciado.** O `element.parent!` e o `explorerService.roots[0]` ausentes viram o `TypeError` que o
//!   JavaScript lançaria ao ler `.resource` de `undefined`.

use std::cell::Cell;
use std::ops::Range;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::buffer::VSBuffer;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::error_message::to_error_message_of;
use je_base::errors::Error;
use je_base::js_string::is_js_whitespace;
use je_base::nls::localize;
use je_base::path::PathModule;
use je_base::platform::OperatingSystem;
use je_base::promise::Promise;
use je_base::resources::distinct_parents;
use je_base::severity::Severity;
use je_base::uri::URI;
use je_editor::bulk_edit_service::ResourceFileEdit;
use je_editor::code_action::languages::WorkspaceFileEditOptions;
use je_editor::services::clipboard_service::{I_CLIPBOARD_SERVICE, IClipboardService};
use je_instantiation::ServicesAccessor;
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::dialogs::dialogs::{
    BaseDialogOptions, Checkbox, Confirmation, DialogDetail, DialogTypeOrSeverity, FileNameOrResource, I_DIALOG_SERVICE, IDialogService, get_file_names_message,
};
use je_platform::files::i_file_service::{I_FILE_SERVICE, IFileService};
use je_platform::notification::notification::{I_NOTIFICATION_SERVICE, INotificationService, NotificationMessage};
use je_platform::resources::{ext_uri, fs_path, host_environment, join_path, uri_file};
use je_platform::uri_identity::uri_identity::{I_URI_IDENTITY_SERVICE, IUriIdentityService};
use zyarr::RegExp;

use super::browser_files::{ExplorerBulkEditOptions, I_EXPLORER_SERVICE, IExplorerService, ValidationMessage};
use super::common_files::{IncrementalNaming, IncrementalNamingSetting, UndoConfirmLevel};
use super::explorer_model::ExplorerItem;
use crate::services::editor::editor_service::{I_EDITOR_SERVICE, IEditorService};
use crate::services::path::path_service::IPathService;

/// `Constants.MAX_SAFE_SMALL_INTEGER` de `src/vs/base/common/uint.ts` (`1 << 30`).
const MAX_SAFE_SMALL_INTEGER: f64 = (1u32 << 30) as f64;

thread_local! {
    /// `let pasteShouldMove = false`.
    static PASTE_SHOULD_MOVE: Cell<bool> = const { Cell::new(false) };
}

fn paste_should_move() -> bool {
    PASTE_SHOULD_MOVE.with(Cell::get)
}

fn set_paste_should_move(value: bool) {
    PASTE_SHOULD_MOVE.with(|flag| flag.set(value));
}

// region: o módulo `path` nativo (`extname`, `basename` e `isAbsolute` de `base/common/path`)

fn native_extname(path: &str) -> String {
    let env = host_environment();
    PathModule::native(env.as_ref()).extname(path)
}

fn native_basename(path: &str, suffix: Option<&str>) -> String {
    let env = host_environment();
    PathModule::native(env.as_ref()).basename(path, suffix)
}

fn native_is_absolute(path: &str) -> bool {
    let env = host_environment();
    PathModule::native(env.as_ref()).is_absolute(path)
}

// endregion

/// O `TypeError` de ler uma propriedade de `undefined`.
fn undefined_property(property: &str) -> Error {
    Error::type_error(format!("Cannot read properties of undefined (reading '{property}')"))
}

/// `explorer.<name>` da configuração (`configurationService.getValue<IFilesConfiguration>().explorer.<name>`).
fn explorer_setting(configuration_service: &dyn IConfigurationService, name: &str) -> Dynamic {
    configuration_service.get_value(None, None).get_str("explorer").get_str(name)
}

/// O texto de uma configuração (`None` para `undefined` ou um valor que não é texto).
fn setting_text(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

/// `explorer.incrementalNaming`.
fn incremental_naming_setting(configuration_service: &dyn IConfigurationService) -> IncrementalNamingSetting {
    IncrementalNamingSetting::from_setting(setting_text(&explorer_setting(configuration_service, "incrementalNaming")).as_deref())
}

/// `explorer.confirmUndo`.
fn confirm_undo(configuration_service: &dyn IConfigurationService) -> Option<String> {
    setting_text(&explorer_setting(configuration_service, "confirmUndo"))
}

/// O `File` do navegador: o que o colar lê de cada item de um `FileList`.
pub trait NativeFile {
    /// `file.name`.
    fn name(&self) -> String;

    /// `getPathForFile(file)` (`platform/dnd/browser/dnd.ts`): o caminho em disco do arquivo, quando o ambiente o expõe.
    fn path(&self) -> Option<String>;

    /// `file.arrayBuffer()`.
    fn array_buffer(&self) -> Promise<Vec<u8>>;
}

/// `FileList`.
pub type FileList = Vec<Rc<dyn NativeFile>>;

/// `{ resource: URI | string; ... }` de `fileToPaste`.
#[derive(Clone, Debug)]
pub enum PasteResource {
    Uri(URI),
    /// O nome do arquivo (o `string`).
    Name(String),
}

/// O `fileToPaste` de `findValidPasteFileTarget`: `{ resource: URI | string; isDirectory?: boolean; allowOverwrite: boolean }`.
#[derive(Clone, Debug)]
pub struct FileToPaste {
    pub resource: PasteResource,
    pub is_directory: Option<bool>,
    pub allow_overwrite: bool,
}

pub fn find_valid_paste_file_target(
    explorer_service: Rc<dyn IExplorerService>,
    file_service: Rc<dyn IFileService>,
    dialog_service: Rc<dyn IDialogService>,
    target_folder: Rc<ExplorerItem>,
    file_to_paste: FileToPaste,
    incremental_naming: IncrementalNamingSetting,
) -> Promise<Option<URI>> {
    async_fn(async move {
        let mut name = match &file_to_paste.resource {
            PasteResource::Name(name) => name.clone(),
            PasteResource::Uri(resource) => ext_uri().basename_or_authority(resource),
        };
        let mut candidate = join_path(&target_folder.resource(), &[name.as_str()]);

        // In the disabled case we must ask if it's ok to overwrite the file if it exists
        if incremental_naming == IncrementalNamingSetting::Disabled {
            let can_overwrite = ask_for_overwrite(file_service.clone(), dialog_service.clone(), candidate.clone()).await?;
            if !can_overwrite {
                return Ok(None);
            }
        }

        while !file_to_paste.allow_overwrite {
            if explorer_service.find_closest(&candidate).is_none() {
                break;
            }

            if let Some(naming) = incremental_naming.enabled() {
                name = increment_file_name(&name, file_to_paste.is_directory.unwrap_or(false), naming);
            }
            candidate = join_path(&target_folder.resource(), &[name.as_str()]);
        }

        Ok(Some(candidate))
    })
}

// region: incrementFileName

/// `String#length`-preservando: o texto como unidades UTF-16.
fn utf16(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn from_utf16(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// `new RegExp(pattern)`: os padrões do `incrementFileName` são fixos e válidos.
fn regex(pattern: &str) -> RegExp {
    RegExp::from_str(pattern, "").expect("o padrão fixo do incrementFileName é uma expressão regular válida")
}

/// O casamento de `name.match(regex)` / `regex.exec(name)`: a entrada e os grupos de captura.
struct Found {
    input: Vec<u16>,
    captures: Vec<Option<Range<usize>>>,
}

impl Found {
    /// `regex.exec(input)` sem a flag `g`.
    fn exec(regex: &RegExp, input: &[u16]) -> Option<Found> {
        regex.exec(input, 0).map(|found| Found { input: input.to_vec(), captures: found.captures })
    }

    /// `match[n]`: `None` é o grupo que não participou (`undefined`).
    fn group(&self, index: usize) -> Option<&[u16]> {
        self.captures.get(index).and_then(Option::as_ref).map(|range| &self.input[range.clone()])
    }

    /// O grupo que o padrão sempre captura.
    fn required_group(&self, index: usize) -> &[u16] {
        self.group(index).unwrap_or_default()
    }

    /// `input.replace(regex, () => replacement)`: o texto antes do casamento, a troca e o texto depois.
    fn replace_with(&self, replacement: &[u16]) -> Vec<u16> {
        let whole = self.captures[0].clone().expect("o grupo 0 sempre participa do casamento");
        let mut result = self.input[..whole.start].to_vec();
        result.extend_from_slice(replacement);
        result.extend_from_slice(&self.input[whole.end..]);
        result
    }
}

/// `parseInt(text)`: o número inteiro do começo do texto, depois de espaços; `NaN` se não há dígito.
fn parse_int(text: &[u16]) -> f64 {
    let text = from_utf16(text);
    let digits: String = text.trim_start().chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() { f64::NAN } else { digits.parse::<f64>().unwrap_or(f64::INFINITY) }
}

/// `String(number + 1).padStart(length, '0')`, com `number + 1` inteiro (o número é menor que `1 << 30`).
fn pad_incremented(number: f64, length: usize) -> Vec<u16> {
    utf16(&format!("{:0>length$}", number as i64 + 1))
}

fn concat(parts: &[&[u16]]) -> Vec<u16> {
    parts.iter().flat_map(|part| part.iter().copied()).collect()
}

pub fn increment_file_name(name: &str, is_folder: bool, incremental_naming: IncrementalNaming) -> String {
    if incremental_naming == IncrementalNaming::Simple {
        let mut name_prefix = name.to_string();
        let mut ext_suffix = String::new();
        if !is_folder {
            ext_suffix = native_extname(name);
            name_prefix = native_basename(name, Some(&ext_suffix));
        }

        // name copy 5(.txt) => name copy 6(.txt)
        // name copy(.txt) => name copy 2(.txt)
        let suffix_regex = regex(r"^(.+ copy)( \d+)?$");
        if let Some(found) = Found::exec(&suffix_regex, &utf16(&name_prefix)) {
            let g1 = found.required_group(1);
            let g2 = found.group(2);
            let number = match g2 {
                Some(g2) if !g2.is_empty() => parse_int(g2),
                _ => 1.0,
            };
            let replacement = if number == 0.0 {
                g1.to_vec()
            } else if number < MAX_SAFE_SMALL_INTEGER {
                concat(&[g1, &utf16(" "), &utf16(&(number as i64 + 1).to_string())])
            } else {
                concat(&[g1, g2.unwrap_or_default(), &utf16(" copy")])
            };
            return from_utf16(&found.replace_with(&replacement)) + &ext_suffix;
        }

        // name(.txt) => name copy(.txt)
        return format!("{name_prefix} copy{ext_suffix}");
    }

    let separators = r"[\.\-_]";
    let max_number = MAX_SAFE_SMALL_INTEGER;
    let name_units = utf16(name);

    // file.1.txt=>file.2.txt
    let suffix_file_regex = regex(&format!(r"(.*{separators})(\d+)(\..*)$"));
    if !is_folder && let Some(found) = Found::exec(&suffix_file_regex, &name_units) {
        let (g1, g2, g3) = (found.required_group(1), found.required_group(2), found.required_group(3));
        let number = parse_int(g2);
        let replacement = if number < max_number { concat(&[g1, &pad_incremented(number, g2.len()), g3]) } else { concat(&[g1, g2, &utf16(".1"), g3]) };
        return from_utf16(&found.replace_with(&replacement));
    }

    // 1.file.txt=>2.file.txt
    let prefix_file_regex = regex(&format!(r"(\d+)({separators}.*)(\..*)$"));
    if !is_folder && let Some(found) = Found::exec(&prefix_file_regex, &name_units) {
        let (g1, g2, g3) = (found.required_group(1), found.required_group(2), found.required_group(3));
        let number = parse_int(g1);
        let replacement = if number < max_number { concat(&[&pad_incremented(number, g1.len()), g2, g3]) } else { concat(&[g1, g2, &utf16(".1"), g3]) };
        return from_utf16(&found.replace_with(&replacement));
    }

    // 1.txt=>2.txt
    let prefix_file_no_name_regex = regex(r"(\d+)(\..*)$");
    if !is_folder && let Some(found) = Found::exec(&prefix_file_no_name_regex, &name_units) {
        let (g1, g2) = (found.required_group(1), found.required_group(2));
        let number = parse_int(g1);
        let replacement = if number < max_number { concat(&[&pad_incremented(number, g1.len()), g2]) } else { concat(&[g1, &utf16(".1"), g2]) };
        return from_utf16(&found.replace_with(&replacement));
    }

    // file.txt=>file.1.txt
    let last_index_of_dot = name_units.iter().rposition(|&unit| unit == u16::from(b'.'));
    if !is_folder && let Some(index) = last_index_of_dot {
        return format!("{}.1{}", from_utf16(&name_units[..index]), from_utf16(&name_units[index..]));
    }

    // 123 => 124
    let no_name_no_extension_regex = regex(r"(\d+)$");
    if !is_folder && last_index_of_dot.is_none() && let Some(found) = Found::exec(&no_name_no_extension_regex, &name_units) {
        let g1 = found.required_group(1);
        let number = parse_int(g1);
        let replacement = if number < max_number { pad_incremented(number, g1.len()) } else { concat(&[g1, &utf16(".1")]) };
        return from_utf16(&found.replace_with(&replacement));
    }

    // file => file1
    // file1 => file2
    let no_extension_regex = regex(r"(.*)(\d*)$");
    if !is_folder && last_index_of_dot.is_none() && let Some(found) = Found::exec(&no_extension_regex, &name_units) {
        let (g1, g2) = (found.required_group(1), found.required_group(2));
        let mut number = parse_int(g2);
        if number.is_nan() {
            number = 0.0;
        }
        let replacement = if number < max_number { concat(&[g1, &pad_incremented(number, g2.len())]) } else { concat(&[g1, g2, &utf16(".1")]) };
        return from_utf16(&found.replace_with(&replacement));
    }

    // folder.1=>folder.2
    let folder_suffix_regex = regex(r"(\d+)$");
    if is_folder && let Some(found) = Found::exec(&folder_suffix_regex, &name_units) {
        let g1 = found.required_group(1);
        let number = parse_int(g1);
        let replacement = if number < max_number { pad_incremented(number, g1.len()) } else { concat(&[g1, &utf16(".1")]) };
        return from_utf16(&found.replace_with(&replacement));
    }

    // 1.folder=>2.folder
    if is_folder && Found::exec(&regex(r"^(\d+)"), &name_units).is_some() {
        let folder_prefix_regex = regex(r"^(\d+)(.*)$");
        if let Some(found) = Found::exec(&folder_prefix_regex, &name_units) {
            let (g1, g2) = (found.required_group(1), found.required_group(2));
            let number = parse_int(g1);
            let replacement = if number < max_number { concat(&[&pad_incremented(number, g1.len()), g2]) } else { concat(&[g1, g2, &utf16(".1")]) };
            return from_utf16(&found.replace_with(&replacement));
        }
    }

    // file/folder=>file.1/folder.1
    format!("{name}.1")
}

// endregion

/// Checks to see if the resource already exists, if so prompts the user if they would be ok with it being overwritten.
///
/// `file_service`: the file service. `dialog_service`: the dialog service. `target_resource`: the resource to be overwritten.
/// Resolves with a boolean indicating if the user is ok with resource being overwritten, if the resource does not exist it
/// returns `true`.
async fn ask_for_overwrite(file_service: Rc<dyn IFileService>, dialog_service: Rc<dyn IDialogService>, target_resource: URI) -> Result<bool, Error> {
    let exists = file_service.exists(&target_resource).await?;
    if !exists {
        return Ok(true);
    }
    // Ask for overwrite confirmation
    let message = localize(
        "confirmOverwrite",
        "A file or folder with the name '{0}' already exists in the destination folder. Do you want to replace it?",
        &[&native_basename(&target_resource.path().to_string_lossy(), None)],
    );
    let confirmation = dialog_service
        .confirm(Confirmation {
            base: BaseDialogOptions { dialog_type: Some(DialogTypeOrSeverity::Severity(Severity::Warning)), ..BaseDialogOptions::new(message) },
            primary_button: Some(localize("replaceButtonLabel", "&&Replace", &[])),
            cancel_button: None,
        })
        .await?;
    Ok(confirmation.confirmed)
}

/// `copyFileHandler`.
pub fn copy_file_handler(accessor: &ServicesAccessor) -> Promise<()> {
    let explorer_service = accessor.get(I_EXPLORER_SERVICE);
    async_fn(async move {
        let stats = explorer_service.get_context(true, None);
        if !stats.is_empty() {
            explorer_service.set_to_copy(stats, false).await?;
            set_paste_should_move(false);
        }
        Ok(())
    })
}

/// `cutFileHandler`.
pub fn cut_file_handler(accessor: &ServicesAccessor) -> Promise<()> {
    let explorer_service = accessor.get(I_EXPLORER_SERVICE);
    async_fn(async move {
        let stats = explorer_service.get_context(true, None);
        if !stats.is_empty() {
            explorer_service.set_to_copy(stats, true).await?;
            set_paste_should_move(true);
        }
        Ok(())
    })
}

/// Os serviços e o destino que o colar usa em cada arquivo.
struct PasteContext {
    explorer_service: Rc<dyn IExplorerService>,
    file_service: Rc<dyn IFileService>,
    dialog_service: Rc<dyn IDialogService>,
    editor_service: Rc<dyn IEditorService>,
    configuration_service: Rc<dyn IConfigurationService>,
    uri_identity_service: Rc<dyn IUriIdentityService>,
    /// `element`: o item de destino (`context[0]` ou a primeira raiz).
    element: Option<Rc<ExplorerItem>>,
    incremental_naming: IncrementalNamingSetting,
}

impl PasteContext {
    /// `element`, que o JavaScript desreferencia sem checar.
    fn element(&self) -> Result<Rc<ExplorerItem>, Error> {
        self.element.clone().ok_or_else(|| undefined_property("resource"))
    }

    /// `allowOverwrite: pasteShouldMove || incrementalNaming === 'disabled'`.
    fn allow_overwrite(&self) -> bool {
        paste_should_move() || self.incremental_naming == IncrementalNamingSetting::Disabled
    }

    /// `overwrite: incrementalNaming === 'disabled'`.
    fn overwrite(&self) -> bool {
        self.incremental_naming == IncrementalNamingSetting::Disabled
    }

    fn find_valid_target(&self, target: Rc<ExplorerItem>, file_to_paste: FileToPaste) -> Promise<Option<URI>> {
        find_valid_paste_file_target(
            self.explorer_service.clone(),
            self.file_service.clone(),
            self.dialog_service.clone(),
            target,
            file_to_paste,
            self.incremental_naming,
        )
    }

    /// `applyCopyResourceEdit(targets, resourceFileEdits)`.
    async fn apply_copy_resource_edit(&self, targets: &[URI], resource_file_edits: Vec<ResourceFileEdit>) -> Result<(), Error> {
        let undo_level = confirm_undo(self.configuration_service.as_ref());
        let first_target = || targets.first().map(|target| ext_uri().basename_or_authority(target)).ok_or_else(|| undefined_property("path"));
        let options = ExplorerBulkEditOptions {
            confirm_before_undo: Some(UndoConfirmLevel::Default.is(undo_level.as_deref()) || UndoConfirmLevel::Verbose.is(undo_level.as_deref())),
            progress_label: if targets.len() > 1 {
                localize("copyingBulkEdit", "Copying {0} files", &[&targets.len().to_string()])
            } else {
                localize("copyingFileBulkEdit", "Copying {0}", &[&first_target()?])
            },
            undo_label: if targets.len() > 1 {
                localize("copyBulkEdit", "Paste {0} files", &[&targets.len().to_string()])
            } else {
                localize("copyFileBulkEdit", "Paste {0}", &[&first_target()?])
            },
            progress_location: None,
        };
        self.explorer_service.apply_bulk_edit(resource_file_edits, options).await
    }
}

/// O `{ source, target }` de `sourceTargetPairs`.
#[derive(Clone)]
struct SourceTargetPair {
    source: URI,
    target: URI,
}

/// Um dos arquivos do colar a partir de caminhos em disco (o corpo do `map` de `sourceTargetPairs`).
async fn find_source_target_pair(context: Rc<PasteContext>, file_to_paste: URI) -> Result<Option<SourceTargetPair>, Error> {
    let element = context.element()?;

    // Check if target is ancestor of pasted folder
    if element.resource().to_string(false) != file_to_paste.to_string(false) && ext_uri().is_equal_or_parent(&element.resource(), &file_to_paste, false, false) {
        return Err(Error::new(localize("fileIsAncestor", "File to paste is an ancestor of the destination folder", &[])));
    }
    let file_to_paste_stat = context.file_service.stat(&file_to_paste).await?;

    // Find target
    let target = if context.uri_identity_service.ext_uri().is_equal(Some(&element.resource()), Some(&file_to_paste), false) {
        element.parent()
    } else if element.is_directory() {
        Some(element.clone())
    } else {
        element.parent()
    };
    let target = target.ok_or_else(|| undefined_property("resource"))?;

    let target_file = context
        .find_valid_target(
            target,
            FileToPaste { resource: PasteResource::Uri(file_to_paste.clone()), is_directory: Some(file_to_paste_stat.is_directory), allow_overwrite: context.allow_overwrite() },
        )
        .await?;

    Ok(target_file.map(|target| SourceTargetPair { source: file_to_paste, target }))
}

/// O `{ target, edit }` de `targetAndEdits`.
#[derive(Clone)]
struct TargetAndEdit {
    target: URI,
    edit: ResourceFileEdit,
}

/// Um dos arquivos do colar a partir de dados (o corpo do `map` de `targetAndEdits`).
async fn find_target_and_edit(context: Rc<PasteContext>, file: Rc<dyn NativeFile>) -> Result<Option<TargetAndEdit>, Error> {
    let element = context.element()?;
    let target = if element.is_directory() { Some(element.clone()) } else { element.parent() };
    let target = target.ok_or_else(|| undefined_property("resource"))?;

    let target_file = context
        .find_valid_target(target, FileToPaste { resource: PasteResource::Name(file.name()), is_directory: Some(false), allow_overwrite: context.allow_overwrite() })
        .await?;
    let Some(target_file) = target_file else { return Ok(None) };

    let options = WorkspaceFileEditOptions {
        overwrite: Some(context.overwrite()),
        contents: Some(file.array_buffer().then(VSBuffer::wrap)),
        ..WorkspaceFileEditOptions::default()
    };
    Ok(Some(TargetAndEdit { target: target_file.clone(), edit: ResourceFileEdit::new(None, Some(target_file), Some(options), None) }))
}

/// O corpo do `try` do `pasteFileHandler`: cola e seleciona (e abre) o resultado.
async fn paste_files(context: Rc<PasteContext>, to_paste: FilesToPaste) -> Result<(), Error> {
    let targets: Vec<URI>;

    match to_paste {
        // Pasting from files on disk
        FilesToPaste::Paths(files) => {
            let pairs = Promise::all(files.into_iter().map(|file| async_fn(find_source_target_pair(context.clone(), file))).collect()).await?;
            let source_target_pairs: Vec<SourceTargetPair> = pairs.into_iter().flatten().collect();

            if !source_target_pairs.is_empty() {
                // Move/Copy File
                if paste_should_move() {
                    let resource_file_edits = source_target_pairs
                        .iter()
                        .map(|pair| {
                            ResourceFileEdit::new(
                                Some(pair.source.clone()),
                                Some(pair.target.clone()),
                                Some(WorkspaceFileEditOptions { overwrite: Some(context.overwrite()), ..WorkspaceFileEditOptions::default() }),
                                None,
                            )
                        })
                        .collect();
                    let first_target_name = || ext_uri().basename_or_authority(&source_target_pairs[0].target);
                    let many = source_target_pairs.len() > 1;
                    let count = source_target_pairs.len().to_string();
                    let options = ExplorerBulkEditOptions {
                        confirm_before_undo: Some(UndoConfirmLevel::Verbose.is(confirm_undo(context.configuration_service.as_ref()).as_deref())),
                        progress_label: if many { localize("movingBulkEdit", "Moving {0} files", &[&count]) } else { localize("movingFileBulkEdit", "Moving {0}", &[&first_target_name()]) },
                        undo_label: if many { localize("moveBulkEdit", "Move {0} files", &[&count]) } else { localize("moveFileBulkEdit", "Move {0}", &[&first_target_name()]) },
                        progress_location: None,
                    };
                    context.explorer_service.apply_bulk_edit(resource_file_edits, options).await?;
                } else {
                    let resource_file_edits = source_target_pairs
                        .iter()
                        .map(|pair| {
                            ResourceFileEdit::new(
                                Some(pair.source.clone()),
                                Some(pair.target.clone()),
                                Some(WorkspaceFileEditOptions { copy: Some(true), overwrite: Some(context.overwrite()), ..WorkspaceFileEditOptions::default() }),
                                None,
                            )
                        })
                        .collect();
                    let copy_targets: Vec<URI> = source_target_pairs.iter().map(|pair| pair.target.clone()).collect();
                    context.apply_copy_resource_edit(&copy_targets, resource_file_edits).await?;
                }
            }

            targets = source_target_pairs.into_iter().map(|pair| pair.target).collect();
        }

        // Pasting from file data
        FilesToPaste::Data(files) => {
            let found = Promise::all(files.into_iter().map(|file| async_fn(find_target_and_edit(context.clone(), file))).collect()).await?;
            let target_and_edits: Vec<TargetAndEdit> = found.into_iter().flatten().collect();

            let edit_targets: Vec<URI> = target_and_edits.iter().map(|pair| pair.target.clone()).collect();
            context.apply_copy_resource_edit(&edit_targets, target_and_edits.into_iter().map(|pair| pair.edit).collect()).await?;
            targets = edit_targets;
        }
    }

    if let Some(first_target) = targets.first() {
        context.explorer_service.select(first_target, None).await?;
        if targets.len() == 1 {
            let item = context.explorer_service.find_closest(first_target);
            if let Some(item) = item
                && !item.is_directory()
            {
                use crate::common::editor::{TextEditorOptions, UntypedEditorInput};
                let mut options = TextEditorOptions::default();
                options.pinned = Some(true);
                options.preserve_focus = Some(true);
                context.editor_service.open_editor(UntypedEditorInput { resource: Some(item.resource()), options: Some(options), ..UntypedEditorInput::default() }).await?;
            }
        }
    }
    Ok(())
}

/// `pasteFileHandler`.
pub fn paste_file_handler(accessor: &ServicesAccessor, file_list: Option<FileList>) -> Promise<()> {
    let clipboard_service = accessor.get(I_CLIPBOARD_SERVICE);
    let explorer_service = accessor.get(I_EXPLORER_SERVICE);
    let file_service = accessor.get(I_FILE_SERVICE);
    let notification_service = accessor.get(I_NOTIFICATION_SERVICE);
    let editor_service = accessor.get(I_EDITOR_SERVICE);
    let configuration_service = accessor.get(I_CONFIGURATION_SERVICE);
    let uri_identity_service = accessor.get(I_URI_IDENTITY_SERVICE);
    let dialog_service = accessor.get(I_DIALOG_SERVICE);

    async_fn(async move {
        let context = explorer_service.get_context(false, None);
        let has_native_files_to_paste = file_list.as_ref().is_some_and(|files| !files.is_empty());
        let confirm_paste_native = has_native_files_to_paste && configuration_service.get_value(Some("explorer.confirmPasteNative"), None).is_truthy();

        let to_paste = get_files_to_paste(file_list, clipboard_service).await?;

        if confirm_paste_native && to_paste.len() >= 1 {
            let message = if to_paste.len() > 1 {
                localize("confirmMultiPasteNative", "Are you sure you want to paste the following {0} items?", &[&to_paste.len().to_string()])
            } else {
                localize("confirmPasteNative", "Are you sure you want to paste '{0}'?", &[&native_basename(&to_paste.names()[0], None)])
            };
            let detail = if to_paste.len() > 1 {
                let names: Vec<FileNameOrResource> = to_paste.names().into_iter().map(FileNameOrResource::Name).collect();
                Some(get_file_names_message(&names))
            } else {
                None
            };
            let confirmation = dialog_service
                .confirm(Confirmation {
                    base: BaseDialogOptions {
                        detail: detail.map(DialogDetail::Text),
                        checkbox: Some(Checkbox { label: localize("doNotAskAgain", "Do not ask me again", &[]), checked: None }),
                        ..BaseDialogOptions::new(message)
                    },
                    primary_button: Some(localize("pasteButtonLabel", "&&Paste", &[])),
                    cancel_button: None,
                })
                .await?;

            if !confirmation.confirmed {
                return Ok(());
            }

            // Check for confirmation checkbox
            if confirmation.checkbox_checked == Some(true) {
                configuration_service.update_value("explorer.confirmPasteNative", Dynamic::Bool(false), None, None).await?;
            }
        }
        let element = context.first().cloned().or_else(|| explorer_service.roots().first().cloned());
        let incremental_naming = incremental_naming_setting(configuration_service.as_ref());

        let editable_item = explorer_service.get_editable();
        // If it's an editable item, just do nothing
        if editable_item.is_some() {
            return Ok(());
        }

        let paste_context = Rc::new(PasteContext {
            explorer_service: explorer_service.clone(),
            file_service,
            dialog_service,
            editor_service,
            configuration_service,
            uri_identity_service,
            element,
            incremental_naming,
        });
        if let Err(error) = paste_files(paste_context, to_paste).await {
            notification_service.error(vec![NotificationMessage::Text(to_error_message_of(&error, false))]);
        }

        // finally
        if paste_should_move() {
            // Cut is done. Make sure to clear cut state.
            explorer_service.set_to_copy(Vec::new(), false).await?;
            set_paste_should_move(false);
        }
        Ok(())
    })
}

/// `FilesToPaste`: `{ type: 'paths'; files: URI[] } | { type: 'data'; files: File[] }`.
enum FilesToPaste {
    Paths(Vec<URI>),
    Data(Vec<Rc<dyn NativeFile>>),
}

impl FilesToPaste {
    fn len(&self) -> usize {
        match self {
            FilesToPaste::Paths(files) => files.len(),
            FilesToPaste::Data(files) => files.len(),
        }
    }

    /// O texto de cada item na confirmação do colar: o `fsPath` do recurso e o `name` do arquivo.
    fn names(&self) -> Vec<String> {
        match self {
            FilesToPaste::Paths(files) => files.iter().map(fs_path).collect(),
            FilesToPaste::Data(files) => files.iter().map(|file| file.name()).collect(),
        }
    }
}

async fn get_files_to_paste(file_list: Option<FileList>, clipboard_service: Rc<dyn IClipboardService>) -> Result<FilesToPaste, Error> {
    match file_list {
        Some(file_list) if !file_list.is_empty() => {
            // with a `fileList` we support natively pasting file from disk from clipboard
            let resources: Vec<URI> = file_list
                .iter()
                .filter_map(|file| file.path())
                .filter(|file_path| !file_path.is_empty() && native_is_absolute(file_path))
                .map(|file_path| uri_file(&file_path))
                .collect();
            if !resources.is_empty() {
                return Ok(FilesToPaste::Paths(resources));
            }

            // Support pasting files that we can't read from disk
            Ok(FilesToPaste::Data(file_list.into_iter().filter(|file| file.path().is_none_or(|path| path.is_empty())).collect()))
        }
        // otherwise we fallback to reading resources from our clipboard service
        _ => {
            let resources = clipboard_service.read_resources().await?;
            let distinct: Vec<URI> = distinct_parents(&resources, |resource| resource.clone(), &ext_uri()).into_iter().cloned().collect();
            Ok(FilesToPaste::Paths(distinct))
        }
    }
}

/// `validateFileName(pathService, item, name, os)`: `name` é `None` para o `null!` dos testes.
pub fn validate_file_name(path_service: &dyn IPathService, item: &Rc<ExplorerItem>, name: Option<&str>, os: OperatingSystem) -> Option<ValidationMessage> {
    // Produce a well formed file name
    let name = name.map(get_well_formed_file_name);

    // Name not provided
    let Some(name) = name.filter(|name| !name.is_empty() && !name.chars().all(is_js_whitespace_char)) else {
        return Some(ValidationMessage { content: localize("emptyFileNameError", "A file or folder name must be provided.", &[]), severity: Severity::Error });
    };

    // Relative paths only
    if name.starts_with('/') || name.starts_with('\\') {
        return Some(ValidationMessage {
            content: localize("fileNameStartsWithSlashError", "A file or folder name cannot start with a slash.", &[]),
            severity: Severity::Error,
        });
    }

    let names: Vec<&str> = name.split(['\\', '/']).filter(|part| !part.is_empty()).collect();
    let parent = item.parent();

    if name != item.name() {
        // Do not allow to overwrite existing file
        if let Some(child) = parent.and_then(|parent| parent.get_child(&name))
            && !Rc::ptr_eq(&child, item)
        {
            return Some(ValidationMessage {
                content: localize(
                    "fileNameExistsError",
                    "A file or folder **{0}** already exists at this location. Please choose a different name.",
                    &[&name],
                ),
                severity: Severity::Error,
            });
        }
    }

    // Check for invalid file name.
    if names.iter().any(|folder_name| !path_service.has_valid_basename_for_os(&item.resource(), os, Some(*folder_name))) {
        // Escape * characters
        let escaped_name = name.replace('*', "\\*");
        return Some(ValidationMessage {
            content: localize(
                "invalidFileNameError",
                "The name **{0}** is not valid as a file or folder name. Please choose a different name.",
                &[&trim_long_name(&escaped_name)],
            ),
            severity: Severity::Error,
        });
    }

    if names.iter().any(|name| name.chars().next().is_some_and(is_js_whitespace_char) || name.chars().next_back().is_some_and(is_js_whitespace_char)) {
        return Some(ValidationMessage {
            content: localize("fileNameWhitespaceWarning", "Leading or trailing whitespace detected in file or folder name.", &[]),
            severity: Severity::Warning,
        });
    }

    None
}

/// O `\s` das expressões regulares do JavaScript.
fn is_js_whitespace_char(character: char) -> bool {
    u16::try_from(u32::from(character)).is_ok_and(is_js_whitespace)
}

/// `trimLongName(name)`: `name.length > 255` conta unidades UTF-16 e o `substr(0, 255)` corta nelas.
fn trim_long_name(name: &str) -> String {
    let units: Vec<u16> = name.encode_utf16().collect();
    if units.len() > 255 {
        return format!("{}...", from_utf16(&units[..255]));
    }

    name.to_string()
}

/// `getWellFormedFileName(filename)`.
fn get_well_formed_file_name(filename: &str) -> String {
    if filename.is_empty() {
        return filename.to_string();
    }

    // Trim tabs
    let filename = filename.trim_matches('\t');

    // Remove trailing slashes
    let filename = filename.trim_end_matches('/');
    filename.trim_end_matches('\\').to_string()
}
