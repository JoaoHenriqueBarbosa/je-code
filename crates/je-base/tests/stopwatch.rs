//! Testes próprios de `src/vs/base/common/stopwatch.ts` (o VS Code não tem `stopwatch.test.ts`).

use je_base::runtime;
use je_base::stopwatch::StopWatch;

#[test]
fn high_resolution_elapsed_is_monotonic_and_stop_freezes_it() {
    let mut watch = StopWatch::create(None);
    let first = watch.elapsed();
    assert!(first >= 0.0);
    watch.stop();
    let frozen = watch.elapsed();
    std::thread::sleep(std::time::Duration::from_millis(5));
    assert_eq!(watch.elapsed(), frozen);
    watch.reset();
    assert!(watch.elapsed() < 5.0 + frozen.max(1000.0));
}

#[test]
fn low_resolution_uses_the_loop_clock() {
    runtime::with_faked_timers(|| {
        let mut watch = StopWatch::new(Some(false));
        runtime::advance_fake_time(250);
        assert_eq!(watch.elapsed(), 250.0);
        watch.stop();
        runtime::advance_fake_time(100);
        assert_eq!(watch.elapsed(), 250.0);
        watch.reset();
        runtime::advance_fake_time(7);
        assert_eq!(watch.elapsed(), 7.0);
    });
}
