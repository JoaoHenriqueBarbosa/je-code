//! Porte de `src/vs/workbench/contrib/update/common/updateUtils.ts`: progresso, tempo restante, velocidade,
//! versões e formatação do que o workbench mostra durante uma atualização.
//!
//! Decisões de tradução:
//! - Os números do original são `f64`. `Date.now()` é [`runtime::date_now`], que acompanha o relógio virtual
//!   dos testes.
//! - `Math.round` é [`js_round`] (o meio vai para cima, sem o erro de `floor(x + 0.5)` em ponto flutuante).
//! - `tryParseDate` (`Date.parse`) cobre as formas ISO 8601: com `Z` ou deslocamento, data e hora sem
//!   deslocamento (fuso local, como no JavaScript) e só a data (UTC).
//! - `formatDate` (`toLocaleDateString(undefined, { year, month: 'short', day })`) usa o fuso local e o
//!   formato da locale padrão do porte (`en-US`: `Jan 15, 2024`).

use je_base::dynamic::number_to_js_string;
use je_base::nls::localize;
use je_base::runtime;
use je_platform::update::Downloading;
use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;

/// `Math.round`.
fn js_round(x: f64) -> f64 {
    let floor = x.floor();
    if x - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// Returns the progress percentage based on the current and maximum progress values.
pub fn compute_progress_percent(current: Option<f64>, max: Option<f64>) -> Option<f64> {
    let (current, max) = (current?, max?);
    if max <= 0.0 {
        return None;
    }

    Some(js_round((current / max) * 100.0).min(100.0).max(0.0))
}

/// Computes an estimate of remaining download time in seconds.
pub fn compute_download_time_remaining(state: &Downloading) -> Option<f64> {
    let (downloaded_bytes, total_bytes, start_time) = (state.downloaded_bytes?, state.total_bytes?, state.start_time?);

    let elapsed_ms = runtime::date_now() as f64 - start_time;
    if downloaded_bytes <= 0.0 || total_bytes <= 0.0 || elapsed_ms <= 0.0 {
        return None;
    }

    let remaining_bytes = total_bytes - downloaded_bytes;
    if remaining_bytes <= 0.0 {
        return Some(0.0);
    }

    let bytes_per_ms = downloaded_bytes / elapsed_ms;
    if bytes_per_ms <= 0.0 {
        return None;
    }

    let remaining_ms = remaining_bytes / bytes_per_ms;
    Some((remaining_ms / 1000.0).ceil())
}

/// Computes the current download speed in bytes per second.
pub fn compute_download_speed(state: &Downloading) -> Option<f64> {
    let (downloaded_bytes, start_time) = (state.downloaded_bytes?, state.start_time?);

    let elapsed_ms = runtime::date_now() as f64 - start_time;
    if elapsed_ms <= 0.0 || downloaded_bytes <= 0.0 {
        return None;
    }

    Some((downloaded_bytes / elapsed_ms) * 1000.0)
}

/// Computes the version to use for fetching update info.
/// - If the minor version differs: returns `{major}.{minor}` (e.g., 1.108.2 -> 1.109.5 => 1.109)
/// - If the same minor: returns the target version as-is (e.g., 1.109.2 -> 1.109.5 => 1.109.5)
pub fn compute_update_info_version(current_version: &str, target_version: &str) -> Option<String> {
    let current = try_parse_version(Some(current_version))?;
    let target = try_parse_version(Some(target_version))?;

    if current.minor != target.minor || current.major != target.major {
        return Some(format!("{}.{}", target.major, target.minor));
    }

    Some(format!("{}.{}.{}", target.major, target.minor, target.patch))
}

/// Computes the URL to fetch update info from.
/// Follows the release notes URL pattern but with `_update` suffix.
pub fn get_update_info_url(version: &str) -> String {
    let underscored = version.replace('.', "_");
    let version_label = underscored.strip_suffix("_0").unwrap_or(&underscored);
    format!("https://code.visualstudio.com/raw/v{version_label}_update.md")
}

/// Formats the time remaining as a human-readable string.
pub fn format_time_remaining(seconds: f64) -> String {
    let hours = seconds / 3600.0;
    if hours >= 1.0 {
        let formatted_hours = format_decimal(hours);
        return if formatted_hours == "1" {
            localize("update.timeRemainingHour", "{0} hour", &[&formatted_hours])
        } else {
            localize("update.timeRemainingHours", "{0} hours", &[&formatted_hours])
        };
    }

    let minutes = (seconds / 60.0).floor();
    if minutes >= 1.0 {
        return localize("update.timeRemainingMinutes", "{0} min", &[&number_to_js_string(minutes)]);
    }

    localize("update.timeRemainingSeconds", "{0}s", &[&number_to_js_string(seconds)])
}

/// Formats a byte count as a human-readable string.
pub fn format_bytes(bytes: f64) -> String {
    if bytes < 1024.0 {
        return localize("update.bytes", "{0} B", &[&number_to_js_string(bytes)]);
    }

    let kb = bytes / 1024.0;
    if kb < 1024.0 {
        return localize("update.kilobytes", "{0} KB", &[&format_decimal(kb)]);
    }

    let mb = kb / 1024.0;
    if mb < 1024.0 {
        return localize("update.megabytes", "{0} MB", &[&format_decimal(mb)]);
    }

    let gb = mb / 1024.0;
    localize("update.gigabytes", "{0} GB", &[&format_decimal(gb)])
}

/// Tries to parse a date string and returns the timestamp or `None` if parsing fails.
pub fn try_parse_date(date: Option<&str>) -> Option<f64> {
    let date = date?;

    if let Ok(timestamp) = date.parse::<Timestamp>() {
        return Some(timestamp.as_millisecond() as f64);
    }
    if let Ok(local) = date.parse::<DateTime>() {
        let zoned = local.to_zoned(TimeZone::system()).ok()?;
        return Some(zoned.timestamp().as_millisecond() as f64);
    }
    let day = date.parse::<Date>().ok()?;
    let zoned = day.to_zoned(TimeZone::UTC).ok()?;
    Some(zoned.timestamp().as_millisecond() as f64)
}

const SHORT_MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

/// Formats a timestamp as a localized date string.
pub fn format_date(timestamp: f64) -> String {
    let Ok(instant) = Timestamp::from_millisecond(timestamp as i64) else {
        return "Invalid Date".to_owned();
    };
    let zoned = instant.to_zoned(TimeZone::system());
    let month = SHORT_MONTHS[usize::try_from(zoned.month() - 1).unwrap_or(0)];
    format!("{month} {}, {}", zoned.day(), zoned.year())
}

/// Formats a number to 1 decimal place, omitting ".0" for whole numbers.
pub fn format_decimal(value: f64) -> String {
    let rounded = js_round(value * 10.0) / 10.0;
    if rounded % 1.0 == 0.0 { number_to_js_string(rounded) } else { format!("{rounded:.1}") }
}

/// `IVersion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

/// Lê de 1 a 10 dígitos ASCII do começo de `text` (`\d{1,10}`, guloso) e devolve o valor e o resto. Se
/// `must_end_before_dot` é verdadeiro, a sequência inteira de dígitos precisa caber em 10 e ser seguida de
/// `.` (o backtracking da regex não acharia outra divisão).
fn take_number(text: &str, must_end_before_dot: bool) -> Option<(u64, &str)> {
    let run = text.bytes().take_while(u8::is_ascii_digit).count();
    if run == 0 {
        return None;
    }
    let len = if must_end_before_dot {
        if run > 10 || text.as_bytes().get(run) != Some(&b'.') {
            return None;
        }
        run
    } else {
        run.min(10)
    };
    let value = text[..len].parse::<u64>().ok()?;
    Some((value, &text[len..]))
}

/// Parses a version string in the format "major.minor.patch" and returns the components.
pub fn try_parse_version(version: Option<&str>) -> Option<Version> {
    let (major, rest) = take_number(version?, true)?;
    let (minor, rest) = take_number(rest.strip_prefix('.')?, true)?;
    let (patch, _) = take_number(rest.strip_prefix('.')?, false)?;
    Some(Version { major, minor, patch })
}

/// Processes an error message and returns a user-friendly version of it, or `None` if the error should be
/// ignored.
pub fn preprocess_error(error: Option<&str>) -> Option<String> {
    let error = error.filter(|error| !error.is_empty())?;

    let lower = error.to_lowercase();
    if lower.contains("the request timed out") || lower.contains("the network connection was lost") {
        return None;
    }

    Some(error.replacen(
        "See https://github.com/Squirrel/Squirrel.Mac/issues/182 for more information",
        "This might mean the application was put on quarantine by macOS. See [this link](https://github.com/microsoft/vscode/issues/7426#issuecomment-425093469) for more information",
        1,
    ))
}

/// Determines whether there is a major or minor version change between two versions.
pub fn is_major_minor_version_change(previous_version: Option<&str>, new_version: Option<&str>) -> bool {
    match (try_parse_version(previous_version), try_parse_version(new_version)) {
        (Some(previous), Some(current)) => previous.major != current.major || previous.minor != current.minor,
        _ => false,
    }
}
