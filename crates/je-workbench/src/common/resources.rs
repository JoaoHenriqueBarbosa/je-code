//! Porte de `src/vs/workbench/common/resources.ts`: o `ResourceGlobMatcher`, que casa recursos contra uma expressão
//! glob lida da configuração, uma por pasta do workspace mais uma global.
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados (`@IWorkspaceContextService`, `@IConfigurationService`) são argumentos do
//!   construtor [`ResourceGlobMatcher::new`], na ordem do original; o original estende `Disposable` e aqui o
//!   `Disposable` fica no campo `base`.
//! - **`IExpression`.** A expressão que a configuração devolve é um [`Dynamic`] objeto (`getExpression` devolve
//!   `Option<Dynamic>`); o `equals` do original é o `objects::equals` sobre ele e o `parse` compila as entradas para
//!   [`Expression`] (`Expression::from_dynamic`): `false`/`true` são `ExprValue::Bool`, `{ when: string }` é
//!   `ExprValue::When` e qualquer outro valor é `ExprValue::Other` (vale como `true`).
//! - **`Map<string | null, ...>`.** A chave `NO_FOLDER` (`null`) é `None` e as pastas são `Some(uri.toString())`.
//! - **`hasSibling`.** É o [`HasSibling`] do glob (`boolean | Promise<boolean>`); um resultado pendente conta como casamento
//!   (a promessa do original é truthy).
//! - **`this`.** Os ouvintes registrados no construtor capturam um `Weak` do próprio matcher (`Rc::new_cyclic`).

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::{Rc, Weak};

use je_base::dynamic::Dynamic;
use je_base::event::{Emitter, Event};
use je_base::extpath::get_drive_letter;
use je_base::glob::{self, ExprMatch, Expression, GlobOptions, HasSibling, ParsedExpression};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::ResourceSet;
use je_base::network::schemas;
use je_base::objects;
use je_base::path::PathModule;
use je_base::uri::URI;
use je_platform::configuration::models::ConfigurationChangeEvent;
use je_platform::configuration::service::IConfigurationService;
use je_platform::resources::{ext_uri, fs_path, host_environment};
use je_platform::workspace::IWorkspaceContextService;

/// `IConfiguredExpression`.
#[derive(Clone)]
struct ConfiguredExpression {
    expression: Dynamic,
    has_absolute_path: bool,
}

/// `getExpression: (folder?: URI) => IExpression | undefined`.
pub type GetExpression = Rc<dyn Fn(Option<&URI>) -> Option<Dynamic>>;

/// `shouldUpdate: (event: IConfigurationChangeEvent) => boolean`.
pub type ShouldUpdate = Rc<dyn Fn(&ConfigurationChangeEvent) -> bool>;

/// Os dois mapas do original (`mapFolderToParsedExpression` e `mapFolderToConfiguredExpression`), pela chave da pasta.
#[derive(Default)]
struct ExpressionMaps {
    parsed: HashMap<Option<String>, ParsedExpression>,
    configured: HashMap<Option<String>, ConfiguredExpression>,
}

/// `ResourceGlobMatcher`.
pub struct ResourceGlobMatcher {
    base: Rc<Disposable>,
    on_expression_change: Rc<Emitter<()>>,
    maps: RefCell<ExpressionMaps>,
    get_expression: GetExpression,
    context_service: Rc<dyn IWorkspaceContextService>,
}

/// `ResourceGlobMatcher.NO_FOLDER`.
const NO_FOLDER: Option<String> = None;

impl ResourceGlobMatcher {
    pub fn new(
        get_expression: GetExpression,
        should_update: ShouldUpdate,
        context_service: Rc<dyn IWorkspaceContextService>,
        configuration_service: Rc<dyn IConfigurationService>,
    ) -> Rc<Self> {
        let base = Disposable::new();
        let on_expression_change = Emitter::new();
        base.register(on_expression_change.clone());

        let this = Rc::new(Self { base, on_expression_change, maps: RefCell::new(ExpressionMaps::default()), get_expression, context_service });

        this.update_expressions(false);

        this.register_listeners(&configuration_service, should_update);

        this
    }

    /// `onExpressionChange`.
    pub fn on_expression_change(&self) -> Event<()> {
        self.on_expression_change.event()
    }

    fn register_listeners(self: &Rc<Self>, configuration_service: &Rc<dyn IConfigurationService>, should_update: ShouldUpdate) {
        let weak: Weak<Self> = Rc::downgrade(self);
        self.base.register(configuration_service.on_did_change_configuration().listen({
            let weak = weak.clone();
            move |e| {
                if let Some(this) = weak.upgrade()
                    && should_update(e)
                {
                    this.update_expressions(true);
                }
            }
        }));

        self.base.register(self.context_service.on_did_change_workspace_folders().listen(move |_| {
            if let Some(this) = weak.upgrade() {
                this.update_expressions(true);
            }
        }));
    }

    fn update_expressions(&self, from_event: bool) {
        let mut changed = false;
        let folders = self.context_service.get_workspace().folders();

        // Add expressions per workspaces that got added
        for folder in &folders {
            let folder_uri_str = Some(folder.uri.to_string(false).to_string_lossy());

            let new_expression = self.do_get_expression(Some(&folder.uri));
            let current_expression = self.maps.borrow().configured.get(&folder_uri_str).cloned();

            if let Some(new_expression) = new_expression {
                if current_expression.is_none_or(|current| !objects::equals(&current.expression, &new_expression.expression)) {
                    changed = true;

                    let mut maps = self.maps.borrow_mut();
                    maps.parsed.insert(folder_uri_str.clone(), parse_expression(&new_expression.expression));
                    maps.configured.insert(folder_uri_str, new_expression);
                }
            } else if current_expression.is_some() {
                changed = true;

                let mut maps = self.maps.borrow_mut();
                maps.parsed.remove(&folder_uri_str);
                maps.configured.remove(&folder_uri_str);
            }
        }

        // Remove expressions per workspace no longer present
        let folders_map = ResourceSet::from_entries(folders.iter().map(|folder| folder.uri.clone()), None);
        let configured_folders: Vec<Option<String>> = self.maps.borrow().configured.keys().cloned().collect();
        for folder in configured_folders {
            let Some(folder_uri) = &folder else {
                continue; // always keep this one
            };

            if !folders_map.has(&URI::parse(folder_uri.as_str(), false)) {
                let mut maps = self.maps.borrow_mut();
                maps.parsed.remove(&folder);
                maps.configured.remove(&folder);

                changed = true;
            }
        }

        // Always set for resources outside workspace as well
        let global_new_expression = self.do_get_expression(None);
        let global_current_expression = self.maps.borrow().configured.get(&NO_FOLDER).cloned();
        if let Some(global_new_expression) = global_new_expression {
            if global_current_expression.is_none_or(|current| !objects::equals(&current.expression, &global_new_expression.expression)) {
                changed = true;

                let mut maps = self.maps.borrow_mut();
                maps.parsed.insert(NO_FOLDER, parse_expression(&global_new_expression.expression));
                maps.configured.insert(NO_FOLDER, global_new_expression);
            }
        } else if global_current_expression.is_some() {
            changed = true;

            let mut maps = self.maps.borrow_mut();
            maps.parsed.remove(&NO_FOLDER);
            maps.configured.remove(&NO_FOLDER);
        }

        if from_event && changed {
            self.on_expression_change.fire(());
        }
    }

    fn do_get_expression(&self, resource: Option<&URI>) -> Option<ConfiguredExpression> {
        let expression = (self.get_expression)(resource)?;

        let keys = expression.own_keys();
        if keys.is_empty() {
            return None;
        }

        let env = host_environment();
        let path = PathModule::native(env.as_ref());
        let mut has_absolute_path = false;

        // Check the expression for absolute paths/globs and specifically for Windows, make sure the
        // drive letter is lowercased, because we later check with `URI.fsPath` which is always putting
        // the drive letter lowercased.

        let mut massaged_entries: Vec<(String, Dynamic)> = Vec::with_capacity(keys.len());
        for key in keys {
            let key_text = key.to_string_lossy();
            if !has_absolute_path {
                has_absolute_path = path.is_absolute(&key_text);
            }

            let mut massaged_key = key_text.clone();

            if let Some(drive_letter) = get_drive_letter(&massaged_key, true /* probe for windows */) {
                let drive_letter_lower = drive_letter.to_lowercase().to_string();
                if drive_letter.to_string() != drive_letter_lower {
                    massaged_key = format!("{drive_letter_lower}{}", &massaged_key[drive_letter.len_utf8()..]);
                }
            }

            massaged_entries.push((massaged_key, expression.get(&key)));
        }

        Some(ConfiguredExpression { expression: Dynamic::object(massaged_entries), has_absolute_path })
    }

    pub fn matches(&self, resource: &URI, has_sibling: Option<HasSibling<'_>>) -> bool {
        let (expression_for_folder, expression_config_for_folder, folder) = {
            let maps = self.maps.borrow();
            if maps.parsed.is_empty() {
                return false; // return early: no expression for this matcher
            }

            let folder = self.context_service.get_workspace_folder(resource);
            let folder_key = folder.as_ref().map(|folder| Some(folder.uri.to_string(false).to_string_lossy()));
            match folder_key.filter(|key| maps.parsed.contains_key(key)) {
                Some(key) => (maps.parsed.get(&key).cloned(), maps.configured.get(&key).cloned(), folder),
                None => (maps.parsed.get(&NO_FOLDER).cloned(), maps.configured.get(&NO_FOLDER).cloned(), folder),
            }
        };

        let Some(expression_for_folder) = expression_for_folder else {
            return false; // return early: no expression for this resource
        };

        // If the resource if from a workspace, convert its absolute path to a relative
        // path so that glob patterns have a higher probability to match. For example
        // a glob pattern of "src/**" will not match on an absolute path "/folder/src/file.txt"
        // but can match on "src/file.txt"

        let resource_path_to_match: Option<String> = match &folder {
            Some(folder) => ext_uri().relative_path(&folder.uri, resource),
            None => Some(uri_to_path(resource)),
        };

        let is_match = |path: &str| !matches!(expression_for_folder.matches(path, None, has_sibling), ExprMatch::None);

        if resource_path_to_match.as_deref().is_some_and(is_match) {
            return true;
        }

        // If the configured expression has an absolute path, we also check for absolute paths
        // to match, otherwise we potentially miss out on matches. We only do that if we previously
        // matched on the relative path.

        if resource_path_to_match.as_deref() != Some(uri_to_path(resource).as_str()) && expression_config_for_folder.is_some_and(|config| config.has_absolute_path) {
            return is_match(&uri_to_path(resource));
        }

        false
    }
}

impl IDisposable for ResourceGlobMatcher {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// `uriToPath`.
fn uri_to_path(uri: &URI) -> String {
    if uri.scheme().to_string_lossy() == schemas::FILE {
        return fs_path(uri);
    }

    uri.path().to_string_lossy()
}

/// `parse(expression)` sobre o objeto da configuração.
fn parse_expression(expression: &Dynamic) -> ParsedExpression {
    glob::parse_expression(&Expression::from_dynamic(expression), &GlobOptions::default())
}
