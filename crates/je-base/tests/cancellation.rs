//! Porte de `src/vs/base/test/common/cancellation.test.ts` (suíte `CancellationToken`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `assert.strictEqual(typeof CancellationToken.None.onCancellationRequested, 'function')` é garantido
//!   pelo tipo (o método existe); o teste só confere `isCancellationRequested`.
//! - `new Promise(resolve => { ...; resolve() })` vira uma flag conferida depois de o loop esvaziar.

mod common;

use std::cell::Cell;
use std::rc::Rc;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::cancellation::{AbortController, CancellationToken, CancellationTokenPool, CancellationTokenSource, abort_on_dispose};
use je_base::errors::is_cancellation_error;
use je_base::lifecycle::{DisposableStore, IDisposable};
use je_base::runtime;

mod cancellation_token {
    use super::*;

    #[test]
    fn none() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        assert!(!CancellationToken::None.is_cancellation_requested());
    }

    #[test]
    fn cancel_before_token() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        let source = CancellationTokenSource::new(None);
        assert!(!source.token().is_cancellation_requested());
        source.cancel();

        assert!(source.token().is_cancellation_requested());

        let resolved = Rc::new(Cell::new(false));
        let flag = resolved.clone();
        runtime::with_faked_timers(|| {
            source.token().on_cancellation_requested().listen(move |_| flag.set(true));
        });
        assert!(resolved.get());
    }

    #[test]
    fn cancel_happens_only_once() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        let source = CancellationTokenSource::new(None);
        assert!(!source.token().is_cancellation_requested());

        let cancel_count = Rc::new(Cell::new(0));
        let count = cancel_count.clone();
        ds.add(source.token().on_cancellation_requested().listen(move |_| count.set(count.get() + 1)));

        source.cancel();
        source.cancel();

        assert_eq!(cancel_count.get(), 1);
    }

    #[test]
    fn cancel_calls_all_listeners() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        let count = Rc::new(Cell::new(0));

        let source = CancellationTokenSource::new(None);
        for _ in 0..3 {
            let count = count.clone();
            ds.add(source.token().on_cancellation_requested().listen(move |_| count.set(count.get() + 1)));
        }

        source.cancel();
        assert_eq!(count.get(), 3);
    }

    #[test]
    fn token_stays_the_same() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();

        let source = CancellationTokenSource::new(None);
        let token = source.token();
        assert!(token.is_same(&source.token())); // não muda ao ler

        source.cancel();
        assert!(token.is_same(&source.token())); // não muda depois de cancelar

        source.cancel();
        assert!(token.is_same(&source.token())); // não muda depois do 2º cancel

        let source = CancellationTokenSource::new(None);
        source.cancel();
        let token = source.token();
        assert!(token.is_same(&source.token())); // não muda ao ler
    }

    #[test]
    fn dispose_calls_no_listeners() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        let count = Rc::new(Cell::new(0));

        let source = CancellationTokenSource::new(None);
        let counter = count.clone();
        ds.add(source.token().on_cancellation_requested().listen(move |_| counter.set(counter.get() + 1)));

        source.dispose();
        source.cancel();
        assert_eq!(count.get(), 0);
    }

    #[test]
    fn dispose_calls_no_listeners_unless_told_to_cancel() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        let count = Rc::new(Cell::new(0));

        let source = CancellationTokenSource::new(None);
        let counter = count.clone();
        ds.add(source.token().on_cancellation_requested().listen(move |_| counter.set(counter.get() + 1)));

        source.dispose_with(true);
        // source.cancel();
        assert_eq!(count.get(), 1);
    }

    #[test]
    fn dispose_does_not_cancel() {
        let _ds = ensure_no_disposables_are_leaked_in_test_suite();
        let source = CancellationTokenSource::new(None);
        source.dispose();
        assert!(!source.token().is_cancellation_requested());
    }

    #[test]
    fn parent_cancels_child() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();

        let parent = CancellationTokenSource::new(None);
        let child = CancellationTokenSource::new(Some(&parent.token()));

        let count = Rc::new(Cell::new(0));
        let counter = count.clone();
        ds.add(child.token().on_cancellation_requested().listen(move |_| counter.set(counter.get() + 1)));

        parent.cancel();

        assert_eq!(count.get(), 1);
        assert!(child.token().is_cancellation_requested());
        assert!(parent.token().is_cancellation_requested());

        child.dispose();
        parent.dispose();
    }

    #[test]
    fn from_abort_signal_reflects_current_and_future_cancellation() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let lifetime = ds.add_rc(DisposableStore::new());
        let controller = AbortController::new();
        let token = CancellationToken::from_abort_signal(&controller.signal(), &lifetime);
        let initially_cancelled = token.is_cancellation_requested();
        let notifications = Rc::new(Cell::new(0));
        let counter = notifications.clone();
        lifetime.add(token.on_cancellation_requested().listen(move |_| counter.set(counter.get() + 1)));
        controller.abort(None);
        controller.abort(None);
        assert!(!initially_cancelled);
        assert!(token.is_cancellation_requested());
        assert!(CancellationToken::from_abort_signal(&controller.signal(), &lifetime).is_cancellation_requested());
        assert_eq!(notifications.get(), 1);
    }

    #[test]
    fn from_abort_signal_stops_forwarding_after_lifetime_disposal() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let lifetime = ds.add_rc(DisposableStore::new());
        let controller = AbortController::new();
        let token = CancellationToken::from_abort_signal(&controller.signal(), &lifetime);
        lifetime.dispose();
        let aborted_on_dispose = controller.signal().aborted();
        controller.abort(None);
        assert!(!aborted_on_dispose);
        assert!(controller.signal().aborted());
        assert!(!token.is_cancellation_requested());
    }

    #[test]
    fn to_abort_signal_reflects_current_and_future_cancellation() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let lifetime = ds.add_rc(DisposableStore::new());
        let source = CancellationTokenSource::new(None);
        let token = source.token();
        let signal = CancellationToken::to_abort_signal(&token, &lifetime);
        let initially_aborted = signal.aborted();
        source.cancel();
        let cancelled_signals = [
            signal,
            CancellationToken::to_abort_signal(&token, &lifetime),
            CancellationToken::to_abort_signal(&CancellationToken::Cancelled, &lifetime),
        ];
        assert!(!initially_aborted);
        for signal in &cancelled_signals {
            assert!(signal.aborted());
            assert!(signal.reason().is_some_and(|reason| is_cancellation_error(&reason)));
        }
        assert!(!CancellationToken::to_abort_signal(&CancellationToken::None, &lifetime).aborted());
        source.dispose();
    }

    #[test]
    fn to_abort_signal_stops_forwarding_after_lifetime_disposal() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let lifetime = ds.add_rc(DisposableStore::new());
        let source = CancellationTokenSource::new(None);
        let signal = CancellationToken::to_abort_signal(&source.token(), &lifetime);
        lifetime.dispose();
        let cancelled_on_dispose = source.token().is_cancellation_requested();
        source.cancel();
        assert!(!cancelled_on_dispose);
        assert!(source.token().is_cancellation_requested());
        assert!(!signal.aborted());
        source.dispose();
    }

    #[test]
    fn abort_on_dispose_aborts_when_the_store_is_disposed() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let lifetime = ds.add_rc(DisposableStore::new());
        let signal = abort_on_dispose(&lifetime);
        let initially_aborted = signal.aborted();
        lifetime.dispose();
        assert!(!initially_aborted);
        assert!(signal.aborted());
        assert!(signal.reason().is_some_and(|reason| is_cancellation_error(&reason)));
    }
}

mod cancellation_token_pool {
    use super::*;

    fn new_pool(ds: &common::LeakGuard) -> Rc<CancellationTokenPool> {
        ds.add_rc(Rc::new(CancellationTokenPool::new()))
    }

    #[test]
    fn empty_pool_token_is_not_cancelled() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let pool = new_pool(&ds);

        assert!(!pool.token().is_cancellation_requested());
    }

    #[test]
    fn pool_token_cancels_when_all_tokens_are_cancelled() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let pool = new_pool(&ds);

        let source1 = CancellationTokenSource::new(None);
        let source2 = CancellationTokenSource::new(None);
        let source3 = CancellationTokenSource::new(None);

        pool.add(&source1.token());
        pool.add(&source2.token());
        pool.add(&source3.token());

        assert!(!pool.token().is_cancellation_requested());

        source1.cancel();
        assert!(!pool.token().is_cancellation_requested());

        source2.cancel();
        assert!(!pool.token().is_cancellation_requested());

        source3.cancel();
        assert!(pool.token().is_cancellation_requested());

        source1.dispose();
        source2.dispose();
        source3.dispose();
    }

    #[test]
    fn pool_token_fires_cancellation_event_when_all_tokens_are_cancelled() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let resolved = Rc::new(Cell::new(false));
        let pool = new_pool(&ds);

        let source1 = CancellationTokenSource::new(None);
        let source2 = CancellationTokenSource::new(None);

        pool.add(&source1.token());
        pool.add(&source2.token());

        let flag = resolved.clone();
        ds.add(pool.token().on_cancellation_requested().listen(move |_| flag.set(true)));

        source1.cancel();
        source2.cancel();

        source1.dispose();
        source2.dispose();
        assert!(resolved.get());
    }

    #[test]
    fn adding_already_cancelled_token_counts_immediately() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let pool = new_pool(&ds);

        let source1 = CancellationTokenSource::new(None);
        let source2 = CancellationTokenSource::new(None);

        source1.cancel(); // Cancela antes de entrar no pool

        pool.add(&source1.token());
        assert!(pool.token().is_cancellation_requested()); // 1 de 1 cancelado, então o pool cancela

        pool.add(&source2.token()); // Adicionar com o pool pronto não tem efeito
        assert!(pool.token().is_cancellation_requested());

        source2.cancel(); // Sem efeito, o pool já terminou
        assert!(pool.token().is_cancellation_requested());

        source1.dispose();
        source2.dispose();
    }

    #[test]
    fn adding_single_already_cancelled_token_cancels_pool_immediately() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let pool = new_pool(&ds);

        let source = CancellationTokenSource::new(None);
        source.cancel();

        pool.add(&source.token());
        assert!(pool.token().is_cancellation_requested()); // 1 de 1 cancelado

        source.dispose();
    }

    #[test]
    fn adding_token_after_pool_is_done_has_no_effect() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let pool = new_pool(&ds);

        let source1 = CancellationTokenSource::new(None);
        let source2 = CancellationTokenSource::new(None);

        pool.add(&source1.token());
        source1.cancel(); // O pool deve estar pronto agora

        assert!(pool.token().is_cancellation_requested());

        // Adicionar outro token não tem efeito
        pool.add(&source2.token());
        source2.cancel();

        assert!(pool.token().is_cancellation_requested());

        source1.dispose();
        source2.dispose();
    }

    #[test]
    fn single_token_pool_behaviour() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let pool = new_pool(&ds);

        let source = CancellationTokenSource::new(None);
        pool.add(&source.token());

        assert!(!pool.token().is_cancellation_requested());

        source.cancel();
        assert!(pool.token().is_cancellation_requested());

        source.dispose();
    }

    #[test]
    fn pool_with_only_cancelled_tokens() {
        let ds = ensure_no_disposables_are_leaked_in_test_suite();
        let pool = new_pool(&ds);

        let source1 = CancellationTokenSource::new(None);
        let source2 = CancellationTokenSource::new(None);

        source1.cancel();
        source2.cancel();

        pool.add(&source1.token());
        pool.add(&source2.token());

        assert!(pool.token().is_cancellation_requested());

        source1.dispose();
        source2.dispose();
    }
}
