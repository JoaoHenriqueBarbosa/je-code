//! Porte de `src/vs/base/test/common/date.test.ts` (suíte `Date`), caso a caso e na mesma ordem.
//!
//! As suítes aninhadas do original são módulos. O `ensureNoDisposablesAreLeakedInTestSuite` não entra,
//! porque `date` não cria `IDisposable`. `new Date()` do original é `runtime::date_now()` e o fuso local
//! é `local_time_zone()`; os `set*` de `Date` viram aritmética em milissegundos e, onde o calendário
//! importa, `jiff`.

use jiff::{ToSpan, Timestamp};
use je_base::collator::CollatorOptions;
use je_base::date::{
    from_now, from_now_by_day, get_duration_string, get_expiration_time, get_remaining_time_in_seconds, is_expired,
    local_time_zone, safe_intl,
};
use je_base::platform::LANGUAGE_DEFAULT;
use je_base::runtime;

fn now() -> f64 {
    runtime::date_now() as f64
}

#[test]
fn get_remaining_time_in_seconds_cases() {
    assert_eq!(get_remaining_time_in_seconds(None, Some(1000.0)), None);
    assert_eq!(get_remaining_time_in_seconds(Some(1000.0), Some(1000.0)), None);
    assert_eq!(get_remaining_time_in_seconds(Some(1001.0), Some(1000.0)), Some(1.0));
    assert_eq!(get_remaining_time_in_seconds(Some(2001.0), Some(1000.0)), Some(2.0));
}

#[test]
fn get_expiration_time_cases() {
    assert_eq!(get_expiration_time(None, Some(1000.0)), None);
    assert_eq!(get_expiration_time(Some(0.0), Some(1000.0)), Some(1000.0));
    assert_eq!(get_expiration_time(Some(2.0), Some(1000.0)), Some(3000.0));
}

#[test]
fn is_expired_cases() {
    assert!(!is_expired(None, Some(1000.0)));
    assert!(is_expired(Some(1000.0), Some(1000.0)));
    assert!(!is_expired(Some(1001.0), Some(1000.0)));
}

mod from_now_suite {
    use super::*;

    #[test]
    fn append_ago_label() {
        assert_eq!(from_now(Some(now() - 35000.0), false, false, false), "35 secs");
        assert_eq!(from_now(Some(now() - 35000.0), false, false, false), "35 secs");
        assert_eq!(from_now(Some(now() - 35000.0), true, false, false), "35 secs ago");
    }

    #[test]
    fn use_full_time_words() {
        assert_eq!(from_now(Some(now() - 35000.0), false, false, false), "35 secs");
        assert_eq!(from_now(Some(now() - 35000.0), false, false, false), "35 secs");
        assert_eq!(from_now(Some(now() - 35000.0), false, true, false), "35 seconds");
        assert_eq!(from_now(Some(now() + 4.0 * 60.0 * 60.0 * 1000.0), false, true, false), "in 4 hours");
    }

    #[test]
    fn disallow_now() {
        assert_eq!(from_now(Some(now() - 5000.0), false, false, false), "now");
        assert_eq!(from_now(Some(now() - 5000.0), false, false, false), "now");
        assert_eq!(from_now(Some(now() - 5000.0), false, false, true), "5 secs");
    }
}

mod from_now_by_day_suite {
    use super::*;

    #[test]
    fn today() {
        assert_eq!(from_now_by_day(now(), false, false, &local_time_zone()), "Today");
    }

    #[test]
    fn yesterday() {
        let zone = local_time_zone();
        // `yesterday.setDate(yesterday.getDate() - 1); yesterday.setHours(12)`
        let day = Timestamp::from_millisecond(now() as i64).unwrap().to_zoned(zone.clone());
        let yesterday = day.yesterday().unwrap().with().hour(12).build().unwrap();
        assert_eq!(from_now_by_day(yesterday.timestamp().as_millisecond() as f64, false, false, &zone), "Yesterday");
    }

    #[test]
    fn days_ago() {
        let zone = local_time_zone();
        // `daysAgo.setDate(daysAgo.getDate() - 5); daysAgo.setHours(daysAgo.getHours() - 2)`
        let day = Timestamp::from_millisecond(now() as i64).unwrap().to_zoned(zone.clone());
        let days_ago = day.checked_sub(5.days()).unwrap().checked_sub(2.hours()).unwrap();
        assert_eq!(from_now_by_day(days_ago.timestamp().as_millisecond() as f64, true, false, &zone), "5 days ago");
    }
}

mod get_duration_string_suite {
    use super::*;

    #[test]
    fn basic() {
        assert_eq!(get_duration_string(1.0, false), "1ms");
        assert_eq!(get_duration_string(999.0, false), "999ms");
        assert_eq!(get_duration_string(1000.0, false), "1s");
        assert_eq!(get_duration_string(1000.0 * 60.0 - 1.0, false), "59.999s");
        assert_eq!(get_duration_string(1000.0 * 60.0, false), "1 mins");
        assert_eq!(get_duration_string(1000.0 * 60.0 * 60.0 - 1.0, false), "60 mins");
        assert_eq!(get_duration_string(1000.0 * 60.0 * 60.0, false), "1 hrs");
        assert_eq!(get_duration_string(1000.0 * 60.0 * 60.0 * 24.0 - 1.0, false), "24 hrs");
        assert_eq!(get_duration_string(1000.0 * 60.0 * 60.0 * 24.0, false), "1 days");
    }

    #[test]
    fn use_full_time_words() {
        assert_eq!(get_duration_string(1.0, true), "1 milliseconds");
        assert_eq!(get_duration_string(999.0, true), "999 milliseconds");
        assert_eq!(get_duration_string(1000.0, true), "1 seconds");
        assert_eq!(get_duration_string(1000.0 * 60.0 - 1.0, true), "59.999 seconds");
        assert_eq!(get_duration_string(1000.0 * 60.0, true), "1 minutes");
        assert_eq!(get_duration_string(1000.0 * 60.0 * 60.0 - 1.0, true), "60 minutes");
        assert_eq!(get_duration_string(1000.0 * 60.0 * 60.0, true), "1 hours");
        assert_eq!(get_duration_string(1000.0 * 60.0 * 60.0 * 24.0 - 1.0, true), "24 hours");
        assert_eq!(get_duration_string(1000.0 * 60.0 * 60.0 * 24.0, true), "1 days");
    }

    mod safe_intl_suite {
        use super::*;

        #[test]
        fn collator_fallback() {
            let collator = safe_intl::collator(Some("en_IT"), CollatorOptions::default()).value();
            let comparison = collator.compare(&[u16::from(b'a')], &[u16::from(b'b')]);
            assert_eq!(comparison, -1);
        }

        #[test]
        fn locale_fallback() {
            let locale = safe_intl::locale("en_IT").value();
            assert_eq!(safe_intl::base_name(&locale), LANGUAGE_DEFAULT);
        }
    }
}
