//! Porte de `src/vs/workbench/contrib/terminal/browser/environmentVariableInfo.ts`: a informação que um terminal guarda sobre a
//! coleção de variáveis de ambiente das extensões (`EnvironmentVariableInfoStale`, quando o ambiente mudou e pede relançamento,
//! e `EnvironmentVariableInfoChangesActive`, quando as contribuições estão ativas) e o status que cada uma mostra.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`], na ordem do original, e os
//!   estáticos são os `Args`.
//! - **`Codicon.warning`** é o `ThemeIcon` de id `warning`.
//! - **`executeCommand(ShowEnvironmentContributions, scope)`.** O escopo chega ao comando como o objeto `{ workspaceFolder: { uri,
//!   name, index } }`, com o `uri` no texto de `URI.toString()` (o `Dynamic` não carrega um `URI`); sem escopo vai `undefined`.
//! - **`extensionService.extensions.find(e => e.id === id)?.displayName`** lê o `display_name` do manifesto da extensão.
//! - **`Set<string>` de extensões** é um `Vec` sem repetição, na ordem de inserção.

use std::rc::Rc;

use je_base::dynamic::Dynamic;
use je_base::nls::localize;
use je_base::severity::Severity;
use je_base::themables::ThemeIcon;
use je_instantiation::Injectable;
use je_platform::commands::commands::ICommandService;
use je_platform::terminal::environment_variable::{
    EnvironmentVariableScope, IExtensionOwnedEnvironmentVariableMutator, IMergedEnvironmentVariableCollection, IMergedEnvironmentVariableCollectionDiff,
};

use super::terminal::ITerminalService;
use super::terminal_status_list::TerminalStatus;
use crate::contrib::terminal::common::environment_variable::IEnvironmentVariableInfo;
use crate::contrib::terminal::common::terminal::{ITerminalStatus, ITerminalStatusHoverAction, TerminalCommandId};
use crate::services::extensions::extensions::IExtensionService;

/// `EnvironmentVariableInfoStale`.
pub struct EnvironmentVariableInfoStale {
    diff: IMergedEnvironmentVariableCollectionDiff,
    terminal_id: u32,
    collection: Rc<dyn IMergedEnvironmentVariableCollection>,
    terminal_service: Rc<dyn ITerminalService>,
    extension_service: Rc<dyn IExtensionService>,
}

impl EnvironmentVariableInfoStale {
    fn get_info(&self, scope: Option<&EnvironmentVariableScope>) -> String {
        let mut ext_set: Vec<String> = Vec::new();
        add_extension_identifiers(&mut ext_set, self.diff.added.iter().map(|(_, mutators)| mutators));
        add_extension_identifiers(&mut ext_set, self.diff.removed.iter().map(|(_, mutators)| mutators));
        add_extension_identifiers(&mut ext_set, self.diff.changed.iter().map(|(_, mutators)| mutators));

        let mut message = localize(
            "extensionEnvironmentContributionInfoStale",
            "The following extensions want to relaunch the terminal to contribute to its environment:",
            &[],
        );
        message.push_str(&get_merged_description(&*self.collection, scope, &*self.extension_service, &ext_set));
        message
    }

    fn get_actions(&self) -> Vec<ITerminalStatusHoverAction> {
        let terminal_service = self.terminal_service.clone();
        let terminal_id = self.terminal_id;
        vec![ITerminalStatusHoverAction {
            label: localize("relaunchTerminalLabel", "Relaunch Terminal", &[]),
            run: Rc::new(move || {
                if let Some(instance) = terminal_service.get_instance_from_id(terminal_id) {
                    instance.relaunch();
                }
            }),
            command_id: TerminalCommandId::Relaunch.as_str().to_owned(),
        }]
    }
}

impl IEnvironmentVariableInfo for EnvironmentVariableInfoStale {
    fn requires_action(&self) -> bool {
        true
    }

    fn get_status(&self, scope: Option<&EnvironmentVariableScope>) -> ITerminalStatus {
        ITerminalStatus {
            id: TerminalStatus::RelaunchNeeded.as_str().to_owned(),
            severity: Severity::Warning,
            icon: Some(ThemeIcon::from_id("warning")),
            tooltip: Some(self.get_info(scope)),
            detailed_tooltip: None,
            hover_actions: Some(self.get_actions()),
        }
    }
}

impl Injectable for EnvironmentVariableInfoStale {
    type Args = (IMergedEnvironmentVariableCollectionDiff, u32, Rc<dyn IMergedEnvironmentVariableCollection>);
    type Deps = (Rc<dyn ITerminalService>, Rc<dyn IExtensionService>);

    fn create((diff, terminal_id, collection): Self::Args, (terminal_service, extension_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { diff, terminal_id, collection, terminal_service, extension_service })
    }
}

/// `EnvironmentVariableInfoChangesActive`.
pub struct EnvironmentVariableInfoChangesActive {
    collection: Rc<dyn IMergedEnvironmentVariableCollection>,
    command_service: Rc<dyn ICommandService>,
    extension_service: Rc<dyn IExtensionService>,
}

impl EnvironmentVariableInfoChangesActive {
    fn get_info(&self, scope: Option<&EnvironmentVariableScope>) -> String {
        let mut ext_set: Vec<String> = Vec::new();
        add_extension_identifiers(&mut ext_set, self.collection.get_variable_map(scope).iter().map(|(_, mutators)| mutators));

        let mut message = localize("extensionEnvironmentContributionInfoActive", "The following extensions have contributed to this terminal's environment:", &[]);
        message.push_str(&get_merged_description(&*self.collection, scope, &*self.extension_service, &ext_set));
        message
    }

    fn get_actions(&self, scope: Option<&EnvironmentVariableScope>) -> Vec<ITerminalStatusHoverAction> {
        let command_service = self.command_service.clone();
        let scope_argument = scope_to_command_argument(scope);
        vec![ITerminalStatusHoverAction {
            label: localize("showEnvironmentContributions", "Show Environment Contributions", &[]),
            run: Rc::new(move || {
                let _ = command_service.execute_command(TerminalCommandId::ShowEnvironmentContributions.as_str(), vec![scope_argument.clone()]);
            }),
            command_id: TerminalCommandId::ShowEnvironmentContributions.as_str().to_owned(),
        }]
    }
}

impl IEnvironmentVariableInfo for EnvironmentVariableInfoChangesActive {
    fn requires_action(&self) -> bool {
        false
    }

    fn get_status(&self, scope: Option<&EnvironmentVariableScope>) -> ITerminalStatus {
        ITerminalStatus {
            id: TerminalStatus::EnvironmentVariableInfoChangesActive.as_str().to_owned(),
            severity: Severity::Info,
            icon: None,
            tooltip: None, // The action is present when details aren't shown
            detailed_tooltip: Some(self.get_info(scope)),
            hover_actions: Some(self.get_actions(scope)),
        }
    }
}

impl Injectable for EnvironmentVariableInfoChangesActive {
    type Args = (Rc<dyn IMergedEnvironmentVariableCollection>,);
    type Deps = (Rc<dyn ICommandService>, Rc<dyn IExtensionService>);

    fn create((collection,): Self::Args, (command_service, extension_service): Self::Deps) -> Rc<Self> {
        Rc::new(Self { collection, command_service, extension_service })
    }
}

/// O escopo como argumento de comando (ver "Divergências").
fn scope_to_command_argument(scope: Option<&EnvironmentVariableScope>) -> Dynamic {
    let Some(workspace_folder) = scope.and_then(|scope| scope.workspace_folder.as_ref()) else {
        return Dynamic::Undefined;
    };
    Dynamic::object([(
        "workspaceFolder",
        Dynamic::object([
            ("uri", Dynamic::string(&workspace_folder.uri.to_string(false).to_string_lossy())),
            ("name", Dynamic::string(&workspace_folder.name)),
            ("index", Dynamic::Number(workspace_folder.index as f64)),
        ]),
    )])
}

fn get_merged_description(
    collection: &dyn IMergedEnvironmentVariableCollection,
    scope: Option<&EnvironmentVariableScope>,
    extension_service: &dyn IExtensionService,
    ext_set: &[String],
) -> String {
    let mut message: Vec<String> = vec!["\n".to_owned()];
    let global_descriptions = collection.get_description_map(None);
    let workspace_descriptions = collection.get_description_map(scope);
    for ext in ext_set {
        let global_description = global_descriptions.get(ext).and_then(|description| description.as_ref()).filter(|description| !description.is_empty());
        if let Some(global_description) = global_description {
            message.push(format!("\n- `{}`", get_extension_name(ext, extension_service)));
            message.push(format!(": {global_description}"));
        }
        let workspace_description = workspace_descriptions.get(ext).and_then(|description| description.as_ref()).filter(|description| !description.is_empty());
        if let Some(workspace_description) = workspace_description {
            // Only show '(workspace)' suffix if there is already a description for the extension.
            let workspace_suffix = if global_description.is_some() { format!(" ({})", localize("ScopedEnvironmentContributionInfo", "workspace", &[])) } else { String::new() };
            message.push(format!("\n- `{}{workspace_suffix}`", get_extension_name(ext, extension_service)));
            message.push(format!(": {workspace_description}"));
        }
        if global_description.is_none() && workspace_description.is_none() {
            message.push(format!("\n- `{}`", get_extension_name(ext, extension_service)));
        }
    }
    message.join("")
}

fn add_extension_identifiers<'a>(ext_set: &mut Vec<String>, diff: impl Iterator<Item = &'a Vec<IExtensionOwnedEnvironmentVariableMutator>>) {
    for mutators in diff {
        for mutator in mutators {
            if !ext_set.contains(&mutator.extension_identifier) {
                ext_set.push(mutator.extension_identifier.clone());
            }
        }
    }
}

fn get_extension_name(id: &str, extension_service: &dyn IExtensionService) -> String {
    extension_service
        .extensions()
        .into_iter()
        .find(|extension| extension.id == id)
        .and_then(|extension| extension.manifest.display_name.filter(|display_name| !display_name.is_empty()))
        .unwrap_or_else(|| id.to_owned())
}
