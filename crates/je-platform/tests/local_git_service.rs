//! Porte de `src/vs/platform/git/test/node/localGitService.test.ts`.
//!
//! Os 23 casos saem um a um e na mesma ordem do original, com os mesmos esperados literais.
//!
//! Divergências:
//! - O `createExecFile` do original devolve um `cp.execFile` falso que confere o comando e os argumentos e responde por
//!   `queueMicrotask`; aqui é o [`MockExecFile`], com as mesmas conferências e a mesma resposta por
//!   `runtime::queue_microtask`. O `process.env` que o original altera com `Object.assign(process.env, ...)` é injetado
//!   no serviço (o Rust não permite mudar o ambiente do processo sem `unsafe`), sempre partindo do ambiente nativo.
//! - `ensureNoDisposablesAreLeakedInTestSuite()` é o `LeakGuard` criado no começo de cada caso. O `teardown` que apaga as
//!   pastas temporárias é o `Drop` do `TempDir`.
//! - `assert.rejects(promise, validação)` é o erro da promessa rejeitada conferido caso a caso. No caso do cancelamento o
//!   tratador entra já (como o `assert.rejects` do original), antes de o `cancel` rodar.
//! - O caso do repositório real roda o executável `git` de verdade, como o original, e o `.timeout(20_000)` do mocha não
//!   tem equivalente.
#![cfg(not(target_arch = "wasm32"))]

mod common;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::fs;
use std::future::Future;
use std::path::Path;
use std::process::Command;
use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite, null_log, run_with_real_timers};
use je_base::dynamic::Dynamic;
use je_base::errors::{Error, is_cancellation_error};
use je_base::process::ProcessEnvironment;
use je_base::runtime;
use je_platform::git::local_git_service::{IGitAuthentication, IGitNetworkOptions, IGitPullOptions, ILocalGitService};
use je_platform::git::node_local_git_service::{ChildProcess, ExecFile, ExecFileCallback, ExecFileOptions, LocalGitService, native_process_environment};
use je_platform::log::{LogArg, LogMessage, LogServiceRef, NullLogService};

/// `IExecFileExpectation`.
#[derive(Default)]
struct Expectation {
    args: Vec<String>,
    environment: Vec<(String, String)>,
    absent_environment: Vec<String>,
    stdout: String,
    stderr: String,
    error: Option<Error>,
}

fn expect(args: &[&str]) -> Expectation {
    Expectation { args: args.iter().map(|arg| (*arg).to_owned()).collect(), ..Expectation::default() }
}

impl Expectation {
    fn stdout(mut self, stdout: &str) -> Self {
        self.stdout = stdout.to_owned();
        self
    }

    fn stderr(mut self, stderr: &str) -> Self {
        self.stderr = stderr.to_owned();
        self
    }

    fn error(mut self, error: Error) -> Self {
        self.error = Some(error);
        self
    }

    fn environment(mut self, environment: Vec<(String, String)>) -> Self {
        self.environment = environment;
        self
    }

    fn absent_environment(mut self, keys: &[&str]) -> Self {
        self.absent_environment = keys.iter().map(|key| (*key).to_owned()).collect();
        self
    }
}

type Expectations = Rc<RefCell<VecDeque<Expectation>>>;

/// O filho falso: `{ kill: () => true }`.
struct MockChild;

impl ChildProcess for MockChild {
    fn kill(&self) {}
}

/// `createExecFile(expectations)`.
struct MockExecFile {
    expectations: Expectations,
}

impl ExecFile for MockExecFile {
    fn exec_file(&self, command: &str, args: &[String], options: &ExecFileOptions, callback: ExecFileCallback) -> Rc<dyn ChildProcess> {
        assert_eq!(command, "git");

        let expectation = self.expectations.borrow_mut().pop_front().unwrap_or_else(|| panic!("Unexpected git call: {}", args.join(" ")));
        assert_eq!(args, expectation.args.as_slice());
        for (key, value) in &expectation.environment {
            assert_eq!(options.env.as_ref().and_then(|env| env.get(key)), Some(value));
        }
        for key in &expectation.absent_environment {
            assert_eq!(options.env.as_ref().and_then(|env| env.get(key)), None);
        }

        let Expectation { stdout, stderr, error, .. } = expectation;
        runtime::queue_microtask(move || callback(error, stdout, stderr));

        Rc::new(MockChild)
    }
}

struct Harness {
    service: LocalGitService,
    expectations: Expectations,
}

impl Harness {
    /// `expectations.length`.
    fn remaining(&self) -> usize {
        self.expectations.borrow().len()
    }
}

fn harness_with(expectations: Vec<Expectation>, log_service: LogServiceRef, process_environment: Rc<dyn Fn() -> ProcessEnvironment>) -> Harness {
    let expectations: Expectations = Rc::new(RefCell::new(expectations.into()));
    let service = LocalGitService::with_dependencies(log_service, Rc::new(MockExecFile { expectations: expectations.clone() }), process_environment);
    Harness { service, expectations }
}

/// `new LocalGitService(new NullLogService(), createExecFile(expectations))`.
fn harness(expectations: Vec<Expectation>) -> Harness {
    harness_with(expectations, null_log(), Rc::new(native_process_environment))
}

/// `createExecFile` com o `process.env` trocado por `environment`.
fn harness_with_environment(expectations: Vec<Expectation>, environment: ProcessEnvironment) -> Harness {
    harness_with(expectations, null_log(), Rc::new(move || environment.clone()))
}

/// `createDivergedPullError()`.
fn create_diverged_pull_error() -> Error {
    create_pull_error("fatal: Not possible to fast-forward, aborting.", "fatal: Not possible to fast-forward, aborting.")
}

/// `createPullError(message, stderr, code = 128)`.
fn create_pull_error(message: &str, stderr: &str) -> Error {
    Error::new(message).with_stack(format!("Error: {message}\n    at createPullError")).with_prop("code", 128).with_prop("stderr", stderr)
}

const PRIVATE_URL: &str = "https://github.com/test/private.git";

/// `{ authentication: { url, authorizationHeader } }`.
fn authentication(url: &str, authorization_header: &str) -> IGitNetworkOptions {
    IGitNetworkOptions {
        authentication: Some(IGitAuthentication { url: url.to_owned(), authorization_header: authorization_header.to_owned() }),
        log_errors: None,
    }
}

fn pair(key: &str, value: &str) -> (String, String) {
    (key.to_owned(), value.to_owned())
}

/// O caso assíncrono: o `LeakGuard` do `ensureNoDisposablesAreLeakedInTestSuite()` e o laço de eventos no relógio real.
fn run(body: impl Future<Output = ()> + 'static) {
    let _guard: LeakGuard = ensure_no_disposables_are_leaked_in_test_suite();
    run_with_real_timers(body);
}

/// `/^http\..+\.extraheader$/i`.
fn is_extra_header(key: &str) -> bool {
    let lower = key.to_lowercase();
    lower.len() >= 18 && lower.starts_with("http.") && lower.ends_with(".extraheader")
}

/// `/^GIT_CONFIG_(?:COUNT|PARAMETERS|KEY_\d+|VALUE_\d+)$/i`.
fn is_config_environment_name(key: &str) -> bool {
    let upper = key.to_uppercase();
    upper == "GIT_CONFIG_COUNT"
        || upper == "GIT_CONFIG_PARAMETERS"
        || ["GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"].iter().any(|prefix| upper.strip_prefix(prefix).is_some_and(|index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())))
}

fn temporary_directory(prefix: &str) -> tempfile::TempDir {
    tempfile::Builder::new().prefix(prefix).tempdir().expect("pasta temporária")
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[test]
fn clone_passes_scoped_http_authentication_through_git_config_environment_variables() {
    run(async {
        let process_env = native_process_environment();
        let environment_names: HashMap<String, String> = process_env.keys().map(|key| (key.to_uppercase(), key.clone())).collect();
        let configured_count = environment_names.get("GIT_CONFIG_COUNT").and_then(|name| process_env[name].trim().parse::<u64>().ok());
        let mut index = 0;
        for inherited_index in 0..configured_count.unwrap_or(0) {
            let key = environment_names.get(&format!("GIT_CONFIG_KEY_{inherited_index}")).map(|name| &process_env[name]);
            if key.is_some_and(|key| !is_extra_header(key)) {
                index += 1;
            }
        }
        let header_key = "http.https://github.com/test/private.git.extraHeader";
        let test = harness(vec![
            expect(&["--version"]).stdout("git version 2.31.0\n"),
            expect(&["clone", "--", PRIVATE_URL, "/tmp/private"]).environment(vec![
                pair("GIT_TRACE2", "0"),
                pair("GIT_TRACE2_EVENT", "0"),
                pair("GIT_TRACE2_PERF", "0"),
                pair("GIT_TRACE_REDACT", "1"),
                pair("GIT_CONFIG_COUNT", &(index + 2).to_string()),
                pair(&format!("GIT_CONFIG_KEY_{index}"), header_key),
                pair(&format!("GIT_CONFIG_VALUE_{index}"), ""),
                pair(&format!("GIT_CONFIG_KEY_{}", index + 1), header_key),
                pair(&format!("GIT_CONFIG_VALUE_{}", index + 1), "Authorization: Basic secret"),
            ]),
        ]);

        test.service
            .clone_repository("test-op", PRIVATE_URL, "/tmp/private", None, Some(&authentication(PRIVATE_URL, "Authorization: Basic secret")))
            .await
            .unwrap();

        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn clone_fetches_and_checks_out_a_full_commit_sha() {
    run(async {
        let commit = "AABBCCDDEEFF00112233445566778899AABBCCDD";
        let normalized_commit = commit.to_lowercase();
        let test = harness(vec![
            expect(&["clone", "--", "https://github.com/test/repo.git", "/tmp/repo"]),
            expect(&["fetch", "origin", &normalized_commit]),
            expect(&["rev-parse", &format!("{normalized_commit}^{{commit}}")]).stdout(&format!("{normalized_commit}\n")),
            expect(&["checkout", "--detach", &normalized_commit]),
            expect(&["rev-parse", "HEAD"]).stdout(&format!("{normalized_commit}\n")),
        ]);

        test.service.clone_repository("test-op", "https://github.com/test/repo.git", "/tmp/repo", Some(commit), None).await.unwrap();

        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn clone_removes_a_full_sha_repository_when_checkout_fails() {
    run(async {
        let commit = "aabbccddeeff00112233445566778899aabbccdd";
        let parent = temporary_directory("vscode-plugin-git-clone-");
        let target = parent.path().join("repo");
        fs::create_dir(&target).unwrap();
        let target_path = path_text(&target);
        let test = harness(vec![
            expect(&["clone", "--", "https://github.com/test/repo.git", &target_path]),
            expect(&["fetch", "origin", commit]),
            expect(&["rev-parse", &format!("{commit}^{{commit}}")]).error(Error::new("Commit is unavailable")),
        ]);

        let error = test.service.clone_repository("test-op", "https://github.com/test/repo.git", &target_path, Some(commit), None).await.unwrap_err();
        assert_eq!((error.name.as_str(), error.message.as_str()), ("Error", "Commit is unavailable"));
        assert!(!target.exists());
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn clone_removes_a_full_sha_repository_when_pinning_is_cancelled() {
    run(async {
        let commit = "aabbccddeeff00112233445566778899aabbccdd";
        let parent = temporary_directory("vscode-plugin-git-clone-");
        let target = parent.path().join("repo");
        fs::create_dir(&target).unwrap();
        let target_path = path_text(&target);
        let test = harness(vec![
            expect(&["clone", "--", "https://github.com/test/repo.git", &target_path]),
            expect(&["fetch", "origin", commit]).error(Error::cancellation()),
        ]);

        let error = test.service.clone_repository("test-op", "https://github.com/test/repo.git", &target_path, Some(commit), None).await.unwrap_err();
        assert!(is_cancellation_error(&error));
        assert!(!target.exists());
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn authenticated_git_removes_tracing_variables_case_insensitively() {
    run(async {
        let inherited = [("git_trace_curl", "1"), ("Git_Curl_Verbose", "1"), ("git_trace2_event", "/tmp/git-trace.json")];
        let mut environment = native_process_environment();
        environment.extend(inherited.iter().map(|(key, value)| pair(key, value)));
        let test = harness_with_environment(
            vec![
                expect(&["--version"]).stdout("git version 2.31.0\n"),
                expect(&["clone", "--", PRIVATE_URL, "/tmp/private"]).absent_environment(&inherited.map(|(key, _)| key)),
            ],
            environment,
        );

        test.service
            .clone_repository("test-op", PRIVATE_URL, "/tmp/private", None, Some(&authentication(PRIVATE_URL, "Authorization: Basic secret")))
            .await
            .unwrap();

        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn authenticated_git_removes_inherited_extra_headers_and_preserves_other_indexed_config() {
    run(async {
        let mut environment: ProcessEnvironment = native_process_environment().into_iter().filter(|(key, _)| !is_config_environment_name(key)).collect();
        environment.extend([
            pair("git_config_count", "3"),
            pair("git_config_key_0", "http.https://github.com/test/.extraHeader"),
            pair("git_config_value_0", "Authorization: Basic inherited"),
            pair("GIT_CONFIG_KEY_1", "http.proxy"),
            pair("GIT_CONFIG_VALUE_1", "http://proxy.test"),
            pair("GIT_CONFIG_KEY_2", "http.https://example.com/.extraHeader"),
            pair("GIT_CONFIG_VALUE_2", "Authorization: Basic unrelated"),
            pair("GIT_CONFIG_PARAMETERS", "'http.https://github.com/.extraHeader=Authorization: Basic parameter'"),
        ]);
        let header_key = "http.https://github.com/test/private.git.extraHeader";
        let test = harness_with_environment(
            vec![
                expect(&["--version"]).stdout("git version 2.31.0\n"),
                expect(&["clone", "--", PRIVATE_URL, "/tmp/private"])
                    .environment(vec![
                        pair("GIT_CONFIG_COUNT", "3"),
                        pair("GIT_CONFIG_KEY_0", "http.proxy"),
                        pair("GIT_CONFIG_VALUE_0", "http://proxy.test"),
                        pair("GIT_CONFIG_KEY_1", header_key),
                        pair("GIT_CONFIG_VALUE_1", ""),
                        pair("GIT_CONFIG_KEY_2", header_key),
                        pair("GIT_CONFIG_VALUE_2", "Authorization: Basic editor"),
                    ])
                    .absent_environment(&["git_config_count", "git_config_key_0", "git_config_value_0", "GIT_CONFIG_PARAMETERS"]),
            ],
            environment,
        );

        test.service
            .clone_repository("test-op", PRIVATE_URL, "/tmp/private", None, Some(&authentication(PRIVATE_URL, "Authorization: Basic editor")))
            .await
            .unwrap();

        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn authenticated_git_rejects_old_versions_and_rechecks_after_an_upgrade() {
    run(async {
        let test = harness(vec![
            expect(&["--version"]).stdout("git version 2.30.9\n"),
            expect(&["--version"]).stdout("git version 2.31.0\n"),
            expect(&["fetch"]),
            expect(&["fetch"]),
        ]);
        let options = authentication(PRIVATE_URL, "Authorization: Basic secret");

        let error = test.service.fetch("old-git", "/tmp/private", Some(&options)).await.unwrap_err();
        assert!(error.message.contains("requires Git 2.31 or later"), "{error}");
        test.service.fetch("upgraded-git", "/tmp/private", Some(&options)).await.unwrap();
        test.service.fetch("cached-version", "/tmp/private", Some(&options)).await.unwrap();

        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn cancelling_the_authentication_version_check_does_not_launch_a_network_operation() {
    run(async {
        let test = harness(vec![expect(&["--version"]).stdout("git version 2.31.0\n")]);
        let operation = test.service.fetch("cancelled", "/tmp/private", Some(&authentication(PRIVATE_URL, "Authorization: Basic secret")));
        // `assert.rejects(operation, isCancellationError)` já ouve a promessa, antes do `cancel`.
        let rejected = operation.then_result(|result| Ok(result.err()));
        test.service.cancel("cancelled").await.unwrap();
        let error = rejected.await.unwrap().expect("a operação deveria ser rejeitada");
        assert!(is_cancellation_error(&error));

        assert_eq!(test.remaining(), 0);
    });
}

/// `new class extends NullLogService { override error(...) { logged.push([message, ...args].join(' ')) } }`.
struct ErrorCapturingLogService {
    inner: NullLogService,
    logged: Rc<RefCell<Vec<String>>>,
}

/// O texto de um elemento de `[message, ...args].join(' ')`.
fn log_text(message: &LogMessage) -> String {
    match message {
        LogMessage::Text(text) => text.clone(),
        LogMessage::Error(error) => error.to_string(),
    }
}

fn log_arg_text(arg: &LogArg) -> String {
    match arg {
        LogArg::Value(Dynamic::String(text)) => text.to_string_lossy(),
        LogArg::Value(other) => format!("{other:?}"),
        LogArg::Error(error) => error.to_string(),
    }
}

je_platform::delegate_logger!(ErrorCapturingLogService, inner, error_args: |this, message, args| {
    let parts: Vec<String> = std::iter::once(log_text(message)).chain(args.iter().map(log_arg_text)).collect();
    this.logged.borrow_mut().push(parts.join(" "));
});

#[test]
fn authenticated_failures_redact_credentials_before_logging_and_rejecting() {
    run(async {
        let logged = Rc::new(RefCell::new(Vec::new()));
        let log_service: LogServiceRef = Rc::new(ErrorCapturingLogService { inner: NullLogService, logged: logged.clone() });
        let error = create_pull_error("Authorization: Basic dummy-credential", "Trace2: dummy-credential");
        let test = harness_with(
            vec![expect(&["--version"]).stdout("git version 2.31.0\n"), expect(&["fetch"]).error(error)],
            log_service,
            Rc::new(native_process_environment),
        );

        let rejected = test
            .service
            .fetch("test-op", "/tmp/private", Some(&authentication(PRIVATE_URL, "Authorization: Basic dummy-credential")))
            .await
            .unwrap_err();

        let observed = (
            rejected.message.clone(),
            rejected.prop("stderr").map(str::to_owned),
            rejected.stack.as_deref().is_some_and(|stack| stack.contains("dummy-credential")),
            logged.borrow().join("\n").contains("dummy-credential"),
        );
        assert_eq!(observed, ("[redacted]".to_owned(), Some("Trace2: [redacted]".to_owned()), false, false));
    });
}

#[test]
fn get_remote_url_returns_the_origin_url() {
    run(async {
        let test = harness(vec![expect(&["remote", "get-url", "origin"]).stdout("https://github.com/test/private.git\n")]);

        assert_eq!(test.service.get_remote_url("test-op", "/tmp/private", None).await.unwrap(), "https://github.com/test/private.git");
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn pull_runs_ff_only_for_normal_updates() {
    run(async {
        let test = harness(vec![expect(&["rev-parse", "HEAD"]).stdout("aaaa\n"), expect(&["pull", "--ff-only"]), expect(&["rev-parse", "HEAD"]).stdout("bbbb\n")]);

        let changed = test.service.pull("test-op", "C:\\repo", None).await.unwrap();

        assert!(changed);
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn pull_skips_an_immutable_detached_checkout_when_requested() {
    run(async {
        let test = harness(vec![expect(&["rev-parse", "--abbrev-ref", "HEAD"]).stdout("HEAD\n")]);

        let changed = test.service.pull("test-op", "/tmp/repo", Some(&IGitPullOptions { skip_detached_head: Some(true), ..IGitPullOptions::default() })).await.unwrap();

        assert_eq!((changed, test.remaining()), (false, 0));
    });
}

#[test]
fn pull_still_updates_tracked_branches_when_detached_checkouts_are_skipped() {
    run(async {
        let test = harness(vec![
            expect(&["rev-parse", "--abbrev-ref", "HEAD"]).stdout("main\n"),
            expect(&["rev-parse", "HEAD"]).stdout("aaaa\n"),
            expect(&["pull", "--ff-only"]),
            expect(&["rev-parse", "HEAD"]).stdout("bbbb\n"),
        ]);

        let changed = test.service.pull("test-op", "/tmp/repo", Some(&IGitPullOptions { skip_detached_head: Some(true), ..IGitPullOptions::default() })).await.unwrap();

        assert_eq!((changed, test.remaining()), (true, 0));
    });
}

/// `{ allowHardResetOnDivergence: true }`.
fn allow_hard_reset() -> IGitPullOptions {
    IGitPullOptions { allow_hard_reset_on_divergence: Some(true), ..IGitPullOptions::default() }
}

#[test]
fn pull_recovers_from_diverged_history_by_resetting_to_upstream() {
    run(async {
        let test = harness(vec![
            expect(&["rev-parse", "HEAD"]).stdout("aaaa\n"),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
            expect(&["fetch", "--prune"]),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
            expect(&["status", "--porcelain"]).stdout(""),
            expect(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).stdout("origin/main\n"),
            expect(&["rev-list", "--count", "HEAD..@{u}"]).stdout("2\n"),
            expect(&["rev-list", "--count", "@{u}..HEAD"]).stdout("1\n"),
            expect(&["reset", "--hard", "origin/main"]),
            expect(&["rev-parse", "HEAD"]).stdout("bbbb\n"),
        ]);

        let changed = test.service.pull("test-op", "C:\\repo", Some(&allow_hard_reset())).await.unwrap();

        assert!(changed);
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn pull_rejects_hard_reset_recovery_when_working_tree_is_dirty() {
    run(async {
        let test = harness(vec![
            expect(&["rev-parse", "HEAD"]).stdout("aaaa\n"),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
            expect(&["fetch", "--prune"]),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
            expect(&["status", "--porcelain"]).stdout(" M package.json\n"),
        ]);

        let error = test.service.pull("test-op", "C:\\repo", Some(&allow_hard_reset())).await.unwrap_err();

        assert!(error.to_string().contains("Not possible to fast-forward"), "{error}");
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn pull_rethrows_non_fast_forward_errors_without_retrying() {
    run(async {
        let pull_error = create_pull_error("fatal: Failed to pull", "fatal: Authentication failed");
        let test = harness(vec![
            expect(&["rev-parse", "HEAD"]).stdout("aaaa\n"),
            expect(&["pull", "--ff-only"]).error(pull_error).stderr("fatal: Authentication failed"),
        ]);

        let error = test.service.pull("test-op", "C:\\repo", Some(&allow_hard_reset())).await.unwrap_err();

        assert!(error.to_string().contains("Failed to pull"), "{error}");
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn pull_rethrows_retry_failures_that_are_not_fast_forward_related() {
    run(async {
        let retry_error = create_pull_error("fatal: Failed to pull", "fatal: Authentication failed");
        let test = harness(vec![
            expect(&["rev-parse", "HEAD"]).stdout("aaaa\n"),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
            expect(&["fetch", "--prune"]),
            expect(&["pull", "--ff-only"]).error(retry_error).stderr("fatal: Authentication failed"),
        ]);

        let error = test.service.pull("test-op", "C:\\repo", Some(&allow_hard_reset())).await.unwrap_err();

        assert!(error.to_string().contains("Failed to pull"), "{error}");
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn pull_succeeds_on_second_ff_only_attempt_after_fetch() {
    run(async {
        let test = harness(vec![
            expect(&["rev-parse", "HEAD"]).stdout("aaaa\n"),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
            expect(&["fetch", "--prune"]),
            expect(&["pull", "--ff-only"]),
            expect(&["rev-parse", "HEAD"]).stdout("bbbb\n"),
        ]);

        let changed = test.service.pull("test-op", "C:\\repo", None).await.unwrap();

        assert!(changed);
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn pull_without_hard_reset_option_does_not_attempt_destructive_recovery() {
    run(async {
        let test = harness(vec![
            expect(&["rev-parse", "HEAD"]).stdout("aaaa\n"),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
            expect(&["fetch", "--prune"]),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
        ]);

        let error = test.service.pull("test-op", "C:\\repo", None).await.unwrap_err();

        assert!(error.to_string().contains("Not possible to fast-forward"), "{error}");
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn pull_rethrows_when_upstream_cannot_be_resolved_during_recovery() {
    run(async {
        let test = harness(vec![
            expect(&["rev-parse", "HEAD"]).stdout("aaaa\n"),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
            expect(&["fetch", "--prune"]),
            expect(&["pull", "--ff-only"]).error(create_diverged_pull_error()),
            expect(&["status", "--porcelain"]).stdout(""),
            expect(&["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"]).error(Error::new("no upstream configured")),
        ]);

        let error = test.service.pull("test-op", "C:\\repo", Some(&allow_hard_reset())).await.unwrap_err();

        assert!(error.to_string().contains("Not possible to fast-forward"), "{error}");
        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn checkout_commit_accepts_uppercase_sha_and_verifies_head() {
    run(async {
        let expected_commit = "AABBCCDDEEFF00112233445566778899AABBCCDD";
        let normalized_commit = expected_commit.to_lowercase();
        let test = harness(vec![
            expect(&["rev-parse", &format!("{normalized_commit}^{{commit}}")]).stdout(&format!("{normalized_commit}\n")),
            expect(&["checkout", "--detach", &normalized_commit]),
            expect(&["rev-parse", "HEAD"]).stdout(&format!("{normalized_commit}\n")),
        ]);

        test.service.checkout_commit("test-op", "C:\\repo", expected_commit).await.unwrap();

        assert_eq!(test.remaining(), 0);
    });
}

#[test]
fn checkout_commit_rejects_when_head_differs_after_checkout() {
    run(async {
        let expected_commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let checked_out_commit = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
        let test = harness(vec![
            expect(&["rev-parse", &format!("{expected_commit}^{{commit}}")]).stdout(&format!("{expected_commit}\n")),
            expect(&["checkout", "--detach", expected_commit]),
            expect(&["rev-parse", "HEAD"]).stdout(&format!("{checked_out_commit}\n")),
        ]);

        let error = test.service.checkout_commit("test-op", "C:\\repo", expected_commit).await.unwrap_err();

        assert!(error.message.contains("was not checked out"), "{error}");
        assert_eq!(test.remaining(), 0);
    });
}

/// `runGit(...args)`: `git -C repoPath ...args`, o `stdout` sem as pontas.
fn run_git(repo_path: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(repo_path).args(args).output().expect("o executável git");
    assert!(output.status.success(), "git {} falhou: {}", args.join(" "), String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).expect("saída em UTF-8").trim().to_owned()
}

#[test]
fn checkout_commit_rejects_a_real_sha_shaped_branch_that_points_to_another_commit() {
    run(async {
        let repo = temporary_directory("vscode-plugin-git-");
        let repo_path = repo.path();

        run_git(repo_path, &["init"]);
        fs::write(repo_path.join("payload.txt"), "branch content").unwrap();
        run_git(repo_path, &["add", "payload.txt"]);
        run_git(repo_path, &["-c", "user.name=VS Code Test", "-c", "user.email=vscode-test@example.com", "commit", "-m", "branch commit"]);
        let initial_commit = run_git(repo_path, &["rev-parse", "HEAD"]);
        let pinned_commit = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        run_git(repo_path, &["branch", pinned_commit]);

        let service = LocalGitService::new(null_log());
        assert!(service.checkout_commit("test-op", &path_text(repo_path), pinned_commit).await.is_err());

        assert_eq!(run_git(repo_path, &["rev-parse", "HEAD"]), initial_commit);
    });
}
