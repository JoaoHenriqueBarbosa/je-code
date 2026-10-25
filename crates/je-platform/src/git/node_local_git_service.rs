//! Porte de `src/vs/platform/git/node/localGitService.ts`: o serviço que executa comandos git na máquina local,
//! chamando o executável `git` por processo (sem biblioteca de git), como o original.
//!
//! Decisões de tradução:
//! - **`cp.execFile` é injetado.** O construtor do original recebe `_execFile: typeof cp.execFile` (o teste passa um falso).
//!   Aqui ele é o trait [`ExecFile`] (`execFile(file, args, { cwd, encoding: 'utf8', env }, callback)`), que devolve o filho
//!   como [`ChildProcess`] (só o `kill`). A implementação real, sobre `std::process`, é a
//!   [`ProcessExecFile`](super::process_exec_file::ProcessExecFile), usada por [`LocalGitService::new`]. O `callback` é
//!   chamado sempre depois de `exec_file` devolver, nunca de dentro dele (no Node é assíncrono; o serviço registra o filho
//!   em `_runningProcesses` só depois da volta).
//! - **`process.env` é injetado** (`process_environment`): o Rust não deixa o teste mexer no ambiente do processo sem
//!   `unsafe`, e os casos do original que fazem `Object.assign(process.env, ...)` passam o ambiente falso por
//!   [`LocalGitService::with_dependencies`]. O valor padrão é o ambiente nativo ([`native_process_environment`]).
//! - **Erros.** A exceção do `execFile` (`ExecFileException`, com `code` e `stderr`) é o [`Error`] do porte com as
//!   propriedades `code` (o texto do código de saída, ou `ENOENT`) e `stderr`; `gitError.stderr ??= stderr` só grava a
//!   propriedade se ela faltar. `error.stack` é o `stack` do [`Error`], quando existe.
//! - **`async`.** Os métodos devolvem [`Promise`] por [`async_fn`]: o corpo roda já até o primeiro `await`, como no
//!   original (o cancelamento do teste depende disso). O serviço é um punho barato de clonar (todos os campos são `Rc`),
//!   que cada corpo `async` captura como o `this` do original.
//! - **`rm(targetPath, { recursive, force, maxRetries: 3, retryDelay: 100 })`** é o `pfs::rm(.., RimRafMode::Unlink, ..)`, que
//!   faz as mesmas três tentativas com espera de 100 ms vezes a tentativa.
//! - **`Number(x)`** é o `string_to_number` do porte e `Number.parseInt(x, 10)` é [`parse_int`].

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use je_base::async_::async_fn;
use je_base::errors::Error;
use je_base::nls::localize;
use je_base::process::{ProcessEnvironment, SafeProcess};
use je_base::promise::Promise;
use je_base::uuid::generate_uuid;

use super::local_git_service::{IGitNetworkOptions, IGitPullOptions, ILocalGitService};
use super::process_exec_file::ProcessExecFile;
use crate::files::pfs::{self, RimRafMode};
use crate::js::string_to_number;
use crate::log::{LogArg, LogMessage, LogServiceRef, Logger};

/// `FULL_COMMIT_SHA_PATTERN = /^[0-9a-f]{40}$/i`.
fn is_full_commit_sha(text: &str) -> bool {
    text.len() == 40 && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// O `callback(err, stdout, stderr)` do `cp.execFile`.
pub type ExecFileCallback = Box<dyn FnOnce(Option<Error>, String, String)>;

/// O terceiro argumento do `cp.execFile`: `{ cwd, encoding: 'utf8', env }`. `env` ausente herda o ambiente do processo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExecFileOptions {
    pub cwd: Option<String>,
    pub env: Option<ProcessEnvironment>,
}

/// O `cp.ChildProcess` do Node, só com o que o serviço usa: o `kill()`.
pub trait ChildProcess {
    fn kill(&self);
}

/// O `cp.execFile` (`typeof cp.execFile`).
pub trait ExecFile {
    /// `execFile(command, args, options, callback)`. O `callback` não pode ser chamado antes de a função voltar.
    fn exec_file(&self, command: &str, args: &[String], options: &ExecFileOptions, callback: ExecFileCallback) -> Rc<dyn ChildProcess>;
}

/// `process.env`.
pub fn native_process_environment() -> ProcessEnvironment {
    SafeProcess::native().env
}

/// `Number.parseInt(text, 10)` quando o resultado é um inteiro `>= 0` (o `Number.isInteger(n) && n >= 0` do original);
/// `None` para `NaN` e para negativo.
fn parse_int(text: &str) -> Option<u64> {
    let text = text.trim_start();
    let (negative, digits) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let digits: String = digits.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() {
        return None;
    }
    let value = digits.parse::<u64>().unwrap_or(u64::MAX);
    if negative && value != 0 { None } else { Some(value) }
}

/// `/^http\..+\.extraheader$/i`.
fn is_extra_header_key(key: &str) -> bool {
    const PREFIX: &str = "http.";
    const SUFFIX: &str = ".extraheader";
    let lower = key.to_lowercase();
    lower.len() > PREFIX.len() + SUFFIX.len()
        && lower.starts_with(PREFIX)
        && lower.ends_with(SUFFIX)
        && !lower[PREFIX.len()..lower.len() - SUFFIX.len()].contains(['\n', '\r', '\u{2028}', '\u{2029}'])
}

/// `/^GIT_CONFIG_(?:KEY|VALUE)_\d+$/` sobre o nome já em maiúsculas.
fn is_indexed_config_name(name: &str) -> bool {
    ["GIT_CONFIG_KEY_", "GIT_CONFIG_VALUE_"].iter().any(|prefix| name.strip_prefix(prefix).is_some_and(|index| !index.is_empty() && index.bytes().all(|byte| byte.is_ascii_digit())))
}

/// `header.replace(/^Authorization:\s*\S+\s+/i, '')`.
fn strip_authorization_scheme(header: &str) -> String {
    const NAME: &str = "authorization:";
    let Some(rest) = header.get(NAME.len()..).filter(|_| header.get(..NAME.len()).is_some_and(|head| head.eq_ignore_ascii_case(NAME))) else {
        return header.to_owned();
    };
    let rest = rest.trim_start();
    let token_length = rest.find(char::is_whitespace).unwrap_or(rest.len());
    if token_length == 0 {
        return header.to_owned();
    }
    let after_token = &rest[token_length..];
    let remaining = after_token.trim_start();
    if remaining.len() == after_token.len() {
        return header.to_owned();
    }
    remaining.to_owned()
}

/// `/^git version (?<major>\d+)\.(?<minor>\d+)/` e `major > 2 || (major === 2 && minor >= 31)`: o Git 2.31 é o primeiro
/// que lê a configuração por variáveis de ambiente (`GIT_CONFIG_COUNT`).
fn supports_authentication_environment(version: &str) -> bool {
    let Some(rest) = version.strip_prefix("git version ") else {
        return false;
    };
    let major: String = rest.chars().take_while(char::is_ascii_digit).collect();
    let Some(after_major) = rest[major.len()..].strip_prefix('.') else {
        return false;
    };
    let minor: String = after_major.chars().take_while(char::is_ascii_digit).collect();
    if major.is_empty() || minor.is_empty() {
        return false;
    }
    let (major, minor) = (major.parse::<f64>().unwrap_or(f64::NAN), minor.parse::<f64>().unwrap_or(f64::NAN));
    major > 2.0 || (major == 2.0 && minor >= 31.0)
}

/// `error.<name> = value`.
fn set_prop(error: &mut Error, name: &str, value: String) {
    match error.props.iter_mut().find(|(prop, _)| prop == name) {
        Some(entry) => entry.1 = value,
        None => error.props.push((name.to_owned(), value)),
    }
}

/// `_isFastForwardPullFailure(err)`.
fn is_fast_forward_pull_failure(error: &Error) -> bool {
    if error.prop("code") != Some("128") {
        return false;
    }
    let details = format!("{}\n{}", error.prop("stderr").unwrap_or(""), error.message).to_lowercase();
    details.contains("not possible to fast-forward") || details.contains("non-fast-forward")
}

/// `Number(text.trim())`.
fn js_number(text: &str) -> f64 {
    let units: Vec<u16> = text.trim().encode_utf16().collect();
    string_to_number(&units)
}

/// `LocalGitService`. Clonar o serviço compartilha o estado (os processos em andamento e a versão do git já conferida).
#[derive(Clone)]
pub struct LocalGitService {
    log_service: LogServiceRef,
    exec_file: Rc<dyn ExecFile>,
    process_environment: Rc<dyn Fn() -> ProcessEnvironment>,
    running_processes: Rc<RefCell<HashMap<String, Rc<dyn ChildProcess>>>>,
    supports_authentication_environment: Rc<Cell<bool>>,
}

impl LocalGitService {
    /// `new LocalGitService(logService)`: roda o `git` do sistema.
    pub fn new(log_service: LogServiceRef) -> Self {
        Self::with_dependencies(log_service, Rc::new(ProcessExecFile), Rc::new(native_process_environment))
    }

    /// `new LocalGitService(logService, execFile)`, com o `process.env` também injetável.
    pub fn with_dependencies(log_service: LogServiceRef, exec_file: Rc<dyn ExecFile>, process_environment: Rc<dyn Fn() -> ProcessEnvironment>) -> Self {
        Self {
            log_service,
            exec_file,
            process_environment,
            running_processes: Rc::new(RefCell::new(HashMap::new())),
            supports_authentication_environment: Rc::new(Cell::new(false)),
        }
    }

    /// `_exec(operationId, args, cwd?, options?)`: o `stdout` do comando.
    fn exec(&self, operation_id: &str, args: &[&str], cwd: Option<&str>, options: Option<&IGitNetworkOptions>) -> Promise<String> {
        let this = self.clone();
        let operation_id = operation_id.to_owned();
        let args: Vec<String> = args.iter().map(|arg| (*arg).to_owned()).collect();
        let cwd = cwd.map(str::to_owned);
        let options = options.cloned();
        async_fn(async move {
            if options.as_ref().is_some_and(|options| options.authentication.is_some()) && !this.supports_authentication_environment.get() {
                let version = this.exec(&operation_id, &["--version"], None, None).await?;
                if !supports_authentication_environment(&version) {
                    return Err(Error::new(localize("gitAuthenticationRequiresNewerGit", "VS Code GitHub authentication requires Git 2.31 or later. Update Git and retry.", &[])));
                }
                this.supports_authentication_environment.set(true);
            }
            Promise::with_executor(|resolver| {
                let cwd_suffix = cwd.as_deref().filter(|cwd| !cwd.is_empty()).map_or_else(String::new, |cwd| format!(" (cwd: {cwd})"));
                this.log_service.trace(&format!("[LocalGitService] git {}{cwd_suffix}", args.join(" ")));
                let exec_options = ExecFileOptions { cwd: cwd.clone(), env: this.get_environment(options.as_ref()) };
                let callback: ExecFileCallback = {
                    let (this, operation_id, args, options) = (this.clone(), operation_id.clone(), args.clone(), options.clone());
                    Box::new(move |error, stdout, stderr| {
                        if this.running_processes.borrow_mut().remove(&operation_id).is_none() {
                            resolver.reject(Error::cancellation());
                            return;
                        }
                        let Some(mut git_error) = error else {
                            resolver.resolve(stdout);
                            return;
                        };
                        if git_error.prop("stderr").is_none() {
                            set_prop(&mut git_error, "stderr", stderr);
                        }
                        let header = options.as_ref().and_then(|options| options.authentication.as_ref()).map(|authentication| authentication.authorization_header.as_str());
                        if let Some(header) = header.filter(|header| !header.is_empty()) {
                            for secret in [header.to_owned(), strip_authorization_scheme(header)] {
                                if !secret.is_empty() {
                                    git_error.message = git_error.message.replace(&secret, "[redacted]");
                                    let redacted_stderr = git_error.prop("stderr").unwrap_or("").replace(&secret, "[redacted]");
                                    set_prop(&mut git_error, "stderr", redacted_stderr);
                                    git_error.stack = git_error.stack.take().map(|stack| stack.replace(&secret, "[redacted]"));
                                }
                            }
                        }
                        if options.as_ref().and_then(|options| options.log_errors) != Some(false) {
                            this.log_git_error(&args, &git_error);
                        }
                        resolver.reject(git_error);
                    })
                };
                let process = this.exec_file.exec_file("git", &args, &exec_options, callback);
                this.running_processes.borrow_mut().insert(operation_id.clone(), process);
            })
            .await
        })
    }

    /// `_logGitError(args, error)`.
    fn log_git_error(&self, args: &[String], error: &Error) {
        let message = format!("[LocalGitService] git {} failed:", args.first().map_or("undefined", String::as_str));
        self.log_service.error_args(&LogMessage::from(message), &[LogArg::from(error.message.as_str()), LogArg::from(error.prop("stderr").unwrap_or(""))]);
    }

    /// `_getEnvironment(options)`: o ambiente com a autenticação por cabeçalho HTTP, sem rastreamento do git que pudesse
    /// vazar a credencial, e com a configuração indexada herdada preservada (menos os `extraHeader`).
    fn get_environment(&self, options: Option<&IGitNetworkOptions>) -> Option<ProcessEnvironment> {
        let authentication = options?.authentication.as_ref()?;

        let mut environment = (self.process_environment)();
        let environment_names: HashMap<String, String> = environment.keys().map(|key| (key.to_uppercase(), key.clone())).collect();
        let configured_count = environment_names.get("GIT_CONFIG_COUNT").and_then(|name| parse_int(environment.get(name).map_or("", String::as_str)));
        let mut inherited_config: Vec<(String, String)> = Vec::new();
        for index in 0..configured_count.unwrap_or(0) {
            let key = environment_names.get(&format!("GIT_CONFIG_KEY_{index}")).and_then(|name| environment.get(name));
            let value = environment_names.get(&format!("GIT_CONFIG_VALUE_{index}")).and_then(|name| environment.get(name));
            if let (Some(key), Some(value)) = (key, value)
                && !is_extra_header_key(key)
            {
                inherited_config.push((key.clone(), value.clone()));
            }
        }
        environment.retain(|key, _| {
            let normalized = key.to_uppercase();
            !(normalized.starts_with("GIT_TRACE")
                || normalized == "GIT_CURL_VERBOSE"
                || normalized == "GIT_CONFIG_PARAMETERS"
                || normalized == "GIT_CONFIG_COUNT"
                || is_indexed_config_name(&normalized))
        });
        // Explicit values also override Trace2 targets configured outside the environment.
        for name in ["GIT_TRACE2", "GIT_TRACE2_EVENT", "GIT_TRACE2_PERF"] {
            environment.insert(name.to_owned(), "0".to_owned());
        }
        environment.insert("GIT_TRACE_REDACT".to_owned(), "1".to_owned());
        let index = inherited_config.len();
        for (position, (key, value)) in inherited_config.into_iter().enumerate() {
            environment.insert(format!("GIT_CONFIG_KEY_{position}"), key);
            environment.insert(format!("GIT_CONFIG_VALUE_{position}"), value);
        }
        environment.insert("GIT_CONFIG_COUNT".to_owned(), (index + 2).to_string());
        let header_key = format!("http.{}.extraHeader", authentication.url);
        environment.insert(format!("GIT_CONFIG_KEY_{index}"), header_key.clone());
        environment.insert(format!("GIT_CONFIG_VALUE_{index}"), String::new());
        environment.insert(format!("GIT_CONFIG_KEY_{}", index + 1), header_key);
        environment.insert(format!("GIT_CONFIG_VALUE_{}", index + 1), authentication.authorization_header.clone());
        Some(environment)
    }

    /// `_getSafeHardResetTarget(operationId, repoPath)`: o upstream para onde um `reset --hard` é seguro (árvore limpa e
    /// histórico divergido nos dois lados), ou `None`.
    fn get_safe_hard_reset_target(&self, operation_id: &str, repo_path: &str) -> Promise<Option<String>> {
        let this = self.clone();
        let (operation_id, repo_path) = (operation_id.to_owned(), repo_path.to_owned());
        async_fn(async move {
            let status = this.exec(&operation_id, &["status", "--porcelain"], Some(&repo_path), None).await?;
            if !status.trim().is_empty() {
                return Ok(None);
            }

            let upstream = match this.exec(&operation_id, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{u}"], Some(&repo_path), None).await {
                Ok(upstream) => upstream.trim().to_owned(),
                Err(_) => return Ok(None),
            };

            let behind = this.checked_rev_list_count(&operation_id, &repo_path, "HEAD", "@{u}").await?;
            let ahead = this.checked_rev_list_count(&operation_id, &repo_path, "@{u}", "HEAD").await?;
            Ok(match (ahead, behind) {
                (Some(ahead), Some(behind)) if ahead > 0.0 && behind > 0.0 => Some(upstream),
                _ => None,
            })
        })
    }

    /// `_revListCount(operationId, repoPath, fromRef, toRef)`: `None` (com aviso) se o resultado não é um número finito.
    fn checked_rev_list_count(&self, operation_id: &str, repo_path: &str, from_ref: &str, to_ref: &str) -> Promise<Option<f64>> {
        let this = self.clone();
        let (operation_id, repo_path, from_ref, to_ref) = (operation_id.to_owned(), repo_path.to_owned(), from_ref.to_owned(), to_ref.to_owned());
        async_fn(async move {
            let result = this.exec(&operation_id, &["rev-list", "--count", &format!("{from_ref}..{to_ref}")], Some(&repo_path), None).await?;
            let parsed = js_number(&result);
            if !parsed.is_finite() {
                this.log_service.warn(&format!("[LocalGitService] Failed to parse rev-list count for {from_ref}..{to_ref} in {repo_path}: {result}"));
                return Ok(None);
            }
            Ok(Some(parsed))
        })
    }
}

impl ILocalGitService for LocalGitService {
    fn clone_repository(&self, operation_id: &str, clone_url: &str, target_path: &str, git_ref: Option<&str>, options: Option<&IGitNetworkOptions>) -> Promise<()> {
        let this = self.clone();
        let (operation_id, clone_url, target_path) = (operation_id.to_owned(), clone_url.to_owned(), target_path.to_owned());
        let git_ref = git_ref.map(str::to_owned);
        let options = options.cloned();
        async_fn(async move {
            let mut args = vec!["clone"];
            let pinned_commit = git_ref.as_deref().map(|git_ref| git_ref.trim().to_lowercase()).filter(|normalized| is_full_commit_sha(normalized));
            if let Some(git_ref) = git_ref.as_deref().filter(|git_ref| !git_ref.is_empty())
                && pinned_commit.is_none()
            {
                args.extend(["--branch", git_ref]);
            }
            args.extend(["--", clone_url.as_str(), target_path.as_str()]);
            this.exec(&operation_id, &args, None, options.as_ref()).await?;
            if let Some(pinned_commit) = pinned_commit {
                let prepared: Result<(), Error> = async {
                    this.exec(&operation_id, &["fetch", "origin", &pinned_commit], Some(&target_path), options.as_ref()).await?;
                    this.checkout_commit(&operation_id, &target_path, &pinned_commit).await
                }
                .await;
                if let Err(error) = prepared {
                    if let Err(cleanup_error) = pfs::rm(&target_path, RimRafMode::Unlink, None).await {
                        return Err(Error::aggregate(
                            vec![error, cleanup_error.into()],
                            localize("pluginsPinnedCloneCleanupFailed", "Failed to prepare and clean up the plugin repository at '{0}'.", &[&target_path]),
                        ));
                    }
                    return Err(error);
                }
            }
            Ok(())
        })
    }

    fn pull(&self, operation_id: &str, repo_path: &str, options: Option<&IGitPullOptions>) -> Promise<bool> {
        let this = self.clone();
        let (operation_id, repo_path) = (operation_id.to_owned(), repo_path.to_owned());
        let options = options.cloned();
        async_fn(async move {
            let network = options.as_ref().map(|options| &options.network);
            if options.as_ref().and_then(|options| options.skip_detached_head) == Some(true)
                && this.exec(&operation_id, &["rev-parse", "--abbrev-ref", "HEAD"], Some(&repo_path), None).await?.trim() == "HEAD"
            {
                return Ok(false);
            }
            let before = this.exec(&operation_id, &["rev-parse", "HEAD"], Some(&repo_path), None).await?.trim().to_owned();

            if let Err(error) = this.exec(&operation_id, &["pull", "--ff-only"], Some(&repo_path), network).await {
                if !is_fast_forward_pull_failure(&error) {
                    return Err(error);
                }

                this.log_service.warn(&format!("[LocalGitService] Fast-forward pull failed for {repo_path}: {}. Retrying after fetch.", error.message));
                this.exec(&operation_id, &["fetch", "--prune"], Some(&repo_path), network).await?;

                if let Err(retry_error) = this.exec(&operation_id, &["pull", "--ff-only"], Some(&repo_path), network).await {
                    if !is_fast_forward_pull_failure(&retry_error) {
                        return Err(retry_error);
                    }

                    if options.as_ref().and_then(|options| options.allow_hard_reset_on_divergence) != Some(true) {
                        return Err(retry_error);
                    }

                    let Some(upstream) = this.get_safe_hard_reset_target(&operation_id, &repo_path).await? else {
                        return Err(retry_error);
                    };

                    this.log_service.warn(&format!("[LocalGitService] Pull retries exhausted for {repo_path}. Performing hard reset to {upstream}."));
                    this.exec(&operation_id, &["reset", "--hard", &upstream], Some(&repo_path), None).await?;
                }
            }

            let after = this.exec(&operation_id, &["rev-parse", "HEAD"], Some(&repo_path), None).await?.trim().to_owned();
            Ok(before != after)
        })
    }

    fn checkout(&self, operation_id: &str, repo_path: &str, treeish: &str, detached: Option<bool>) -> Promise<()> {
        let this = self.clone();
        let (operation_id, repo_path, treeish) = (operation_id.to_owned(), repo_path.to_owned(), treeish.to_owned());
        async_fn(async move {
            let mut args = vec!["checkout"];
            if detached == Some(true) {
                args.push("--detach");
            }
            args.push(&treeish);
            this.exec(&operation_id, &args, Some(&repo_path), None).await?;
            Ok(())
        })
    }

    fn checkout_commit(&self, operation_id: &str, repo_path: &str, commit: &str) -> Promise<()> {
        let this = self.clone();
        let (operation_id, repo_path, commit) = (operation_id.to_owned(), repo_path.to_owned(), commit.to_owned());
        async_fn(async move {
            let expected_commit = commit.trim().to_lowercase();
            if !is_full_commit_sha(&expected_commit) {
                return Err(Error::new(localize("pluginsInvalidPinnedCommit", "Pinned plugin commit '{0}' is not a full SHA-1 hash.", &[&commit])));
            }

            let resolved_commit = this.exec(&operation_id, &["rev-parse", &format!("{expected_commit}^{{commit}}")], Some(&repo_path), None).await?.trim().to_lowercase();
            if resolved_commit != expected_commit {
                return Err(Error::new(localize(
                    "pluginsPinnedCommitResolutionMismatch",
                    "Pinned plugin commit '{0}' resolved to a different commit '{1}'.",
                    &[&commit, &resolved_commit],
                )));
            }

            this.exec(&operation_id, &["checkout", "--detach", &resolved_commit], Some(&repo_path), None).await?;
            let checked_out_commit = this.exec(&operation_id, &["rev-parse", "HEAD"], Some(&repo_path), None).await?.trim().to_lowercase();
            if checked_out_commit != expected_commit {
                return Err(Error::new(localize(
                    "pluginsPinnedCommitCheckoutMismatch",
                    "Pinned plugin commit '{0}' was not checked out. The repository is at commit '{1}'.",
                    &[&commit, &checked_out_commit],
                )));
            }
            Ok(())
        })
    }

    fn rev_parse(&self, repo_path: &str, git_ref: &str) -> Promise<String> {
        let this = self.clone();
        let (repo_path, git_ref) = (repo_path.to_owned(), git_ref.to_owned());
        async_fn(async move { Ok(this.exec(&generate_uuid(), &["rev-parse", &git_ref], Some(&repo_path), None).await?.trim().to_owned()) })
    }

    fn get_remote_url(&self, operation_id: &str, repo_path: &str, options: Option<&IGitNetworkOptions>) -> Promise<String> {
        let this = self.clone();
        let (operation_id, repo_path) = (operation_id.to_owned(), repo_path.to_owned());
        let options = options.cloned();
        async_fn(async move { Ok(this.exec(&operation_id, &["remote", "get-url", "origin"], Some(&repo_path), options.as_ref()).await?.trim().to_owned()) })
    }

    fn fetch(&self, operation_id: &str, repo_path: &str, options: Option<&IGitNetworkOptions>) -> Promise<()> {
        let this = self.clone();
        let (operation_id, repo_path) = (operation_id.to_owned(), repo_path.to_owned());
        let options = options.cloned();
        async_fn(async move {
            this.exec(&operation_id, &["fetch"], Some(&repo_path), options.as_ref()).await?;
            Ok(())
        })
    }

    fn rev_list_count(&self, repo_path: &str, from_ref: &str, to_ref: &str) -> Promise<f64> {
        let this = self.clone();
        let (repo_path, range) = (repo_path.to_owned(), format!("{from_ref}..{to_ref}"));
        async_fn(async move {
            let result = this.exec(&generate_uuid(), &["rev-list", "--count", &range], Some(&repo_path), None).await?;
            // `Number(result.trim()) || 0`
            let count = js_number(&result);
            Ok(if count.is_nan() { 0.0 } else { count })
        })
    }

    fn cancel(&self, operation_id: &str) -> Promise<()> {
        let process = self.running_processes.borrow_mut().remove(operation_id);
        if let Some(process) = process {
            process.kill();
        }
        Promise::resolved(())
    }
}
