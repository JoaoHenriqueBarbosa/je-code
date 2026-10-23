//! Porte de `src/vs/workbench/contrib/update/test/common/updateUtils.test.ts`, caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o `LeakGuard` criado no começo de cada caso.
//! - `sinon.useFakeTimers()` e `clock.restore()` viram `runtime::with_faked_timers`, que liga o relógio
//!   virtual (começa em 0) e o restaura ao sair; `Date.now()` é `runtime::date_now()`.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::runtime;
use je_platform::update::Downloading;
use je_workbench::contrib::update::update_utils::{
    compute_download_speed, compute_download_time_remaining, compute_progress_percent, compute_update_info_version, format_bytes,
    format_date, format_time_remaining, get_update_info_url, is_major_minor_version_change, try_parse_date,
};

/// O `setup`/`teardown` da suíte: guarda de vazamento e relógio virtual.
fn run(body: impl FnOnce()) {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    runtime::with_faked_timers(body);
}

fn now() -> f64 {
    runtime::date_now() as f64
}

#[allow(non_snake_case)]
fn DownloadingState(downloaded_bytes: Option<f64>, total_bytes: Option<f64>, start_time: Option<f64>) -> Downloading {
    Downloading { update: None, explicit: true, overwrite: false, downloaded_bytes, total_bytes, start_time }
}

// suite('computeProgressPercent')

#[test]
fn compute_progress_percent_handles_invalid_values() {
    run(|| {
        assert_eq!(compute_progress_percent(None, Some(100.0)), None);
        assert_eq!(compute_progress_percent(Some(50.0), None), None);
        assert_eq!(compute_progress_percent(None, None), None);
        assert_eq!(compute_progress_percent(Some(50.0), Some(0.0)), None);
        assert_eq!(compute_progress_percent(Some(50.0), Some(-10.0)), None);
    });
}

#[test]
fn compute_progress_percent_computes_correct_percentage() {
    run(|| {
        assert_eq!(compute_progress_percent(Some(0.0), Some(100.0)), Some(0.0));
        assert_eq!(compute_progress_percent(Some(50.0), Some(100.0)), Some(50.0));
        assert_eq!(compute_progress_percent(Some(100.0), Some(100.0)), Some(100.0));
        assert_eq!(compute_progress_percent(Some(1.0), Some(3.0)), Some(33.0));
        assert_eq!(compute_progress_percent(Some(2.0), Some(3.0)), Some(67.0));
    });
}

#[test]
fn compute_progress_percent_clamps_to_0_100_range() {
    run(|| {
        assert_eq!(compute_progress_percent(Some(-10.0), Some(100.0)), Some(0.0));
        assert_eq!(compute_progress_percent(Some(200.0), Some(100.0)), Some(100.0));
    });
}

// suite('computeDownloadTimeRemaining')

#[test]
fn compute_download_time_remaining_returns_undefined_for_invalid_or_incomplete_input() {
    run(|| {
        let now = now();

        // Missing parameters
        assert_eq!(compute_download_time_remaining(&DownloadingState(None, None, None)), None);
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(500.0), None, Some(now))), None);
        assert_eq!(compute_download_time_remaining(&DownloadingState(None, Some(1000.0), Some(now))), None);
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(500.0), Some(1000.0), None)), None);

        // Zero or negative values
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(0.0), Some(1000.0), Some(now - 1000.0))), None);
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(500.0), Some(0.0), Some(now - 1000.0))), None);
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(500.0), Some(1000.0), Some(now + 1000.0))), None);
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(-100.0), Some(1000.0), Some(now - 1000.0))), None);
    });
}

#[test]
fn compute_download_time_remaining_returns_0_when_download_is_complete_or_over_downloaded() {
    run(|| {
        let now = now();
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(1000.0), Some(1000.0), Some(now - 1000.0))), Some(0.0));
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(1500.0), Some(1000.0), Some(now - 1000.0))), Some(0.0));
    });
}

#[test]
fn compute_download_time_remaining_computes_correct_time_remaining() {
    run(|| {
        let now = now();

        // Simple case: Downloaded 500 bytes of 1000 in 1000ms => 1s remaining
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(500.0), Some(1000.0), Some(now - 1000.0))), Some(1.0));

        // 10 seconds remaining: Downloaded 100MB of 200MB in 10s
        let downloaded_bytes = 100.0 * 1024.0 * 1024.0;
        let total_bytes = 200.0 * 1024.0 * 1024.0;
        assert_eq!(
            compute_download_time_remaining(&DownloadingState(Some(downloaded_bytes), Some(total_bytes), Some(now - 10000.0))),
            Some(10.0)
        );

        // Rounds up: 900 of 1000 bytes in 900ms => 100ms remaining => rounds to 1s
        assert_eq!(compute_download_time_remaining(&DownloadingState(Some(900.0), Some(1000.0), Some(now - 900.0))), Some(1.0));

        // Realistic scenario: 50MB of 100MB in 50s => 50s remaining
        let downloaded_50_mb = 50.0 * 1024.0 * 1024.0;
        let total_100_mb = 100.0 * 1024.0 * 1024.0;
        assert_eq!(
            compute_download_time_remaining(&DownloadingState(Some(downloaded_50_mb), Some(total_100_mb), Some(now - 50000.0))),
            Some(50.0)
        );
    });
}

// suite('computeDownloadSpeed')

#[test]
fn compute_download_speed_returns_undefined_for_invalid_or_incomplete_input() {
    run(|| {
        let now = now();
        assert_eq!(compute_download_speed(&DownloadingState(None, Some(1000.0), Some(now - 1000.0))), None);
        assert_eq!(compute_download_speed(&DownloadingState(Some(500.0), Some(1000.0), None)), None);
        assert_eq!(compute_download_speed(&DownloadingState(None, None, None)), None);
    });
}

#[test]
fn compute_download_speed_returns_undefined_for_zero_or_negative_elapsed_time() {
    run(|| {
        let now = now();
        assert_eq!(compute_download_speed(&DownloadingState(Some(500.0), Some(1000.0), Some(now + 1000.0))), None);
    });
}

#[test]
fn compute_download_speed_returns_undefined_for_zero_downloaded_bytes() {
    run(|| {
        let now = now();
        assert_eq!(compute_download_speed(&DownloadingState(Some(0.0), Some(1000.0), Some(now - 1000.0))), None);
    });
}

#[test]
fn compute_download_speed_computes_correct_download_speed_in_bytes_per_second() {
    run(|| {
        let now = now();

        // 1000 bytes in 1 second = 1000 B/s
        let speed1 = compute_download_speed(&DownloadingState(Some(1000.0), Some(2000.0), Some(now - 1000.0)));
        let speed1 = speed1.expect("a velocidade existe");
        assert!((speed1 - 1000.0).abs() < 50.0); // Allow small timing variance

        // 10 MB in 10 seconds = 1 MB/s = 1048576 B/s
        let ten_mb = 10.0 * 1024.0 * 1024.0;
        let speed2 = compute_download_speed(&DownloadingState(Some(ten_mb), Some(ten_mb * 2.0), Some(now - 10000.0)));
        let speed2 = speed2.expect("a velocidade existe");
        let expected_speed = 1024.0 * 1024.0; // 1 MB/s
        assert!((speed2 - expected_speed).abs() < expected_speed * 0.01); // Within 1%
    });
}

// suite('computeUpdateInfoVersion')

#[test]
fn compute_update_info_version_returns_minor_0_version_when_minor_differs() {
    run(|| {
        assert_eq!(compute_update_info_version("1.108.2", "1.109.5").as_deref(), Some("1.109"));
        assert_eq!(compute_update_info_version("1.108.0", "1.109.0").as_deref(), Some("1.109"));
        assert_eq!(compute_update_info_version("1.107.3", "1.110.1").as_deref(), Some("1.110"));
    });
}

#[test]
fn compute_update_info_version_returns_target_version_as_is_when_same_minor() {
    run(|| {
        assert_eq!(compute_update_info_version("1.109.2", "1.109.5").as_deref(), Some("1.109.5"));
        assert_eq!(compute_update_info_version("1.109.0", "1.109.3").as_deref(), Some("1.109.3"));
    });
}

#[test]
fn compute_update_info_version_returns_minor_0_version_when_major_differs() {
    run(|| {
        assert_eq!(compute_update_info_version("1.109.2", "2.0.1").as_deref(), Some("2.0"));
    });
}

#[test]
fn compute_update_info_version_returns_undefined_for_invalid_versions() {
    run(|| {
        assert_eq!(compute_update_info_version("invalid", "1.109.5"), None);
        assert_eq!(compute_update_info_version("1.109.2", "invalid"), None);
    });
}

// suite('getUpdateInfoUrl')

#[test]
fn get_update_info_url_constructs_correct_url_for_0_versions() {
    run(|| {
        assert_eq!(get_update_info_url("1.109.0"), "https://code.visualstudio.com/raw/v1_109_update.md");
    });
}

#[test]
fn get_update_info_url_constructs_correct_url_for_patch_versions() {
    run(|| {
        assert_eq!(get_update_info_url("1.109.5"), "https://code.visualstudio.com/raw/v1_109_5_update.md");
    });
}

// suite('formatTimeRemaining')

#[test]
fn format_time_remaining_formats_seconds_for_values_less_than_1_minute() {
    run(|| {
        assert_eq!(format_time_remaining(1.0), "1s");
        assert_eq!(format_time_remaining(30.0), "30s");
        assert_eq!(format_time_remaining(59.0), "59s");
    });
}

#[test]
fn format_time_remaining_formats_minutes_for_values_between_1_minute_and_1_hour() {
    run(|| {
        assert_eq!(format_time_remaining(60.0), "1 min");
        assert_eq!(format_time_remaining(120.0), "2 min");
        assert_eq!(format_time_remaining(90.0), "1 min"); // Floors to 1 min
        assert_eq!(format_time_remaining(3599.0), "59 min");
    });
}

#[test]
fn format_time_remaining_formats_fractional_hours_for_values_gte_1_hour() {
    run(|| {
        assert_eq!(format_time_remaining(3600.0), "1 hour");
        assert_eq!(format_time_remaining(5400.0), "1.5 hours"); // 1.5 hours
        assert_eq!(format_time_remaining(7200.0), "2 hours");
        assert_eq!(format_time_remaining(9000.0), "2.5 hours"); // 2.5 hours
        assert_eq!(format_time_remaining(3960.0), "1.1 hours"); // 1 hour 6 min = 1.1 hours
    });
}

// suite('formatBytes')

#[test]
fn format_bytes_formats_bytes_for_values_less_than_1_kb() {
    run(|| {
        assert_eq!(format_bytes(0.0), "0 B");
        assert_eq!(format_bytes(1.0), "1 B");
        assert_eq!(format_bytes(512.0), "512 B");
        assert_eq!(format_bytes(1023.0), "1023 B");
    });
}

#[test]
fn format_bytes_formats_kilobytes_for_values_between_1_kb_and_1_mb() {
    run(|| {
        assert_eq!(format_bytes(1024.0), "1 KB");
        assert_eq!(format_bytes(1536.0), "1.5 KB"); // 1.5 KB
        assert_eq!(format_bytes(2048.0), "2 KB");
        assert_eq!(format_bytes(1024.0 * 100.0), "100 KB");
        assert_eq!(format_bytes(1024.0 * 1023.0), "1023 KB");
    });
}

#[test]
fn format_bytes_formats_megabytes_for_values_between_1_mb_and_1_gb() {
    run(|| {
        assert_eq!(format_bytes(1024.0 * 1024.0), "1 MB");
        assert_eq!(format_bytes(1024.0 * 1024.0 * 1.5), "1.5 MB");
        assert_eq!(format_bytes(1024.0 * 1024.0 * 100.0), "100 MB");
        assert_eq!(format_bytes(1024.0 * 1024.0 * 512.0), "512 MB");
    });
}

#[test]
fn format_bytes_formats_gigabytes_for_values_gte_1_gb() {
    run(|| {
        assert_eq!(format_bytes(1024.0 * 1024.0 * 1024.0), "1 GB");
        assert_eq!(format_bytes(1024.0 * 1024.0 * 1024.0 * 1.5), "1.5 GB");
        assert_eq!(format_bytes(1024.0 * 1024.0 * 1024.0 * 10.0), "10 GB");
    });
}

#[test]
fn format_bytes_rounds_to_one_decimal_place_correctly() {
    run(|| {
        assert_eq!(format_bytes(1126.0), "1.1 KB");
        assert_eq!(format_bytes(1075.0), "1 KB");
        assert_eq!(format_bytes(1024.0 * 1024.0 * 25.35), "25.4 MB");
    });
}

// suite('tryParseDate')

#[test]
fn try_parse_date_returns_undefined_for_undefined_input() {
    run(|| {
        assert_eq!(try_parse_date(None), None);
    });
}

#[test]
fn try_parse_date_returns_undefined_for_invalid_date_strings() {
    run(|| {
        assert_eq!(try_parse_date(Some("")), None);
        assert_eq!(try_parse_date(Some("not-a-date")), None);
    });
}

#[test]
fn try_parse_date_parses_valid_iso_date_strings() {
    run(|| {
        let result = try_parse_date(Some("2026-02-06T05:03:03.991Z"));
        let result = result.expect("a data ISO é válida");
        assert!(result > 0.0);
    });
}

// suite('formatDate')

#[test]
fn format_date_formats_a_timestamp_as_a_readable_date() {
    run(|| {
        let result = format_date(1705276800000.0);
        assert!(!result.is_empty());
        assert!(result.contains("2024"));
    });
}

// suite('isMajorMinorVersionChange')

#[test]
fn is_major_minor_version_change_returns_true_for_major_version_change() {
    run(|| {
        assert!(is_major_minor_version_change(Some("1.90.0"), Some("2.0.0")));
    });
}

#[test]
fn is_major_minor_version_change_returns_true_for_minor_version_change() {
    run(|| {
        assert!(is_major_minor_version_change(Some("1.90.0"), Some("1.91.0")));
    });
}

#[test]
fn is_major_minor_version_change_returns_false_for_patch_only_change() {
    run(|| {
        assert!(!is_major_minor_version_change(Some("1.90.0"), Some("1.90.1")));
    });
}

#[test]
fn is_major_minor_version_change_returns_false_for_identical_versions() {
    run(|| {
        assert!(!is_major_minor_version_change(Some("1.90.0"), Some("1.90.0")));
    });
}

#[test]
fn is_major_minor_version_change_returns_false_when_previous_version_is_undefined() {
    run(|| {
        assert!(!is_major_minor_version_change(None, Some("1.90.0")));
    });
}

#[test]
fn is_major_minor_version_change_returns_false_when_new_version_is_undefined() {
    run(|| {
        assert!(!is_major_minor_version_change(Some("1.90.0"), None));
    });
}

#[test]
fn is_major_minor_version_change_returns_false_when_both_versions_are_undefined() {
    run(|| {
        assert!(!is_major_minor_version_change(None, None));
    });
}

#[test]
fn is_major_minor_version_change_returns_false_for_unparseable_versions() {
    run(|| {
        assert!(!is_major_minor_version_change(Some("invalid"), Some("1.90.0")));
        assert!(!is_major_minor_version_change(Some("1.90.0"), Some("invalid")));
    });
}
