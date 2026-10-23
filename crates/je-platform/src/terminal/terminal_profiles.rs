//! Porte de `src/vs/platform/terminal/common/terminalProfiles.ts`: os valores e descrições do esquema de
//! perfis de terminal e as comparações de argumentos e de ícones.
//!
//! Divergências: `createProfileSchemaEnums` devolve `values` e `markdownDescriptions` tipados como
//! `| undefined`, mas o corpo sempre os preenche; o porte os devolve como `Vec` sem `Option`. O `Codicon.terminal.id` é o
//! id `terminal` (o `je-platform` não depende do `je-codicons`). A interpolação `${icon}` de um ícone que não
//! é `ThemeIcon` (um `URI` vira o `toString()`, o objeto `{ light, dark }` ou o objeto simples viram
//! `[object Object]`) está em [`icon_description_id`].

use je_base::arrays::OneOrMany;
use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_base::objects::safe_stringify;

use super::terminal::{ContributedIcon, IExtensionTerminalProfile, ITerminalEnvironment, ITerminalProfile, TerminalEnvironmentValue, TerminalIcon, UriLike};

/// `Codicon.terminal.id`.
const CODICON_TERMINAL_ID: &str = "terminal";

/// O resultado de `createProfileSchemaEnums`: `{ values, markdownDescriptions }`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProfileSchemaEnums {
    pub values: Vec<Option<String>>,
    pub markdown_descriptions: Vec<String>,
}

pub fn create_profile_schema_enums(detected_profiles: &[ITerminalProfile], extension_profiles: Option<&[IExtensionTerminalProfile]>) -> ProfileSchemaEnums {
    let mut result: Vec<(Option<String>, String)> = vec![(None, localize("terminalAutomaticProfile", "Automatically detect the default", &[]))];
    result.extend(detected_profiles.iter().map(|profile| (Some(profile.profile_name.clone()), create_profile_description(profile))));
    if let Some(extension_profiles) = extension_profiles {
        result.extend(extension_profiles.iter().map(|profile| (Some(profile.title.clone()), create_extension_profile_description(profile))));
    }
    ProfileSchemaEnums {
        values: result.iter().map(|(name, _)| name.clone()).collect(),
        markdown_descriptions: result.into_iter().map(|(_, description)| description).collect(),
    }
}

/// O `${ThemeIcon.isThemeIcon(icon) ? icon.id : icon ? icon : Codicon.terminal.id}` do original, para um
/// ícone que não é texto.
fn icon_description_id(icon: Option<&TerminalIcon>) -> String {
    match icon {
        None => CODICON_TERMINAL_ID.to_owned(),
        Some(TerminalIcon::Theme(theme_icon)) => theme_icon.id.clone(),
        // `${uri}` chama `URI#toString()`.
        Some(TerminalIcon::Uri(UriLike::Uri(uri))) => uri.to_string(false).to_string_lossy(),
        Some(TerminalIcon::Uri(UriLike::Components(_)) | TerminalIcon::LightDark(_)) => "[object Object]".to_owned(),
    }
}

fn create_profile_description(profile: &ITerminalProfile) -> String {
    let mut description = format!("$({}) {}\n- path: {}", icon_description_id(profile.icon.as_ref()), profile.profile_name, profile.path);
    // `if (profile.args)`: a string vazia é falsa, o array vazio é verdadeiro.
    match &profile.args {
        Some(OneOrMany::One(args)) if !args.is_empty() => description.push_str(&format!("\n- args: \"{args}\"")),
        Some(OneOrMany::Many(args)) => {
            let joined = if args.is_empty() { String::new() } else { format!("'{}'", args.join("','")) };
            description.push_str(&format!("\n- args: [{joined}]"));
        }
        _ => {}
    }
    if let Some(override_name) = profile.override_name {
        description.push_str(&format!("\n- overrideName: {override_name}"));
    }
    if let Some(color) = profile.color.as_ref().filter(|color| !color.is_empty()) {
        description.push_str(&format!("\n- color: {color}"));
    }
    if let Some(env) = &profile.env {
        description.push_str(&format!("\n- env: {}", environment_to_json(env)));
    }
    description
}

/// `JSON.stringify(env)`: o `undefined` omite a chave, o `null` sai como `null`.
fn environment_to_json(env: &ITerminalEnvironment) -> String {
    let entries = env.iter().map(|(key, value)| {
        let value = match value {
            TerminalEnvironmentValue::Value(text) => Dynamic::string(text),
            TerminalEnvironmentValue::Null => Dynamic::Null,
            TerminalEnvironmentValue::Undefined => Dynamic::Undefined,
        };
        (key.as_str(), value)
    });
    safe_stringify(&Dynamic::object(entries)).unwrap_or_default()
}

fn create_extension_profile_description(profile: &IExtensionTerminalProfile) -> String {
    let icon_id = match &profile.icon {
        // `icon ? icon : Codicon.terminal.id`: o texto vazio é falso.
        Some(ContributedIcon::Text(text)) if !text.is_empty() => text.clone(),
        Some(ContributedIcon::Icon(icon)) => icon_description_id(Some(icon)),
        _ => icon_description_id(None),
    };
    format!("$({icon_id}) {}\n- extensionIdentifier: {}", profile.title, profile.extension_identifier)
}

/// `!args`: `undefined` e a string vazia são falsos.
fn args_are_falsy(args: Option<&OneOrMany<String>>) -> bool {
    match args {
        None => true,
        Some(OneOrMany::One(args)) => args.is_empty(),
        Some(OneOrMany::Many(_)) => false,
    }
}

pub fn terminal_profile_args_match(args1: Option<&OneOrMany<String>>, args2: Option<&OneOrMany<String>>) -> bool {
    if args_are_falsy(args1) && args_are_falsy(args2) {
        return true;
    }
    match (args1, args2) {
        (Some(OneOrMany::One(a)), Some(OneOrMany::One(b))) => a == b,
        (Some(OneOrMany::Many(a)), Some(OneOrMany::Many(b))) => a == b,
        _ => false,
    }
}

pub fn terminal_icons_equal(a: Option<&TerminalIcon>, b: Option<&TerminalIcon>) -> bool {
    let (a, b) = match (a, b) {
        (None, None) => return true,
        (Some(a), Some(b)) => (a, b),
        _ => return false,
    };

    match (a, b) {
        (TerminalIcon::Theme(a), TerminalIcon::Theme(b)) => a.id == b.id && a.color == b.color,
        (TerminalIcon::LightDark(a), TerminalIcon::LightDark(b)) => {
            // Dois objetos `{ light, dark }` só se igualam pelos caminhos; sem componentes de URI nos quatro
            // campos o original cai no último ramo, que também devolve `false` (um objeto `{ light, dark }`
            // não tem `scheme` nem é `UriComponents`).
            let all_uri_like = [&a.light, &a.dark, &b.light, &b.dark].iter().all(|uri| uri.is_uri_components());
            all_uri_like && a.light.path() == b.light.path() && a.dark.path() == b.dark.path()
        }
        (TerminalIcon::Uri(a), TerminalIcon::Uri(b)) => {
            let comparable = (a.is_uri() && b.is_uri()) || a.is_uri_components() || b.is_uri_components();
            comparable && a.path() == b.path() && a.scheme() == b.scheme()
        }
        // Famílias diferentes: o `path` ou o `scheme` de um lado é `undefined` e o do outro não.
        _ => false,
    }
}
