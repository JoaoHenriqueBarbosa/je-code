//! Porte de `src/vs/editor/contrib/snippet/browser/snippetVariables.ts`: os resolvedores das variáveis de
//! snippet (`$TM_FILENAME`, `$SELECTION`, `$CLIPBOARD`, `$CURRENT_YEAR`, `$WORKSPACE_NAME`, `$RANDOM`...).
//!
//! Decisões de tradução:
//! - Cada classe do original é uma struct que implementa [`VariableResolver`] (o `resolve(variable)` devolve
//!   `Option<JsString>`, o `string | undefined`). Os resolvedores são compartilhados por `Rc<dyn VariableResolver>`
//!   no [`CompositeSnippetVariableResolver`], porque o `SnippetSession` reaproveita o mesmo
//!   `ModelBasedVariableResolver` em todos os cursores.
//! - `ILabelService` (`platform/label/common/label.ts`) ainda não tem porte em `je-platform`: o trait
//!   [`ILabelService`] traz só o que o `snippetVariables.ts` chama, o `getUriLabel(resource, options?)`, com as
//!   opções em [`UriLabelOptions`]. O mesmo vale para o `OvertypingCapturer` (`suggestOvertypingCapturer.ts`, que
//!   depende do `SuggestModel`): o trait [`IOvertypingCapturer`] é o `getLastOvertypedInfo(idx)` que o
//!   [`SelectionBasedVariableResolver`] consulta.
//! - `IWorkspaceContextService` e as funções de `workspace.ts` são os de `je-platform`. O serviço é opcional
//!   (`IWorkspaceContextService | undefined`) e chega como `Option<Rc<dyn IWorkspaceContextService>>`.
//! - `ILanguageConfigurationService` do `CommentBasedVariableResolver` é o do crate (`@ILanguageConfigurationService`
//!   vira argumento do construtor).
//! - `path` (o módulo nativo do `path.ts`) é o [`PathModule`] sobre o ambiente do hospedeiro, e `dirname` de
//!   `resources.ts` é o do `ExtUri` sem viés. `uri.fsPath` é o da plataforma do hospedeiro.
//! - `IReadClipboardText` é o [`ReadClipboardText`]: `() => string | undefined` (o `null` que um teste passa de
//!   contrabando é o mesmo `None`).
//! - `new Date()` do `TimeBasedVariableResolver` é o relógio de [`runtime::date_now`] (o que acompanha o relógio
//!   virtual dos testes) no fuso local; [`TimeBasedVariableResolver::at`] fixa o instante, o que o `sinon` faz no
//!   original. `Intl.DateTimeFormat().resolvedOptions().timeZone` é o nome IANA do fuso do sistema (`UTC` quando
//!   o sistema não tem um).
//! - `Math.random()` é a fração de 53 bits de [`fill_bytes`]; `toString(16)` do V8 é a expansão hexadecimal
//!   exata da fração, sem zeros à direita.

use std::cell::RefCell;
use std::rc::Rc;

use je_base::JsString;
use je_base::char_code as cc;
use je_base::dynamic::number_to_js_string;
use je_base::labels::normalize_drive_letter;
use je_base::nls::localize;
use je_base::path::{PathModule, Platform};
use je_base::random::fill_bytes;
use je_base::resources::ExtUri;
use je_base::runtime;
use je_base::strings::{common_prefix_length, get_leading_whitespace, is_falsy_or_whitespace, split_lines};
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_platform::resources::{fs_path, host_environment, host_platform};
use je_platform::workspace::{
    AnyWorkspaceIdentifier, IWorkspaceContextService, WORKSPACE_EXTENSION, to_workspace_identifier,
};
use jiff::Timestamp;
use jiff::Zoned;
use jiff::tz::TimeZone;

use super::snippet_parser::{MarkerRef, VariableResolver};
use crate::cursor_common::CursorSimpleModel;
use crate::language_configuration_registry::ILanguageConfigurationService;
use crate::position::Position;
use crate::selection::Selection;
use crate::text_model::TextModel;

/// `KnownSnippetVariableNames`: o conjunto dos nomes que o editor sabe resolver.
pub const KNOWN_SNIPPET_VARIABLE_NAMES: &[&str] = &[
    "CURRENT_YEAR",
    "CURRENT_YEAR_SHORT",
    "CURRENT_MONTH",
    "CURRENT_DATE",
    "CURRENT_HOUR",
    "CURRENT_MINUTE",
    "CURRENT_SECOND",
    "CURRENT_MILLISECOND",
    "CURRENT_DAY_NAME",
    "CURRENT_DAY_NAME_SHORT",
    "CURRENT_MONTH_NAME",
    "CURRENT_MONTH_NAME_SHORT",
    "CURRENT_SECONDS_UNIX",
    "CURRENT_MILLISECONDS_UNIX",
    "CURRENT_TIMEZONE_OFFSET",
    "CURRENT_TIMEZONE_NAME",
    "SELECTION",
    "CLIPBOARD",
    "TM_SELECTED_TEXT",
    "TM_CURRENT_LINE",
    "TM_CURRENT_WORD",
    "TM_LINE_INDEX",
    "TM_LINE_NUMBER",
    "TM_FILENAME",
    "TM_FILENAME_BASE",
    "TM_DIRECTORY",
    "TM_DIRECTORY_BASE",
    "TM_FILEPATH",
    // 0-offset
    "CURSOR_INDEX",
    // 1-offset
    "CURSOR_NUMBER",
    "RELATIVE_FILEPATH",
    "BLOCK_COMMENT_START",
    "BLOCK_COMMENT_END",
    "LINE_COMMENT",
    "WORKSPACE_NAME",
    "WORKSPACE_FOLDER",
    "RANDOM",
    "RANDOM_HEX",
    "UUID",
];

/// `name in KnownSnippetVariableNames`.
pub fn is_known_snippet_variable_name(name: &str) -> bool {
    KNOWN_SNIPPET_VARIABLE_NAMES.contains(&name)
}

/// As opções de `ILabelService.getUriLabel(resource, options?)`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UriLabelOptions {
    /// `relative`: o caminho relativo à pasta do workspace.
    pub relative: bool,
    /// `noPrefix`: sem o nome da pasta raiz na frente do caminho relativo.
    pub no_prefix: bool,
    /// `separator`: `/` ou `\`.
    pub separator: Option<char>,
}

/// `ILabelService`, só o que o `snippetVariables.ts` usa (ver o topo do módulo).
pub trait ILabelService {
    /// `getUriLabel(resource, options?)`.
    fn get_uri_label(&self, resource: &URI, options: &UriLabelOptions) -> String;
}

/// O que `OvertypingCapturer.getLastOvertypedInfo(idx)` devolve: o texto que o último `type` sobrescreveu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OvertypedInfo {
    pub value: JsString,
    pub multiline: bool,
}

/// `OvertypingCapturer`, só o `getLastOvertypedInfo` (ver o topo do módulo).
pub trait IOvertypingCapturer {
    /// `getLastOvertypedInfo(idx)`.
    fn get_last_overtyped_info(&self, idx: usize) -> Option<OvertypedInfo>;
}

/// `IReadClipboardText`: `() => string | undefined`.
pub type ReadClipboardText = Rc<dyn Fn() -> Option<JsString>>;

/// `variable.name` de um marcador `Variable`.
fn variable_name(variable: &MarkerRef) -> &str {
    variable.as_variable().expect("o resolvedor só recebe marcadores Variable").name.as_str()
}

/// `path.<função>(...)`: o módulo `path` nativo do hospedeiro.
fn with_native_path<R>(f: impl FnOnce(&PathModule<'_>) -> R) -> R {
    let env = host_environment();
    f(&PathModule::native(&*env))
}

/// `dirname(uri)` de `resources.ts`.
fn uri_dirname(resource: &URI) -> URI {
    ExtUri::unbiased(host_environment()).dirname(resource)
}

/// `value.substr(0, value.length - suffix_units - 1)`: o texto sem o sufixo e sem o separador que o precede
/// (os comprimentos contam unidades UTF-16, como no JavaScript).
fn chop_suffix_and_separator(value: &str, suffix_units: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().collect();
    let keep = units.len().saturating_sub(suffix_units + 1);
    String::from_utf16_lossy(&units[..keep])
}

/// `value.slice(-count)`: as últimas `count` unidades UTF-16.
fn last_units(value: &str, count: usize) -> String {
    let units: Vec<u16> = value.encode_utf16().collect();
    String::from_utf16_lossy(&units[units.len().saturating_sub(count)..])
}

/// `Math.random()`: uma fração aleatória de 53 bits em `[0, 1)`.
fn math_random() -> f64 {
    let mut bytes = [0u8; 8];
    fill_bytes(&mut bytes);
    (u64::from_le_bytes(bytes) >> 11) as f64 / (1u64 << 53) as f64
}

/// `fraction.toString(16)` para uma fração em `[0, 1)`: `0.` e os dígitos hexadecimais exatos.
fn fraction_to_hex_string(mut fraction: f64) -> String {
    if fraction == 0.0 {
        return "0".to_owned();
    }
    let mut out = String::from("0.");
    while fraction > 0.0 {
        fraction *= 16.0;
        let digit = fraction.floor();
        out.push(char::from_digit(digit as u32, 16).expect("um dígito entre 0 e 15"));
        fraction -= digit;
    }
    out
}

/// `CompositeSnippetVariableResolver`: o primeiro resolvedor que devolve um valor ganha.
pub struct CompositeSnippetVariableResolver {
    delegates: Vec<Rc<dyn VariableResolver>>,
}

impl CompositeSnippetVariableResolver {
    pub fn new(delegates: Vec<Rc<dyn VariableResolver>>) -> Self {
        Self { delegates }
    }
}

impl VariableResolver for CompositeSnippetVariableResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
        self.delegates.iter().find_map(|delegate| delegate.resolve(variable))
    }
}

/// Insere `prefix` depois de cada quebra de linha de `value`: o
/// `value.replace(/(\r\n|\r|\n)(.*)/g, (m, newline, rest) => newline + prefix + rest)` do original.
fn indent_following_lines(value: &JsString, prefix: &[u16]) -> JsString {
    let units = value.units();
    let mut out = JsString::new();
    let mut i = 0;
    while i < units.len() {
        let unit = units[i];
        out.push_unit(unit);
        i += 1;
        if unit == cc::LineFeed || unit == cc::CarriageReturn {
            if unit == cc::CarriageReturn && units.get(i) == Some(&cc::LineFeed) {
                out.push_unit(cc::LineFeed);
                i += 1;
            }
            out.push_units(prefix);
        }
    }
    out
}

/// `SelectionBasedVariableResolver`.
pub struct SelectionBasedVariableResolver {
    model: Rc<TextModel>,
    selection: Selection,
    selection_idx: usize,
    overtyping_capturer: Option<Rc<dyn IOvertypingCapturer>>,
}

impl SelectionBasedVariableResolver {
    pub fn new(
        model: Rc<TextModel>,
        selection: Selection,
        selection_idx: usize,
        overtyping_capturer: Option<Rc<dyn IOvertypingCapturer>>,
    ) -> Self {
        Self { model, selection, selection_idx, overtyping_capturer }
    }

    /// O texto selecionado de `$SELECTION` e `$TM_SELECTED_TEXT`.
    fn resolve_selection(&self, variable: &MarkerRef) -> Option<JsString> {
        let in_range = self.model.get_value_in_range(&self.selection);
        let mut value = (!in_range.is_empty()).then_some(in_range);
        let mut is_multiline = self.selection.start_line_number != self.selection.end_line_number;

        // If there was no selected text, try to get last overtyped text
        if value.is_none()
            && let Some(capturer) = &self.overtyping_capturer
            && let Some(info) = capturer.get_last_overtyped_info(self.selection_idx)
        {
            value = Some(info.value);
            is_multiline = info.multiline;
        }

        if let Some(text) = value.clone().filter(|text| !text.is_empty())
            && is_multiline
            && let Some(snippet) = variable.snippet()
        {
            // Selection is a multiline string which we indentation we now
            // need to adjust. We compare the indentation of this variable
            // with the indentation at the editor position and add potential
            // extra indentation to the value

            let line = self.model.get_line_content(self.selection.start_line_number);
            let line_leading_whitespace =
                get_leading_whitespace(line.units(), 0, (self.selection.start_column - 1).max(0) as usize).to_vec();

            let mut var_leading_whitespace = line_leading_whitespace.clone();
            snippet.walk(|marker| {
                if Rc::ptr_eq(marker, variable) {
                    return false;
                }
                if let Some(text_data) = marker.as_text() {
                    let value = text_data.value.borrow();
                    let last_line = split_lines(value.units()).pop().expect("splitLines devolve ao menos uma linha");
                    var_leading_whitespace = get_leading_whitespace(last_line, 0, last_line.len()).to_vec();
                }
                true
            });
            let whitespace_common_length = common_prefix_length(&var_leading_whitespace, &line_leading_whitespace);

            value = Some(indent_following_lines(&text, &var_leading_whitespace[whitespace_common_length..]));
        }
        value
    }
}

impl VariableResolver for SelectionBasedVariableResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
        match variable_name(variable) {
            "SELECTION" | "TM_SELECTED_TEXT" => self.resolve_selection(variable),
            "TM_CURRENT_LINE" => Some(self.model.get_line_content(self.selection.position_line_number)),
            "TM_CURRENT_WORD" => {
                let info = self
                    .model
                    .get_word_at_position(Position::new(self.selection.position_line_number, self.selection.position_column));
                info.map(|info| info.word).filter(|word| !word.is_empty())
            }
            "TM_LINE_INDEX" => Some(JsString::from((self.selection.position_line_number - 1).to_string())),
            "TM_LINE_NUMBER" => Some(JsString::from(self.selection.position_line_number.to_string())),
            "CURSOR_INDEX" => Some(JsString::from(self.selection_idx.to_string())),
            "CURSOR_NUMBER" => Some(JsString::from((self.selection_idx + 1).to_string())),
            _ => None,
        }
    }
}

/// `ModelBasedVariableResolver`.
pub struct ModelBasedVariableResolver {
    label_service: Rc<dyn ILabelService>,
    model: Rc<TextModel>,
}

impl ModelBasedVariableResolver {
    pub fn new(label_service: Rc<dyn ILabelService>, model: Rc<TextModel>) -> Self {
        Self { label_service, model }
    }
}

impl VariableResolver for ModelBasedVariableResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
        let uri = self.model.uri();
        let uri_fs_path = fs_path(&uri);
        let text = match variable_name(variable) {
            "TM_FILENAME" => with_native_path(|path| path.basename(&uri_fs_path, None)),
            "TM_FILENAME_BASE" => {
                let name = with_native_path(|path| path.basename(&uri_fs_path, None));
                match name.rfind('.') {
                    // `idx <= 0`: sem ponto, ou o ponto é o primeiro caractere
                    None | Some(0) => name,
                    Some(idx) => name[..idx].to_owned(),
                }
            }
            "TM_DIRECTORY" => {
                if with_native_path(|path| path.dirname(&uri_fs_path)) == "." {
                    String::new()
                } else {
                    self.label_service.get_uri_label(&uri_dirname(&uri), &UriLabelOptions::default())
                }
            }
            "TM_DIRECTORY_BASE" => {
                if with_native_path(|path| path.dirname(&uri_fs_path)) == "." {
                    String::new()
                } else {
                    with_native_path(|path| path.basename(&path.dirname(&uri_fs_path), None))
                }
            }
            "TM_FILEPATH" => self.label_service.get_uri_label(&uri, &UriLabelOptions::default()),
            "RELATIVE_FILEPATH" => self
                .label_service
                .get_uri_label(&uri, &UriLabelOptions { relative: true, no_prefix: true, separator: None }),
            _ => return None,
        };
        Some(JsString::from(text))
    }
}

/// `ClipboardBasedVariableResolver`.
pub struct ClipboardBasedVariableResolver {
    read_clipboard_text: ReadClipboardText,
    selection_idx: usize,
    selection_count: usize,
    spread: bool,
}

impl ClipboardBasedVariableResolver {
    pub fn new(read_clipboard_text: ReadClipboardText, selection_idx: usize, selection_count: usize, spread: bool) -> Self {
        Self { read_clipboard_text, selection_idx, selection_count, spread }
    }
}

impl VariableResolver for ClipboardBasedVariableResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
        if variable_name(variable) != "CLIPBOARD" {
            return None;
        }

        let clipboard_text = (self.read_clipboard_text)().filter(|text| !text.is_empty())?;

        // `spread` is assigning each cursor a line of the clipboard
        // text whenever there the line count equals the cursor count
        // and when enabled
        if self.spread {
            let lines: Vec<&[u16]> =
                split_lines(clipboard_text.units()).into_iter().filter(|line| !is_falsy_or_whitespace(Some(*line))).collect();
            if lines.len() == self.selection_count {
                return lines.get(self.selection_idx).map(|line| JsString::from(*line));
            }
        }
        Some(clipboard_text)
    }
}

/// `CommentBasedVariableResolver`.
pub struct CommentBasedVariableResolver {
    model: Rc<TextModel>,
    selection: Selection,
    language_configuration_service: Rc<dyn ILanguageConfigurationService>,
}

impl CommentBasedVariableResolver {
    pub fn new(
        model: Rc<TextModel>,
        selection: Selection,
        language_configuration_service: Rc<dyn ILanguageConfigurationService>,
    ) -> Self {
        Self { model, selection, language_configuration_service }
    }
}

impl VariableResolver for CommentBasedVariableResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
        let name = variable_name(variable);
        let lang_id = self
            .model
            .get_language_id_at_position(self.selection.selection_start_line_number, self.selection.selection_start_column);
        let configuration = self.language_configuration_service.get_language_configuration(&lang_id);
        let config = configuration.comments.as_ref()?;
        let token = match name {
            "LINE_COMMENT" => config.line_comment_token.as_deref(),
            "BLOCK_COMMENT_START" => config.block_comment_start_token.as_deref(),
            "BLOCK_COMMENT_END" => config.block_comment_end_token.as_deref(),
            _ => None,
        };
        token.filter(|token| !token.is_empty()).map(JsString::from)
    }
}

/// `TimeBasedVariableResolver.dayNames`: a chave de tradução e o texto em inglês.
const DAY_NAMES: [(&str, &str); 7] = [
    ("Sunday", "Sunday"),
    ("Monday", "Monday"),
    ("Tuesday", "Tuesday"),
    ("Wednesday", "Wednesday"),
    ("Thursday", "Thursday"),
    ("Friday", "Friday"),
    ("Saturday", "Saturday"),
];

/// `TimeBasedVariableResolver.dayNamesShort`.
const DAY_NAMES_SHORT: [(&str, &str); 7] = [
    ("SundayShort", "Sun"),
    ("MondayShort", "Mon"),
    ("TuesdayShort", "Tue"),
    ("WednesdayShort", "Wed"),
    ("ThursdayShort", "Thu"),
    ("FridayShort", "Fri"),
    ("SaturdayShort", "Sat"),
];

/// `TimeBasedVariableResolver.monthNames`.
const MONTH_NAMES: [(&str, &str); 12] = [
    ("January", "January"),
    ("February", "February"),
    ("March", "March"),
    ("April", "April"),
    ("May", "May"),
    ("June", "June"),
    ("July", "July"),
    ("August", "August"),
    ("September", "September"),
    ("October", "October"),
    ("November", "November"),
    ("December", "December"),
];

/// `TimeBasedVariableResolver.monthNamesShort`.
const MONTH_NAMES_SHORT: [(&str, &str); 12] = [
    ("JanuaryShort", "Jan"),
    ("FebruaryShort", "Feb"),
    ("MarchShort", "Mar"),
    ("AprilShort", "Apr"),
    ("MayShort", "May"),
    ("JuneShort", "Jun"),
    ("JulyShort", "Jul"),
    ("AugustShort", "Aug"),
    ("SeptemberShort", "Sep"),
    ("OctoberShort", "Oct"),
    ("NovemberShort", "Nov"),
    ("DecemberShort", "Dec"),
];

/// `nls.localize(key, message)` de um item das tabelas de nomes.
fn localized_name((key, message): (&str, &str)) -> String {
    localize(key, message, &[])
}

/// `number < 10 ? '0' + number : number`.
fn pad_below_ten(number: f64) -> String {
    if number < 10.0 { format!("0{}", number_to_js_string(number)) } else { number_to_js_string(number) }
}

/// `TimeBasedVariableResolver`: o instante é fixado na criação, para todas as variáveis de uma inserção
/// concordarem entre si.
pub struct TimeBasedVariableResolver {
    date: Zoned,
    timezone_name: RefCell<Option<String>>,
}

impl Default for TimeBasedVariableResolver {
    /// `new Date()`: o instante de agora, no fuso local.
    fn default() -> Self {
        Self::at(runtime::date_now() as i64)
    }
}

impl TimeBasedVariableResolver {
    /// O resolvedor de um instante dado (milissegundos desde a época Unix), no fuso local.
    pub fn at(epoch_milliseconds: i64) -> Self {
        let date = Timestamp::from_millisecond(epoch_milliseconds)
            .expect("o instante cabe no intervalo do jiff")
            .to_zoned(TimeZone::system());
        Self { date, timezone_name: RefCell::new(None) }
    }
}

impl VariableResolver for TimeBasedVariableResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
        let date = &self.date;
        let weekday = usize::try_from(date.weekday().to_sunday_zero_offset()).expect("o dia da semana é de 0 a 6");
        let month_index = usize::try_from(date.month() - 1).expect("o mês é de 1 a 12");
        let epoch_milliseconds = date.timestamp().as_millisecond();

        let text = match variable_name(variable) {
            "CURRENT_YEAR" => date.year().to_string(),
            "CURRENT_YEAR_SHORT" => last_units(&date.year().to_string(), 2),
            "CURRENT_MONTH" => format!("{:02}", date.month()),
            "CURRENT_DATE" => format!("{:02}", date.day()),
            "CURRENT_HOUR" => format!("{:02}", date.hour()),
            "CURRENT_MINUTE" => format!("{:02}", date.minute()),
            "CURRENT_SECOND" => format!("{:02}", date.second()),
            "CURRENT_MILLISECOND" => format!("{:03}", date.millisecond()),
            "CURRENT_DAY_NAME" => localized_name(DAY_NAMES[weekday]),
            "CURRENT_DAY_NAME_SHORT" => localized_name(DAY_NAMES_SHORT[weekday]),
            "CURRENT_MONTH_NAME" => localized_name(MONTH_NAMES[month_index]),
            "CURRENT_MONTH_NAME_SHORT" => localized_name(MONTH_NAMES_SHORT[month_index]),
            "CURRENT_SECONDS_UNIX" => epoch_milliseconds.div_euclid(1000).to_string(),
            "CURRENT_MILLISECONDS_UNIX" => epoch_milliseconds.to_string(),
            "CURRENT_TIMEZONE_OFFSET" => {
                // `getTimezoneOffset()`: os minutos a oeste de UTC
                let raw_time_offset = -f64::from(date.offset().seconds()) / 60.0;
                let sign = if raw_time_offset > 0.0 { '-' } else { '+' };
                let hours = (raw_time_offset / 60.0).abs().trunc();
                let minutes = raw_time_offset.abs() - hours * 60.0;
                format!("{sign}{}:{}", pad_below_ten(hours), pad_below_ten(minutes))
            }
            "CURRENT_TIMEZONE_NAME" => self
                .timezone_name
                .borrow_mut()
                .get_or_insert_with(|| date.time_zone().iana_name().unwrap_or("UTC").to_owned())
                .clone(),
            _ => return None,
        };
        Some(JsString::from(text))
    }
}

/// `WorkspaceBasedVariableResolver`.
pub struct WorkspaceBasedVariableResolver {
    workspace_service: Option<Rc<dyn IWorkspaceContextService>>,
}

impl WorkspaceBasedVariableResolver {
    pub fn new(workspace_service: Option<Rc<dyn IWorkspaceContextService>>) -> Self {
        Self { workspace_service }
    }

    fn resolve_workspace_name(workspace_identifier: &AnyWorkspaceIdentifier) -> Option<String> {
        match workspace_identifier {
            AnyWorkspaceIdentifier::SingleFolder(folder) => {
                Some(with_native_path(|path| path.basename(&folder.uri.path().to_string_lossy(), None)))
            }
            AnyWorkspaceIdentifier::Workspace(workspace) => {
                let filename = with_native_path(|path| path.basename(&workspace.config_path.path().to_string_lossy(), None));
                Some(if filename.ends_with(WORKSPACE_EXTENSION) {
                    chop_suffix_and_separator(&filename, WORKSPACE_EXTENSION.encode_utf16().count())
                } else {
                    filename
                })
            }
            AnyWorkspaceIdentifier::Empty(_) => None,
        }
    }

    fn resolve_workspace_path(workspace_identifier: &AnyWorkspaceIdentifier) -> Option<String> {
        let normalize = |path: &str| normalize_drive_letter(path, host_platform() == Platform::Win32);
        match workspace_identifier {
            AnyWorkspaceIdentifier::SingleFolder(folder) => Some(normalize(&fs_path(&folder.uri))),
            AnyWorkspaceIdentifier::Workspace(workspace) => {
                let filename = with_native_path(|path| path.basename(&workspace.config_path.path().to_string_lossy(), None));
                let mut folder_path = fs_path(&workspace.config_path);
                if folder_path.ends_with(&filename) {
                    folder_path = chop_suffix_and_separator(&folder_path, filename.encode_utf16().count());
                }
                Some(if folder_path.is_empty() { "/".to_owned() } else { normalize(&folder_path) })
            }
            AnyWorkspaceIdentifier::Empty(_) => None,
        }
    }
}

impl VariableResolver for WorkspaceBasedVariableResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
        let workspace_service = self.workspace_service.as_ref()?;

        let workspace = workspace_service.get_workspace();
        let workspace_identifier = to_workspace_identifier(&*workspace);
        if workspace_identifier.is_empty_workspace_identifier() {
            return None;
        }

        let text = match variable_name(variable) {
            "WORKSPACE_NAME" => Self::resolve_workspace_name(&workspace_identifier),
            "WORKSPACE_FOLDER" => Self::resolve_workspace_path(&workspace_identifier),
            _ => None,
        };
        text.map(JsString::from)
    }
}

/// `RandomBasedVariableResolver`.
pub struct RandomBasedVariableResolver;

impl VariableResolver for RandomBasedVariableResolver {
    fn resolve(&self, variable: &MarkerRef) -> Option<JsString> {
        let text = match variable_name(variable) {
            "RANDOM" => last_units(&number_to_js_string(math_random()), 6),
            "RANDOM_HEX" => last_units(&fraction_to_hex_string(math_random()), 6),
            "UUID" => generate_uuid(),
            _ => return None,
        };
        Some(JsString::from(text))
    }
}
