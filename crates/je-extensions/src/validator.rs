//! Porte de `src/vs/platform/extensions/common/extensionValidator.ts`: a validação do manifesto e a compatibilidade
//! da versão de `engines.vscode` com a do produto, pelo esquema de versão do VS Code (não é o semver completo:
//! aceita `x`, `^`, `>=` e a data de "não antes de" no pré-lançamento).
//!
//! O manifesto é validado sobre o JSON cru (`serde_json::Value`): a validação existe justamente para acusar campo
//! com o tipo errado, que um tipo estático já teria descartado.

use je_base::severity::Severity;
use serde_json::Value;

use crate::{paths, semver};

/// `IParsedVersion`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParsedVersion {
    pub has_caret: bool,
    pub has_greater_equals: bool,
    pub major_base: u64,
    pub major_must_equal: bool,
    pub minor_base: u64,
    pub minor_must_equal: bool,
    pub patch_base: u64,
    pub patch_must_equal: bool,
    pub pre_release: Option<String>,
}

/// `INormalizedVersion`.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedVersion {
    pub major_base: u64,
    pub major_must_equal: bool,
    pub minor_base: u64,
    pub minor_must_equal: bool,
    pub patch_base: u64,
    pub patch_must_equal: bool,
    /// Milissegundos desde a época, ou 0.
    pub not_before: f64,
    pub is_minimum: bool,
}

/// Um dos três componentes do regex: um número ou `x`.
fn parse_part(part: &str) -> Option<(u64, bool)> {
    if part == "x" {
        return Some((0, false));
    }
    if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Some((part.parse::<u64>().unwrap_or(u64::MAX), true))
}

/// `^(\^|>=)?((\d+)|x)\.((\d+)|x)\.((\d+)|x)(\-.*)?$` aplicado a um texto já sem espaços nas pontas.
fn match_version(version: &str) -> Option<ParsedVersion> {
    let (has_caret, has_greater_equals, rest) = if let Some(rest) = version.strip_prefix('^') {
        (true, false, rest)
    } else if let Some(rest) = version.strip_prefix(">=") {
        (false, true, rest)
    } else {
        (false, false, version)
    };
    // `.*` do final não casa quebra de linha.
    if rest.contains(['\n', '\r', '\u{2028}', '\u{2029}']) {
        return None;
    }
    let (numbers, pre_release) = match rest.find('-') {
        // O traço só abre o pré-lançamento depois dos três componentes; um `-` antes disso quebra o casamento.
        Some(index) => (&rest[..index], Some(&rest[index..])),
        None => (rest, None),
    };
    let mut parts = numbers.split('.');
    let (major_base, major_must_equal) = parse_part(parts.next()?)?;
    let (minor_base, minor_must_equal) = parse_part(parts.next()?)?;
    let (patch_base, patch_must_equal) = parse_part(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some(ParsedVersion {
        has_caret,
        has_greater_equals,
        major_base,
        major_must_equal,
        minor_base,
        minor_must_equal,
        patch_base,
        patch_must_equal,
        pre_release: pre_release.filter(|p| !p.is_empty()).map(str::to_owned),
    })
}

/// `isValidVersionStr`.
pub fn is_valid_version_str(version: &str) -> bool {
    let version = version.trim();
    version == "*" || match_version(version).is_some()
}

/// `parseVersion`.
pub fn parse_version(version: &str) -> Option<ParsedVersion> {
    if !is_valid_version_str(version) {
        return None;
    }
    let version = version.trim();
    if version == "*" {
        return Some(ParsedVersion {
            has_caret: false,
            has_greater_equals: false,
            major_base: 0,
            major_must_equal: false,
            minor_base: 0,
            minor_must_equal: false,
            patch_base: 0,
            patch_must_equal: false,
            pre_release: None,
        });
    }
    match_version(version)
}

/// Dias desde 1970-01-01 de uma data civil (calendário gregoriano proléptico).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_index = (month + 9) % 12;
    let day_of_year = (153 * month_index + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `Date.UTC(year, month - 1, day, hours, minutes)` em milissegundos.
fn date_utc(year: i64, month: i64, day: i64, hours: i64, minutes: i64) -> f64 {
    ((days_from_civil(year, month, day) * 24 + hours) * 60 + minutes) as f64 * 60_000.0
}

/// `^-(\d{4})(\d{2})(\d{2})(\d{2})?(\d{2})?$`.
fn not_before(pre_release: &str) -> f64 {
    let Some(digits) = pre_release.strip_prefix('-') else { return 0.0 };
    if !digits.bytes().all(|b| b.is_ascii_digit()) || !matches!(digits.len(), 8 | 10 | 12) {
        return 0.0;
    }
    let field = |range: std::ops::Range<usize>| digits.get(range).and_then(|s| s.parse::<i64>().ok()).unwrap_or(0);
    date_utc(field(0..4), field(4..6), field(6..8), field(8..10), field(10..12))
}

/// `normalizeVersion`.
pub fn normalize_version(version: Option<&ParsedVersion>) -> Option<NormalizedVersion> {
    let version = version?;
    let mut minor_must_equal = version.minor_must_equal;
    let mut patch_must_equal = version.patch_must_equal;
    if version.has_caret {
        if version.major_base == 0 {
            patch_must_equal = false;
        } else {
            minor_must_equal = false;
            patch_must_equal = false;
        }
    }
    let not_before = version.pre_release.as_deref().map_or(0.0, not_before);
    Some(NormalizedVersion {
        major_base: version.major_base,
        major_must_equal: version.major_must_equal,
        minor_base: version.minor_base,
        minor_must_equal,
        patch_base: version.patch_base,
        patch_must_equal,
        is_minimum: version.has_greater_equals,
        not_before,
    })
}

/// `new Date(text).getTime()` para o que o produto escreve: `YYYY-MM-DD` ou data e hora ISO (`Z` ou deslocamento).
/// Texto que não é data dá `None` (o `NaN` do JavaScript, que o original trata como "sem data").
pub fn parse_date_ms(text: &str) -> Option<f64> {
    let text = text.trim();
    let (date, time) = match text.split_once(['T', ' ']) {
        Some((date, time)) => (date, Some(time)),
        None => (text, None),
    };
    let mut parts = date.split('-');
    let year: i64 = parts.next()?.parse().ok()?;
    let month: i64 = parts.next().map_or(Some(1), |m| m.parse().ok())?;
    let day: i64 = parts.next().map_or(Some(1), |d| d.parse().ok())?;
    if parts.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    let mut millis = date_utc(year, month, day, 0, 0);
    if let Some(time) = time {
        let (clock, offset_minutes) = if let Some(clock) = time.strip_suffix('Z') {
            (clock, 0)
        } else if let Some(index) = time.rfind(['+', '-']).filter(|i| *i > 0) {
            let (clock, offset) = time.split_at(index);
            let sign = if offset.starts_with('-') { -1 } else { 1 };
            let digits: String = offset[1..].chars().filter(char::is_ascii_digit).collect();
            let hours: i64 = digits.get(0..2)?.parse().ok()?;
            let minutes: i64 = digits.get(2..4).map_or(Some(0), |m| m.parse().ok())?;
            (clock, sign * (hours * 60 + minutes))
        } else {
            (time, 0)
        };
        let mut fields = clock.split(':');
        let hours: f64 = fields.next()?.parse().ok()?;
        let minutes: f64 = fields.next().map_or(Some(0.0), |m| m.parse().ok())?;
        let seconds: f64 = fields.next().map_or(Some(0.0), |s| s.parse().ok())?;
        millis += hours * 3_600_000.0 + minutes * 60_000.0 + seconds * 1000.0 - offset_minutes as f64 * 60_000.0;
    }
    Some(millis)
}

/// O que o original aceita como `ProductDate`: `string | Date | undefined`.
pub type ProductDate<'a> = Option<&'a str>;

fn product_timestamp(date: ProductDate) -> Option<f64> {
    date.and_then(parse_date_ms).filter(|t| *t != 0.0)
}

/// A versão desejada como texto (`string`) ou já normalizada (`INormalizedVersion`).
pub enum VersionInput<'a> {
    Text(&'a str),
    Normalized(&'a NormalizedVersion),
}

impl<'a> From<&'a str> for VersionInput<'a> {
    fn from(text: &'a str) -> Self {
        Self::Text(text)
    }
}

impl<'a> From<&'a NormalizedVersion> for VersionInput<'a> {
    fn from(version: &'a NormalizedVersion) -> Self {
        Self::Normalized(version)
    }
}

impl VersionInput<'_> {
    fn resolve(&self) -> Option<NormalizedVersion> {
        match self {
            Self::Text(text) => normalize_version(parse_version(text).as_ref()),
            Self::Normalized(version) => Some((*version).clone()),
        }
    }
}

/// `isValidVersion`.
pub fn is_valid_version<'a, 'b>(input_version: impl Into<VersionInput<'a>>, input_date: ProductDate, desired_version: impl Into<VersionInput<'b>>) -> bool {
    let version = input_version.into().resolve();
    let product_ts = product_timestamp(input_date);
    let desired = desired_version.into().resolve();
    let (Some(version), Some(desired)) = (version, desired) else { return false };

    let (major_base, minor_base, patch_base) = (version.major_base, version.minor_base, version.patch_base);
    let (mut desired_major, mut desired_minor, mut desired_patch) = (desired.major_base, desired.minor_base, desired.patch_base);
    let desired_not_before = desired.not_before;
    let (mut major_must_equal, mut minor_must_equal, mut patch_must_equal) =
        (desired.major_must_equal, desired.minor_must_equal, desired.patch_must_equal);

    let too_early = |product_ts: Option<f64>| product_ts.is_some_and(|ts| ts < desired_not_before);

    if desired.is_minimum {
        if major_base > desired_major {
            return true;
        }
        if major_base < desired_major {
            return false;
        }
        if minor_base > desired_minor {
            return true;
        }
        if minor_base < desired_minor {
            return false;
        }
        if too_early(product_ts) {
            return false;
        }
        return patch_base >= desired_patch;
    }

    // Anything < 1.0.0 is compatible with >= 1.0.0, except exact matches
    if major_base == 1 && desired_major == 0 && (!major_must_equal || !minor_must_equal || !patch_must_equal) {
        desired_major = 1;
        desired_minor = 0;
        desired_patch = 0;
        major_must_equal = true;
        minor_must_equal = false;
        patch_must_equal = false;
    }

    if major_base < desired_major {
        return false;
    }
    if major_base > desired_major {
        return !major_must_equal;
    }
    if minor_base < desired_minor {
        return false;
    }
    if minor_base > desired_minor {
        return !minor_must_equal;
    }
    if patch_base < desired_patch {
        return false;
    }
    if patch_base > desired_patch {
        return !patch_must_equal;
    }
    !too_early(product_ts)
}

fn is_string_array(value: &Value) -> bool {
    value.as_array().is_some_and(|items| items.iter().all(Value::is_string))
}

fn mandatory(name: &str, kind: &str) -> String {
    format!("property `{name}` is mandatory and must be of type `{kind}`")
}

fn optional_string_array(name: &str) -> String {
    format!("property `{name}` can be omitted or must be of type `string[]`")
}

/// `validateExtensionManifest`. `extension_location` é o caminho da URI da pasta da extensão.
pub fn validate_extension_manifest(
    product_version: &str,
    product_date: ProductDate,
    extension_location: &str,
    manifest: &Value,
    extension_is_builtin: bool,
) -> Vec<(Severity, String)> {
    let mut validations: Vec<(Severity, String)> = Vec::new();
    let field = |name: &str| manifest.get(name);
    let defined = |name: &str| field(name).is_some_and(|v| !v.is_null());

    if field("publisher").is_some_and(|v| !v.is_null() && !v.is_string()) {
        validations.push((Severity::Error, "property publisher must be of type `string`.".to_owned()));
        return validations;
    }
    if !field("name").is_some_and(Value::is_string) {
        validations.push((Severity::Error, mandatory("name", "string")));
        return validations;
    }
    if !field("version").is_some_and(Value::is_string) {
        validations.push((Severity::Error, mandatory("version", "string")));
        return validations;
    }
    // `!extensionManifest.engines`: ausente, nulo, `false`, `0` e `""` falham; um objeto passa.
    let engines = field("engines").filter(|v| match v {
        Value::Null | Value::Bool(false) => false,
        Value::String(s) => !s.is_empty(),
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        _ => true,
    });
    let Some(engines) = engines else {
        validations.push((Severity::Error, mandatory("engines", "object")));
        return validations;
    };
    let Some(engines_vscode) = engines.get("vscode").and_then(Value::as_str) else {
        validations.push((Severity::Error, mandatory("engines.vscode", "string")));
        return validations;
    };
    if defined("extensionDependencies") && !is_string_array(&manifest["extensionDependencies"]) {
        validations.push((Severity::Error, optional_string_array("extensionDependencies")));
        return validations;
    }
    if defined("extensionAffinity") && !is_string_array(&manifest["extensionAffinity"]) {
        validations.push((Severity::Error, optional_string_array("extensionAffinity")));
        return validations;
    }
    if defined("activationEvents") {
        if !is_string_array(&manifest["activationEvents"]) {
            validations.push((Severity::Error, optional_string_array("activationEvents")));
            return validations;
        }
        if !defined("main") && !defined("browser") {
            validations.push((
                Severity::Error,
                "property `activationEvents` should be omitted if the extension doesn't have a `main` or `browser` property.".to_owned(),
            ));
            return validations;
        }
    }
    if defined("extensionKind") && !defined("main") {
        validations.push((Severity::Warning, "property `extensionKind` can be defined only if property `main` is also defined.".to_owned()));
        // not a failure case
    }
    for property in ["main", "browser"] {
        if !defined(property) {
            continue;
        }
        let Some(relative) = field(property).and_then(Value::as_str) else {
            validations.push((Severity::Error, format!("property `{property}` can be omitted or must be of type `string`")));
            return validations;
        };
        let location = paths::join_path(extension_location, &[relative]);
        if !paths::is_equal_or_parent(&location, extension_location) {
            validations.push((
                Severity::Warning,
                format!(
                    "Expected `{property}` ({location}) to be included inside extension's folder ({extension_location}). This might make the extension non-portable."
                ),
            ));
            // not a failure case
        }
    }

    let version = field("version").and_then(Value::as_str).unwrap_or_default();
    if semver::valid(version).is_none() {
        validations.push((Severity::Error, "Extension version is not semver compatible.".to_owned()));
        return validations;
    }

    let mut notices = Vec::new();
    let has_entry_point = defined("main") || defined("browser");
    if !is_valid_extension_version_for(product_version, product_date, engines_vscode, has_entry_point, extension_is_builtin, &mut notices) {
        for notice in notices {
            validations.push((Severity::Error, notice));
        }
    }
    validations
}

/// `isValidExtensionVersion`, sobre o manifesto cru.
pub fn is_valid_extension_version(
    product_version: &str,
    product_date: ProductDate,
    manifest: &Value,
    extension_is_builtin: bool,
    notices: &mut Vec<String>,
) -> bool {
    let defined = |name: &str| manifest.get(name).is_some_and(|v| !v.is_null());
    let engines_vscode = manifest.pointer("/engines/vscode").and_then(Value::as_str).unwrap_or_default();
    is_valid_extension_version_for(product_version, product_date, engines_vscode, defined("main") || defined("browser"), extension_is_builtin, notices)
}

fn is_valid_extension_version_for(
    product_version: &str,
    product_date: ProductDate,
    engines_vscode: &str,
    has_entry_point: bool,
    extension_is_builtin: bool,
    notices: &mut Vec<String>,
) -> bool {
    if extension_is_builtin || !has_entry_point {
        // No version check for builtin or declarative extensions
        return true;
    }
    is_version_valid(product_version, product_date, engines_vscode, notices)
}

/// `isEngineValid`.
pub fn is_engine_valid(engine: &str, version: &str, date: ProductDate) -> bool {
    // TODO@joao: discuss with alex '*' doesn't seem to be a valid engine version
    engine == "*" || is_version_valid(version, date, engine, &mut Vec::new())
}

fn is_version_valid(current_version: &str, date: ProductDate, requested_version: &str, notices: &mut Vec<String>) -> bool {
    let Some(desired) = normalize_version(parse_version(requested_version).as_ref()) else {
        notices.push(format!("Could not parse `engines.vscode` value {requested_version}. Please use, for example: ^1.22.0, ^1.22.x, etc."));
        return false;
    };

    // enforce that a breaking API version is specified.
    // for 0.X.Y, that means up to 0.X must be specified
    // otherwise for Z.X.Y, that means Z must be specified
    if desired.major_base == 0 {
        // force that major and minor must be specific
        if !desired.major_must_equal || !desired.minor_must_equal {
            notices.push(format!(
                "Version specified in `engines.vscode` ({requested_version}) is not specific enough. For vscode versions before 1.0.0, please define at a minimum the major and minor desired version. E.g. ^0.10.0, 0.10.x, 0.11.0, etc."
            ));
            return false;
        }
    } else if !desired.major_must_equal {
        // force that major must be specific
        notices.push(format!(
            "Version specified in `engines.vscode` ({requested_version}) is not specific enough. For vscode versions after 1.0.0, please define at a minimum the major desired version. E.g. ^1.10.0, 1.10.x, 1.x.x, 2.x.x, etc."
        ));
        return false;
    }

    if !is_valid_version(current_version, date, &desired) {
        notices.push(format!("Extension is not compatible with Code {current_version}. Extension requires: {requested_version}."));
        return false;
    }
    true
}
