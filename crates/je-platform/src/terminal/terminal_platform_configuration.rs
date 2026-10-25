//! Porte de `src/vs/platform/terminal/common/terminalPlatformConfiguration.ts`: a configuração do terminal que o
//! processo compartilhado e o servidor remoto precisam registrar (perfis, perfil de automação, herança do
//! ambiente, nomes de processo ignorados...).
//!
//! Representação:
//! - Os esquemas JSON (`IJSONSchema`) são [`Dynamic`] objetos, e cada propriedade é uma [`PropertySchema`] (como em
//!   `je_workbench::...::terminal_configuration`). O `...spread` de `terminalProfileBaseProperties` e de
//!   `terminalIconSchema` é a junção das listas de pares, na ordem do original.
//! - `Array.from(getAllCodicons(), icon => icon.id)` são os ids do `je-codicons` (`je_codicons::ids`); os ids de
//!   `Codicon.terminalPowershell.id` e companhia são os mesmos de [`super::node_terminal_profiles`].
//! - `terminalPlatformConfiguration` e `defaultProfilesConfiguration` (variável de módulo) são `thread_local`s: o nó
//!   é criado uma vez e registrado de novo a cada [`register_terminal_platform_configuration`], como no original.
//! - `PlatformToString` dos três sistemas é o nome por extenso (`Linux`, `Mac`, `Windows`).

use std::cell::RefCell;

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_base::platform::OperatingSystem;

use super::terminal::{IExtensionTerminalProfile, ITerminalProfile};
use super::terminal_profiles::create_profile_schema_enums;
use super::terminal_settings::TerminalSettingId;
use crate::configuration::registry::{ConfigurationNode, ConfigurationNodeData, ConfigurationScope, PropertySchema, configuration_registry};

type Entries = Vec<(&'static str, Dynamic)>;

fn text(value: &str) -> Dynamic {
    Dynamic::string(value)
}

fn object<const N: usize>(entries: [(&str, Dynamic); N]) -> Dynamic {
    Dynamic::object(entries)
}

fn strings<const N: usize>(items: [&str; N]) -> Dynamic {
    Dynamic::array(items.map(Dynamic::string))
}

fn schema_object(entries: Entries) -> Dynamic {
    Dynamic::object(entries)
}

/// `terminalColorSchema`.
pub fn terminal_color_schema() -> Entries {
    vec![
        ("type", strings(["string", "null"])),
        (
            "enum",
            strings([
                "terminal.ansiBlack",
                "terminal.ansiRed",
                "terminal.ansiGreen",
                "terminal.ansiYellow",
                "terminal.ansiBlue",
                "terminal.ansiMagenta",
                "terminal.ansiCyan",
                "terminal.ansiWhite",
            ]),
        ),
        ("default", Dynamic::Null),
    ]
}

/// `terminalIconSchema`.
pub fn terminal_icon_schema() -> Entries {
    vec![
        ("type", text("string")),
        ("enum", Dynamic::array(je_codicons::ids().map(Dynamic::string))),
        ("markdownEnumDescriptions", Dynamic::array(je_codicons::ids().map(|id| Dynamic::string(&format!("$({id})"))))),
    ]
}

/// `terminalProfileBaseProperties`.
pub fn terminal_profile_base_properties() -> Vec<(&'static str, Dynamic)> {
    let mut icon = vec![("description", text(&localize("terminalProfile.icon", "A codicon ID to associate with the terminal icon.", &[])))];
    icon.extend(terminal_icon_schema());
    let mut color = vec![("description", text(&localize("terminalProfile.color", "A theme color ID to associate with the terminal icon.", &[])))];
    color.extend(terminal_color_schema());
    vec![
        (
            "args",
            object([
                ("description", text(&localize("terminalProfile.args", "An optional set of arguments to run the shell executable with.", &[]))),
                ("type", text("array")),
                ("items", object([("type", text("string"))])),
            ]),
        ),
        ("icon", schema_object(icon)),
        ("color", schema_object(color)),
        (
            "env",
            object([
                (
                    "markdownDescription",
                    text(&localize(
                        "terminalProfile.env",
                        "An object with environment variables that will be added to the terminal profile process. Set to `null` to delete environment variables from the base environment.",
                        &[],
                    )),
                ),
                ("type", text("object")),
                ("additionalProperties", object([("type", strings(["string", "null"]))])),
                ("default", object([])),
            ]),
        ),
    ]
}

/// `{ ...entries, ...terminalProfileBaseProperties }` como o objeto `properties` de um esquema.
fn with_base_properties(mut properties: Entries) -> Dynamic {
    properties.extend(terminal_profile_base_properties());
    schema_object(properties)
}

fn terminal_profile_schema() -> Dynamic {
    object([
        ("type", text("object")),
        ("required", strings(["path"])),
        (
            "properties",
            with_base_properties(vec![
                (
                    "path",
                    object([
                        (
                            "description",
                            text(&localize("terminalProfile.path", "A single path to a shell executable or an array of paths that will be used as fallbacks when one fails.", &[])),
                        ),
                        ("type", strings(["string", "array"])),
                        ("items", object([("type", text("string"))])),
                    ]),
                ),
                (
                    "overrideName",
                    object([
                        (
                            "description",
                            text(&localize(
                                "terminalProfile.overrideName",
                                "Whether or not to replace the dynamic terminal title that detects what program is running with the static profile name.",
                                &[],
                            )),
                        ),
                        ("type", text("boolean")),
                    ]),
                ),
            ]),
        ),
    ])
}

fn terminal_automation_profile_schema() -> Dynamic {
    object([
        ("type", text("object")),
        ("required", strings(["path"])),
        (
            "properties",
            with_base_properties(vec![(
                "path",
                object([
                    ("description", text(&localize("terminalAutomationProfile.path", "A path to a shell executable.", &[]))),
                    ("type", strings(["string"])),
                    ("items", object([("type", text("string"))])),
                ]),
            )]),
        ),
    ])
}

/// `createTerminalProfileMarkdownDescription(platform)`.
fn create_terminal_profile_markdown_description(platform: OperatingSystem) -> String {
    let (key, name) = match platform {
        OperatingSystem::Linux => ("linux", "Linux"),
        OperatingSystem::Macintosh => ("osx", "Mac"),
        OperatingSystem::Windows => ("windows", "Windows"),
    };
    localize(
        "terminal.integrated.profile",
        "A set of terminal profile customizations for {0} which allows adding, removing or changing how terminals are launched. Profiles are made up of a mandatory path, optional arguments and other presentation options.\n\nTo override an existing profile use its profile name as the key, for example:\n\n{1}\n\n{2}Read more about configuring profiles{3}.",
        &[name, &format!("```json\n\"terminal.integrated.profile.{key}\": {{\n  \"bash\": null\n}}\n```"), "[", "](https://code.visualstudio.com/docs/terminal/profiles)"],
    )
}

/// `defaultSnippets` das propriedades de perfil: `[{ body: { path: '${1}', icon: '${2}' } }]`.
fn path_icon_default_snippets() -> Dynamic {
    Dynamic::array([object([("body", object([("path", text("${1}")), ("icon", text("${2}"))]))])])
}

/// O `anyOf` de uma propriedade de perfil de automação ou do agent host: `null`, e (se `with_string`) o texto, e o esquema.
fn automation_any_of(with_string: bool) -> Dynamic {
    let mut items = vec![object([("type", text("null"))])];
    if with_string {
        items.push(object([("type", text("string"))]));
    }
    items.push(terminal_automation_profile_schema());
    Dynamic::array(items)
}

fn automation_profile_property(markdown_description: String) -> PropertySchema {
    PropertySchema::from_entries([
        ("restricted", Dynamic::Bool(true)),
        ("markdownDescription", text(&markdown_description)),
        ("type", strings(["object", "null"])),
        ("default", Dynamic::Null),
        ("anyOf", automation_any_of(false)),
        ("defaultSnippets", path_icon_default_snippets()),
    ])
}

fn agent_host_profile_property(os_label: &str, profiles_key: &str, default_profile_key: &str) -> PropertySchema {
    let description = localize(
        &format!("terminal.integrated.agentHostProfile.{os_label}"),
        &format!(
            "The terminal profile to use on {} for agent host terminals, including shells launched by AI agent tools. Accepts either a profile name from {{0}} or an inline profile object. When unset, falls back to {{1}}. Currently applies to the local agent host. Only the executable `path` is honored today; `args` and `env` from the profile are ignored. Remote agent hosts need remote-side shell configuration because local resolved paths may be invalid on the remote.",
            match os_label {
                "linux" => "Linux",
                "osx" => "macOS",
                _ => "Windows",
            }
        ),
        &[profiles_key, default_profile_key],
    );
    PropertySchema::from_entries([
        ("restricted", Dynamic::Bool(true)),
        ("markdownDescription", text(&description)),
        ("type", strings(["string", "object", "null"])),
        ("default", Dynamic::Null),
        ("anyOf", automation_any_of(true)),
        ("defaultSnippets", path_icon_default_snippets()),
    ])
}

/// O `additionalProperties.anyOf` dos perfis de um sistema: a extensão (com as chaves de `localize` do sistema), o `null` e o
/// esquema de perfil. `extra_first` é o esquema `source` do Windows, que vem antes da extensão.
fn profiles_additional_properties(os_prefix: &str, extra_first: Option<Dynamic>) -> Dynamic {
    let extension = object([
        ("type", text("object")),
        ("required", strings(["extensionIdentifier", "id", "title"])),
        (
            "properties",
            with_base_properties(vec![
                (
                    "extensionIdentifier",
                    object([
                        ("description", text(&localize(&format!("terminalProfile.{os_prefix}ExtensionIdentifier"), "The extension that contributed this profile.", &[]))),
                        ("type", text("string")),
                    ]),
                ),
                (
                    "id",
                    object([
                        ("description", text(&localize(&format!("terminalProfile.{os_prefix}ExtensionId"), "The id of the extension terminal", &[]))),
                        ("type", text("string")),
                    ]),
                ),
                (
                    "title",
                    object([
                        ("description", text(&localize(&format!("terminalProfile.{os_prefix}ExtensionTitle"), "The name of the extension terminal", &[]))),
                        ("type", text("string")),
                    ]),
                ),
            ]),
        ),
    ]);
    let mut any_of: Vec<Dynamic> = extra_first.into_iter().collect();
    any_of.push(extension);
    any_of.push(object([("type", text("null"))]));
    any_of.push(terminal_profile_schema());
    object([("anyOf", Dynamic::array(any_of))])
}

fn windows_source_schema() -> Dynamic {
    object([
        ("type", text("object")),
        ("required", strings(["source"])),
        (
            "properties",
            with_base_properties(vec![(
                "source",
                object([
                    (
                        "description",
                        text(&localize(
                            "terminalProfile.windowsSource",
                            "A profile source that will auto detect the paths to the shell. Note that non-standard executable locations are not supported and must be created manually in a new profile.",
                            &[],
                        )),
                    ),
                    ("enum", strings(["PowerShell", "Git Bash"])),
                ]),
            )]),
        ),
    ])
}

fn profiles_windows_property() -> PropertySchema {
    PropertySchema::from_entries([
        ("restricted", Dynamic::Bool(true)),
        ("markdownDescription", text(&create_terminal_profile_markdown_description(OperatingSystem::Windows))),
        ("type", text("object")),
        (
            "default",
            object([
                ("PowerShell", object([("source", text("PowerShell")), ("icon", text("terminal-powershell"))])),
                (
                    "Command Prompt",
                    object([
                        ("path", strings(["${env:windir}\\Sysnative\\cmd.exe", "${env:windir}\\System32\\cmd.exe"])),
                        ("args", Dynamic::array([])),
                        ("icon", text("terminal-cmd")),
                    ]),
                ),
                ("Git Bash", object([("source", text("Git Bash")), ("icon", text("terminal-git-bash"))])),
            ]),
        ),
        ("additionalProperties", profiles_additional_properties("windows", Some(windows_source_schema()))),
    ])
}

fn profiles_macos_property() -> PropertySchema {
    PropertySchema::from_entries([
        ("restricted", Dynamic::Bool(true)),
        ("markdownDescription", text(&create_terminal_profile_markdown_description(OperatingSystem::Macintosh))),
        ("type", text("object")),
        (
            "default",
            object([
                ("bash", object([("path", text("bash")), ("args", strings(["-l"])), ("icon", text("terminal-bash"))])),
                ("zsh", object([("path", text("zsh")), ("args", strings(["-l"]))])),
                ("fish", object([("path", text("fish")), ("args", strings(["-l"]))])),
                ("tmux", object([("path", text("tmux")), ("icon", text("terminal-tmux"))])),
                ("pwsh", object([("path", text("pwsh")), ("icon", text("terminal-powershell"))])),
            ]),
        ),
        ("additionalProperties", profiles_additional_properties("osx", None)),
    ])
}

fn profiles_linux_property() -> PropertySchema {
    PropertySchema::from_entries([
        ("restricted", Dynamic::Bool(true)),
        ("markdownDescription", text(&create_terminal_profile_markdown_description(OperatingSystem::Linux))),
        ("type", text("object")),
        (
            "default",
            object([
                ("bash", object([("path", text("bash")), ("icon", text("terminal-bash"))])),
                ("zsh", object([("path", text("zsh"))])),
                ("fish", object([("path", text("fish"))])),
                ("tmux", object([("path", text("tmux")), ("icon", text("terminal-tmux"))])),
                ("pwsh", object([("path", text("pwsh")), ("icon", text("terminal-powershell"))])),
            ]),
        ),
        ("additionalProperties", profiles_additional_properties("linux", None)),
    ])
}

fn terminal_platform_configuration() -> ConfigurationNode {
    let application = ConfigurationScope::Application.to_dynamic();
    let properties: Vec<(&str, PropertySchema)> = vec![
        (
            TerminalSettingId::AutomationProfileLinux.as_str(),
            automation_profile_property(localize(
                "terminal.integrated.automationProfile.linux",
                "The terminal profile to use on Linux for automation-related terminal usage like tasks and debug.",
                &[],
            )),
        ),
        (
            TerminalSettingId::AutomationProfileMacOs.as_str(),
            automation_profile_property(localize(
                "terminal.integrated.automationProfile.osx",
                "The terminal profile to use on macOS for automation-related terminal usage like tasks and debug.",
                &[],
            )),
        ),
        (
            TerminalSettingId::AutomationProfileWindows.as_str(),
            automation_profile_property(localize(
                "terminal.integrated.automationProfile.windows",
                "The terminal profile to use for automation-related terminal usage like tasks and debug. This setting will currently be ignored if {0} (now deprecated) is set.",
                &["`terminal.integrated.automationShell.windows`"],
            )),
        ),
        (TerminalSettingId::AgentHostProfileLinux.as_str(), agent_host_profile_property("linux", "`#terminal.integrated.profiles.linux#`", "`#terminal.integrated.defaultProfile.linux#`")),
        (TerminalSettingId::AgentHostProfileMacOs.as_str(), agent_host_profile_property("osx", "`#terminal.integrated.profiles.osx#`", "`#terminal.integrated.defaultProfile.osx#`")),
        (
            TerminalSettingId::AgentHostProfileWindows.as_str(),
            agent_host_profile_property("windows", "`#terminal.integrated.profiles.windows#`", "`#terminal.integrated.defaultProfile.windows#`"),
        ),
        (TerminalSettingId::ProfilesWindows.as_str(), profiles_windows_property()),
        (TerminalSettingId::ProfilesMacOs.as_str(), profiles_macos_property()),
        (TerminalSettingId::ProfilesLinux.as_str(), profiles_linux_property()),
        (
            TerminalSettingId::UseWslProfiles.as_str(),
            PropertySchema::from_entries([
                ("description", text(&localize("terminal.integrated.useWslProfiles", "Controls whether or not WSL distros are shown in the terminal dropdown", &[]))),
                ("type", text("boolean")),
                ("default", Dynamic::Bool(true)),
            ]),
        ),
        (
            TerminalSettingId::InheritEnv.as_str(),
            PropertySchema::from_entries([
                ("scope", application.clone()),
                (
                    "description",
                    text(&localize(
                        "terminal.integrated.inheritEnv",
                        "Whether new shells should inherit their environment from VS Code, which may source a login shell to ensure $PATH and other development variables are initialized. This has no effect on Windows.",
                        &[],
                    )),
                ),
                ("type", text("boolean")),
                ("default", Dynamic::Bool(true)),
            ]),
        ),
        (
            TerminalSettingId::PersistentSessionScrollback.as_str(),
            PropertySchema::from_entries([
                ("scope", application.clone()),
                (
                    "markdownDescription",
                    text(&localize(
                        "terminal.integrated.persistentSessionScrollback",
                        "Controls the maximum amount of lines that will be restored when reconnecting to a persistent terminal session. Increasing this will restore more lines of scrollback at the cost of more memory and increase the time it takes to connect to terminals on start up. This setting requires a restart to take effect and should be set to a value less than or equal to `#terminal.integrated.scrollback#`.",
                        &[],
                    )),
                ),
                ("type", text("number")),
                ("default", Dynamic::Number(100.0)),
            ]),
        ),
        (
            TerminalSettingId::ShowLinkHover.as_str(),
            PropertySchema::from_entries([
                ("scope", application),
                ("description", text(&localize("terminal.integrated.showLinkHover", "Whether to show hovers for links in the terminal output.", &[]))),
                ("type", text("boolean")),
                ("default", Dynamic::Bool(true)),
            ]),
        ),
        (
            TerminalSettingId::IgnoreProcessNames.as_str(),
            PropertySchema::from_entries([
                (
                    "markdownDescription",
                    text(&localize("terminal.integrated.confirmIgnoreProcesses", "A set of process names to ignore when using the {0} setting.", &["`#terminal.integrated.confirmOnKill#`"])),
                ),
                ("type", text("array")),
                ("items", object([("type", text("string")), ("uniqueItems", Dynamic::Bool(true))])),
                // Popular prompt programs, these should not count as child processes
                // Git bash may runs a subprocess of itself (bin\bash.exe -> usr\bin\bash.exe)
                ("default", strings(["starship", "oh-my-posh", "bash", "zsh"])),
            ]),
        ),
    ];
    ConfigurationNode::new(
        ConfigurationNodeData {
            id: Some("terminal".to_owned()),
            order: Some(100.0),
            title: Some(localize("terminalIntegratedConfigurationTitle", "Integrated Terminal", &[])),
            type_: Some(text("object")),
            ..Default::default()
        }
        .with_properties(properties),
    )
}

thread_local! {
    static TERMINAL_PLATFORM_CONFIGURATION: ConfigurationNode = terminal_platform_configuration();
    static DEFAULT_PROFILES_CONFIGURATION: RefCell<Option<ConfigurationNode>> = const { RefCell::new(None) };
}

/// Registers terminal configurations required by shared process and remote server.
pub fn register_terminal_platform_configuration() {
    let node = TERMINAL_PLATFORM_CONFIGURATION.with(Clone::clone);
    configuration_registry().register_configuration(node);
    register_terminal_default_profile_configuration(None, None);
}

/// `registerTerminalDefaultProfileConfiguration(detectedProfiles?, extensionContributedProfiles?)`: `detected_profiles` é
/// `{ os, profiles }`.
pub fn register_terminal_default_profile_configuration(detected_profiles: Option<(OperatingSystem, &[ITerminalProfile])>, extension_contributed_profiles: Option<&[IExtensionTerminalProfile]>) {
    let registry = configuration_registry();
    let profile_enum = detected_profiles.map(|(_, profiles)| create_profile_schema_enums(profiles, extension_contributed_profiles));
    let old_default_profiles_configuration = DEFAULT_PROFILES_CONFIGURATION.with(|current| current.borrow().clone());

    let default_profile_property = |os: OperatingSystem, description: String| {
        let mut entries = vec![
            ("restricted", Dynamic::Bool(true)),
            ("markdownDescription", text(&description)),
            ("type", strings(["string", "null"])),
            ("default", Dynamic::Null),
        ];
        if let (Some((detected_os, _)), Some(profile_enum)) = (detected_profiles, &profile_enum)
            && detected_os == os
        {
            entries.push(("enum", Dynamic::array(profile_enum.values.iter().map(|value| value.as_deref().map_or(Dynamic::Null, Dynamic::string)))));
            entries.push(("markdownEnumDescriptions", Dynamic::array(profile_enum.markdown_descriptions.iter().map(|description| Dynamic::string(description)))));
        }
        PropertySchema::from_entries(entries)
    };

    let default_profiles_configuration = ConfigurationNode::new(
        ConfigurationNodeData {
            id: Some("terminal".to_owned()),
            order: Some(100.0),
            title: Some(localize("terminalIntegratedConfigurationTitle", "Integrated Terminal", &[])),
            type_: Some(text("object")),
            ..Default::default()
        }
        .with_properties(vec![
            (
                TerminalSettingId::DefaultProfileLinux.as_str(),
                default_profile_property(OperatingSystem::Linux, localize("terminal.integrated.defaultProfile.linux", "The default terminal profile on Linux.", &[])),
            ),
            (
                TerminalSettingId::DefaultProfileMacOs.as_str(),
                default_profile_property(OperatingSystem::Macintosh, localize("terminal.integrated.defaultProfile.osx", "The default terminal profile on macOS.", &[])),
            ),
            (
                TerminalSettingId::DefaultProfileWindows.as_str(),
                default_profile_property(OperatingSystem::Windows, localize("terminal.integrated.defaultProfile.windows", "The default terminal profile on Windows.", &[])),
            ),
        ]),
    );
    DEFAULT_PROFILES_CONFIGURATION.with(|current| *current.borrow_mut() = Some(default_profiles_configuration.clone()));
    let remove: Vec<ConfigurationNode> = old_default_profiles_configuration.into_iter().collect();
    registry.update_configurations(&[default_profiles_configuration], &remove);
}
