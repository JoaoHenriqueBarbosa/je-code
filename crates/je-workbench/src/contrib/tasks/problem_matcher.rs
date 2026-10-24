//! Porte de `src/vs/workbench/contrib/tasks/common/problemMatcher.ts`: o casador de problemas das tarefas.
//! Transforma as linhas de saída de uma tarefa em marcadores (`IMarkerData`), com os padrões nomeados
//! (`ProblemPatternRegistry`), os casadores nomeados (`ProblemMatcherRegistry`), o leitor das descrições
//! do `tasks.json` e das contribuições de extensão (`ProblemPatternParser`, `ProblemMatcherParser`) e os
//! esquemas JSON dessas descrições (`Schemas`).
//!
//! Divergências:
//! - Os tipos de `namespace Config` (a descrição escrita pelo usuário) são structs tipadas no módulo [`config`],
//!   com todos os campos `Option` e [`Default`]; as uniões do original (`string | IProblemPattern | ...`) são
//!   enums. O `is` de cada tipo da união (`CheckedProblemPattern.is` e companhia) vira a escolha da variante
//!   mais a conferência de `regexp` em [`config`]; o resultado de `ProblemPatternParser.parse` é o enum
//!   [`ParsedProblemPattern`] no lugar das sobrecargas.
//! - `IProblemPattern.loop` é o campo `r#loop`, `IHandleResult.match` e `continue` são `r#match` e `r#continue`.
//! - `ProblemMatcher` e `INamedProblemMatcher` são a mesma struct: os campos `name`, `label` e `deprecated` do
//!   nomeado e o `tscWatch` que o registro grava no `tsc-watch` são `Option`, como as propriedades que um
//!   objeto JavaScript ganha em tempo de execução. O `deepClone` do original é [`Clone`] (o `RegExp` é
//!   compartilhado, como o `deepClone` o devolve), e `ProblemMatcherRegistry.get` devolve uma cópia.
//! - `checkProblemMatcherValid` não confere `pattern` nem `fileLocation` ausentes: os tipos os tornam
//!   obrigatórios, e o `createProblemMatcher` só devolve um casador quando os dois existem.
//! - Os grupos do `RegExpExecArray` são lidos como `Vec<Option<String>>` (texto decodificado de UTF-16), e o
//!   número de um grupo que não é inteiro (`parseInt` devolve `NaN`) vira `0` ao entrar no marcador, porque
//!   `IMarkerData` guarda inteiros.
//! - O `try`/`catch` de `getMarkerMatch` não existe: nada do corpo pode lançar em Rust.
//! - O ponto de extensão do original (`registerExtensionPoint` com `jsonSchema`, `deps` e `setHandler`) não cabe no
//!   registro reduzido de [`crate::services::extensions::extensions_registry`]. As funções
//!   [`register_problem_patterns_extension_point`] e [`register_problem_matchers_extension_point`] registram o
//!   nome do ponto (não são chamadas na criação dos registros, para não lançar `Duplicate extension point`), o
//!   `jsonSchema` é [`problem_patterns_extension_point_schema`] e [`problem_matchers_extension_point_schema`], e o
//!   corpo do tratador é `handle_delta` de cada registro, que o hospedeiro de extensões chama com
//!   `delta.removed` e `delta.added` ([`ExtensionPointUser`], [`ExtensionPointDelta`] e
//!   [`ExtensionMessageCollector`] são os contratos mínimos do original, definidos aqui até o
//!   `extensionsRegistry` ser portado por inteiro).
//! - `JSON.stringify(x, null, 4)` das mensagens de erro é o `to_json` dos tipos de [`config`] reindentado com 4
//!   espaços, na ordem dos campos da struct (o original usa a ordem em que o usuário escreveu as chaves).
//! - `Platform.OS` e o módulo `path` nativo vêm do hospedeiro (`Platform::detect`, `host_environment`).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;
use std::time::Instant;

use indexmap::IndexMap;
use je_base::JsString;
use je_base::arrays::{OneOrMany, as_array};
use je_base::assert::ok as assert_ok;
use je_base::dynamic::Dynamic;
use je_base::errors::{self, Error};
use je_base::event::{Emitter, Event};
use je_base::js_regexp::JsRegExp;
use je_base::network::schemas as network_schemas;
use je_base::nls::localize;
use je_base::objects::deep_clone;
use je_base::parsers::{Parser, ProblemReporter, ValidationState, ValidationStatus};
use je_base::path::PathModule;
use je_base::platform::Platform;
use je_base::promise::{Promise, Resolver};
use je_base::severity::Severity;
use je_base::strings;
use je_base::uri::URI;
use je_base::uuid::generate_uuid;
use je_platform::files::files::{FileSystemProvider, FileSystemProviderRef, FileType};
use je_platform::files::i_file_service::IFileService;
use je_platform::js::set_property;
use je_platform::log::{LogArg, LogService, Logger};
use je_platform::markers::{MarkerCode, MarkerData, MarkerSeverity};
use je_platform::resources::{host_environment, host_platform};

use crate::services::extensions::extensions_registry::{ExtensionPoint, ExtensionPointDescriptor, extensions_registry};

// ---------------------------------------------------------------------------------------------
// Enums e tipos de dados
// ---------------------------------------------------------------------------------------------

/// `FileLocationKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileLocationKind {
    Default,
    Relative,
    Absolute,
    AutoDetect,
    Search,
}

impl FileLocationKind {
    /// `FileLocationKind.fromString`.
    pub fn from_string(value: &str) -> Option<FileLocationKind> {
        match value.to_lowercase().as_str() {
            "absolute" => Some(FileLocationKind::Absolute),
            "relative" => Some(FileLocationKind::Relative),
            "autodetect" => Some(FileLocationKind::AutoDetect),
            "search" => Some(FileLocationKind::Search),
            _ => None,
        }
    }
}

/// `ProblemLocationKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProblemLocationKind {
    File,
    Location,
}

impl ProblemLocationKind {
    /// `ProblemLocationKind.fromString`.
    pub fn from_string(value: &str) -> Option<ProblemLocationKind> {
        match value.to_lowercase().as_str() {
            "file" => Some(ProblemLocationKind::File),
            "location" => Some(ProblemLocationKind::Location),
            _ => None,
        }
    }
}

/// `IProblemPattern`: os campos `file`, `message` e os demais são o índice do grupo da expressão regular.
#[derive(Clone, Debug)]
pub struct ProblemPattern {
    pub regexp: Rc<JsRegExp>,
    pub kind: Option<ProblemLocationKind>,
    pub file: Option<usize>,
    pub message: Option<usize>,
    pub location: Option<usize>,
    pub line: Option<usize>,
    pub character: Option<usize>,
    pub end_line: Option<usize>,
    pub end_character: Option<usize>,
    pub code: Option<usize>,
    pub severity: Option<usize>,
    pub r#loop: Option<bool>,
}

impl ProblemPattern {
    /// Um padrão só com a expressão regular (`{ regexp }`).
    pub fn new(regexp: Rc<JsRegExp>) -> Self {
        Self {
            regexp,
            kind: None,
            file: None,
            message: None,
            location: None,
            line: None,
            character: None,
            end_line: None,
            end_character: None,
            code: None,
            severity: None,
            r#loop: None,
        }
    }
}

/// A igualdade do `assert.deepStrictEqual` do original: o `RegExp` compara por `source` e `flags`.
impl PartialEq for ProblemPattern {
    fn eq(&self, other: &Self) -> bool {
        self.regexp.source() == other.regexp.source()
            && self.regexp.flags() == other.regexp.flags()
            && self.kind == other.kind
            && self.file == other.file
            && self.message == other.message
            && self.location == other.location
            && self.line == other.line
            && self.character == other.character
            && self.end_line == other.end_line
            && self.end_character == other.end_character
            && self.code == other.code
            && self.severity == other.severity
            && self.r#loop == other.r#loop
    }
}

/// `INamedProblemPattern`.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedProblemPattern {
    pub pattern: ProblemPattern,
    pub name: String,
}

/// `MultiLineProblemPattern`.
pub type MultiLineProblemPattern = Vec<ProblemPattern>;

/// `IWatchingPattern`.
#[derive(Clone, Debug)]
pub struct WatchingPattern {
    pub regexp: Rc<JsRegExp>,
    pub file: Option<usize>,
}

/// `IWatchingMatcher`.
#[derive(Clone, Debug)]
pub struct WatchingMatcher {
    pub active_on_start: bool,
    pub begins_pattern: WatchingPattern,
    pub ends_pattern: WatchingPattern,
}

/// `ApplyToKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApplyToKind {
    AllDocuments,
    OpenDocuments,
    ClosedDocuments,
}

impl ApplyToKind {
    /// `ApplyToKind.fromString`.
    pub fn from_string(value: &str) -> Option<ApplyToKind> {
        match value.to_lowercase().as_str() {
            "alldocuments" => Some(ApplyToKind::AllDocuments),
            "opendocuments" => Some(ApplyToKind::OpenDocuments),
            "closeddocuments" => Some(ApplyToKind::ClosedDocuments),
            _ => None,
        }
    }
}

/// `string | Config.SearchFileLocationArgs` do `filePrefix`.
#[derive(Clone, Debug, PartialEq)]
pub enum FilePrefix {
    Path(String),
    Search(config::SearchFileLocationArgs),
}

/// `uriProvider?: (path: string) => URI`.
#[derive(Clone)]
pub struct UriProvider(pub Rc<dyn Fn(&str) -> URI>);

impl std::fmt::Debug for UriProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UriProvider(..)")
    }
}

/// `ProblemMatcher` e `INamedProblemMatcher` (ver as divergências no topo do módulo).
#[derive(Clone, Debug)]
pub struct ProblemMatcher {
    pub owner: String,
    pub source: Option<String>,
    pub apply_to: ApplyToKind,
    pub file_location: FileLocationKind,
    pub file_prefix: Option<FilePrefix>,
    pub pattern: OneOrMany<ProblemPattern>,
    pub severity: Option<Severity>,
    pub watching: Option<WatchingMatcher>,
    pub uri_provider: Option<UriProvider>,
    /// `INamedProblemMatcher.name`.
    pub name: Option<String>,
    /// `INamedProblemMatcher.label`.
    pub label: Option<String>,
    /// `INamedProblemMatcher.deprecated`.
    pub deprecated: Option<bool>,
    /// A propriedade `tscWatch` que o registro grava no casador `tsc-watch`.
    pub tsc_watch: Option<bool>,
}

impl ProblemMatcher {
    /// Um casador com os quatro campos obrigatórios.
    pub fn new(owner: impl Into<String>, apply_to: ApplyToKind, file_location: FileLocationKind, pattern: OneOrMany<ProblemPattern>) -> Self {
        Self {
            owner: owner.into(),
            source: None,
            apply_to,
            file_location,
            file_prefix: None,
            pattern,
            severity: None,
            watching: None,
            uri_provider: None,
            name: None,
            label: None,
            deprecated: None,
            tsc_watch: None,
        }
    }
}

/// `INamedMultiLineProblemPattern`.
#[derive(Clone, Debug, PartialEq)]
pub struct NamedMultiLineProblemPattern {
    pub name: String,
    pub label: String,
    pub patterns: MultiLineProblemPattern,
}

/// `isNamedProblemMatcher`.
pub fn is_named_problem_matcher(value: Option<&ProblemMatcher>) -> bool {
    value.is_some_and(|matcher| matcher.name.is_some())
}

#[derive(Clone, Copy, Debug)]
struct Location {
    start_line_number: f64,
    start_character: f64,
    end_line_number: f64,
    end_character: f64,
}

/// `IProblemData`: os campos lidos do casamento, ainda como texto.
#[derive(Clone, Debug, Default)]
struct ProblemData {
    kind: Option<ProblemLocationKind>,
    file: Option<String>,
    location: Option<String>,
    line: Option<String>,
    character: Option<String>,
    end_line: Option<String>,
    end_character: Option<String>,
    message: Option<String>,
    severity: Option<String>,
    code: Option<String>,
}

/// As propriedades de `IProblemData` que `fillProperty` e `appendProperty` recebem pelo nome.
#[derive(Clone, Copy)]
enum Property {
    File,
    Message,
    Code,
    Severity,
    Location,
    Line,
    Character,
    EndLine,
    EndCharacter,
}

impl ProblemData {
    fn slot(&mut self, property: Property) -> &mut Option<String> {
        match property {
            Property::File => &mut self.file,
            Property::Message => &mut self.message,
            Property::Code => &mut self.code,
            Property::Severity => &mut self.severity,
            Property::Location => &mut self.location,
            Property::Line => &mut self.line,
            Property::Character => &mut self.character,
            Property::EndLine => &mut self.end_line,
            Property::EndCharacter => &mut self.end_character,
        }
    }
}

impl ProblemPattern {
    fn group(&self, property: Property) -> Option<usize> {
        match property {
            Property::File => self.file,
            Property::Message => self.message,
            Property::Code => self.code,
            Property::Severity => self.severity,
            Property::Location => self.location,
            Property::Line => self.line,
            Property::Character => self.character,
            Property::EndLine => self.end_line,
            Property::EndCharacter => self.end_character,
        }
    }
}

/// `IProblemMatch`.
#[derive(Clone)]
pub struct ProblemMatch {
    pub resource: Promise<URI>,
    pub marker: MarkerData,
    pub description: ProblemMatcher,
}

/// `IHandleResult`.
#[derive(Clone)]
pub struct HandleResult {
    pub r#match: Option<ProblemMatch>,
    pub r#continue: bool,
}

fn is_truthy(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(|text| !text.is_empty())
}

fn as_strings(value: Option<&OneOrMany<String>>) -> Vec<String> {
    match value {
        None => Vec::new(),
        // `args.include || []`: a string vazia é falsa.
        Some(OneOrMany::One(text)) if text.is_empty() => Vec::new(),
        Some(other) => as_array(other.clone()),
    }
}

// ---------------------------------------------------------------------------------------------
// getResource
// ---------------------------------------------------------------------------------------------

/// `getResource`.
pub fn get_resource(filename: &str, matcher: &ProblemMatcher, file_service: Option<Rc<dyn IFileService>>) -> Promise<URI> {
    let kind = matcher.file_location;
    let mut full_path: Option<String> = None;
    if kind == FileLocationKind::Absolute {
        full_path = Some(filename.to_string());
    } else if kind == FileLocationKind::Relative
        && let Some(FilePrefix::Path(prefix)) = &matcher.file_prefix
        && !prefix.is_empty()
    {
        let env = host_environment();
        full_path = Some(PathModule::native(env.as_ref()).join(&[prefix.as_str(), filename]));
    } else if kind == FileLocationKind::AutoDetect {
        let mut matcher_clone = matcher.clone();
        matcher_clone.file_location = FileLocationKind::Relative;
        let mut absolute = matcher.clone();
        absolute.file_location = FileLocationKind::Absolute;
        let filename_owned = filename.to_string();
        return match file_service {
            Some(file_service) => get_resource(filename, &matcher_clone, None).then_promise(move |relative| {
                file_service.stat(&relative).then_result_promise(move |stat| match stat {
                    Ok(_) => Promise::resolved(relative),
                    // Do nothing, we just need to catch file resolution errors.
                    Err(_) => get_resource(&filename_owned, &absolute, None),
                })
            }),
            None => get_resource(filename, &absolute, None),
        };
    } else if kind == FileLocationKind::Search
        && let Some(file_service) = &file_service
    {
        if let Some(provider) = file_service.get_provider(network_schemas::FILE) {
            let args = match &matcher.file_prefix {
                Some(FilePrefix::Search(args)) => args.clone(),
                Some(FilePrefix::Path(_)) => config::SearchFileLocationArgs::default(),
                None => {
                    return Promise::rejected(Error::type_error("Cannot read properties of undefined (reading 'exclude')"));
                }
            };
            let filename_owned = filename.to_string();
            let matcher_owned = matcher.clone();
            return search_for_file_location(filename, provider, &args)
                .then_promise(move |uri| resource_from_search(uri.map(|uri| uri.path().to_string_lossy()), &filename_owned, &matcher_owned));
        }
        return resource_from_search(None, filename, matcher);
    }
    finish_resource(full_path, matcher)
}

/// O trecho de `getResource` depois da busca: sem caminho achado, vale o nome como caminho absoluto.
fn resource_from_search(full_path: Option<String>, filename: &str, matcher: &ProblemMatcher) -> Promise<URI> {
    match full_path {
        Some(path) if !path.is_empty() => finish_resource(Some(path), matcher),
        _ => {
            let mut absolute_matcher = matcher.clone();
            absolute_matcher.file_location = FileLocationKind::Absolute;
            get_resource(filename, &absolute_matcher, None)
        }
    }
}

/// O fim comum de `getResource`: normaliza o caminho e o transforma em URI.
fn finish_resource(full_path: Option<String>, matcher: &ProblemMatcher) -> Promise<URI> {
    let Some(full_path) = full_path else {
        return Promise::rejected(Error::new("FileLocationKind is not actionable. Does the matcher have a filePrefix? This should never happen."));
    };
    let env = host_environment();
    let mut full_path = PathModule::native(env.as_ref()).normalize(&full_path);
    full_path = full_path.replace('\\', "/");
    if !full_path.starts_with('/') {
        full_path = format!("/{full_path}");
    }
    let uri = match &matcher.uri_provider {
        Some(provider) => (provider.0)(&full_path),
        None => URI::file(full_path.as_str(), host_platform()),
    };
    Promise::resolved(uri)
}

struct SearchContext {
    filename: String,
    provider: FileSystemProviderRef,
    exclusions: HashSet<String>,
}

fn search_for_file_location(filename: &str, provider: FileSystemProviderRef, args: &config::SearchFileLocationArgs) -> Promise<Option<URI>> {
    let platform = host_platform();
    let exclusions: HashSet<String> =
        as_strings(args.exclude.as_ref()).iter().map(|exclusion| URI::file(exclusion.as_str(), platform).path().to_string_lossy()).collect();
    let include: Vec<URI> = as_strings(args.include.as_ref()).iter().map(|dir| URI::file(dir.as_str(), platform)).collect();
    let context = Rc::new(SearchContext { filename: filename.to_string(), provider, exclusions });
    search_directories(context, Rc::new(include), 0)
}

/// `for (const dir of dirs) { const hit = await search(dir); if (hit) { return hit; } }`.
fn search_directories(context: Rc<SearchContext>, dirs: Rc<Vec<URI>>, index: usize) -> Promise<Option<URI>> {
    let Some(dir) = dirs.get(index).cloned() else {
        return Promise::resolved(None);
    };
    search_directory(context.clone(), dir).then_promise(move |hit| match hit {
        Some(hit) => Promise::resolved(Some(hit)),
        None => search_directories(context, dirs, index + 1),
    })
}

/// A função `search(dir)` interna de `searchForFileLocation`.
fn search_directory(context: Rc<SearchContext>, dir: URI) -> Promise<Option<URI>> {
    if context.exclusions.contains(&dir.path().to_string_lossy()) {
        return Promise::resolved(None);
    }
    let platform = host_platform();
    context.provider.readdir(&dir).then_promise(move |entries| {
        let mut subdirs: Vec<URI> = Vec::new();
        for (name, file_type) in entries {
            if file_type == FileType::DIRECTORY {
                subdirs.push(URI::join_path(&dir, &[name.as_str()], platform));
                continue;
            }
            if file_type == FileType::FILE {
                // Note that sometimes the given `filename` could be a relative
                // path (not just the "name.ext" part). For example, the
                // `filename` can be "/subdir/name.ext". So, just comparing
                // `name` as `filename` is not sufficient. The workaround here
                // is to form the URI with `dir` and `name` and check if it ends
                // with the given `filename`.
                let full_uri = URI::join_path(&dir, &[name.as_str()], platform);
                if full_uri.path().to_string_lossy().ends_with(context.filename.as_str()) {
                    return Promise::resolved(Some(full_uri));
                }
            }
        }
        search_directories(context, Rc::new(subdirs), 0)
    })
}

// ---------------------------------------------------------------------------------------------
// Os casadores de linha
// ---------------------------------------------------------------------------------------------

/// `ILineMatcher`.
pub trait ILineMatcher {
    fn match_length(&self) -> usize;
    fn next(&mut self, line: &str) -> Option<ProblemMatch>;
    fn handle(&mut self, lines: &[String], start: usize) -> HandleResult;
}

/// `createLineMatcher`.
pub fn create_line_matcher(matcher: ProblemMatcher, file_service: Option<Rc<dyn IFileService>>, log_service: Option<Rc<dyn LogService>>) -> Box<dyn ILineMatcher> {
    match matcher.pattern.clone() {
        OneOrMany::Many(patterns) => Box::new(MultiLineMatcher::new(matcher, patterns, file_service, log_service)),
        OneOrMany::One(pattern) => Box::new(SingleLineMatcher::new(matcher, pattern, file_service, log_service)),
    }
}

fn end_of_line() -> &'static str {
    if Platform::detect().is_windows { "\r\n" } else { "\n" }
}

/// `Strings.trim(value)`: tira os espaços (`' '`) das pontas.
fn trim_spaces(value: &str) -> String {
    let units: Vec<u16> = value.encode_utf16().collect();
    String::from_utf16_lossy(strings::trim(&units, &[0x20]))
}

/// `parseInt(value)` (base 10, ou 16 com o prefixo `0x`): `NaN` quando não começa por um dígito.
fn parse_int(value: &str) -> f64 {
    let text = value.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (radix, rest) = match rest.strip_prefix("0x").or_else(|| rest.strip_prefix("0X")) {
        Some(hex) => (16, hex),
        None => (10, rest),
    };
    let mut number = 0f64;
    let mut any = false;
    for digit in rest.chars().map_while(|c| c.to_digit(radix)) {
        number = number * f64::from(radix) + f64::from(digit);
        any = true;
    }
    if !any {
        return f64::NAN;
    }
    if negative { -number } else { number }
}

fn fill_property(data: &mut ProblemData, property: Property, pattern: &ProblemPattern, matches: &[Option<String>], trim: bool) {
    let pattern_group = pattern.group(property);
    if data.slot(property).is_none()
        && let Some(group) = pattern_group
        && group < matches.len()
        && let Some(value) = &matches[group]
    {
        let value = if trim { trim_spaces(value) } else { value.clone() };
        *data.slot(property) = Some(value);
    }
}

fn append_property(data: &mut ProblemData, property: Property, pattern: &ProblemPattern, matches: &[Option<String>], trim: bool) {
    let pattern_group = pattern.group(property);
    if data.slot(property).is_none() {
        fill_property(data, property, pattern, matches, trim);
    } else if let Some(group) = pattern_group
        && group < matches.len()
    {
        // Um grupo que não participou é `undefined`, que a concatenação escreve como `undefined`.
        let value = match &matches[group] {
            Some(value) if trim => trim_spaces(value),
            Some(value) => value.clone(),
            None => "undefined".to_string(),
        };
        if let Some(current) = data.slot(property) {
            *current = format!("{current}{}{value}", end_of_line());
        }
    }
}

/// `fillProblemData`: `false` quando não há dado a preencher.
fn fill_problem_data(data: Option<&mut ProblemData>, pattern: &ProblemPattern, matches: &[Option<String>]) -> bool {
    let Some(data) = data else {
        return false;
    };
    fill_property(data, Property::File, pattern, matches, true);
    append_property(data, Property::Message, pattern, matches, true);
    fill_property(data, Property::Code, pattern, matches, true);
    fill_property(data, Property::Severity, pattern, matches, true);
    fill_property(data, Property::Location, pattern, matches, true);
    fill_property(data, Property::Line, pattern, matches, false);
    fill_property(data, Property::Character, pattern, matches, false);
    fill_property(data, Property::EndLine, pattern, matches, false);
    fill_property(data, Property::EndCharacter, pattern, matches, false);
    true
}

/// `AbstractLineMatcher` sem os métodos que cada subclasse sobrescreve.
struct AbstractLineMatcher {
    matcher: ProblemMatcher,
    file_service: Option<Rc<dyn IFileService>>,
    log_service: Option<Rc<dyn LogService>>,
}

impl AbstractLineMatcher {
    fn new(matcher: ProblemMatcher, file_service: Option<Rc<dyn IFileService>>, log_service: Option<Rc<dyn LogService>>) -> Self {
        Self { matcher, file_service, log_service }
    }

    /// `regexpExec`: o `RegExpExecArray` como a lista dos grupos (`None` é o `undefined` de um grupo que não participou).
    fn regexp_exec(&self, regexp: &JsRegExp, line: &str) -> Option<Vec<Option<String>>> {
        let start = Instant::now();
        let input: Vec<u16> = line.encode_utf16().collect();
        let found = regexp.exec(&input);
        let elapsed = start.elapsed().as_millis();
        if elapsed > 5
            && let Some(log_service) = &self.log_service
        {
            log_service.trace_args(
                &format!("ProblemMatcher: slow regexp took {elapsed}ms to execute"),
                &[LogArg::Value(Dynamic::string(&String::from_utf16_lossy(regexp.source())))],
            );
        }
        let found = found?;
        Some(found.captures.iter().map(|capture| capture.as_ref().map(|range| String::from_utf16_lossy(&input[range.clone()]))).collect())
    }

    fn get_marker_match(&self, data: &ProblemData) -> Option<ProblemMatch> {
        let location = self.get_location(data);
        if is_truthy(&data.file)
            && let Some(location) = location
            && is_truthy(&data.message)
        {
            let mut marker = MarkerData::new(
                self.get_severity(data),
                data.message.clone().unwrap_or_default(),
                location.start_line_number as i64,
                location.start_character as i64,
                location.end_line_number as i64,
                location.end_character as i64,
            );
            if let Some(code) = &data.code {
                marker.code = Some(MarkerCode::Text(code.clone()));
            }
            if let Some(source) = &self.matcher.source {
                marker.source = Some(source.clone());
            }
            return Some(ProblemMatch {
                description: self.matcher.clone(),
                resource: get_resource(data.file.as_deref().unwrap_or_default(), &self.matcher, self.file_service.clone()),
                marker,
            });
        }
        None
    }

    fn get_location(&self, data: &ProblemData) -> Option<Location> {
        if data.kind == Some(ProblemLocationKind::File) {
            return Some(create_location(0.0, Some(0.0), Some(0.0), Some(0.0)));
        }
        if is_truthy(&data.location) {
            return parse_location_info(data.location.as_deref().unwrap_or_default());
        }
        if !is_truthy(&data.line) {
            return None;
        }
        let start_line = parse_int(data.line.as_deref().unwrap_or_default());
        let start_column = is_truthy(&data.character).then(|| parse_int(data.character.as_deref().unwrap_or_default()));
        let end_line = is_truthy(&data.end_line).then(|| parse_int(data.end_line.as_deref().unwrap_or_default()));
        let end_column = is_truthy(&data.end_character).then(|| parse_int(data.end_character.as_deref().unwrap_or_default()));
        Some(create_location(start_line, start_column, end_line, end_column))
    }

    fn get_severity(&self, data: &ProblemData) -> MarkerSeverity {
        let mut result: Option<Severity> = None;
        if let Some(value) = data.severity.as_deref()
            && !value.is_empty()
        {
            let mut severity = Severity::from_value(value);
            if severity == Severity::Ignore {
                if value == "E" {
                    severity = Severity::Error;
                } else if value == "W" {
                    severity = Severity::Warning;
                } else if value == "I" {
                    severity = Severity::Info;
                } else if value.eq_ignore_ascii_case("hint") {
                    severity = Severity::Info;
                } else if value.eq_ignore_ascii_case("note") {
                    severity = Severity::Info;
                }
            }
            result = Some(severity);
        }
        let severity = match result {
            Some(severity) if severity != Severity::Ignore => severity,
            // `this.matcher.severity || Severity.Error`: `Severity.Ignore` é 0, logo falso.
            _ => self.matcher.severity.filter(|severity| *severity != Severity::Ignore).unwrap_or(Severity::Error),
        };
        MarkerSeverity::from_severity(severity)
    }
}

fn parse_location_info(value: &str) -> Option<Location> {
    // `!value.match(/(\d+|\d+,\d+|\d+,\d+,\d+,\d+)/)`: a primeira alternativa já exige um dígito.
    if value.is_empty() || !value.chars().any(|c| c.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<&str> = value.split(',').collect();
    let start_line = parse_int(parts[0]);
    let start_column = (parts.len() > 1).then(|| parse_int(parts[1]));
    if parts.len() > 3 {
        Some(create_location(start_line, start_column, Some(parse_int(parts[2])), Some(parse_int(parts[3]))))
    } else {
        Some(create_location(start_line, start_column, None, None))
    }
}

fn create_location(start_line: f64, start_column: Option<f64>, end_line: Option<f64>, end_column: Option<f64>) -> Location {
    if let (Some(start_column), Some(end_column)) = (start_column, end_column) {
        // `endLine || startLine`.
        let end_line = end_line.filter(|line| *line != 0.0 && !line.is_nan()).unwrap_or(start_line);
        return Location { start_line_number: start_line, start_character: start_column, end_line_number: end_line, end_character: end_column };
    }
    if let Some(start_column) = start_column {
        return Location { start_line_number: start_line, start_character: start_column, end_line_number: start_line, end_character: start_column };
    }
    // See https://github.com/microsoft/vscode/issues/80288#issuecomment-650636442 for discussion
    Location { start_line_number: start_line, start_character: 1.0, end_line_number: start_line, end_character: 2147483647.0 }
}

struct SingleLineMatcher {
    base: AbstractLineMatcher,
    pattern: ProblemPattern,
}

impl SingleLineMatcher {
    fn new(matcher: ProblemMatcher, pattern: ProblemPattern, file_service: Option<Rc<dyn IFileService>>, log_service: Option<Rc<dyn LogService>>) -> Self {
        Self { base: AbstractLineMatcher::new(matcher, file_service, log_service), pattern }
    }
}

impl ILineMatcher for SingleLineMatcher {
    fn match_length(&self) -> usize {
        1
    }

    fn handle(&mut self, lines: &[String], start: usize) -> HandleResult {
        assert_ok(&(lines.len() as i64 - start as i64 == 1), None);
        let mut data = ProblemData::default();
        if self.pattern.kind.is_some() {
            data.kind = self.pattern.kind;
        }
        if let Some(matches) = self.base.regexp_exec(&self.pattern.regexp, &lines[start]) {
            fill_problem_data(Some(&mut data), &self.pattern, &matches);
            if data.kind == Some(ProblemLocationKind::Location) && !is_truthy(&data.location) && !is_truthy(&data.line) && is_truthy(&data.file) {
                data.kind = Some(ProblemLocationKind::File);
            }
            if let Some(matched) = self.base.get_marker_match(&data) {
                return HandleResult { r#match: Some(matched), r#continue: false };
            }
        }
        HandleResult { r#match: None, r#continue: false }
    }

    fn next(&mut self, _line: &str) -> Option<ProblemMatch> {
        None
    }
}

struct MultiLineMatcher {
    base: AbstractLineMatcher,
    patterns: Vec<ProblemPattern>,
    data: Option<ProblemData>,
}

impl MultiLineMatcher {
    fn new(matcher: ProblemMatcher, patterns: Vec<ProblemPattern>, file_service: Option<Rc<dyn IFileService>>, log_service: Option<Rc<dyn LogService>>) -> Self {
        Self { base: AbstractLineMatcher::new(matcher, file_service, log_service), patterns, data: None }
    }
}

impl ILineMatcher for MultiLineMatcher {
    fn match_length(&self) -> usize {
        self.patterns.len()
    }

    fn handle(&mut self, lines: &[String], start: usize) -> HandleResult {
        assert_ok(&(lines.len() as i64 - start as i64 == self.patterns.len() as i64), None);
        self.data = Some(ProblemData { kind: self.patterns[0].kind, ..ProblemData::default() });
        // O `data = Objects.deepClone(data)` do último padrão com `loop`: a cópia recebe o último preenchimento e
        // `this.data` fica com o que os padrões anteriores preencheram.
        let mut looped: Option<ProblemData> = None;
        let last = self.patterns.len() - 1;
        for i in 0..self.patterns.len() {
            let pattern = &self.patterns[i];
            let Some(matches) = self.base.regexp_exec(&pattern.regexp, &lines[i + start]) else {
                return HandleResult { r#match: None, r#continue: false };
            };
            // Only the last pattern can loop
            if pattern.r#loop == Some(true) && i == last {
                looped = self.data.clone();
            }
            let target = match looped.as_mut() {
                Some(data) => Some(data),
                None => self.data.as_mut(),
            };
            fill_problem_data(target, pattern, &matches);
        }
        let r#loop = self.patterns[last].r#loop == Some(true);
        let data = match looped {
            Some(data) => data,
            None => self.data.clone().unwrap_or_default(),
        };
        if !r#loop {
            self.data = None;
        }
        let marker_match = self.base.get_marker_match(&data);
        HandleResult { r#match: marker_match, r#continue: r#loop }
    }

    fn next(&mut self, line: &str) -> Option<ProblemMatch> {
        let pattern = self.patterns[self.patterns.len() - 1].clone();
        assert_ok(&(pattern.r#loop == Some(true)), None);
        let Some(matches) = self.base.regexp_exec(&pattern.regexp, line) else {
            self.data = None;
            return None;
        };
        let mut data = self.data.clone();
        let mut problem_match: Option<ProblemMatch> = None;
        if fill_problem_data(data.as_mut(), &pattern, &matches)
            && let Some(data) = &data
        {
            problem_match = self.base.get_marker_match(data);
        }
        problem_match
    }
}

// ---------------------------------------------------------------------------------------------
// namespace Config
// ---------------------------------------------------------------------------------------------

/// `namespace Config`: a descrição de padrões e de casadores como o usuário a escreve (`tasks.json`) ou a
/// extensão a contribui (`package.json`).
pub mod config {
    use je_base::arrays::OneOrMany;
    use serde_json::{Map, Value};

    fn put(map: &mut Map<String, Value>, key: &str, value: Option<Value>) {
        if let Some(value) = value {
            map.insert(key.to_string(), value);
        }
    }

    fn many_to_json(value: &OneOrMany<String>) -> Value {
        match value {
            OneOrMany::One(text) => Value::from(text.as_str()),
            OneOrMany::Many(texts) => Value::Array(texts.iter().map(|text| Value::from(text.as_str())).collect()),
        }
    }

    /// `JSON.stringify(value, null, 4)`.
    pub fn stringify(value: &Value) -> String {
        let pretty = serde_json::to_string_pretty(value).unwrap_or_default();
        pretty
            .lines()
            .map(|line| {
                let indent = line.len() - line.trim_start_matches(' ').len();
                format!("{}{}", " ".repeat(indent * 2), &line[indent..])
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// `Config.IProblemPattern`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct ProblemPattern {
        /// The regular expression to find a problem in the console output of an executed task.
        pub regexp: Option<String>,
        /// Whether the pattern matches a whole file, or a location (file/line). Only valid on the first problem
        /// pattern in a multi line problem matcher.
        pub kind: Option<String>,
        /// The match group index of the filename. If omitted 1 is used.
        pub file: Option<usize>,
        /// The match group index of the problem's location.
        pub location: Option<usize>,
        /// The match group index of the problem's line in the source file. Defaults to 2.
        pub line: Option<usize>,
        /// The match group index of the problem's column in the source file. Defaults to 3.
        pub column: Option<usize>,
        /// The match group index of the problem's end line in the source file.
        pub end_line: Option<usize>,
        /// The match group index of the problem's end column in the source file.
        pub end_column: Option<usize>,
        /// The match group index of the problem's severity.
        pub severity: Option<usize>,
        /// The match group index of the problem's code.
        pub code: Option<usize>,
        /// The match group index of the message.
        pub message: Option<usize>,
        /// Specifies if the last pattern in a multi line problem matcher should loop as long as it does match a
        /// line consequently. Only valid on the last problem pattern in a multi line problem matcher.
        pub r#loop: Option<bool>,
    }

    impl ProblemPattern {
        pub fn to_json(&self) -> Value {
            let mut map = Map::new();
            put(&mut map, "regexp", self.regexp.as_deref().map(Value::from));
            put(&mut map, "kind", self.kind.as_deref().map(Value::from));
            put(&mut map, "file", self.file.map(Value::from));
            put(&mut map, "location", self.location.map(Value::from));
            put(&mut map, "line", self.line.map(Value::from));
            put(&mut map, "column", self.column.map(Value::from));
            put(&mut map, "endLine", self.end_line.map(Value::from));
            put(&mut map, "endColumn", self.end_column.map(Value::from));
            put(&mut map, "severity", self.severity.map(Value::from));
            put(&mut map, "code", self.code.map(Value::from));
            put(&mut map, "message", self.message.map(Value::from));
            put(&mut map, "loop", self.r#loop.map(Value::from));
            Value::Object(map)
        }
    }

    /// `CheckedProblemPattern.is`: `regexp` é uma string.
    pub fn is_checked_problem_pattern(value: &ProblemPattern) -> bool {
        value.regexp.is_some()
    }

    /// `Config.INamedProblemPattern`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct NamedProblemPattern {
        /// The name of the problem pattern.
        pub name: String,
        /// A human readable label.
        pub label: Option<String>,
        pub pattern: ProblemPattern,
    }

    impl NamedProblemPattern {
        pub fn to_json(&self) -> Value {
            let Value::Object(pattern) = self.pattern.to_json() else {
                return Value::Null;
            };
            let mut map = Map::new();
            map.insert("name".to_string(), Value::from(self.name.as_str()));
            put(&mut map, "label", self.label.as_deref().map(Value::from));
            map.extend(pattern);
            Value::Object(map)
        }
    }

    /// `Config.MultiLineProblemPattern`.
    pub type MultiLineProblemPattern = Vec<ProblemPattern>;

    /// `MultiLineCheckedProblemPattern.is`: todos os elementos têm `regexp` (um array vazio passa).
    pub fn is_multi_line_checked_problem_pattern(value: &[ProblemPattern]) -> bool {
        value.iter().all(is_checked_problem_pattern)
    }

    /// `Config.INamedMultiLineCheckedProblemPattern`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct NamedMultiLineCheckedProblemPattern {
        /// The name of the problem pattern.
        pub name: String,
        /// A human readable label.
        pub label: Option<String>,
        /// The actual patterns.
        pub patterns: MultiLineProblemPattern,
    }

    impl NamedMultiLineCheckedProblemPattern {
        pub fn to_json(&self) -> Value {
            let mut map = Map::new();
            map.insert("name".to_string(), Value::from(self.name.as_str()));
            put(&mut map, "label", self.label.as_deref().map(Value::from));
            map.insert("patterns".to_string(), Value::Array(self.patterns.iter().map(ProblemPattern::to_json).collect()));
            Value::Object(map)
        }
    }

    /// `NamedMultiLineCheckedProblemPattern.is`: o nome e todos os padrões com `regexp`.
    pub fn is_named_multi_line_checked_problem_pattern(value: &NamedMultiLineCheckedProblemPattern) -> bool {
        is_multi_line_checked_problem_pattern(&value.patterns)
    }

    /// Um elemento de `Config.NamedProblemPatterns`.
    #[derive(Clone, Debug, PartialEq)]
    pub enum NamedProblemPatternEntry {
        Single(NamedProblemPattern),
        Multi(NamedMultiLineCheckedProblemPattern),
    }

    impl NamedProblemPatternEntry {
        pub fn name(&self) -> &str {
            match self {
                NamedProblemPatternEntry::Single(pattern) => &pattern.name,
                NamedProblemPatternEntry::Multi(pattern) => &pattern.name,
            }
        }

        pub fn to_json(&self) -> Value {
            match self {
                NamedProblemPatternEntry::Single(pattern) => pattern.to_json(),
                NamedProblemPatternEntry::Multi(pattern) => pattern.to_json(),
            }
        }
    }

    /// `Config.NamedProblemPatterns`.
    pub type NamedProblemPatterns = Vec<NamedProblemPatternEntry>;

    /// `Config.IWatchingPattern`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct WatchingPattern {
        /// The actual regular expression.
        pub regexp: Option<String>,
        /// The match group index of the filename. If provided the expression is matched for that file only.
        pub file: Option<usize>,
    }

    /// `string | IWatchingPattern`.
    #[derive(Clone, Debug, PartialEq)]
    pub enum WatchingPatternValue {
        Regexp(String),
        Pattern(WatchingPattern),
    }

    impl WatchingPatternValue {
        fn to_json(&self) -> Value {
            match self {
                WatchingPatternValue::Regexp(regexp) => Value::from(regexp.as_str()),
                WatchingPatternValue::Pattern(pattern) => {
                    let mut map = Map::new();
                    put(&mut map, "regexp", pattern.regexp.as_deref().map(Value::from));
                    put(&mut map, "file", pattern.file.map(Value::from));
                    Value::Object(map)
                }
            }
        }
    }

    /// `Config.IBackgroundMonitor`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct BackgroundMonitor {
        /// If set to true the watcher starts in active mode.
        pub active_on_start: Option<bool>,
        /// If matched in the output the start of a watching task is signaled.
        pub begins_pattern: Option<WatchingPatternValue>,
        /// If matched in the output the end of a watching task is signaled.
        pub ends_pattern: Option<WatchingPatternValue>,
    }

    impl BackgroundMonitor {
        fn to_json(&self) -> Value {
            let mut map = Map::new();
            put(&mut map, "activeOnStart", self.active_on_start.map(Value::from));
            put(&mut map, "beginsPattern", self.begins_pattern.as_ref().map(WatchingPatternValue::to_json));
            put(&mut map, "endsPattern", self.ends_pattern.as_ref().map(WatchingPatternValue::to_json));
            Value::Object(map)
        }
    }

    /// `Config.SearchFileLocationArgs`.
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct SearchFileLocationArgs {
        pub include: Option<OneOrMany<String>>,
        pub exclude: Option<OneOrMany<String>>,
    }

    impl SearchFileLocationArgs {
        fn to_json(&self) -> Value {
            let mut map = Map::new();
            put(&mut map, "include", self.include.as_ref().map(many_to_json));
            put(&mut map, "exclude", self.exclude.as_ref().map(many_to_json));
            Value::Object(map)
        }
    }

    /// `ProblemMatcher.fileLocation`: `SingleOrMany<string> | ['search', SearchFileLocationArgs]`.
    #[derive(Clone, Debug, PartialEq)]
    pub enum FileLocation {
        /// Uma string (`'absolute'`, `'relative'`, `'autoDetect'` ou `'search'`).
        Single(String),
        /// Um array só de strings (`['relative', 'path value']`).
        Strings(Vec<String>),
        /// O array com o objeto de argumentos: o primeiro elemento (`'search'`) e o segundo, se houver.
        SearchArray(String, Option<SearchFileLocationArgs>),
    }

    impl FileLocation {
        fn to_json(&self) -> Value {
            match self {
                FileLocation::Single(value) => Value::from(value.as_str()),
                FileLocation::Strings(values) => Value::Array(values.iter().map(|value| Value::from(value.as_str())).collect()),
                FileLocation::SearchArray(kind, args) => {
                    let mut items = vec![Value::from(kind.as_str())];
                    if let Some(args) = args {
                        items.push(args.to_json());
                    }
                    Value::Array(items)
                }
            }
        }
    }

    /// `ProblemMatcher.pattern`: `string | SingleOrMany<IProblemPattern>`.
    #[derive(Clone, Debug, PartialEq)]
    pub enum PatternValue {
        /// O nome de um padrão contribuído ou predefinido (`$name`).
        Name(String),
        Single(ProblemPattern),
        Multi(MultiLineProblemPattern),
    }

    impl PatternValue {
        fn to_json(&self) -> Value {
            match self {
                PatternValue::Name(name) => Value::from(name.as_str()),
                PatternValue::Single(pattern) => pattern.to_json(),
                PatternValue::Multi(patterns) => Value::Array(patterns.iter().map(ProblemPattern::to_json).collect()),
            }
        }
    }

    /// `Config.ProblemMatcher` e `Config.INamedProblemMatcher` (os campos `name` e `label`).
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct ProblemMatcher {
        /// The name of a base problem matcher to use.
        pub base: Option<String>,
        /// The owner of the produced VSCode problem.
        pub owner: Option<String>,
        /// A human-readable string describing the source of this problem.
        pub source: Option<String>,
        /// To which kind of documents the problems are applied (`allDocuments`, `openDocuments`, `closedDocuments`).
        pub apply_to: Option<String>,
        /// The severity of the VSCode problem produced by this problem matcher.
        pub severity: Option<String>,
        /// How the filename reported in a problem pattern should be read.
        pub file_location: Option<FileLocation>,
        /// The name of a predefined problem pattern, the inline definition or an array of problem patterns.
        pub pattern: Option<PatternValue>,
        /// A regular expression signaling that a watched tasks begins executing triggered through file watching.
        pub watched_task_begins_reg_exp: Option<String>,
        /// A regular expression signaling that a watched tasks ends executing.
        pub watched_task_ends_reg_exp: Option<String>,
        /// Deprecated, use `background`.
        pub watching: Option<BackgroundMonitor>,
        pub background: Option<BackgroundMonitor>,
        /// `INamedProblemMatcher.name`.
        pub name: Option<String>,
        /// `INamedProblemMatcher.label`.
        pub label: Option<String>,
    }

    impl ProblemMatcher {
        pub fn to_json(&self) -> Value {
            let mut map = Map::new();
            put(&mut map, "base", self.base.as_deref().map(Value::from));
            put(&mut map, "owner", self.owner.as_deref().map(Value::from));
            put(&mut map, "source", self.source.as_deref().map(Value::from));
            put(&mut map, "applyTo", self.apply_to.as_deref().map(Value::from));
            put(&mut map, "severity", self.severity.as_deref().map(Value::from));
            put(&mut map, "fileLocation", self.file_location.as_ref().map(FileLocation::to_json));
            put(&mut map, "pattern", self.pattern.as_ref().map(PatternValue::to_json));
            put(&mut map, "watchedTaskBeginsRegExp", self.watched_task_begins_reg_exp.as_deref().map(Value::from));
            put(&mut map, "watchedTaskEndsRegExp", self.watched_task_ends_reg_exp.as_deref().map(Value::from));
            put(&mut map, "watching", self.watching.as_ref().map(BackgroundMonitor::to_json));
            put(&mut map, "background", self.background.as_ref().map(BackgroundMonitor::to_json));
            put(&mut map, "name", self.name.as_deref().map(Value::from));
            put(&mut map, "label", self.label.as_deref().map(Value::from));
            Value::Object(map)
        }
    }

    /// `Config.isNamedProblemMatcher`.
    pub fn is_named_problem_matcher(value: &ProblemMatcher) -> bool {
        value.name.is_some()
    }
}

// ---------------------------------------------------------------------------------------------
// ProblemPatternParser
// ---------------------------------------------------------------------------------------------

/// O argumento de `ProblemPatternParser.parse` (a união das quatro sobrecargas).
#[derive(Clone, Debug)]
pub enum PatternInput {
    Single(config::ProblemPattern),
    Multi(config::MultiLineProblemPattern),
    Named(config::NamedProblemPattern),
    NamedMultiLine(config::NamedMultiLineCheckedProblemPattern),
}

/// O resultado de `ProblemPatternParser.parse` (o tipo de retorno de cada sobrecarga).
#[derive(Clone, Debug, PartialEq)]
pub enum ParsedProblemPattern {
    Single(ProblemPattern),
    Multi(MultiLineProblemPattern),
    Named(NamedProblemPattern),
    NamedMultiLine(NamedMultiLineProblemPattern),
}

/// `ProblemPatternParser`.
pub struct ProblemPatternParser {
    parser: Parser,
}

impl ProblemPatternParser {
    pub fn new(logger: Rc<dyn ProblemReporter>) -> Self {
        Self { parser: Parser::new(logger) }
    }

    pub fn problem_reporter(&self) -> &Rc<dyn ProblemReporter> {
        self.parser.problem_reporter()
    }

    pub fn reset(&self) {
        self.parser.reset();
    }

    pub fn parse(&self, value: PatternInput) -> Option<ParsedProblemPattern> {
        match value {
            PatternInput::NamedMultiLine(value) if config::is_named_multi_line_checked_problem_pattern(&value) => {
                self.create_named_multi_line_problem_pattern(&value).map(ParsedProblemPattern::NamedMultiLine)
            }
            PatternInput::Multi(value) if config::is_multi_line_checked_problem_pattern(&value) => {
                self.create_multi_line_problem_pattern(&value).map(ParsedProblemPattern::Multi)
            }
            PatternInput::Named(value) if config::is_checked_problem_pattern(&value.pattern) => {
                let Some(result) = self.create_single_problem_pattern(&value.pattern) else {
                    // `result.name = value.name` sobre `null` lança no original.
                    errors::throw(Error::type_error("Cannot set properties of null (setting 'name')"));
                };
                Some(ParsedProblemPattern::Named(NamedProblemPattern { pattern: result, name: value.name }))
            }
            PatternInput::Single(value) if config::is_checked_problem_pattern(&value) => {
                self.create_single_problem_pattern(&value).map(ParsedProblemPattern::Single)
            }
            _ => {
                self.parser.error(&localize("ProblemPatternParser.problemPattern.missingRegExp", "The problem pattern is missing a regular expression.", &[]));
                None
            }
        }
    }

    fn create_single_problem_pattern(&self, value: &config::ProblemPattern) -> Option<ProblemPattern> {
        let mut result = self.do_create_single_problem_pattern(value, true)?;
        if result.kind.is_none() {
            result.kind = Some(ProblemLocationKind::Location);
        }
        if self.validate_problem_pattern(std::slice::from_ref(&result)) { Some(result) } else { None }
    }

    fn create_named_multi_line_problem_pattern(&self, value: &config::NamedMultiLineCheckedProblemPattern) -> Option<NamedMultiLineProblemPattern> {
        let valid_patterns = self.create_multi_line_problem_pattern(&value.patterns)?;
        Some(NamedMultiLineProblemPattern {
            name: value.name.clone(),
            label: match &value.label {
                Some(label) if !label.is_empty() => label.clone(),
                _ => value.name.clone(),
            },
            patterns: valid_patterns,
        })
    }

    fn create_multi_line_problem_pattern(&self, values: &[config::ProblemPattern]) -> Option<MultiLineProblemPattern> {
        let mut result: MultiLineProblemPattern = Vec::new();
        for (i, value) in values.iter().enumerate() {
            let mut pattern = self.do_create_single_problem_pattern(value, false)?;
            if i + 1 < values.len() && pattern.r#loop == Some(true) {
                pattern.r#loop = Some(false);
                self.parser.error(&localize("ProblemPatternParser.loopProperty.notLast", "The loop property is only supported on the last line matcher.", &[]));
            }
            result.push(pattern);
        }
        if result.is_empty() {
            self.parser.error(&localize(
                "ProblemPatternParser.problemPattern.emptyPattern",
                "The problem pattern is invalid. It must contain at least one pattern.",
                &[],
            ));
            return None;
        }
        if result[0].kind.is_none() {
            result[0].kind = Some(ProblemLocationKind::Location);
        }
        if self.validate_problem_pattern(&result) { Some(result) } else { None }
    }

    fn do_create_single_problem_pattern(&self, value: &config::ProblemPattern, set_defaults: bool) -> Option<ProblemPattern> {
        let regexp = self.create_regular_expression(value.regexp.as_deref().unwrap_or_default())?;
        let mut result = ProblemPattern::new(regexp);
        if let Some(kind) = value.kind.as_deref()
            && !kind.is_empty()
        {
            result.kind = ProblemLocationKind::from_string(kind);
        }
        result.file = value.file;
        result.location = value.location;
        result.line = value.line;
        result.character = value.column;
        result.end_line = value.end_line;
        result.end_character = value.end_column;
        result.severity = value.severity;
        result.code = value.code;
        result.message = value.message;
        if let Some(r#loop) = value.r#loop {
            result.r#loop = Some(r#loop);
        }
        if set_defaults {
            // `Objects.mixin(result, defaultValue, false)`: só preenche o que falta.
            if result.location.is_some_and(|location| location != 0) || result.kind == Some(ProblemLocationKind::File) {
                result.file.get_or_insert(1);
                result.message.get_or_insert(0);
            } else {
                result.file.get_or_insert(1);
                result.line.get_or_insert(2);
                result.character.get_or_insert(3);
                result.message.get_or_insert(0);
            }
        }
        Some(result)
    }

    fn validate_problem_pattern(&self, values: &[ProblemPattern]) -> bool {
        if values.is_empty() {
            self.parser.error(&localize(
                "ProblemPatternParser.problemPattern.emptyPattern",
                "The problem pattern is invalid. It must contain at least one pattern.",
                &[],
            ));
            return false;
        }
        let mut file = false;
        let mut message = false;
        let mut location = false;
        let mut line = false;
        let location_kind = values[0].kind.unwrap_or(ProblemLocationKind::Location);

        for (i, pattern) in values.iter().enumerate() {
            // `pattern.kind` é verdadeiro só para `Location` (`ProblemLocationKind.File` é 0).
            if i != 0 && pattern.kind == Some(ProblemLocationKind::Location) {
                self.parser.error(&localize(
                    "ProblemPatternParser.problemPattern.kindProperty.notFirst",
                    "The problem pattern is invalid. The kind property must be provided only in the first element",
                    &[],
                ));
            }
            file = file || pattern.file.is_some();
            message = message || pattern.message.is_some();
            location = location || pattern.location.is_some();
            line = line || pattern.line.is_some();
        }
        if !(file && message) {
            self.parser.error(&localize(
                "ProblemPatternParser.problemPattern.missingProperty",
                "The problem pattern is invalid. It must have at least have a file and a message.",
                &[],
            ));
            return false;
        }
        if location_kind == ProblemLocationKind::Location && !(location || line) {
            self.parser.error(&localize(
                "ProblemPatternParser.problemPattern.missingLocation",
                "The problem pattern is invalid. It must either have kind: \"file\" or have a line or location match group.",
                &[],
            ));
            return false;
        }
        true
    }

    fn create_regular_expression(&self, value: &str) -> Option<Rc<JsRegExp>> {
        match JsRegExp::from_str(value, "") {
            Ok(regexp) => Some(Rc::new(regexp)),
            Err(_) => {
                self.parser.error(&localize("ProblemPatternParser.invalidRegexp", "Error: The string {0} is not a valid regular expression.\n", &[value]));
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Contratos mínimos do ponto de extensão e o relator
// ---------------------------------------------------------------------------------------------

/// `ExtensionMessageCollector`: onde as mensagens de validação de uma extensão aparecem.
pub trait ExtensionMessageCollector {
    fn error(&self, message: &str);
    fn warn(&self, message: &str);
    fn info(&self, message: &str);
}

/// `IExtensionPointUser<T>`: o valor contribuído por uma extensão e o coletor das mensagens dela.
#[derive(Clone)]
pub struct ExtensionPointUser<T> {
    pub value: T,
    pub collector: Rc<dyn ExtensionMessageCollector>,
}

/// `IExtensionPointUserDelta<T>`: `delta.added` e `delta.removed`.
#[derive(Clone)]
pub struct ExtensionPointDelta<T> {
    pub added: Vec<ExtensionPointUser<T>>,
    pub removed: Vec<ExtensionPointUser<T>>,
}

/// `ExtensionRegistryReporter`.
pub struct ExtensionRegistryReporter {
    collector: Rc<dyn ExtensionMessageCollector>,
    validation_status: ValidationStatus,
}

impl ExtensionRegistryReporter {
    pub fn new(collector: Rc<dyn ExtensionMessageCollector>) -> Self {
        Self { collector, validation_status: ValidationStatus::new() }
    }

    /// O construtor com o `ValidationStatus` dado.
    pub fn with_status(collector: Rc<dyn ExtensionMessageCollector>, validation_status: ValidationStatus) -> Self {
        Self { collector, validation_status }
    }
}

impl ProblemReporter for ExtensionRegistryReporter {
    fn info(&self, message: &str) {
        self.validation_status.set_state(ValidationState::Info);
        self.collector.info(message);
    }

    fn warn(&self, message: &str) {
        self.validation_status.set_state(ValidationState::Warning);
        self.collector.warn(message);
    }

    fn error(&self, message: &str) {
        self.validation_status.set_state(ValidationState::Error);
        self.collector.error(message);
    }

    fn fatal(&self, message: &str) {
        self.validation_status.set_state(ValidationState::Fatal);
        self.collector.error(message);
    }

    fn status(&self) -> &ValidationStatus {
        &self.validation_status
    }
}

// ---------------------------------------------------------------------------------------------
// Schemas
// ---------------------------------------------------------------------------------------------

fn desc(key: &str, message: &str) -> Dynamic {
    Dynamic::string(&localize(key, message, &[]))
}

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn number(value: f64) -> Dynamic {
    Dynamic::Number(value)
}

fn strings_array(values: &[&str]) -> Dynamic {
    Dynamic::array(values.iter().map(|value| Dynamic::string(value)))
}

fn typed(kind: &str, key: &str, message: &str) -> Dynamic {
    Dynamic::object([("type", text(kind)), ("description", desc(key, message))])
}

fn set(target: &Dynamic, key: &str, value: Dynamic) {
    set_property(target, &JsString::from(key), value);
}

/// `namespace Schemas`: os esquemas JSON (`IJSONSchema`) das descrições, que o `tasks.json` e o ponto de extensão
/// usam. Os nós compartilhados (`Schemas.ProblemPattern` dentro de `MultiLineProblemPattern` e `PatternType`) são o
/// mesmo [`Dynamic`], como no original.
pub struct Schemas {
    pub problem_pattern: Dynamic,
    pub named_problem_pattern: Dynamic,
    pub multi_line_problem_pattern: Dynamic,
    pub named_multi_line_problem_pattern: Dynamic,
    pub watching_pattern: Dynamic,
    pub pattern_type: Dynamic,
    pub problem_matcher: Dynamic,
    pub legacy_problem_matcher: Dynamic,
    pub named_problem_matcher: Dynamic,
}

thread_local! {
    static SCHEMAS: Rc<Schemas> = Rc::new(Schemas::new());
}

/// `Schemas`.
pub fn schemas() -> Rc<Schemas> {
    SCHEMAS.with(Rc::clone)
}

fn file_location_schema() -> Dynamic {
    let kinds = || Dynamic::object([("type", text("string")), ("enum", strings_array(&["absolute", "relative", "autoDetect", "search"]))]);
    let include_exclude = || Dynamic::object([("oneOf", Dynamic::array([Dynamic::object([("type", text("string"))]), Dynamic::object([("type", text("array")), ("items", Dynamic::object([("type", text("string"))]))])]))]);
    Dynamic::object([
        (
            "oneOf",
            Dynamic::array([
                kinds(),
                Dynamic::object([
                    ("type", text("array")),
                    ("prefixItems", Dynamic::array([kinds()])),
                    ("minItems", number(1.0)),
                    ("maxItems", number(1.0)),
                    ("additionalItems", Dynamic::Bool(false)),
                ]),
                Dynamic::object([
                    ("type", text("array")),
                    (
                        "prefixItems",
                        Dynamic::array([
                            Dynamic::object([("type", text("string")), ("enum", strings_array(&["relative", "autoDetect"]))]),
                            Dynamic::object([("type", text("string"))]),
                        ]),
                    ),
                    ("minItems", number(2.0)),
                    ("maxItems", number(2.0)),
                    ("additionalItems", Dynamic::Bool(false)),
                    (
                        "examples",
                        Dynamic::array([
                            strings_array(&["relative", "${workspaceFolder}"]),
                            strings_array(&["autoDetect", "${workspaceFolder}"]),
                        ]),
                    ),
                ]),
                Dynamic::object([
                    ("type", text("array")),
                    (
                        "prefixItems",
                        Dynamic::array([
                            Dynamic::object([("type", text("string")), ("enum", strings_array(&["search"]))]),
                            Dynamic::object([
                                ("type", text("object")),
                                ("properties", Dynamic::object([("include", include_exclude()), ("exclude", include_exclude())])),
                                ("required", strings_array(&["include"])),
                            ]),
                        ]),
                    ),
                    ("minItems", number(2.0)),
                    ("maxItems", number(2.0)),
                    ("additionalItems", Dynamic::Bool(false)),
                    (
                        "examples",
                        Dynamic::array([
                            Dynamic::array([text("search"), Dynamic::object([("include", strings_array(&["${workspaceFolder}"]))])]),
                            Dynamic::array([text("search"), Dynamic::object([("include", strings_array(&["${workspaceFolder}"])), ("exclude", Dynamic::array([]))])]),
                        ]),
                    ),
                ]),
            ]),
        ),
        (
            "description",
            desc(
                "ProblemMatcherSchema.fileLocation",
                "Defines how file names reported in a problem pattern should be interpreted. A relative fileLocation may be an array, where the second element of the array is the path of the relative file location. The search fileLocation mode, performs a deep (and, possibly, heavy) file system search within the directories specified by the include/exclude properties of the second element (or the current workspace directory if not specified).",
            ),
        ),
    ])
}

impl Schemas {
    fn new() -> Self {
        let problem_pattern = Dynamic::object([
            (
                "default",
                Dynamic::object([
                    ("regexp", text(r"^([^\\s].*)\\((\\d+,\\d+)\\):\\s*(.*)$")),
                    ("file", number(1.0)),
                    ("location", number(2.0)),
                    ("message", number(3.0)),
                ]),
            ),
            ("type", text("object")),
            ("additionalProperties", Dynamic::Bool(false)),
            (
                "properties",
                Dynamic::object([
                    ("regexp", typed("string", "ProblemPatternSchema.regexp", "The regular expression to find an error, warning or info in the output.")),
                    ("kind", typed("string", "ProblemPatternSchema.kind", "whether the pattern matches a location (file and line) or only a file.")),
                    ("file", typed("integer", "ProblemPatternSchema.file", "The match group index of the filename. If omitted 1 is used.")),
                    (
                        "location",
                        typed(
                            "integer",
                            "ProblemPatternSchema.location",
                            "The match group index of the problem's location. Valid location patterns are: (line), (line,column) and (startLine,startColumn,endLine,endColumn). If omitted (line,column) is assumed.",
                        ),
                    ),
                    ("line", typed("integer", "ProblemPatternSchema.line", "The match group index of the problem's line. Defaults to 2")),
                    ("column", typed("integer", "ProblemPatternSchema.column", "The match group index of the problem's line character. Defaults to 3")),
                    ("endLine", typed("integer", "ProblemPatternSchema.endLine", "The match group index of the problem's end line. Defaults to undefined")),
                    ("endColumn", typed("integer", "ProblemPatternSchema.endColumn", "The match group index of the problem's end line character. Defaults to undefined")),
                    ("severity", typed("integer", "ProblemPatternSchema.severity", "The match group index of the problem's severity. Defaults to undefined")),
                    ("code", typed("integer", "ProblemPatternSchema.code", "The match group index of the problem's code. Defaults to undefined")),
                    (
                        "message",
                        typed("integer", "ProblemPatternSchema.message", "The match group index of the message. If omitted it defaults to 4 if location is specified. Otherwise it defaults to 5."),
                    ),
                    (
                        "loop",
                        typed(
                            "boolean",
                            "ProblemPatternSchema.loop",
                            "In a multi line matcher loop indicated whether this pattern is executed in a loop as long as it matches. Can only specified on a last pattern in a multi line pattern.",
                        ),
                    ),
                ]),
            ),
        ]);

        let named_problem_pattern = deep_clone(&problem_pattern);
        let named_properties = deep_clone(&named_problem_pattern.get_str("properties"));
        set(&named_problem_pattern, "properties", named_properties.clone());
        set(&named_properties, "name", typed("string", "NamedProblemPatternSchema.name", "The name of the problem pattern."));

        let multi_line_problem_pattern = Dynamic::object([("type", text("array")), ("items", problem_pattern.clone())]);

        let named_multi_line_problem_pattern = Dynamic::object([
            ("type", text("object")),
            ("additionalProperties", Dynamic::Bool(false)),
            (
                "properties",
                Dynamic::object([
                    ("name", typed("string", "NamedMultiLineProblemPatternSchema.name", "The name of the problem multi line problem pattern.")),
                    (
                        "patterns",
                        Dynamic::object([
                            ("type", text("array")),
                            ("description", desc("NamedMultiLineProblemPatternSchema.patterns", "The actual patterns.")),
                            ("items", problem_pattern.clone()),
                        ]),
                    ),
                ]),
            ),
        ]);

        let watching_pattern = Dynamic::object([
            ("type", text("object")),
            ("additionalProperties", Dynamic::Bool(false)),
            (
                "properties",
                Dynamic::object([
                    ("regexp", typed("string", "WatchingPatternSchema.regexp", "The regular expression to detect the begin or end of a background task.")),
                    ("file", typed("integer", "WatchingPatternSchema.file", "The match group index of the filename. Can be omitted.")),
                ]),
            ),
        ]);

        let pattern_type = Dynamic::object([
            (
                "anyOf",
                Dynamic::array([
                    typed("string", "PatternTypeSchema.name", "The name of a contributed or predefined pattern"),
                    problem_pattern.clone(),
                    multi_line_problem_pattern.clone(),
                ]),
            ),
            (
                "description",
                desc(
                    "PatternTypeSchema.description",
                    "A problem pattern or the name of a contributed or predefined problem pattern. Can be omitted if base is specified.",
                ),
            ),
        ]);

        let string_or_watching = || Dynamic::array([Dynamic::object([("type", text("string"))]), watching_pattern.clone()]);

        let background = Dynamic::object([
            ("type", text("object")),
            ("additionalProperties", Dynamic::Bool(false)),
            ("description", desc("ProblemMatcherSchema.background", "Patterns to track the begin and end of a matcher active on a background task.")),
            (
                "properties",
                Dynamic::object([
                    (
                        "activeOnStart",
                        typed(
                            "boolean",
                            "ProblemMatcherSchema.background.activeOnStart",
                            "If set to true the background monitor starts in active mode. This is the same as outputting a line that matches beginsPattern when the task starts.",
                        ),
                    ),
                    (
                        "beginsPattern",
                        Dynamic::object([
                            ("oneOf", string_or_watching()),
                            ("description", desc("ProblemMatcherSchema.background.beginsPattern", "If matched in the output the start of a background task is signaled.")),
                        ]),
                    ),
                    (
                        "endsPattern",
                        Dynamic::object([
                            ("oneOf", string_or_watching()),
                            ("description", desc("ProblemMatcherSchema.background.endsPattern", "If matched in the output the end of a background task is signaled.")),
                        ]),
                    ),
                ]),
            ),
        ]);

        let watching = Dynamic::object([
            ("type", text("object")),
            ("additionalProperties", Dynamic::Bool(false)),
            ("deprecationMessage", desc("ProblemMatcherSchema.watching.deprecated", "The watching property is deprecated. Use background instead.")),
            ("description", desc("ProblemMatcherSchema.watching", "Patterns to track the begin and end of a watching matcher.")),
            (
                "properties",
                Dynamic::object([
                    (
                        "activeOnStart",
                        typed(
                            "boolean",
                            "ProblemMatcherSchema.watching.activeOnStart",
                            "If set to true the watcher starts in active mode. This is the same as outputting a line that matches beginsPattern when the task starts.",
                        ),
                    ),
                    (
                        "beginsPattern",
                        Dynamic::object([
                            ("oneOf", string_or_watching()),
                            ("description", desc("ProblemMatcherSchema.watching.beginsPattern", "If matched in the output the start of a watching task is signaled.")),
                        ]),
                    ),
                    (
                        "endsPattern",
                        Dynamic::object([
                            ("oneOf", string_or_watching()),
                            ("description", desc("ProblemMatcherSchema.watching.endsPattern", "If matched in the output the end of a watching task is signaled.")),
                        ]),
                    ),
                ]),
            ),
        ]);

        let problem_matcher = Dynamic::object([
            ("type", text("object")),
            ("additionalProperties", Dynamic::Bool(false)),
            (
                "properties",
                Dynamic::object([
                    ("base", typed("string", "ProblemMatcherSchema.base", "The name of a base problem matcher to use.")),
                    (
                        "owner",
                        typed(
                            "string",
                            "ProblemMatcherSchema.owner",
                            "The owner of the problem inside Code. Can be omitted if base is specified. Defaults to 'external' if omitted and base is not specified.",
                        ),
                    ),
                    (
                        "source",
                        typed(
                            "string",
                            "ProblemMatcherSchema.source",
                            "A human-readable string describing the source of this diagnostic, e.g. 'typescript' or 'super lint'.",
                        ),
                    ),
                    (
                        "severity",
                        Dynamic::object([
                            ("type", text("string")),
                            ("enum", strings_array(&["error", "warning", "info"])),
                            (
                                "description",
                                desc(
                                    "ProblemMatcherSchema.severity",
                                    "The default severity for captures problems. Is used if the pattern doesn't define a match group for severity.",
                                ),
                            ),
                        ]),
                    ),
                    (
                        "applyTo",
                        Dynamic::object([
                            ("type", text("string")),
                            ("enum", strings_array(&["allDocuments", "openDocuments", "closedDocuments"])),
                            (
                                "description",
                                desc("ProblemMatcherSchema.applyTo", "Controls if a problem reported on a text document is applied only to open, closed or all documents."),
                            ),
                        ]),
                    ),
                    ("pattern", pattern_type.clone()),
                    ("fileLocation", file_location_schema()),
                    ("background", background),
                    ("watching", watching),
                ]),
            ),
        ]);

        let legacy_problem_matcher = deep_clone(&problem_matcher);
        let legacy_properties = deep_clone(&legacy_problem_matcher.get_str("properties"));
        set(&legacy_problem_matcher, "properties", legacy_properties.clone());
        set(
            &legacy_properties,
            "watchedTaskBeginsRegExp",
            Dynamic::object([
                ("type", text("string")),
                ("deprecationMessage", desc("LegacyProblemMatcherSchema.watchedBegin.deprecated", "This property is deprecated. Use the watching property instead.")),
                (
                    "description",
                    desc(
                        "LegacyProblemMatcherSchema.watchedBegin",
                        "A regular expression signaling that a watched tasks begins executing triggered through file watching.",
                    ),
                ),
            ]),
        );
        set(
            &legacy_properties,
            "watchedTaskEndsRegExp",
            Dynamic::object([
                ("type", text("string")),
                ("deprecationMessage", desc("LegacyProblemMatcherSchema.watchedEnd.deprecated", "This property is deprecated. Use the watching property instead.")),
                ("description", desc("LegacyProblemMatcherSchema.watchedEnd", "A regular expression signaling that a watched tasks ends executing.")),
            ]),
        );

        let named_problem_matcher = deep_clone(&problem_matcher);
        let named_matcher_properties = deep_clone(&named_problem_matcher.get_str("properties"));
        set(&named_problem_matcher, "properties", named_matcher_properties.clone());
        set(&named_matcher_properties, "name", typed("string", "NamedProblemMatcherSchema.name", "The name of the problem matcher used to refer to it."));
        set(&named_matcher_properties, "label", typed("string", "NamedProblemMatcherSchema.label", "A human readable label of the problem matcher."));

        Self {
            problem_pattern,
            named_problem_pattern,
            multi_line_problem_pattern,
            named_multi_line_problem_pattern,
            watching_pattern,
            pattern_type,
            problem_matcher,
            legacy_problem_matcher,
            named_problem_matcher,
        }
    }
}

/// O `jsonSchema` do ponto de extensão `problemPatterns`.
pub fn problem_patterns_extension_point_schema() -> Dynamic {
    let schemas = schemas();
    Dynamic::object([
        ("description", desc("ProblemPatternExtPoint", "Contributes problem patterns")),
        ("type", text("array")),
        (
            "items",
            Dynamic::object([("anyOf", Dynamic::array([schemas.named_problem_pattern.clone(), schemas.named_multi_line_problem_pattern.clone()]))]),
        ),
    ])
}

/// O `jsonSchema` do ponto de extensão `problemMatchers`.
pub fn problem_matchers_extension_point_schema() -> Dynamic {
    Dynamic::object([
        ("description", desc("ProblemMatcherExtPoint", "Contributes problem matchers")),
        ("type", text("array")),
        ("items", schemas().named_problem_matcher.clone()),
    ])
}

/// `registerExtensionPoint({ extensionPoint: 'problemPatterns', ... })`.
pub fn register_problem_patterns_extension_point() -> Rc<ExtensionPoint> {
    extensions_registry().register_extension_point(ExtensionPointDescriptor {
        extension_point: "problemPatterns".to_string(),
        default_extension_kind: None,
        can_handle_resolver: None,
    })
}

/// `registerExtensionPoint({ extensionPoint: 'problemMatchers', deps: [problemPatternExtPoint], ... })`.
pub fn register_problem_matchers_extension_point() -> Rc<ExtensionPoint> {
    extensions_registry().register_extension_point(ExtensionPointDescriptor {
        extension_point: "problemMatchers".to_string(),
        default_extension_kind: None,
        can_handle_resolver: None,
    })
}

// ---------------------------------------------------------------------------------------------
// ProblemPatternRegistry
// ---------------------------------------------------------------------------------------------

/// `IProblemPatternRegistry`.
pub trait IProblemPatternRegistry {
    fn on_ready(&self) -> Promise<()>;

    fn get(&self, key: &str) -> Option<OneOrMany<ProblemPattern>>;
}

/// `ProblemPatternRegistryImpl`.
pub struct ProblemPatternRegistryImpl {
    patterns: RefCell<IndexMap<String, OneOrMany<ProblemPattern>>>,
    ready_promise: Promise<()>,
    ready_resolver: Resolver<()>,
}

fn regexp(source: &str) -> Rc<JsRegExp> {
    Rc::new(JsRegExp::from_str(source, "").expect("expressão regular literal válida"))
}

impl Default for ProblemPatternRegistryImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl ProblemPatternRegistryImpl {
    pub fn new() -> Self {
        let (ready_promise, ready_resolver) = Promise::new();
        let registry = Self { patterns: RefCell::new(IndexMap::new()), ready_promise, ready_resolver };
        registry.fill_defaults();
        registry
    }

    /// O corpo do `problemPatternExtPoint.setHandler`: `delta.removed`, `delta.added` e, no fim, a resolução de
    /// `onReady` (as extensões estáticas chegam todas no primeiro lote).
    pub fn handle_delta(&self, delta: &ExtensionPointDelta<config::NamedProblemPatterns>) {
        // We get all statically know extension during startup in one batch
        let _ = errors::catch(|| {
            for extension in &delta.removed {
                for pattern in &extension.value {
                    self.patterns.borrow_mut().shift_remove(pattern.name());
                }
            }
            for extension in &delta.added {
                let parser = ProblemPatternParser::new(Rc::new(ExtensionRegistryReporter::new(extension.collector.clone())));
                for pattern in &extension.value {
                    let valid = |parser: &ProblemPatternParser| parser.problem_reporter().status().state() < ValidationState::Error;
                    let input = match pattern {
                        config::NamedProblemPatternEntry::Multi(multi) => PatternInput::NamedMultiLine(multi.clone()),
                        config::NamedProblemPatternEntry::Single(single) => PatternInput::Named(single.clone()),
                    };
                    let result = parser.parse(input);
                    if valid(&parser) {
                        match result {
                            Some(ParsedProblemPattern::NamedMultiLine(result)) => self.add(&result.name, OneOrMany::Many(result.patterns)),
                            Some(ParsedProblemPattern::Named(result)) => self.add(pattern.name(), OneOrMany::One(result.pattern)),
                            _ => {}
                        }
                    } else {
                        extension.collector.error(&localize("ProblemPatternRegistry.error", "Invalid problem pattern. The pattern will be ignored.", &[]));
                        extension.collector.error(&config::stringify(&pattern.to_json()));
                    }
                    parser.reset();
                }
            }
        });
        self.ready_resolver.resolve(());
    }

    pub fn add(&self, key: &str, value: OneOrMany<ProblemPattern>) {
        self.patterns.borrow_mut().insert(key.to_string(), value);
    }

    fn fill_defaults(&self) {
        let location = Some(ProblemLocationKind::Location);
        self.add(
            "msCompile",
            OneOrMany::One(ProblemPattern {
                kind: location,
                file: Some(1),
                location: Some(2),
                severity: Some(4),
                code: Some(5),
                message: Some(6),
                ..ProblemPattern::new(regexp(
                    r"^\s*(?:\s*\d+>)?(\S.*?)(?:\((\d+|\d+,\d+|\d+,\d+,\d+,\d+)\))?\s*:\s+(?:(\S+)\s+)?((?:fatal +)?error|warning|info)\s+(\w+\d+)?\s*:\s*(.*)$",
                ))
            }),
        );
        self.add(
            "gulp-tsc",
            OneOrMany::One(ProblemPattern {
                kind: location,
                file: Some(1),
                location: Some(2),
                code: Some(3),
                message: Some(4),
                ..ProblemPattern::new(regexp(r"^([^\s].*)\((\d+|\d+,\d+|\d+,\d+,\d+,\d+)\):\s+(\d+)\s+(.*)$"))
            }),
        );
        self.add(
            "cpp",
            OneOrMany::One(ProblemPattern {
                kind: location,
                file: Some(1),
                location: Some(2),
                severity: Some(3),
                code: Some(4),
                message: Some(5),
                ..ProblemPattern::new(regexp(r"^(\S.*)\((\d+|\d+,\d+|\d+,\d+,\d+,\d+)\):\s+(error|warning|info)\s+(C\d+)\s*:\s*(.*)$"))
            }),
        );
        self.add(
            "csc",
            OneOrMany::One(ProblemPattern {
                kind: location,
                file: Some(1),
                location: Some(2),
                severity: Some(3),
                code: Some(4),
                message: Some(5),
                ..ProblemPattern::new(regexp(r"^(\S.*)\((\d+|\d+,\d+|\d+,\d+,\d+,\d+)\):\s+(error|warning|info)\s+(CS\d+)\s*:\s*(.*)$"))
            }),
        );
        self.add(
            "vb",
            OneOrMany::One(ProblemPattern {
                kind: location,
                file: Some(1),
                location: Some(2),
                severity: Some(3),
                code: Some(4),
                message: Some(5),
                ..ProblemPattern::new(regexp(r"^(\S.*)\((\d+|\d+,\d+|\d+,\d+,\d+,\d+)\):\s+(error|warning|info)\s+(BC\d+)\s*:\s*(.*)$"))
            }),
        );
        self.add(
            "lessCompile",
            OneOrMany::One(ProblemPattern {
                kind: location,
                message: Some(1),
                file: Some(2),
                line: Some(3),
                ..ProblemPattern::new(regexp(r"^\s*(.*) in file (.*) line no. (\d+)$"))
            }),
        );
        self.add(
            "jshint",
            OneOrMany::One(ProblemPattern {
                kind: location,
                file: Some(1),
                line: Some(2),
                character: Some(3),
                message: Some(4),
                severity: Some(5),
                code: Some(6),
                ..ProblemPattern::new(regexp(r"^(.*):\s+line\s+(\d+),\s+col\s+(\d+),\s(.+?)(?:\s+\((\w)(\d+)\))?$"))
            }),
        );
        self.add(
            "jshint-stylish",
            OneOrMany::Many(vec![
                ProblemPattern { kind: location, file: Some(1), ..ProblemPattern::new(regexp(r"^(.+)$")) },
                ProblemPattern {
                    line: Some(1),
                    character: Some(2),
                    message: Some(3),
                    severity: Some(4),
                    code: Some(5),
                    r#loop: Some(true),
                    ..ProblemPattern::new(regexp(r"^\s+line\s+(\d+)\s+col\s+(\d+)\s+(.+?)(?:\s+\((\w)(\d+)\))?$"))
                },
            ]),
        );
        self.add(
            "eslint-compact",
            OneOrMany::One(ProblemPattern {
                file: Some(1),
                kind: location,
                line: Some(2),
                character: Some(3),
                severity: Some(4),
                message: Some(5),
                code: Some(6),
                ..ProblemPattern::new(regexp(r"^(.+):\sline\s(\d+),\scol\s(\d+),\s(Error|Warning|Info)\s-\s(.+)\s\((.+)\)$"))
            }),
        );
        self.add(
            "eslint-stylish",
            OneOrMany::Many(vec![
                ProblemPattern { kind: location, file: Some(1), ..ProblemPattern::new(regexp(r"^((?:[a-zA-Z]:)*[./\\]+.*?)$")) },
                ProblemPattern {
                    line: Some(1),
                    character: Some(2),
                    severity: Some(3),
                    message: Some(4),
                    code: Some(5),
                    r#loop: Some(true),
                    ..ProblemPattern::new(regexp(r"^\s+(\d+):(\d+)\s+(error|warning|info)\s+(.+?)(?:\s\s+(.*))?$"))
                },
            ]),
        );
        self.add(
            "go",
            OneOrMany::One(ProblemPattern {
                kind: location,
                file: Some(2),
                line: Some(4),
                character: Some(6),
                message: Some(7),
                ..ProblemPattern::new(regexp(r"^([^:]*: )?((.:)?[^:]*):(\d+)(:(\d+))?: (.*)$"))
            }),
        );
    }
}

impl IProblemPatternRegistry for ProblemPatternRegistryImpl {
    fn on_ready(&self) -> Promise<()> {
        self.ready_promise.clone()
    }

    fn get(&self, key: &str) -> Option<OneOrMany<ProblemPattern>> {
        self.patterns.borrow().get(key).cloned()
    }
}

thread_local! {
    static PROBLEM_PATTERN_REGISTRY: Rc<ProblemPatternRegistryImpl> = Rc::new(ProblemPatternRegistryImpl::new());
}

/// `ProblemPatternRegistry`.
pub fn problem_pattern_registry() -> Rc<ProblemPatternRegistryImpl> {
    PROBLEM_PATTERN_REGISTRY.with(Rc::clone)
}

// ---------------------------------------------------------------------------------------------
// ProblemMatcherParser
// ---------------------------------------------------------------------------------------------

/// `ProblemMatcherParser`.
pub struct ProblemMatcherParser {
    parser: Parser,
}

impl ProblemMatcherParser {
    pub fn new(logger: Rc<dyn ProblemReporter>) -> Self {
        Self { parser: Parser::new(logger) }
    }

    pub fn problem_reporter(&self) -> &Rc<dyn ProblemReporter> {
        self.parser.problem_reporter()
    }

    pub fn reset(&self) {
        self.parser.reset();
    }

    pub fn parse(&self, json: &config::ProblemMatcher) -> Option<ProblemMatcher> {
        let result = self.create_problem_matcher(json);
        let mut result = self.check_problem_matcher_valid(json, result)?;
        self.add_watching_matcher(json, &mut result);
        Some(result)
    }

    fn check_problem_matcher_valid(&self, external_problem_matcher: &config::ProblemMatcher, problem_matcher: Option<ProblemMatcher>) -> Option<ProblemMatcher> {
        let json = || config::stringify(&external_problem_matcher.to_json());
        let Some(problem_matcher) = problem_matcher else {
            self.parser.error(&localize(
                "ProblemMatcherParser.noProblemMatcher",
                "Error: the description can't be converted into a problem matcher:\n{0}\n",
                &[&json()],
            ));
            return None;
        };
        if problem_matcher.owner.is_empty() {
            self.parser.error(&localize("ProblemMatcherParser.noOwner", "Error: the description doesn't define an owner:\n{0}\n", &[&json()]));
            return None;
        }
        Some(problem_matcher)
    }

    fn create_problem_matcher(&self, description: &config::ProblemMatcher) -> Option<ProblemMatcher> {
        let mut result: Option<ProblemMatcher> = None;

        let owner = description.owner.clone().unwrap_or_else(generate_uuid);
        let source = description.source.clone();
        let apply_to = description.apply_to.as_deref().and_then(ApplyToKind::from_string).unwrap_or(ApplyToKind::AllDocuments);
        let mut file_location: Option<FileLocationKind> = None;
        let mut file_prefix: Option<FilePrefix> = None;

        match &description.file_location {
            None => {
                file_location = Some(FileLocationKind::Relative);
                file_prefix = Some(FilePrefix::Path("${workspaceFolder}".to_string()));
            }
            Some(config::FileLocation::Single(value)) => {
                let kind = FileLocationKind::from_string(value);
                if let Some(kind) = kind {
                    file_location = Some(kind);
                    if kind == FileLocationKind::Relative || kind == FileLocationKind::AutoDetect {
                        file_prefix = Some(FilePrefix::Path("${workspaceFolder}".to_string()));
                    } else if kind == FileLocationKind::Search {
                        file_prefix = Some(FilePrefix::Search(config::SearchFileLocationArgs {
                            include: Some(OneOrMany::Many(vec!["${workspaceFolder}".to_string()])),
                            exclude: None,
                        }));
                    }
                }
            }
            Some(config::FileLocation::Strings(values)) => {
                if let Some(first) = values.first() {
                    let kind = FileLocationKind::from_string(first);
                    if values.len() == 1 && kind == Some(FileLocationKind::Absolute) {
                        file_location = kind;
                    } else if values.len() == 2
                        && (kind == Some(FileLocationKind::Relative) || kind == Some(FileLocationKind::AutoDetect))
                        && !values[1].is_empty()
                    {
                        file_location = kind;
                        file_prefix = Some(FilePrefix::Path(values[1].clone()));
                    }
                }
            }
            Some(config::FileLocation::SearchArray(first, args)) => {
                let kind = FileLocationKind::from_string(first);
                if kind == Some(FileLocationKind::Search) {
                    file_location = Some(FileLocationKind::Search);
                    file_prefix = Some(FilePrefix::Search(args.clone().unwrap_or_else(|| config::SearchFileLocationArgs {
                        include: Some(OneOrMany::Many(vec!["${workspaceFolder}".to_string()])),
                        exclude: None,
                    })));
                }
            }
        }

        let pattern = description.pattern.as_ref().and_then(|value| self.create_problem_pattern(value));

        let mut severity = description.severity.as_deref().filter(|value| !value.is_empty()).map(Severity::from_value);
        if severity == Some(Severity::Ignore) {
            self.parser.info(&localize(
                "ProblemMatcherParser.unknownSeverity",
                "Info: unknown severity {0}. Valid values are error, warning and info.\n",
                &[description.severity.as_deref().unwrap_or_default()],
            ));
            severity = Some(Severity::Error);
        }

        if let Some(variable_name) = &description.base {
            if variable_name.chars().count() > 1
                && let Some(base_name) = variable_name.strip_prefix('$')
                && let Some(base) = problem_matcher_registry().get(base_name)
            {
                let mut cloned = base;
                if description.owner.is_some() {
                    cloned.owner = owner;
                }
                if description.source.is_some() {
                    cloned.source = source;
                }
                if description.file_location.is_some()
                    && let Some(file_location) = file_location
                {
                    cloned.file_location = file_location;
                    cloned.file_prefix = file_prefix;
                }
                if description.pattern.is_some()
                    && let Some(pattern) = pattern
                {
                    cloned.pattern = pattern;
                }
                if description.severity.is_some()
                    && let Some(severity) = severity
                {
                    cloned.severity = Some(severity);
                }
                if description.apply_to.is_some() {
                    cloned.apply_to = apply_to;
                }
                result = Some(cloned);
            }
        } else if let (Some(file_location), Some(pattern)) = (file_location, pattern) {
            let mut matcher = ProblemMatcher::new(owner, apply_to, file_location, pattern);
            if let Some(source) = source
                && !source.is_empty()
            {
                matcher.source = Some(source);
            }
            if file_prefix.as_ref().is_some_and(|prefix| !matches!(prefix, FilePrefix::Path(path) if path.is_empty())) {
                matcher.file_prefix = file_prefix;
            }
            if let Some(severity) = severity
                && severity != Severity::Ignore
            {
                matcher.severity = Some(severity);
            }
            result = Some(matcher);
        }
        if config::is_named_problem_matcher(description) {
            // `(result as INamedProblemMatcher).name = ...` sobre `null` lança no original.
            let Some(named) = result.as_mut() else {
                errors::throw(Error::type_error("Cannot set properties of null (setting 'name')"));
            };
            named.name = description.name.clone();
            named.label = match &description.label {
                Some(label) => Some(label.clone()),
                None => description.name.clone(),
            };
        }
        result
    }

    fn create_problem_pattern(&self, value: &config::PatternValue) -> Option<OneOrMany<ProblemPattern>> {
        match value {
            config::PatternValue::Name(variable_name) => {
                if variable_name.chars().count() > 1 && variable_name.starts_with('$') {
                    let result = problem_pattern_registry().get(&variable_name[1..]);
                    if result.is_none() {
                        self.parser.error(&localize(
                            "ProblemMatcherParser.noDefinedPatter",
                            "Error: the pattern with the identifier {0} doesn't exist.",
                            &[variable_name],
                        ));
                    }
                    return result;
                }
                if variable_name.is_empty() {
                    self.parser.error(&localize("ProblemMatcherParser.noIdentifier", "Error: the pattern property refers to an empty identifier.", &[]));
                } else {
                    self.parser.error(&localize(
                        "ProblemMatcherParser.noValidIdentifier",
                        "Error: the pattern property {0} is not a valid pattern variable name.",
                        &[variable_name],
                    ));
                }
                None
            }
            config::PatternValue::Single(pattern) => {
                let problem_pattern_parser = ProblemPatternParser::new(self.parser.problem_reporter().clone());
                match problem_pattern_parser.parse(PatternInput::Single(pattern.clone())) {
                    Some(ParsedProblemPattern::Single(result)) => Some(OneOrMany::One(result)),
                    _ => None,
                }
            }
            config::PatternValue::Multi(patterns) => {
                let problem_pattern_parser = ProblemPatternParser::new(self.parser.problem_reporter().clone());
                match problem_pattern_parser.parse(PatternInput::Multi(patterns.clone())) {
                    Some(ParsedProblemPattern::Multi(result)) => Some(OneOrMany::Many(result)),
                    _ => None,
                }
            }
        }
    }

    fn add_watching_matcher(&self, external: &config::ProblemMatcher, internal: &mut ProblemMatcher) {
        let old_begins = self.create_regular_expression(external.watched_task_begins_reg_exp.as_deref());
        let old_ends = self.create_regular_expression(external.watched_task_ends_reg_exp.as_deref());
        if let (Some(old_begins), Some(old_ends)) = (old_begins, old_ends) {
            internal.watching = Some(WatchingMatcher {
                active_on_start: false,
                begins_pattern: WatchingPattern { regexp: old_begins, file: None },
                ends_pattern: WatchingPattern { regexp: old_ends, file: None },
            });
            return;
        }
        let Some(background_monitor) = external.background.as_ref().or(external.watching.as_ref()) else {
            return;
        };
        let begins = self.create_watching_pattern(background_monitor.begins_pattern.as_ref());
        let ends = self.create_watching_pattern(background_monitor.ends_pattern.as_ref());
        if let (Some(begins), Some(ends)) = (&begins, &ends) {
            internal.watching = Some(WatchingMatcher {
                active_on_start: background_monitor.active_on_start.unwrap_or(false),
                begins_pattern: begins.clone(),
                ends_pattern: ends.clone(),
            });
            return;
        }
        if begins.is_some() || ends.is_some() {
            self.parser.error(&localize(
                "ProblemMatcherParser.problemPattern.watchingMatcher",
                "A problem matcher must define both a begin pattern and an end pattern for watching.",
                &[],
            ));
        }
    }

    fn create_watching_pattern(&self, external: Option<&config::WatchingPatternValue>) -> Option<WatchingPattern> {
        let external = external?;
        let regexp: Option<Rc<JsRegExp>>;
        let mut file: Option<usize> = None;
        match external {
            config::WatchingPatternValue::Regexp(value) => {
                regexp = self.create_regular_expression(Some(value));
            }
            config::WatchingPatternValue::Pattern(pattern) => {
                regexp = self.create_regular_expression(pattern.regexp.as_deref());
                if pattern.file.is_some() {
                    file = pattern.file;
                }
            }
        }
        let regexp = regexp?;
        Some(match file {
            Some(file) if file != 0 => WatchingPattern { regexp, file: Some(file) },
            _ => WatchingPattern { regexp, file: Some(1) },
        })
    }

    fn create_regular_expression(&self, value: Option<&str>) -> Option<Rc<JsRegExp>> {
        let value = value.filter(|value| !value.is_empty())?;
        match JsRegExp::from_str(value, "") {
            Ok(regexp) => Some(Rc::new(regexp)),
            Err(_) => {
                self.parser.error(&localize("ProblemMatcherParser.invalidRegexp", "Error: The string {0} is not a valid regular expression.\n", &[value]));
                None
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// ProblemMatcherRegistry
// ---------------------------------------------------------------------------------------------

/// `IProblemMatcherRegistry`.
pub trait IProblemMatcherRegistry {
    fn on_ready(&self) -> Promise<()>;

    fn get(&self, name: &str) -> Option<ProblemMatcher>;

    fn keys(&self) -> Vec<String>;

    fn on_matcher_changed(&self) -> Event<()>;
}

/// `ProblemMatcherRegistryImpl`.
pub struct ProblemMatcherRegistryImpl {
    matchers: RefCell<IndexMap<String, ProblemMatcher>>,
    ready_promise: Promise<()>,
    ready_resolver: Resolver<()>,
    on_matchers_changed: Rc<Emitter<()>>,
}

impl Default for ProblemMatcherRegistryImpl {
    fn default() -> Self {
        Self::new()
    }
}

impl ProblemMatcherRegistryImpl {
    pub fn new() -> Self {
        let (ready_promise, ready_resolver) = Promise::new();
        let registry = Self { matchers: RefCell::new(IndexMap::new()), ready_promise, ready_resolver, on_matchers_changed: Emitter::new() };
        registry.fill_defaults();
        registry
    }

    /// O corpo do `problemMatchersExtPoint.setHandler`: `delta.removed`, `delta.added`, o evento de mudança, a marca
    /// `tscWatch` do `tsc-watch` e a resolução de `onReady`.
    pub fn handle_delta(&self, delta: &ExtensionPointDelta<Vec<config::ProblemMatcher>>) {
        let _ = errors::catch(|| {
            for extension in &delta.removed {
                for matcher in &extension.value {
                    self.matchers.borrow_mut().shift_remove(matcher.name.as_deref().unwrap_or("undefined"));
                }
            }
            for extension in &delta.added {
                let parser = ProblemMatcherParser::new(Rc::new(ExtensionRegistryReporter::new(extension.collector.clone())));
                for matcher in &extension.value {
                    let result = parser.parse(matcher);
                    if let Some(result) = result
                        && is_named_problem_matcher(Some(&result))
                    {
                        self.add(result);
                    }
                }
            }
            if !delta.removed.is_empty() || !delta.added.is_empty() {
                self.on_matchers_changed.fire(());
            }
        });
        if let Some(matcher) = self.matchers.borrow_mut().get_mut("tsc-watch") {
            matcher.tsc_watch = Some(true);
        }
        self.ready_resolver.resolve(());
    }

    pub fn add(&self, matcher: ProblemMatcher) {
        let name = matcher.name.clone().unwrap_or_else(|| "undefined".to_string());
        self.matchers.borrow_mut().insert(name, matcher);
    }

    fn fill_defaults(&self) {
        let patterns = problem_pattern_registry();
        let pattern = |name: &str| patterns.get(name).expect("padrão predefinido");
        let named = |name: &str, label: String, owner: &str, source: &str, apply_to: ApplyToKind, file_location: FileLocationKind, pattern: OneOrMany<ProblemPattern>| {
            let mut matcher = ProblemMatcher::new(owner, apply_to, file_location, pattern);
            matcher.name = Some(name.to_string());
            matcher.label = Some(label);
            matcher.source = Some(source.to_string());
            matcher
        };

        self.add(named(
            "msCompile",
            localize("msCompile", "Microsoft compiler problems", &[]),
            "msCompile",
            "cpp",
            ApplyToKind::AllDocuments,
            FileLocationKind::Absolute,
            pattern("msCompile"),
        ));

        let mut less_compile = named(
            "lessCompile",
            localize("lessCompile", "Less problems", &[]),
            "lessCompile",
            "less",
            ApplyToKind::AllDocuments,
            FileLocationKind::Absolute,
            pattern("lessCompile"),
        );
        less_compile.deprecated = Some(true);
        less_compile.severity = Some(Severity::Error);
        self.add(less_compile);

        let mut gulp_tsc = named(
            "gulp-tsc",
            localize("gulp-tsc", "Gulp TSC Problems", &[]),
            "typescript",
            "ts",
            ApplyToKind::ClosedDocuments,
            FileLocationKind::Relative,
            pattern("gulp-tsc"),
        );
        gulp_tsc.file_prefix = Some(FilePrefix::Path("${workspaceFolder}".to_string()));
        self.add(gulp_tsc);

        self.add(named(
            "jshint",
            localize("jshint", "JSHint problems", &[]),
            "jshint",
            "jshint",
            ApplyToKind::AllDocuments,
            FileLocationKind::Absolute,
            pattern("jshint"),
        ));

        self.add(named(
            "jshint-stylish",
            localize("jshint-stylish", "JSHint stylish problems", &[]),
            "jshint",
            "jshint",
            ApplyToKind::AllDocuments,
            FileLocationKind::Absolute,
            pattern("jshint-stylish"),
        ));

        let mut eslint_compact = named(
            "eslint-compact",
            localize("eslint-compact", "ESLint compact problems", &[]),
            "eslint",
            "eslint",
            ApplyToKind::AllDocuments,
            FileLocationKind::Absolute,
            pattern("eslint-compact"),
        );
        eslint_compact.file_prefix = Some(FilePrefix::Path("${workspaceFolder}".to_string()));
        self.add(eslint_compact);

        self.add(named(
            "eslint-stylish",
            localize("eslint-stylish", "ESLint stylish problems", &[]),
            "eslint",
            "eslint",
            ApplyToKind::AllDocuments,
            FileLocationKind::Absolute,
            pattern("eslint-stylish"),
        ));

        let mut go = named("go", localize("go", "Go problems", &[]), "go", "go", ApplyToKind::AllDocuments, FileLocationKind::Relative, pattern("go"));
        go.file_prefix = Some(FilePrefix::Path("${workspaceFolder}".to_string()));
        self.add(go);
    }
}

impl IProblemMatcherRegistry for ProblemMatcherRegistryImpl {
    fn on_ready(&self) -> Promise<()> {
        let _ = problem_pattern_registry().on_ready();
        self.ready_promise.clone()
    }

    fn get(&self, name: &str) -> Option<ProblemMatcher> {
        self.matchers.borrow().get(name).cloned()
    }

    fn keys(&self) -> Vec<String> {
        self.matchers.borrow().keys().cloned().collect()
    }

    fn on_matcher_changed(&self) -> Event<()> {
        self.on_matchers_changed.event()
    }
}

thread_local! {
    static PROBLEM_MATCHER_REGISTRY: Rc<ProblemMatcherRegistryImpl> = Rc::new(ProblemMatcherRegistryImpl::new());
}

/// `ProblemMatcherRegistry`.
pub fn problem_matcher_registry() -> Rc<ProblemMatcherRegistryImpl> {
    PROBLEM_MATCHER_REGISTRY.with(Rc::clone)
}
