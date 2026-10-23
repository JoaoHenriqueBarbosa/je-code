//! Porte de `src/vs/platform/telemetry/common/telemetryUtils.ts`.
//!
//! Divergências:
//! - `getTelemetryLevel` tem duas formas: [`get_telemetry_level_of`] (o `IConfigurationService`) e
//!   [`get_telemetry_level`], sobre uma leitura de configuração qualquer (`&dyn Fn(&str) -> Dynamic`).
//! - `supportsTelemetry` e `isLoggingOnly` recebem os traits [`IProductService`] e [`EnvironmentService`].
//! - As regex de `cleanData` são [`zyarr::RegExp`] (a mesma semântica ECMAScript do V8); o `lastIndex` de
//!   uma regex global é tratado como no laço `while (exec)` do original.
//! - `verifyMicrosoftInternalDomain` (de `commonProperties.ts`) não é portado: serve só à telemetria interna
//!   da Microsoft. O `TelemetryLogAppender` está em `telemetry_log_appender.rs`.

use std::rc::Rc;

use je_base::JsString;
use je_base::dynamic::Dynamic;
use je_base::event::Event;
use je_base::nls::localize;
use je_base::objects;
use zyarr::RegExp;

use super::telemetry::{
    CommonPropertyValue, ICustomEndpointTelemetryService, ITelemetryData, ITelemetryService, TelemetryConfiguration, TelemetryEndpoint, TelemetryLevel,
    TELEMETRY_CRASH_REPORTER_SETTING_ID, TELEMETRY_OLD_SETTING_ID, TELEMETRY_SETTING_ID,
};
use crate::configuration::service::IConfigurationService;
use crate::environment::environment::EnvironmentService;
use crate::js::to_js_string;
use crate::product::IProductService;

/// `TelemetryTrustedValue`: um valor que não deve ser limpo, porque é confiável (sem caminhos nem dados pessoais).
/// Na forma serializada (extension host) vira um objeto com `isTrustedTelemetryValue`.
#[derive(Clone, Debug)]
pub struct TelemetryTrustedValue {
    pub value: Dynamic,
}

impl TelemetryTrustedValue {
    /// A forma de objeto que o `clean_data` reconhece (`{ isTrustedTelemetryValue: true, value }`).
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([("isTrustedTelemetryValue", Dynamic::Bool(true)), ("value", self.value.clone())])
    }
}

/// `NullTelemetryServiceShape`.
#[derive(Default)]
pub struct NullTelemetryServiceShape;

impl ITelemetryService for NullTelemetryServiceShape {
    fn telemetry_level(&self) -> TelemetryLevel {
        TelemetryLevel::None
    }
    fn session_id(&self) -> String {
        "someValue.sessionId".to_string()
    }
    fn machine_id(&self) -> String {
        "someValue.machineId".to_string()
    }
    fn sqm_id(&self) -> String {
        "someValue.sqmId".to_string()
    }
    fn dev_device_id(&self) -> String {
        "someValue.devDeviceId".to_string()
    }
    fn first_session_date(&self) -> String {
        "someValue.firstSessionDate".to_string()
    }
    fn send_error_telemetry(&self) -> bool {
        false
    }
    fn public_log(&self, _event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log2(&self, _event_name: &str, _data: Option<Dynamic>) {}
    fn public_log_error(&self, _error_event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log_error2(&self, _event_name: &str, _data: Option<Dynamic>) {}
    fn set_experiment_property(&self, _name: &str, _value: &str) {}
    fn set_common_property(&self, _name: &str, _value: Option<CommonPropertyValue>) {}
}

/// `NullTelemetryService`.
pub fn null_telemetry_service() -> Rc<dyn ITelemetryService> {
    Rc::new(NullTelemetryServiceShape)
}

/// `NullEndpointTelemetryService`.
#[derive(Default)]
pub struct NullEndpointTelemetryService;

impl ICustomEndpointTelemetryService for NullEndpointTelemetryService {
    fn public_log(&self, _endpoint: &TelemetryEndpoint, _event_name: &str, _data: Option<ITelemetryData>) {}
    fn public_log_error(&self, _endpoint: &TelemetryEndpoint, _error_event_name: &str, _data: Option<ITelemetryData>) {}
}

pub const TELEMETRY_LOG_ID: &str = "telemetry";

/// `TelemetryLogGroup`: `{ id, name }`.
pub fn telemetry_log_group() -> (String, String) {
    (TELEMETRY_LOG_ID.to_string(), localize("telemetryLogName", "Telemetry", &[]))
}

/// `ITelemetryAppender`.
pub trait ITelemetryAppender {
    fn log(&self, event_name: &str, data: &Dynamic);
    fn flush(&self) -> je_base::promise::Promise<()>;
}

/// `NullAppender`.
#[derive(Default)]
pub struct NullAppender;

impl ITelemetryAppender for NullAppender {
    fn log(&self, _event_name: &str, _data: &Dynamic) {}
    fn flush(&self) -> je_base::promise::Promise<()> {
        je_base::promise::Promise::resolved(())
    }
}

/// `URIDescriptor`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct UriDescriptor {
    pub mime_type: Option<String>,
    pub scheme: Option<String>,
    pub ext: Option<String>,
    pub path: Option<String>,
}

/// Se há suporte a registrar telemetria: o produto pode coletar, mas isso não diz se pode enviar.
pub fn supports_telemetry(product_service: &dyn IProductService, environment_service: &dyn EnvironmentService) -> bool {
    // If it's OSS and telemetry isn't disabled via the CLI we will allow it for logging only purposes
    if !environment_service.is_built() && !environment_service.disable_telemetry() {
        return true;
    }
    !(environment_service.disable_telemetry() || !product_service.enable_telemetry())
}

/// Se estamos no modo só de registro, para depurar a telemetria: nada é enviado, só registrado.
pub fn is_logging_only(product_service: &dyn IProductService, environment_service: &dyn EnvironmentService) -> bool {
    // If we're testing an extension, log telemetry for debug purposes
    if environment_service.extension_tests_location_uri().is_some() {
        return true;
    }
    // Logging only mode is only for OSS
    if environment_service.is_built() {
        return false;
    }
    if environment_service.disable_telemetry() {
        return false;
    }
    if product_service.enable_telemetry() && product_service.ai_config_aria_key().is_some_and(|key| !key.is_empty()) {
        return false;
    }
    true
}

/// `getTelemetryLevel(configurationService)`: como a telemetria é tratada segundo a configuração do usuário.
pub fn get_telemetry_level_of(configuration_service: &dyn IConfigurationService) -> TelemetryLevel {
    get_telemetry_level(&|key| configuration_service.get_value(Some(key), None))
}

/// `getTelemetryLevel`, sobre uma leitura de configuração qualquer (`getValue(key)`).
pub fn get_telemetry_level(get_value: &dyn Fn(&str) -> Dynamic) -> TelemetryLevel {
    let new_config = get_value(TELEMETRY_SETTING_ID);
    let crash_reporter_config = get_value(TELEMETRY_CRASH_REPORTER_SETTING_ID);
    let old_config = get_value(TELEMETRY_OLD_SETTING_ID);

    // If `telemetry.enableCrashReporter` is false or `telemetry.enableTelemetry' is false, disable telemetry
    if old_config == Dynamic::Bool(false) || crash_reporter_config == Dynamic::Bool(false) {
        return TelemetryLevel::None;
    }

    // Maps new telemetry setting to a telemetry level
    let setting = if new_config.is_undefined() { Dynamic::string(TelemetryConfiguration::On.as_str()) } else { new_config };
    match &setting {
        Dynamic::String(value) if *value == TelemetryConfiguration::On.as_str() => TelemetryLevel::Usage,
        Dynamic::String(value) if *value == TelemetryConfiguration::Error.as_str() => TelemetryLevel::Error,
        Dynamic::String(value) if *value == TelemetryConfiguration::Crash.as_str() => TelemetryLevel::Crash,
        _ => TelemetryLevel::None,
    }
}

/// `Properties` e `Measurements` de `validateTelemetryData`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ValidatedTelemetryData {
    pub properties: Vec<(String, String)>,
    pub measurements: Vec<(String, f64)>,
}

impl ValidatedTelemetryData {
    /// O objeto `{ properties, measurements }` que `validateTelemetryData` devolve.
    pub fn to_dynamic(&self) -> Dynamic {
        Dynamic::object([
            ("properties", Dynamic::object(self.properties.iter().map(|(k, v)| (k.as_str(), Dynamic::string(v))))),
            ("measurements", Dynamic::object(self.measurements.iter().map(|(k, v)| (k.as_str(), Dynamic::Number(*v))))),
        ])
    }
}

/// `validateTelemetryData`.
pub fn validate_telemetry_data(data: &Dynamic) -> ValidatedTelemetryData {
    let mut result = ValidatedTelemetryData::default();
    let mut flat: Vec<(JsString, Dynamic)> = Vec::new();
    flatten(data, &mut flat, 0, None);
    for (prop, _) in flat.clone() {
        // enforce property names less than 150 char, take the last 150 char
        let units = prop.units();
        let prop_name = if units.len() > 150 { JsString::from_units(units[units.len() - 149..].to_vec()) } else { prop.clone() };
        // O original relê o valor pelo nome já encurtado (`flat[prop]`).
        let value = flat.iter().rev().find(|(k, _)| *k == prop_name).map_or(Dynamic::Undefined, |(_, v)| v.clone());
        let name = prop_name.to_string_lossy();
        match &value {
            Dynamic::Number(n) => set_measurement(&mut result, name, *n),
            Dynamic::Bool(b) => set_measurement(&mut result, name, if *b { 1.0 } else { 0.0 }),
            Dynamic::String(s) => {
                // enforce property value to be less than 8192 char, take the first 8191
                let text = JsString::from_units(s.units()[..s.len().min(8191)].to_vec()).to_string_lossy();
                set_property(&mut result, name, text);
            }
            Dynamic::Undefined | Dynamic::Null => {}
            other => set_property(&mut result, name, to_js_string(other).to_string_lossy()),
        }
    }
    result
}

fn set_measurement(result: &mut ValidatedTelemetryData, name: String, value: f64) {
    match result.measurements.iter_mut().find(|(k, _)| *k == name) {
        Some(entry) => entry.1 = value,
        None => result.measurements.push((name, value)),
    }
}

fn set_property(result: &mut ValidatedTelemetryData, name: String, value: String) {
    match result.properties.iter_mut().find(|(k, _)| *k == name) {
        Some(entry) => entry.1 = value,
        None => result.properties.push((name, value)),
    }
}

fn flatten(obj: &Dynamic, result: &mut Vec<(JsString, Dynamic)>, order: usize, prefix: Option<&str>) {
    if !matches!(obj, Dynamic::Object(_) | Dynamic::Array(_) | Dynamic::Function(_) | Dynamic::TypedArray(_) | Dynamic::Date(_) | Dynamic::RegExp(_)) {
        return;
    }
    for (item, value) in obj.own_entries() {
        let index = match prefix {
            Some(prefix) if !prefix.is_empty() => format!("{prefix}{}", item.to_string_lossy()),
            _ => item.to_string_lossy(),
        };
        let key = JsString::from(index.clone());
        let mut put = |value: Dynamic| match result.iter_mut().find(|(k, _)| *k == key) {
            Some(entry) => entry.1 = value,
            None => result.push((key.clone(), value)),
        };
        match &value {
            Dynamic::Array(_) => put(safe_stringify(&value)),
            Dynamic::Date(date) => put(match je_base::dynamic::date_to_iso_string(date.time_value()) {
                Some(iso) => Dynamic::string(&iso),
                None => Dynamic::Undefined,
            }),
            Dynamic::Object(_) => {
                if order < 2 {
                    flatten(&value, result, order + 1, Some(&format!("{index}.")));
                } else {
                    put(safe_stringify(&value));
                }
            }
            other => put(other.clone()),
        }
    }
}

/// `safeStringify`: o JSON do valor (os ciclos viram `[Circular]` no original; aqui os dados vêm sem ciclo).
fn safe_stringify(value: &Dynamic) -> Dynamic {
    match crate::js::to_json(value) {
        Some(json) => Dynamic::String(je_base::marshalling::stringify(&je_base::marshalling::from_json(&json))),
        None => Dynamic::Undefined,
    }
}

/// `IRemoteAuthoringConfig`: os nomes de remotos conhecidos.
#[derive(Clone, Debug, Default)]
pub struct RemoteAuthoringConfig {
    pub remote_extension_tips: Option<Vec<String>>,
    pub virtual_workspace_extension_tips: Option<Vec<String>>,
}

/// `cleanRemoteAuthority`.
pub fn clean_remote_authority(remote_authority: Option<&str>, config: Option<&RemoteAuthoringConfig>) -> String {
    let Some(remote_authority) = remote_authority.filter(|a| !a.is_empty()) else {
        return "none".to_string();
    };
    // `getRemoteName`: o que vem antes do primeiro `+`
    let remote_name = match remote_authority.find('+') {
        Some(pos) => &remote_authority[..pos],
        None => remote_authority,
    };
    if let Some(config) = config {
        if config.remote_extension_tips.as_ref().is_some_and(|set| set.iter().any(|n| n == remote_name)) {
            return remote_name.to_string();
        }
        if config.virtual_workspace_extension_tips.as_ref().is_some_and(|set| set.iter().any(|n| n == remote_name)) {
            return remote_name.to_string();
        }
    }
    "other".to_string()
}

// #region Telemetry Cleaning

fn units(text: &str) -> Vec<u16> {
    text.encode_utf16().collect()
}

fn re(pattern: &str, flags: &str) -> RegExp {
    RegExp::from_str(pattern, flags).expect("expressão fixa de limpeza de telemetria")
}

/// Os intervalos `[início, fim)` de todos os casamentos de cada padrão (o laço `while (exec)`: com a flag
/// `g` o `exec` avança; sem ela o original repetiria o mesmo casamento para sempre, então só vale com `g`).
fn get_match_ranges(value: &[u16], patterns: &[RegExp]) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    for regexp in patterns {
        let mut last_index = 0;
        while let Some(found) = regexp.exec(value, last_index) {
            ranges.push((found.start(), found.end()));
            last_index = if found.end() == found.start() { found.end() + 1 } else { found.end() };
            if !regexp.flags().global || last_index > value.len() {
                break;
            }
        }
    }
    ranges
}

fn utf16_to_string(units: &[u16]) -> String {
    String::from_utf16_lossy(units)
}

/// Anonimiza os caminhos de arquivo de uma pilha ou texto.
fn anonymize_file_paths(stack: &str, relative_path_root_patterns: &[RegExp], user_content_root_patterns: &[RegExp]) -> String {
    // Fast check to see if it is a file path to avoid doing unnecessary heavy regex work
    if stack.is_empty() || (!stack.contains('/') && !stack.contains('\\')) {
        return stack.to_string();
    }

    let stack_units = units(stack);
    let clean_up_indexes = get_match_ranges(&stack_units, relative_path_root_patterns);
    let user_content_indexes = get_match_ranges(&stack_units, user_content_root_patterns);

    // Match node_modules or node_modules.asar at any position in the path, capturing the node_modules/... suffix
    let node_modules_regex = re(r"(?:^|[\\\/])((node_modules|node_modules\.asar)[\\\/].*)$", "");
    // Match VS Code extension paths: user (.vscode*/extensions/) and built-in (resources/app/extensions/)
    let vscode_extensions_path_regex = re(r"^(.*?)((?:\.vscode(?:-[a-z]+)*|resources[\\\/]app)[\\\/]extensions[\\\/].*)$", "i");
    let file_regex = re(r"(file:\/\/)?([a-zA-Z]:(\\\\|\\|\/)|(\\\\|\\|\/))?([\w\-\._@]+(\\\\|\\|\/))+[\w\-\._@]*", "g");

    let mut last_index = 0;
    let mut updated = String::new();
    let mut search_from = 0;

    while let Some(result) = file_regex.exec(&stack_units, search_from) {
        let (start, end) = (result.start(), result.end());
        search_from = if end == start { end + 1 } else { end };

        // Check to see if the any cleanupIndexes partially overlap with this match
        let overlapping_range = clean_up_indexes.iter().any(|&(s, e)| start < e && s < end);

        // anonymize user file paths that do not need to be retained or cleaned up.
        if !overlapping_range {
            let is_user_content = user_content_indexes.iter().any(|&(s, e)| start < e && s < end);
            let matched = &stack_units[start..end];
            // Check if this is a VS Code extension path: preserve the .vscode*/extensions/... portion
            let vscode_ext_match = if is_user_content { None } else { vscode_extensions_path_regex.exec(matched, 0) };
            updated.push_str(&utf16_to_string(&stack_units[last_index.min(start)..start]));
            if let Some(m) = vscode_ext_match {
                let group = m.captures.get(2).and_then(|c| c.clone()).map_or(&[][..], |r| &matched[r]);
                updated.push_str("<REDACTED: user-file-path>/");
                updated.push_str(&utf16_to_string(group));
            } else {
                // Check if node_modules appears in the path: preserve node_modules/... suffix
                let node_modules_match = if is_user_content { None } else { node_modules_regex.exec(matched, 0) };
                match node_modules_match {
                    Some(m) => {
                        let group = m.captures.get(1).and_then(|c| c.clone()).map_or(&[][..], |r| &matched[r]);
                        updated.push_str("<REDACTED: user-file-path>/");
                        updated.push_str(&utf16_to_string(group));
                    }
                    None => updated.push_str("<REDACTED: user-file-path>"),
                }
            }
            last_index = end;
        }
    }
    if last_index < stack_units.len() {
        updated.push_str(&utf16_to_string(&stack_units[last_index..]));
    }
    updated
}

/// `userDataRegexes`: rótulo e padrão.
fn user_data_regexes() -> Vec<(&'static str, RegExp)> {
    vec![
        ("URL", re(r"[a-zA-Z][a-zA-Z0-9+.-]*:\/\/[^\s]*", "")),
        // Also matches truncated or mistyped keys, which are still sensitive.
        ("Google API Key", re(r"AIza[0-9A-Za-z_\-]{20,}", "")),
        ("JWT", re(r"eyJ[0eXAiOiJKV1Qi|hbGci|a-zA-Z0-9\-_]+\.[a-zA-Z0-9\-_]+\.[a-zA-Z0-9\-_]+", "")),
        ("Slack Token", re(r"xox[pbar]\-[A-Za-z0-9]", "")),
        ("GitHub Token", re(r"(gh[psuro]_[a-zA-Z0-9]{36}|github_pat_[a-zA-Z0-9]{22}_[a-zA-Z0-9]{59})", "")),
        ("Generic Secret", re(r"(key|token|sig|secret|signature|password|passwd|pwd|android:value)[^a-zA-Z0-9]", "i")),
        (
            "CLI Credentials",
            re(
                r#"((login|psexec|(certutil|psexec)\.exe).{1,50}(\s-u(ser(name)?)?\s+.{3,100})?\s-(admin|user|vm|root)?p(ass(word)?)?\s+["']?[^$\-\/\s]|(^|[\s\r\n\\])net(\.exe)?.{1,5}(user\s+|share\s+\/user:| user -? secrets ? set) \s + [^ $\s \/])"#,
                "",
            ),
        ),
        // Short password flags such as `plink -pw`, `openssl -pass`, `cmdkey /pass:`, and `sshpass -p`.
        // MSBuild's `/p:` property flag is intentionally not matched.
        ("CLI Password Flag", re(r#"(?:^|[\s"'`])(?:--?(?:pw|pass)(?:\s+|=)|\/pass:|sshpass\s+-p\s*)["']?[^\s"'$\-]"#, "i")),
        // `user:password` credentials passed with `-u` or `--user`, such as curl. Short options may be attached.
        // Numeric `uid:gid` pairs are not matched.
        ("CLI Basic Auth", re(r#"(?:^|\s)(?:-u(?:\s+|=)?|--user(?:\s+|=))["']?(?!\d+:\d+\b)[^\s:"']+:[^\s"']+"#, "")),
        ("Microsoft Entra ID", re(r"eyJ(?:0eXAiOiJKV1Qi|hbGci|[a-zA-Z0-9\-_]+\.[a-zA-Z0-9\-_]+\.)", "")),
        ("Email", re(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}", "")),
    ]
}

/// Redige um valor se ele tiver dados pessoais comuns. `probe` é o texto testado contra as heurísticas.
fn redact_if_possible_user_info(value: &str, probe: &str, regexes: &[(&'static str, RegExp)]) -> String {
    let probe_units = units(probe);
    for (label, regex) in regexes {
        if regex.exec(&probe_units, 0).is_some() {
            return format!("<REDACTED: {label}>");
        }
    }
    value.to_string()
}

/// Tenta remover dados pessoais comuns; com várias linhas, redige só as linhas ofensivas.
fn remove_properties_with_possible_user_info(property: &str) -> String {
    if property.is_empty() {
        return property.to_string();
    }
    let regexes = user_data_regexes();
    let property_units = units(property);
    // Fast path: se nada casa, o valor volta intacto.
    if !regexes.iter().any(|(_, regex)| regex.exec(&property_units, 0).is_some()) {
        return property.to_string();
    }
    // Valor de uma linha: redige o valor inteiro.
    if !property.contains('\n') {
        return redact_if_possible_user_info(property, property, &regexes);
    }
    // Várias linhas (pilhas de chamada): redige linha a linha, e o `\n` retirado pelo `split` volta ao teste
    // (menos na última linha), para as heurísticas de borda se comportarem como no valor inteiro.
    let lines: Vec<&str> = property.split('\n').collect();
    let last = lines.len() - 1;
    lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            let probe = if i < last { format!("{line}\n") } else { (*line).to_string() };
            redact_if_possible_user_info(line, &probe, &regexes)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// `String.prototype.replace(regexp, '')`: o primeiro casamento, ou todos com a flag `g`.
fn replace_with_empty(text: &str, regexp: &RegExp) -> String {
    let text_units = units(text);
    let mut out: Vec<u16> = Vec::new();
    let mut copied = 0;
    let mut last_index = 0;
    while let Some(found) = regexp.exec(&text_units, last_index) {
        out.extend_from_slice(&text_units[copied..found.start()]);
        copied = found.end();
        last_index = if found.end() == found.start() { found.end() + 1 } else { found.end() };
        if !regexp.flags().global || last_index > text_units.len() {
            break;
        }
    }
    out.extend_from_slice(&text_units[copied..]);
    utf16_to_string(&out)
}

/// `cleanData`: o melhor esforço para limpar um objeto de dados pessoais.
///
/// `relative_path_root_patterns` (padrão `clean_up_patterns`): raízes cujos caminhos descendentes ficam
/// relativos à raiz. `user_content_root_patterns`: raízes cujos descendentes são sempre redigidos por inteiro.
pub fn clean_data(
    data: Option<&Dynamic>,
    clean_up_patterns: &[RegExp],
    relative_path_root_patterns: Option<&[RegExp]>,
    user_content_root_patterns: &[RegExp],
) -> Dynamic {
    let Some(data) = data.filter(|d| !d.is_nullish()) else {
        return Dynamic::object(Vec::<(&str, Dynamic)>::new());
    };
    let relative_path_root_patterns = relative_path_root_patterns.unwrap_or(clean_up_patterns);
    objects::clone_and_change(data, &|value| {
        // If it's a trusted value it means it's okay to skip cleaning so we don't clean it
        if matches!(value, Dynamic::Object(_)) && value.has_property(&JsString::from("isTrustedTelemetryValue")) {
            return value.get_str("value");
        }

        // We only know how to clean strings
        if let Dynamic::String(text) = value {
            let mut updated = text.to_string_lossy().replace("%20", " ");

            if updated.contains("OneDrive - ") {
                // /OneDrive - (?:[^\\\/\r\n"<>|:*?]+(?=[\\\/])|[^\\\/\r\n"'`]+)/g
                let one_drive = re(r#"OneDrive - (?:[^\\\/\r\n"<>|:*?]+(?=[\\\/])|[^\\\/\r\n"'`]+)"#, "g");
                let updated_units = units(&updated);
                let mut out: Vec<u16> = Vec::new();
                let mut copied = 0;
                let mut last_index = 0;
                while let Some(found) = one_drive.exec(&updated_units, last_index) {
                    out.extend_from_slice(&updated_units[copied..found.start()]);
                    out.extend(units("OneDrive"));
                    copied = found.end();
                    last_index = if found.end() == found.start() { found.end() + 1 } else { found.end() };
                    if last_index > updated_units.len() {
                        break;
                    }
                }
                out.extend_from_slice(&updated_units[copied..]);
                updated = utf16_to_string(&out);
            }

            // First we anonymize any possible file paths
            updated = anonymize_file_paths(&updated, relative_path_root_patterns, user_content_root_patterns);

            // Then we do a simple regex replace with the defined patterns
            for regexp in clean_up_patterns {
                updated = replace_with_empty(&updated, regexp);
            }

            // Lastly, remove commonly leaked PII
            updated = remove_properties_with_possible_user_info(&updated);

            return Dynamic::string(&updated);
        }
        Dynamic::Undefined
    })
}

// #endregion

/// Um evento vazio, para os serviços de teste.
pub fn no_event<T: 'static>() -> Event<T> {
    Event::none()
}
