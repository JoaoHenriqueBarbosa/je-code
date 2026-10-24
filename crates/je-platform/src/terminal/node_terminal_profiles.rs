//! Porte de `src/vs/platform/terminal/node/terminalProfiles.ts`: a detecção dos perfis de terminal disponíveis na
//! máquina (`detectAvailableProfiles`): os perfis automáticos (PowerShell, Git Bash, Cygwin, MSYS2, Cmder e as
//! distribuições do WSL no Windows; os shells de `/etc/shells` nos outros sistemas) mesclados com os perfis da
//! configuração, cada caminho validado contra o sistema de arquivos. O `terminal_profiles` irmão é o de `common/`.
//!
//! Divergências:
//! - `IUnresolvedTerminalProfile` (`ITerminalExecutable | ITerminalProfileSource | null`) é o enum tipado de
//!   [`super::terminal`]: a configuração chega como [`Dynamic`] e [`parse_unresolved_profile`] a converte; o `null`
//!   (e todo valor que não é objeto, ou que não tem `path` nem `source`) é o `None`, que o
//!   `applyConfigProfilesToMap` trata como "remova o perfil". O `value.icon = value.icon || ...` do original muta o
//!   objeto da configuração; aqui a mutação é na cópia tipada.
//! - O `Map<string, IUnresolvedTerminalProfile>` é um `IndexMap` (a ordem de inserção do `Map` do JavaScript; trocar o
//!   valor de uma chave existente mantém a posição, `delete` remove).
//! - `fsProvider?: IFsProvider` é o trait [`IFsProvider`] (métodos que devolvem futuros locais). Os métodos não
//!   emprestam o `self` no futuro devolvido, para o `findExecutable` poder receber o `existsFile` como função. O
//!   padrão é o [`DefaultFsProvider`] (`pfs.SymlinkSupport.existsFile` e `fs.promises.readFile`). O `Buffer` é
//!   `Vec<u8>`.
//! - `shellEnv: typeof process.env = process.env` é um [`ProcessEnvironment`] obrigatório (o chamador passa a cópia do
//!   ambiente do processo, [`je_base::process::SafeProcess::native`]).
//! - `variableResolver` é uma função que devolve um futuro local ([`VariableResolver`]).
//! - O estado do módulo (`profileSources`, `logIfWslNotInstalled`) é por thread (`profileSources`) e do processo
//!   (`logIfWslNotInstalled`): os testes rodam em threads separadas e cada um precisa do seu `profileSources`.
//! - O `Promise.all` de `transformToTerminalProfiles` valida os perfis em sequência (o futuro do porte nunca cede
//!   a execução, e a validação de um perfil não depende da de outro). A recursão de `validateProfilePaths` é um
//!   laço, com os argumentos que cada chamada recursiva do original deixa de repassar (`env`, `overrideName`,
//!   `isAutoDetected`, `requiresUnsafePath`) zerados nos mesmos ramos.
//! - `cp.exec('wsl.exe -l -q', { encoding: 'utf16le', timeout: 1000 })` é o `Command` do Rust com um laço de espera
//!   de mil milissegundos que mata o processo; a saída é decodificada como UTF-16 little endian.
//! - Os ids dos codicons (`Codicon.terminalPowershell` e companhia) são os ids do `je-codicons` (o `je-platform`
//!   não depende dele): `terminal-powershell`, `terminal-git-bash`, `terminal-cmd`, `terminal-bash`,
//!   `terminal-ubuntu`, `terminal-debian` e `terminal-linux`.
//! - `process.env['x']` num template (`${process.env['HOMEDRIVE']}`) vira o texto `undefined` quando a variável
//!   não existe, como no JavaScript.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::io::Read;
use std::process::{Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use indexmap::IndexMap;
use je_base::JsString;
use je_base::arrays::OneOrMany;
use je_base::dynamic::{Dynamic, Truthy};
use je_base::errors::Error;
use je_base::node::pfs::{self, LocalFuture, SymlinkSupport};
use je_base::node::powershell::enumerate_power_shell_installations;
use je_base::node::processes::{FileExists, find_executable};
use je_base::node::windows_version::get_windows_build_number_async;
use je_base::path::{PathModule, ProcessEnvironment as HostEnvironment};
use je_base::platform::Platform;
use je_base::process::{ProcessEnvironment, env_var};
use je_base::themables::{ThemeColor, ThemeIcon};
use je_base::uri::UriComponents;

use super::terminal::{
    ContributedIcon, IBaseUnresolvedTerminalProfile, ITerminalEnvironment, ITerminalExecutable, ITerminalProfile, ITerminalProfileSource,
    IUnresolvedTerminalProfile, LightDarkIcon, ProfilePath, ProfileSource, TerminalEnvironmentValue, TerminalIcon, UriLike,
};
use super::terminal_settings::TerminalSettingId;
use crate::configuration::service::IConfigurationService;
use crate::log::{LogArg, LogService};

/// `Constants.UnixShellsPath`.
const UNIX_SHELLS_PATH: &str = "/etc/shells";

/// `Codicon.terminalPowershell.id` e companhia.
const CODICON_TERMINAL_POWERSHELL: &str = "terminal-powershell";
const CODICON_TERMINAL_GIT_BASH: &str = "terminal-git-bash";
const CODICON_TERMINAL_CMD: &str = "terminal-cmd";
const CODICON_TERMINAL_BASH: &str = "terminal-bash";
const CODICON_TERMINAL_UBUNTU: &str = "terminal-ubuntu";
const CODICON_TERMINAL_DEBIAN: &str = "terminal-debian";
const CODICON_TERMINAL_LINUX: &str = "terminal-linux";

/// `IFsProvider`.
pub trait IFsProvider {
    fn exists_file<'a>(&self, path: &'a str) -> LocalFuture<'a, bool>;
    fn read_file<'a>(&self, path: &'a str) -> LocalFuture<'a, Result<Vec<u8>, Error>>;
}

/// O `fsProvider` padrão: `{ existsFile: pfs.SymlinkSupport.existsFile, readFile: fs.promises.readFile }`.
pub struct DefaultFsProvider;

impl IFsProvider for DefaultFsProvider {
    fn exists_file<'a>(&self, path: &'a str) -> LocalFuture<'a, bool> {
        Box::pin(SymlinkSupport::exists_file(path))
    }

    fn read_file<'a>(&self, path: &'a str) -> LocalFuture<'a, Result<Vec<u8>, Error>> {
        Box::pin(pfs::read_file(path))
    }
}

/// O `variableResolver?: (text: string[]) => Promise<string[]>`.
pub type VariableResolver<'a> = &'a dyn Fn(Vec<String>) -> LocalFuture<'static, Vec<String>>;

/// `IPotentialTerminalProfile`: o que uma fonte (`ProfileSource`) resolve (o `profileName` do original nunca é lido).
struct PotentialTerminalProfile {
    paths: Vec<String>,
    args: Option<Vec<String>>,
    icon: Option<TerminalIcon>,
}

type ProfileSources = HashMap<String, PotentialTerminalProfile>;

thread_local! {
    /// `let profileSources: Map<string, IPotentialTerminalProfile> | undefined`.
    static PROFILE_SOURCES: RefCell<Option<Rc<ProfileSources>>> = const { RefCell::new(None) };
}

/// `let logIfWslNotInstalled: boolean = true`.
static LOG_IF_WSL_NOT_INSTALLED: AtomicBool = AtomicBool::new(true);

fn path_module() -> PathModule<'static> {
    static HOST: HostEnvironment = HostEnvironment;
    PathModule::native(&HOST)
}

fn codicon(id: &str) -> TerminalIcon {
    TerminalIcon::Theme(ThemeIcon::from_id(id))
}

/// `process.env['name']` interpolado num template: `undefined` quando a variável não existe.
fn js_env(name: &str) -> String {
    env_var(name).unwrap_or_else(|| "undefined".to_owned())
}

pub async fn detect_available_profiles(
    profiles: Option<&Dynamic>,
    default_profile: Option<&Dynamic>,
    include_detected_profiles: bool,
    configuration_service: &dyn IConfigurationService,
    shell_env: &ProcessEnvironment,
    fs_provider: Option<&dyn IFsProvider>,
    log_service: Option<&dyn LogService>,
    variable_resolver: Option<VariableResolver<'_>>,
    test_pwsh_source_paths: Option<&[String]>,
) -> Vec<ITerminalProfile> {
    let default_fs_provider = DefaultFsProvider;
    let fs_provider: &dyn IFsProvider = fs_provider.unwrap_or(&default_fs_provider);
    let platform = Platform::detect();

    // `profiles && isObject(profiles) ? { ...profiles } : configurationService.getValue(...)`
    let config_profiles = |setting: TerminalSettingId| match profiles {
        Some(profiles @ Dynamic::Object(_)) => profiles.clone(),
        _ => configuration_service.get_value(Some(setting.as_str()), None),
    };
    // `isString(defaultProfile) ? defaultProfile : configurationService.getValue<string>(...)`
    let default_profile_name = |setting: TerminalSettingId| match default_profile {
        Some(Dynamic::String(name)) => Some(name.to_string_lossy()),
        _ => match configuration_service.get_value(Some(setting.as_str()), None) {
            Dynamic::String(name) => Some(name.to_string_lossy()),
            _ => None,
        },
    };

    if platform.is_windows {
        return detect_available_windows_profiles(
            include_detected_profiles,
            fs_provider,
            shell_env,
            log_service,
            !matches!(configuration_service.get_value(Some(TerminalSettingId::UseWslProfiles.as_str()), None), Dynamic::Bool(false)),
            &config_profiles(TerminalSettingId::ProfilesWindows),
            default_profile_name(TerminalSettingId::DefaultProfileWindows),
            test_pwsh_source_paths,
            variable_resolver,
        )
        .await;
    }
    let (profiles_setting, default_profile_setting) = if platform.is_linux {
        (TerminalSettingId::ProfilesLinux, TerminalSettingId::DefaultProfileLinux)
    } else {
        (TerminalSettingId::ProfilesMacOs, TerminalSettingId::DefaultProfileMacOs)
    };
    detect_available_unix_profiles(
        fs_provider,
        log_service,
        include_detected_profiles,
        &config_profiles(profiles_setting),
        default_profile_name(default_profile_setting),
        test_pwsh_source_paths,
        variable_resolver,
        shell_env,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn detect_available_windows_profiles(
    include_detected_profiles: bool,
    fs_provider: &dyn IFsProvider,
    shell_env: &ProcessEnvironment,
    log_service: Option<&dyn LogService>,
    use_wsl_profiles: bool,
    config_profiles: &Dynamic,
    default_profile_name: Option<String>,
    test_pwsh_source_paths: Option<&[String]>,
    variable_resolver: Option<VariableResolver<'_>>,
) -> Vec<ITerminalProfile> {
    // Determine the correct System32 path. We want to point to Sysnative
    // when the 32-bit version of VS Code is running on a 64-bit machine.
    // The reason for this is because PowerShell's important PSReadline
    // module doesn't work if this is not the case. See #27915.
    let is_32_process_on_64_windows = env_var("PROCESSOR_ARCHITEW6432").is_some();
    let system32_path = format!("{}\\{}", js_env("windir"), if is_32_process_on_64_windows { "Sysnative" } else { "System32" });

    // WSL 2 released in the May 2020 Update, this is where the `-d` flag was added that we depend
    // upon
    let allow_wsl_discovery = get_windows_build_number_async().await >= 19041;

    initialize_windows_profiles(test_pwsh_source_paths).await;

    let mut detected_profiles: IndexMap<String, IUnresolvedTerminalProfile> = IndexMap::new();

    // Add auto detected profiles
    if include_detected_profiles {
        let auto_detected = |icon: Option<&str>| IBaseUnresolvedTerminalProfile {
            icon: icon.map(|id| ContributedIcon::Icon(codicon(id))),
            is_auto_detected: Some(true),
            ..Default::default()
        };
        let source = |source: ProfileSource, base: IBaseUnresolvedTerminalProfile| IUnresolvedTerminalProfile::Source(ITerminalProfileSource { base, source: source.as_str().to_owned() });
        let executable = |path: Vec<ProfilePath>, base: IBaseUnresolvedTerminalProfile| IUnresolvedTerminalProfile::Executable(ITerminalExecutable { base, path });
        let plain = |path: String| vec![ProfilePath::Plain(path)];
        let many = |args: &[&str]| Some(OneOrMany::Many(args.iter().map(|arg| (*arg).to_owned()).collect()));
        let home_drive = js_env("HOMEDRIVE");

        detected_profiles.insert("PowerShell".to_owned(), source(ProfileSource::Pwsh, auto_detected(Some(CODICON_TERMINAL_POWERSHELL))));
        detected_profiles.insert(
            "Windows PowerShell".to_owned(),
            executable(plain(format!("{system32_path}\\WindowsPowerShell\\v1.0\\powershell.exe")), auto_detected(Some(CODICON_TERMINAL_POWERSHELL))),
        );
        detected_profiles.insert("Git Bash".to_owned(), source(ProfileSource::GitBash, auto_detected(Some(CODICON_TERMINAL_GIT_BASH))));
        detected_profiles.insert("Command Prompt".to_owned(), executable(plain(format!("{system32_path}\\cmd.exe")), auto_detected(Some(CODICON_TERMINAL_CMD))));
        detected_profiles.insert(
            "Cygwin".to_owned(),
            executable(
                vec![ProfilePath::unsafe_path(format!("{home_drive}\\cygwin64\\bin\\bash.exe")), ProfilePath::unsafe_path(format!("{home_drive}\\cygwin\\bin\\bash.exe"))],
                IBaseUnresolvedTerminalProfile { args: many(&["--login"]), ..auto_detected(None) },
            ),
        );
        detected_profiles.insert(
            "bash (MSYS2)".to_owned(),
            executable(
                vec![ProfilePath::unsafe_path(format!("{home_drive}\\msys64\\usr\\bin\\bash.exe"))],
                IBaseUnresolvedTerminalProfile {
                    args: many(&["--login", "-i"]),
                    // CHERE_INVOKING retains current working directory
                    env: Some(vec![("CHERE_INVOKING".to_owned(), TerminalEnvironmentValue::Value("1".to_owned()))]),
                    ..auto_detected(Some(CODICON_TERMINAL_BASH))
                },
            ),
        );
        let cmder_root = env_var("CMDER_ROOT").filter(|root| !root.is_empty());
        let cmder_path = format!("{}\\vendor\\bin\\vscode_init.cmd", cmder_root.clone().unwrap_or_else(|| format!("{home_drive}\\cmder")));
        detected_profiles.insert(
            "Cmder".to_owned(),
            executable(
                plain(format!("{system32_path}\\cmd.exe")),
                IBaseUnresolvedTerminalProfile {
                    args: many(&["/K", &cmder_path]),
                    // The path is safe if it was derived from CMDER_ROOT
                    requires_path: Some(if cmder_root.is_some() { ProfilePath::Plain(cmder_path.clone()) } else { ProfilePath::unsafe_path(cmder_path.clone()) }),
                    ..auto_detected(None)
                },
            ),
        );
    }

    apply_config_profiles_to_map(config_profiles, &mut detected_profiles);

    let detector = ProfileDetector {
        fs_provider,
        shell_env,
        log_service,
        variable_resolver,
        default_profile_name: default_profile_name.clone(),
        profile_sources: PROFILE_SOURCES.with(|sources| sources.borrow().clone()),
    };
    let mut result_profiles: Vec<ITerminalProfile> = detector.transform_to_terminal_profiles(&detected_profiles).await;

    if include_detected_profiles && use_wsl_profiles && allow_wsl_discovery {
        match get_wsl_profiles(&format!("{system32_path}\\wsl.exe"), default_profile_name.as_deref()).await {
            Ok(result) => {
                for wsl_profile in result {
                    if !has_own_property(config_profiles, &wsl_profile.profile_name) {
                        result_profiles.push(wsl_profile);
                    }
                }
            }
            Err(_) => {
                if LOG_IF_WSL_NOT_INSTALLED.swap(false, Ordering::SeqCst) {
                    if let Some(log_service) = log_service {
                        log_service.trace("WSL is not installed, so could not detect WSL profiles");
                    }
                }
            }
        }
    }

    result_profiles
}

/// `!configProfiles || !Object.prototype.hasOwnProperty.call(configProfiles, name)`, o avesso: o objeto da
/// configuração tem a chave `name` própria.
fn has_own_property(config_profiles: &Dynamic, name: &str) -> bool {
    !config_profiles.is_nullish() && matches!(config_profiles, Dynamic::Object(_)) && config_profiles.has_property(&JsString::from(name))
}

/// O contexto que `transformToTerminalProfiles`, `getValidatedProfile` e `validateProfilePaths` repassam de uma para a
/// outra no original (cada uma recebia os mesmos argumentos).
struct ProfileDetector<'a> {
    fs_provider: &'a dyn IFsProvider,
    shell_env: &'a ProcessEnvironment,
    log_service: Option<&'a dyn LogService>,
    variable_resolver: Option<VariableResolver<'a>>,
    default_profile_name: Option<String>,
    profile_sources: Option<Rc<ProfileSources>>,
}

/// O `existsFile` do provedor como a função que o `findExecutable` recebe.
fn exists_via(fs_provider: &dyn IFsProvider) -> impl for<'p> Fn(&'p str) -> LocalFuture<'p, bool> + '_ {
    move |path| fs_provider.exists_file(path)
}

/// `icon ? icon : ...` sobre o ícone contribuído: o texto vazio é falso.
fn icon_is_truthy(icon: Option<&ContributedIcon>) -> bool {
    match icon {
        Some(ContributedIcon::Text(text)) => !text.is_empty(),
        Some(ContributedIcon::Icon(_)) => true,
        None => false,
    }
}

/// `args` truthy: a string vazia é falsa, o array vazio é verdadeiro.
fn args_are_truthy(args: &Option<OneOrMany<String>>) -> bool {
    match args {
        None => false,
        Some(OneOrMany::One(args)) => !args.is_empty(),
        Some(OneOrMany::Many(_)) => true,
    }
}

/// `validateIcon(icon)`.
fn validate_icon(icon: Option<&ContributedIcon>) -> Option<TerminalIcon> {
    match icon {
        // `isString(icon)`: `{ id: icon }`.
        Some(ContributedIcon::Text(id)) => Some(TerminalIcon::Theme(ThemeIcon::from_id(id.as_str()))),
        Some(ContributedIcon::Icon(icon)) => Some(icon.clone()),
        None => None,
    }
}

impl ProfileDetector<'_> {
    async fn transform_to_terminal_profiles(&self, entries: &IndexMap<String, IUnresolvedTerminalProfile>) -> Vec<ITerminalProfile> {
        let mut profiles = Vec::new();
        for (profile_name, profile) in entries {
            profiles.extend(self.get_validated_profile(profile_name, profile).await);
        }
        profiles
    }

    async fn get_validated_profile(&self, profile_name: &str, profile: &IUnresolvedTerminalProfile) -> Option<ITerminalProfile> {
        let is_windows = Platform::detect().is_windows;
        let base = profile.base();
        let original_paths: Vec<ProfilePath>;
        let args: Option<OneOrMany<String>>;
        let icon: Option<TerminalIcon>;
        // use calculated values if path is not specified
        match profile {
            IUnresolvedTerminalProfile::Source(profile) => {
                let source = self.profile_sources.as_ref().and_then(|sources| sources.get(&profile.source))?;
                original_paths = source.paths.iter().cloned().map(ProfilePath::Plain).collect();

                // if there are configured args, override the default ones
                args = if args_are_truthy(&base.args) { base.args.clone() } else { source.args.clone().map(OneOrMany::Many) };
                icon = if icon_is_truthy(base.icon.as_ref()) { validate_icon(base.icon.as_ref()) } else { source.icon.clone() };
            }
            IUnresolvedTerminalProfile::Executable(profile) => {
                original_paths = profile.path.clone();
                args = if is_windows {
                    base.args.clone()
                } else {
                    match &base.args {
                        Some(OneOrMany::Many(_)) => base.args.clone(),
                        _ => None,
                    }
                };
                icon = validate_icon(base.icon.as_ref());
            }
        }

        let paths: Vec<ProfilePath> = match self.variable_resolver {
            Some(variable_resolver) => {
                // Convert to string[] for resolve
                let mapped: Vec<String> = original_paths.iter().map(|path| path.path().to_owned()).collect();

                let resolved = variable_resolver(mapped).await;
                // Convert resolved back to (T | string)[]
                original_paths
                    .iter()
                    .enumerate()
                    .map(|(index, original)| {
                        let resolved_path = resolved.get(index).cloned().unwrap_or_default();
                        match original {
                            ProfilePath::Plain(_) => ProfilePath::Plain(resolved_path),
                            ProfilePath::Unsafe(_) => ProfilePath::unsafe_path(resolved_path),
                        }
                    })
                    .collect()
            }
            None => original_paths.clone(),
        };

        let mut requires_unsafe_path: Option<String> = None;
        if let Some(requires_path) = &base.requires_path {
            // `if (profile.requiresPath)`: a string vazia é falsa.
            if !matches!(requires_path, ProfilePath::Plain(path) if path.is_empty()) {
                // Validate requiresPath exists
                let actual_required_path = requires_path.path().to_owned();
                if let ProfilePath::Unsafe(unsafe_path) = requires_path {
                    if unsafe_path.is_unsafe {
                        requires_unsafe_path = Some(actual_required_path.clone());
                    }
                }
                if !self.fs_provider.exists_file(&actual_required_path).await {
                    return None;
                }
            }
        }

        let validated_profile = self
            .validate_profile_paths(profile_name, paths, &args, base.env.clone(), base.override_name, base.is_auto_detected, requires_unsafe_path)
            .await;
        let Some(mut validated_profile) = validated_profile else {
            if let Some(log_service) = self.log_service {
                let original: Vec<Dynamic> = original_paths
                    .iter()
                    .map(|path| match path {
                        ProfilePath::Plain(path) => Dynamic::string(path),
                        ProfilePath::Unsafe(unsafe_path) => Dynamic::object([("path", Dynamic::string(&unsafe_path.path)), ("isUnsafe", Dynamic::Bool(unsafe_path.is_unsafe))]),
                    })
                    .collect();
                log_service.debug_args("Terminal profile not validated", &[LogArg::from(profile_name), LogArg::Value(Dynamic::array(original))]);
            }
            return None;
        };

        validated_profile.is_auto_detected = base.is_auto_detected;
        validated_profile.icon = icon;
        validated_profile.color = base.color.clone();
        Some(validated_profile)
    }

    #[allow(clippy::too_many_arguments)]
    async fn validate_profile_paths(
        &self,
        profile_name: &str,
        potential_paths: Vec<ProfilePath>,
        args: &Option<OneOrMany<String>>,
        env: Option<ITerminalEnvironment>,
        override_name: Option<bool>,
        is_auto_detected: Option<bool>,
        requires_unsafe_path: Option<String>,
    ) -> Option<ITerminalProfile> {
        let path_module = path_module();
        let mut potential_paths: VecDeque<ProfilePath> = potential_paths.into();
        let (mut env, mut override_name, mut is_auto_detected, mut requires_unsafe_path) = (env, override_name, is_auto_detected, requires_unsafe_path);
        loop {
            // `potentialPaths.length === 0`
            let path = potential_paths.pop_front()?;
            if matches!(&path, ProfilePath::Plain(path) if path.is_empty()) {
                requires_unsafe_path = None;
                continue;
            }
            let is_unsafe_path = matches!(&path, ProfilePath::Unsafe(unsafe_path) if unsafe_path.is_unsafe);
            let actual_path = path.path().to_owned();

            let mut profile = ITerminalProfile {
                profile_name: profile_name.to_owned(),
                path: actual_path.clone(),
                args: args.clone(),
                env: env.clone(),
                override_name,
                is_auto_detected,
                is_default: self.default_profile_name.as_deref() == Some(profile_name),
                is_unsafe_path: Some(is_unsafe_path),
                requires_unsafe_path: requires_unsafe_path.clone(),
                ..Default::default()
            };

            // For non-absolute paths, check if it's available on $PATH
            if path_module.basename(&actual_path, None) == actual_path {
                // The executable isn't an absolute path, try find it on the PATH
                let env_paths: Option<Vec<String>> =
                    self.shell_env.get("PATH").filter(|path| !path.is_empty()).map(|path| path.split(path_module.delimiter()).map(str::to_owned).collect());
                let exists = exists_via(self.fs_provider);
                let executable = find_executable(&actual_path, None, env_paths.as_deref(), None, Some(&exists)).await;
                let Some(executable) = executable else {
                    (env, override_name, is_auto_detected, requires_unsafe_path) = (None, None, None, None);
                    continue;
                };
                profile.path = executable;
                profile.is_from_path = Some(true);
                return Some(profile);
            }

            if self.fs_provider.exists_file(&path_module.normalize(&actual_path)).await {
                return Some(profile);
            }

            requires_unsafe_path = None;
        }
    }
}

/// `initializeWindowsProfiles(testPwshSourcePaths?)`.
async fn initialize_windows_profiles(test_pwsh_source_paths: Option<&[String]>) {
    if PROFILE_SOURCES.with(|sources| sources.borrow().is_some()) && test_pwsh_source_paths.is_none() {
        return;
    }

    let git_bash_paths = get_git_bash_paths().await;
    let pwsh_paths = match test_pwsh_source_paths {
        Some(paths) => paths.to_vec(),
        None => get_powershell_paths().await,
    };

    let mut sources = ProfileSources::new();
    sources.insert(
        ProfileSource::GitBash.as_str().to_owned(),
        PotentialTerminalProfile { paths: git_bash_paths, args: Some(vec!["--login".to_owned(), "-i".to_owned()]), icon: None },
    );
    sources.insert(ProfileSource::Pwsh.as_str().to_owned(), PotentialTerminalProfile { paths: pwsh_paths, args: None, icon: Some(codicon(CODICON_TERMINAL_POWERSHELL)) });
    PROFILE_SOURCES.with(|slot| *slot.borrow_mut() = Some(Rc::new(sources)));
}

async fn get_git_bash_paths() -> Vec<String> {
    let path = path_module();
    let mut git_dirs: Vec<String> = Vec::new();
    let add = |git_dirs: &mut Vec<String>, value: String| {
        if !git_dirs.contains(&value) {
            git_dirs.push(value);
        }
    };

    // Look for git.exe on the PATH and use that if found. git.exe is located at
    // `<installdir>/cmd/git.exe`. This is not an unsafe location because the git executable is
    // located on the PATH which is only controlled by the user/admin.
    if let Some(git_exe_path) = find_executable("git.exe", None, None, None, None).await {
        let git_exe_dir = path.dirname(&git_exe_path);
        add(&mut git_dirs, path.resolve(&[&git_exe_dir, "../.."]));
    }

    // Add common git install locations
    for name in ["ProgramW6432", "ProgramFiles", "ProgramFiles(X86)"] {
        if let Some(value) = env_var(name).filter(|value| !value.is_empty()) {
            add(&mut git_dirs, value);
        }
    }
    add(&mut git_dirs, format!("{}\\Program", js_env("LocalAppData")));

    let mut git_bash_paths: Vec<String> = Vec::new();
    for git_dir in &git_dirs {
        git_bash_paths.push(format!("{git_dir}\\Git\\bin\\bash.exe"));
        git_bash_paths.push(format!("{git_dir}\\Git\\usr\\bin\\bash.exe"));
        git_bash_paths.push(format!("{git_dir}\\usr\\bin\\bash.exe")); // using Git for Windows SDK
    }

    // Add special installs that don't follow the standard directory structure
    git_bash_paths.push(format!("{}\\scoop\\apps\\git\\current\\bin\\bash.exe", js_env("UserProfile")));
    git_bash_paths.push(format!("{}\\scoop\\apps\\git-with-openssh\\current\\bin\\bash.exe", js_env("UserProfile")));

    git_bash_paths
}

async fn get_powershell_paths() -> Vec<String> {
    // Add all of the different kinds of PowerShells
    enumerate_power_shell_installations().await.into_iter().map(|pwsh_exe| pwsh_exe.exe_path).collect()
}

/// `cp.exec('wsl.exe -l -q', { encoding: 'utf16le', env: { ...process.env, WSL_UTF8: '0' }, timeout: 1000 })`.
fn exec_wsl_list_distros() -> Result<String, Error> {
    let problem = || Error::new("Problem occurred when getting wsl distros");
    // wsl.exe output is encoded in utf16le (ie. A -> 0x4100) by default, force it in case the
    // user changed https://github.com/microsoft/vscode/issues/276253
    let mut child = Command::new("wsl.exe")
        .args(["-l", "-q"])
        .env("WSL_UTF8", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| problem())?;
    let mut stdout = child.stdout.take().ok_or_else(problem)?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        let _ = stdout.read_to_end(&mut bytes);
        bytes
    });
    let deadline = Instant::now() + Duration::from_millis(1000);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    let bytes = reader.join().unwrap_or_default();
    if !status.is_some_and(|status| status.success()) {
        return Err(problem());
    }
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    Ok(String::from_utf16_lossy(&units))
}

async fn get_wsl_profiles(wsl_path: &str, default_profile_name: Option<&str>) -> Result<Vec<ITerminalProfile>, Error> {
    let mut profiles: Vec<ITerminalProfile> = Vec::new();
    let distro_output = exec_wsl_list_distros()?;
    if distro_output.is_empty() {
        return Ok(Vec::new());
    }
    let normalized = distro_output.replace("\r\n", "\n");
    let distro_names = normalized.split('\n').filter(|name| !name.trim().is_empty());
    for distro_name in distro_names {
        // docker-desktop and docker-desktop-data are treated as implementation details of
        // Docker Desktop for Windows and therefore not exposed
        if distro_name.starts_with("docker-desktop") {
            continue;
        }

        // Create the profile, adding the icon depending on the distro
        let profile_name = format!("{distro_name} (WSL)");
        let profile = ITerminalProfile {
            is_default: Some(profile_name.as_str()) == default_profile_name,
            profile_name,
            path: wsl_path.to_owned(),
            args: Some(OneOrMany::Many(vec!["-d".to_owned(), distro_name.to_owned()])),
            icon: Some(get_wsl_icon(distro_name)),
            is_auto_detected: Some(false),
            ..Default::default()
        };
        // Add the profile
        profiles.push(profile);
    }
    Ok(profiles)
}

fn get_wsl_icon(distro_name: &str) -> TerminalIcon {
    if distro_name.contains("Ubuntu") {
        codicon(CODICON_TERMINAL_UBUNTU)
    } else if distro_name.contains("Debian") {
        codicon(CODICON_TERMINAL_DEBIAN)
    } else {
        codicon(CODICON_TERMINAL_LINUX)
    }
}

#[allow(clippy::too_many_arguments)]
async fn detect_available_unix_profiles(
    fs_provider: &dyn IFsProvider,
    log_service: Option<&dyn LogService>,
    include_detected_profiles: bool,
    config_profiles: &Dynamic,
    default_profile_name: Option<String>,
    test_paths: Option<&[String]>,
    variable_resolver: Option<VariableResolver<'_>>,
    shell_env: &ProcessEnvironment,
) -> Vec<ITerminalProfile> {
    let mut detected_profiles: IndexMap<String, IUnresolvedTerminalProfile> = IndexMap::new();

    // Add non-quick launch profiles
    if include_detected_profiles && fs_provider.exists_file(UNIX_SHELLS_PATH).await {
        // `(await fsProvider.readFile(...)).toString()`: o `readFile` que rejeita é uma exceção do original.
        let contents = String::from_utf8_lossy(&fs_provider.read_file(UNIX_SHELLS_PATH).await.unwrap_or_else(|error| je_base::errors::throw(error))).into_owned();
        let lines: Vec<String> = match test_paths {
            Some(test_paths) => test_paths.to_vec(),
            None => contents.split('\n').map(str::to_owned).collect(),
        };
        let profiles: Vec<String> = lines
            .into_iter()
            .map(|line| match line.find('#') {
                Some(index) => line[..index].to_owned(),
                None => line,
            })
            .filter(|line| !line.trim().is_empty())
            .collect();
        let path = path_module();
        let mut counts: HashMap<String, usize> = HashMap::new();
        for profile in profiles {
            let mut profile_name = path.basename(&profile, None);
            let mut count = counts.get(&profile_name).copied().unwrap_or(0);
            count += 1;
            if count > 1 {
                profile_name = format!("{profile_name} ({count})");
            }
            counts.insert(profile_name.clone(), count);
            detected_profiles.insert(
                profile_name,
                IUnresolvedTerminalProfile::Executable(ITerminalExecutable {
                    base: IBaseUnresolvedTerminalProfile { is_auto_detected: Some(true), ..Default::default() },
                    path: vec![ProfilePath::Plain(profile)],
                }),
            );
        }
    }

    apply_config_profiles_to_map(config_profiles, &mut detected_profiles);

    let detector = ProfileDetector {
        fs_provider,
        shell_env,
        log_service,
        variable_resolver,
        default_profile_name,
        profile_sources: PROFILE_SOURCES.with(|sources| sources.borrow().clone()),
    };
    detector.transform_to_terminal_profiles(&detected_profiles).await
}

/// `applyConfigProfilesToMap(configProfiles, profilesMap)`.
fn apply_config_profiles_to_map(config_profiles: &Dynamic, profiles_map: &mut IndexMap<String, IUnresolvedTerminalProfile>) {
    // `if (!configProfiles) return;`
    if !matches!(config_profiles, Dynamic::Object(_)) {
        return;
    }
    for (profile_name, value) in config_profiles.own_entries() {
        let profile_name = profile_name.to_string_lossy();
        match parse_unresolved_profile(&value) {
            None => {
                profiles_map.shift_remove(&profile_name);
            }
            Some(mut profile) => {
                // value.icon = value.icon || profilesMap.get(profileName)?.icon;
                if !icon_is_truthy(profile.base().icon.as_ref()) {
                    profile.base_mut().icon = profiles_map.get(&profile_name).and_then(|existing| existing.base().icon.clone());
                }
                profiles_map.insert(profile_name, profile);
            }
        }
    }
}

fn string_value(value: &Dynamic) -> Option<String> {
    match value {
        Dynamic::String(text) => Some(text.to_string_lossy()),
        _ => None,
    }
}

fn js_string_value(value: &Dynamic) -> Option<JsString> {
    match value {
        Dynamic::String(text) => Some(text.clone()),
        _ => None,
    }
}

fn bool_value(value: &Dynamic) -> Option<bool> {
    match value {
        Dynamic::Bool(flag) => Some(*flag),
        _ => None,
    }
}

fn parse_uri_components(value: &Dynamic) -> UriComponents {
    let field = |name: &str| if matches!(value, Dynamic::Object(_)) { js_string_value(&value.get_str(name)) } else { None };
    UriComponents { scheme: field("scheme"), authority: field("authority"), path: field("path"), query: field("query"), fragment: field("fragment") }
}

/// `string | ThemeIcon | URI | { light: URI; dark: URI }` vindo da configuração.
fn parse_icon(value: &Dynamic) -> Option<ContributedIcon> {
    match value {
        Dynamic::String(text) => Some(ContributedIcon::Text(text.to_string_lossy())),
        Dynamic::Object(_) => {
            if let Some(id) = string_value(&value.get_str("id")) {
                let color = match value.get_str("color") {
                    color @ Dynamic::Object(_) => string_value(&color.get_str("id")).map(ThemeColor::new),
                    _ => None,
                };
                return Some(ContributedIcon::Icon(TerminalIcon::Theme(ThemeIcon { id, color })));
            }
            let (light, dark) = (value.get_str("light"), value.get_str("dark"));
            if matches!(light, Dynamic::Object(_)) && matches!(dark, Dynamic::Object(_)) {
                return Some(ContributedIcon::Icon(TerminalIcon::LightDark(LightDarkIcon {
                    light: UriLike::Components(parse_uri_components(&light)),
                    dark: UriLike::Components(parse_uri_components(&dark)),
                })));
            }
            if value.has_property(&JsString::from("scheme")) {
                return Some(ContributedIcon::Icon(TerminalIcon::Uri(UriLike::Components(parse_uri_components(value)))));
            }
            None
        }
        _ => None,
    }
}

/// `string | ITerminalUnsafePath`.
fn parse_profile_path(value: &Dynamic) -> Option<ProfilePath> {
    match value {
        Dynamic::String(path) => Some(ProfilePath::Plain(path.to_string_lossy())),
        Dynamic::Object(_) => {
            let path = string_value(&value.get_str("path"))?;
            Some(ProfilePath::Unsafe(super::terminal::ITerminalUnsafePath { path, is_unsafe: value.get_str("isUnsafe").is_truthy() }))
        }
        _ => None,
    }
}

fn parse_environment(value: &Dynamic) -> Option<ITerminalEnvironment> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    Some(
        value
            .own_entries()
            .into_iter()
            .filter_map(|(key, value)| {
                let value = match value {
                    Dynamic::String(text) => TerminalEnvironmentValue::Value(text.to_string_lossy()),
                    Dynamic::Null => TerminalEnvironmentValue::Null,
                    Dynamic::Undefined => TerminalEnvironmentValue::Undefined,
                    _ => return None,
                };
                Some((key.to_string_lossy(), value))
            })
            .collect(),
    )
}

fn parse_base_profile(value: &Dynamic) -> IBaseUnresolvedTerminalProfile {
    let args = match value.get_str("args") {
        Dynamic::String(args) => Some(OneOrMany::One(args.to_string_lossy())),
        Dynamic::Array(args) => Some(OneOrMany::Many(args.to_vec().iter().filter_map(string_value).collect())),
        _ => None,
    };
    IBaseUnresolvedTerminalProfile {
        args,
        is_auto_detected: bool_value(&value.get_str("isAutoDetected")),
        override_name: bool_value(&value.get_str("overrideName")),
        icon: parse_icon(&value.get_str("icon")),
        color: string_value(&value.get_str("color")),
        env: parse_environment(&value.get_str("env")),
        requires_path: parse_profile_path(&value.get_str("requiresPath")),
    }
}

/// Converte um valor da configuração em `IUnresolvedTerminalProfile`. `None` é o valor que o
/// `applyConfigProfilesToMap` manda remover: `null`, o que não é objeto, ou o objeto sem `path` nem `source`.
fn parse_unresolved_profile(value: &Dynamic) -> Option<IUnresolvedTerminalProfile> {
    if !matches!(value, Dynamic::Object(_)) {
        return None;
    }
    if value.has_property(&JsString::from("source")) {
        return Some(IUnresolvedTerminalProfile::Source(ITerminalProfileSource {
            base: parse_base_profile(value),
            source: string_value(&value.get_str("source")).unwrap_or_default(),
        }));
    }
    if value.has_property(&JsString::from("path")) {
        // `Array.isArray(profile.path) ? profile.path : [profile.path]`
        let path = match value.get_str("path") {
            Dynamic::Array(paths) => paths.to_vec().iter().filter_map(parse_profile_path).collect(),
            single => parse_profile_path(&single).into_iter().collect(),
        };
        return Some(IUnresolvedTerminalProfile::Executable(ITerminalExecutable { base: parse_base_profile(value), path }));
    }
    None
}
