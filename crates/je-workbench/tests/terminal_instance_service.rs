//! Porte de `src/vs/workbench/contrib/terminal/test/browser/terminalInstanceService.test.ts` (suíte `Workbench -
//! TerminalInstanceService`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - **`ensureNoDisposablesAreLeakedInTestSuite()` e `setup`.** O [`Fixture`] cria o guarda de vazamentos antes de tudo; o serviço e
//!   os serviços que ele lê passam pelo store do guarda.
//! - **`workbenchInstantiationService(undefined, store)`.** O `InstantiationService` do caso tem só os serviços que o construtor
//!   do `TerminalInstanceService` lê: o `IContextKeyService` (o `ContextKeyService` sobre o `TestConfigurationService`) e o
//!   `IWorkbenchEnvironmentService` (o `TestEnvironmentService`).
//! - **`deepStrictEqual(config, { ... })`.** O `IShellLaunchConfig` do porte não tem igualdade (carrega funções); o caso compara
//!   o [`ConfigView`], com os sete campos que a conversão preenche (`executable`, `cwd`, `args`, `env`, `icon`, `color` e `name`);
//!   os demais campos do `IShellLaunchConfig` só existem no valor padrão.
//! - **`{ executable: '/foo' }` e `{ profileName: 'abc', path: '/foo', isDefault: true }`.** São o `IShellLaunchConfig` e o
//!   `ITerminalProfile` do porte, dentro do [`ShellLaunchConfigOrProfile`].
//! - **`'/bar'` como `cwd`.** É o `ShellCwd::Path`.
//! - **`{ test: 'TEST' }` como `env`.** É o `ITerminalEnvironment` (lista de pares).

#[path = "../../je-base/tests/common/mod.rs"]
mod common;
mod support;

use std::rc::Rc;

use common::{LeakGuard, ensure_no_disposables_are_leaked_in_test_suite};
use je_base::arrays::OneOrMany;
use je_base::uri::URI;
use je_instantiation::{InstantiationService, ServiceCollection};
use je_platform::configuration::service::{I_CONFIGURATION_SERVICE, IConfigurationService};
use je_platform::configuration::test_configuration_service::TestConfigurationService;
use je_platform::contextkey::contextkey::{I_CONTEXT_KEY_SERVICE, IContextKeyService};
use je_platform::contextkey::service::ContextKeyService;
use je_platform::terminal::terminal::{IShellLaunchConfig, ITerminalEnvironment, ITerminalProfile, ShellCwd, TerminalEnvironmentValue, TerminalIcon};
use je_workbench::contrib::terminal::browser::terminal::{ITerminalInstanceService, ShellLaunchConfigOrProfile};
use je_workbench::contrib::terminal::browser::terminal_instance_service::TerminalInstanceService;
use je_workbench::services::environment::environment_service::{I_WORKBENCH_ENVIRONMENT_SERVICE, IWorkbenchEnvironmentService};
use je_workbench::test::browser::workbench_test_services::TestEnvironmentService;

struct Fixture {
    terminal_instance_service: Rc<TerminalInstanceService>,
    // Declarado por último: o `Drop` dele descarta o que o `Fixture` registrou e confere os vazamentos.
    _store: LeakGuard,
}

impl Fixture {
    /// O `setup` da suíte.
    fn new() -> Self {
        let store = ensure_no_disposables_are_leaked_in_test_suite();
        let configuration_service = TestConfigurationService::new(None);
        // O emissor do `TestConfigurationService` é descartado pelo store do caso (o original não o descarta).
        store.add_rc(configuration_service.on_did_change_configuration_emitter.clone());
        let configuration: Rc<dyn IConfigurationService> = configuration_service;
        let context_key_concrete = store.add_rc(ContextKeyService::new(configuration.clone()));
        let context_key_service: Rc<dyn IContextKeyService> = context_key_concrete;
        let environment_service: Rc<dyn IWorkbenchEnvironmentService> = TestEnvironmentService::new();
        let instantiation_service = store.add_rc(InstantiationService::new(
            ServiceCollection::new()
                .with(I_CONFIGURATION_SERVICE, configuration)
                .with(I_CONTEXT_KEY_SERVICE, context_key_service)
                .with(I_WORKBENCH_ENVIRONMENT_SERVICE, environment_service),
        ));
        let terminal_instance_service = store.add_rc(instantiation_service.create_instance::<TerminalInstanceService>(()));
        Self { terminal_instance_service, _store: store }
    }
}

/// Os campos de um `IShellLaunchConfig` que a conversão preenche (ver o cabeçalho).
#[derive(Debug, PartialEq)]
struct ConfigView {
    executable: Option<String>,
    cwd: Option<String>,
    args: Option<OneOrMany<String>>,
    env: Option<ITerminalEnvironment>,
    icon: Option<TerminalIcon>,
    color: Option<String>,
    name: Option<String>,
}

impl ConfigView {
    fn of(config: &IShellLaunchConfig) -> Self {
        Self {
            executable: config.executable.clone(),
            cwd: config.cwd.as_ref().map(|cwd| match cwd {
                ShellCwd::Path(path) => path.clone(),
                ShellCwd::Uri(uri) => uri.to_string(),
            }),
            args: config.args.clone(),
            env: config.env.clone(),
            icon: config.icon.clone(),
            color: config.color.clone(),
            name: config.name.clone(),
        }
    }

    /// `{}`.
    fn empty() -> Self {
        Self { executable: None, cwd: None, args: None, env: None, icon: None, color: None, name: None }
    }
}

fn convert(fixture: &Fixture, config: Option<ShellLaunchConfigOrProfile>, cwd: Option<&str>) -> ConfigView {
    ConfigView::of(&fixture.terminal_instance_service.convert_profile_to_shell_launch_config(config, cwd.map(|cwd| ShellCwd::Path(cwd.to_owned()))))
}

/// `{ executable, cwd, args }` como `IShellLaunchConfig`.
fn launch_config(executable: Option<&str>, cwd: Option<&str>, args: Option<&[&str]>) -> Option<ShellLaunchConfigOrProfile> {
    Some(ShellLaunchConfigOrProfile::LaunchConfig(IShellLaunchConfig {
        executable: executable.map(str::to_owned),
        cwd: cwd.map(|cwd| ShellCwd::Path(cwd.to_owned())),
        args: args.map(|args| OneOrMany::Many(args.iter().map(|arg| (*arg).to_owned()).collect())),
        ..IShellLaunchConfig::default()
    }))
}

/// `{ profileName: 'abc', path: '/foo', isDefault: true }`.
fn abc_profile() -> ITerminalProfile {
    ITerminalProfile { profile_name: "abc".to_owned(), path: "/foo".to_owned(), is_default: true, ..ITerminalProfile::default() }
}

#[test]
fn convert_profile_to_shell_launch_config_should_return_an_empty_shell_launch_config_when_undefined_is_provided() {
    let fixture = Fixture::new();
    assert_eq!(convert(&fixture, None, None), ConfigView::empty());
    assert_eq!(convert(&fixture, None, None), ConfigView::empty());
}

#[test]
fn convert_profile_to_shell_launch_config_should_return_the_same_shell_launch_config_when_provided() {
    let fixture = Fixture::new();
    assert_eq!(convert(&fixture, launch_config(None, None, None), None), ConfigView::empty());
    assert_eq!(
        convert(&fixture, launch_config(Some("/foo"), None, None), None),
        ConfigView { executable: Some("/foo".to_owned()), ..ConfigView::empty() }
    );
    assert_eq!(
        convert(&fixture, launch_config(Some("/foo"), Some("/bar"), Some(&["a", "b"])), None),
        ConfigView {
            executable: Some("/foo".to_owned()),
            cwd: Some("/bar".to_owned()),
            args: Some(OneOrMany::Many(vec!["a".to_owned(), "b".to_owned()])),
            ..ConfigView::empty()
        }
    );
    assert_eq!(
        convert(&fixture, launch_config(Some("/foo"), None, None), Some("/bar")),
        ConfigView { executable: Some("/foo".to_owned()), cwd: Some("/bar".to_owned()), ..ConfigView::empty() }
    );
    assert_eq!(
        convert(&fixture, launch_config(Some("/foo"), Some("/bar"), None), Some("/baz")),
        ConfigView { executable: Some("/foo".to_owned()), cwd: Some("/baz".to_owned()), ..ConfigView::empty() }
    );
}

#[test]
fn convert_profile_to_shell_launch_config_should_convert_a_provided_profile_to_a_shell_launch_config() {
    let fixture = Fixture::new();
    assert_eq!(
        convert(&fixture, Some(ShellLaunchConfigOrProfile::Profile(abc_profile())), None),
        ConfigView { executable: Some("/foo".to_owned()), ..ConfigView::empty() }
    );

    let icon = TerminalIcon::Uri(je_platform::terminal::terminal::UriLike::Uri(URI::file("/icon", je_platform::resources::host_platform())));
    let env: ITerminalEnvironment = vec![("test".to_owned(), TerminalEnvironmentValue::Value("TEST".to_owned()))];
    assert_eq!(
        convert(
            &fixture,
            Some(ShellLaunchConfigOrProfile::Profile(ITerminalProfile {
                args: Some(OneOrMany::Many(vec!["a".to_owned(), "b".to_owned()])),
                color: Some("color".to_owned()),
                env: Some(env.clone()),
                icon: Some(icon.clone()),
                ..abc_profile()
            })),
            Some("/bar"),
        ),
        ConfigView {
            executable: Some("/foo".to_owned()),
            cwd: Some("/bar".to_owned()),
            args: Some(OneOrMany::Many(vec!["a".to_owned(), "b".to_owned()])),
            env: Some(env),
            icon: Some(icon),
            color: Some("color".to_owned()),
            name: None,
        }
    );
}

#[test]
fn convert_profile_to_shell_launch_config_should_respect_override_name_in_profile() {
    let fixture = Fixture::new();
    assert_eq!(
        convert(&fixture, Some(ShellLaunchConfigOrProfile::Profile(ITerminalProfile { override_name: Some(true), ..abc_profile() })), None),
        ConfigView { executable: Some("/foo".to_owned()), name: Some("abc".to_owned()), ..ConfigView::empty() }
    );
}
