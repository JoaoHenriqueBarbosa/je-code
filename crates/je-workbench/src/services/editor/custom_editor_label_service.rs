//! Porte de `src/vs/workbench/services/editor/common/customEditorLabelService.ts`: o serviço que resolve o nome de um
//! editor a partir dos padrões `workbench.editor.customLabels.patterns` (glob para um modelo com `${filename}`,
//! `${extname}`, `${extname(N)}`, `${dirname}` e `${dirname(N)}`).
//!
//! Divergências:
//! - **Injeção.** Os parâmetros decorados do construtor são a tupla `Deps` do [`Injectable`], na ordem do original; a
//!   classe herda de `Disposable`, aqui um `Disposable` guardado no campo `base`.
//!   `registerSingleton(..., InstantiationType.Delayed)` vira [`register`], e o proxy da instanciação atrasada é o
//!   [`CustomEditorLabelServiceProxy`].
//! - **`getValue<boolean>(enabled)`** pode ser `undefined`: o estado é guardado como [`Dynamic`] (comparado com `!==` por
//!   `strict_equals` e lido como `!this.enabled` por `is_truthy`), como no original.
//! - **Regex do modelo.** `_parsedTemplateExpression` e `_filenameCaptureExpression` são varridos à mão, com a mesma
//!   semântica (`/\$\{(dirname|filename|extname|extname\(N\)|dirname\(N\))\}/g`, com `N` no formato `[-+]?\d+`, e
//!   `/(?<filename>^\.*[^.]*)/`). O `parseInt` de um `N` fora do intervalo de `i64` dá um índice fora de qualquer lista,
//!   então o trecho fica como está.
//! - **`for (const pattern in customLabelPatterns)`** é `for_in_entries`, que respeita a ordem de propriedades do
//!   JavaScript. O modelo é lido como texto (`to_js_string`), que é o que o `RegExp.test` faz com o valor.
//! - **`path.ts`** (`isAbsolute`, `parse`, `dirname`) é o [`PathModule::native`] do processo hospedeiro, e `resources.ts`
//!   (`dirname`, `relativePath`) é o `extUri` sem viés.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use je_base::dynamic::{Dynamic, Truthy};
use je_base::event::{Emitter, EmitterOptions, Event};
use je_base::glob::{GlobOptions, ParsedPattern, parse_pattern};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::map::MRUCache;
use je_base::path::{PathModule, ProcessEnvironment};
use je_base::uri::URI;
use je_instantiation::{Injectable, InstantiationType, delayed_proxy, register_singleton, service_id, service_impl};
use je_platform::configuration::service::IConfigurationService;
use je_platform::js::{for_in_entries, to_js_string};
use je_platform::resources::ext_uri;
use je_platform::workspace::IWorkspaceContextService;

/// `ICustomEditorLabelPattern`.
#[derive(Clone)]
struct CustomEditorLabelPattern {
    pattern: String,
    template: String,
    is_absolute_path: bool,
    parsed_pattern: ParsedPattern,
}

/// `ICustomEditorLabelService`.
pub trait ICustomEditorLabelService {
    fn on_did_change(&self) -> Event<()>;
    fn get_name(&self, resource: &URI) -> Option<String>;
}

delayed_proxy!(pub CustomEditorLabelServiceProxy for dyn ICustomEditorLabelService {
    methods {
        fn get_name(&self, resource: &URI) -> Option<String>;
    }
    events {
        fn on_did_change(&self) -> ();
    }
});

service_id!(pub I_CUSTOM_EDITOR_LABEL_SERVICE: dyn ICustomEditorLabelService = "customEditorLabelService", proxy CustomEditorLabelServiceProxy);

/// O `path` do processo hospedeiro (os exports de nível de módulo de `path.ts`).
fn native_path() -> PathModule<'static> {
    PathModule::native(&ProcessEnvironment)
}

/// `CustomEditorLabelService`.
pub struct CustomEditorLabelService {
    this: Weak<CustomEditorLabelService>,
    base: Rc<Disposable>,

    on_did_change: Rc<Emitter<()>>,

    patterns: RefCell<Vec<CustomEditorLabelPattern>>,
    enabled: RefCell<Dynamic>,

    cache: MRUCache<String, Option<String>>,

    configuration_service: Rc<dyn IConfigurationService>,
    workspace_context_service: Rc<dyn IWorkspaceContextService>,
}

impl Injectable for CustomEditorLabelService {
    type Args = ();
    type Deps = (Rc<dyn IConfigurationService>, Rc<dyn IWorkspaceContextService>);

    fn create(_: (), (configuration_service, workspace_context_service): Self::Deps) -> Rc<Self> {
        let base = Disposable::new();

        // increased for users with hundreds of inputs opened
        let on_did_change = Emitter::with_options(EmitterOptions {
            leak_warning_threshold: Some(500),
            leak_warning_name: Some("CustomEditorLabelService._onDidChange".to_string()),
            ..EmitterOptions::default()
        });
        base.register(on_did_change.clone());

        let this = Rc::new_cyclic(|this| Self {
            this: this.clone(),
            base,
            on_did_change,
            patterns: RefCell::new(Vec::new()),
            enabled: RefCell::new(Dynamic::Bool(true)),
            cache: MRUCache::new(1000, 1.0),
            configuration_service,
            workspace_context_service,
        });

        this.store_enablement_state();
        this.store_custom_patterns();

        this.register_listeners();

        this
    }
}

service_impl!(CustomEditorLabelService => dyn ICustomEditorLabelService, disposable);

/// `registerSingleton(ICustomEditorLabelService, CustomEditorLabelService, InstantiationType.Delayed)`.
pub fn register() {
    register_singleton::<CustomEditorLabelService>(I_CUSTOM_EDITOR_LABEL_SERVICE, InstantiationType::Delayed);
}

impl IDisposable for CustomEditorLabelService {
    fn dispose(&self) {
        self.base.dispose();
    }
}

/// O que `_parsedTemplateExpression` reconhece dentro de `${...}`.
enum TemplateVariable {
    Filename,
    Extname,
    ExtnameN(Option<i64>),
    Dirname,
    DirnameN(Option<i64>),
}

impl CustomEditorLabelService {
    pub const SETTING_ID_PATTERNS: &'static str = "workbench.editor.customLabels.patterns";
    pub const SETTING_ID_ENABLED: &'static str = "workbench.editor.customLabels.enabled";

    fn register_listeners(&self) {
        let weak = self.this.clone();
        self.base.register(self.configuration_service.on_did_change_configuration().listen(move |e| {
            let Some(this) = weak.upgrade() else {
                return;
            };

            // Cache the enabled state
            if e.affects_configuration(Self::SETTING_ID_ENABLED, None) {
                let old_enablement = this.enabled.borrow().clone();
                this.store_enablement_state();
                let changed = !old_enablement.strict_equals(&this.enabled.borrow());
                if changed && !this.patterns.borrow().is_empty() {
                    this.on_did_change.fire(());
                }
            }
            // Cache the patterns
            else if e.affects_configuration(Self::SETTING_ID_PATTERNS, None) {
                this.cache.clear();
                this.store_custom_patterns();
                this.on_did_change.fire(());
            }
        }));
    }

    fn store_enablement_state(&self) {
        *self.enabled.borrow_mut() = self.configuration_service.get_value(Some(Self::SETTING_ID_ENABLED), None);
    }

    /// `_templateRegexValidation.test(template)`: `/[a-zA-Z0-9]/`.
    fn is_valid_template(template: &str) -> bool {
        template.chars().any(|c| c.is_ascii_alphanumeric())
    }

    fn store_custom_patterns(&self) {
        let mut patterns = Vec::new();
        let custom_label_patterns = self.configuration_service.get_value(Some(Self::SETTING_ID_PATTERNS), None);
        for (pattern, template) in for_in_entries(&custom_label_patterns) {
            let pattern = pattern.to_string_lossy();
            let template = to_js_string(&template).to_string_lossy();

            if !Self::is_valid_template(&template) {
                continue;
            }

            let is_absolute_path = native_path().is_absolute(&pattern);
            let parsed_pattern = parse_pattern(pattern.as_str(), &GlobOptions { ignore_case: Some(true), ..GlobOptions::default() });

            patterns.push(CustomEditorLabelPattern { pattern, template, is_absolute_path, parsed_pattern });
        }

        patterns.sort_by(|a, b| Self::pattern_weight(&b.pattern).cmp(&Self::pattern_weight(&a.pattern)));
        *self.patterns.borrow_mut() = patterns;
    }

    fn pattern_weight(pattern: &str) -> u32 {
        let mut weight = 0;
        for fragment in pattern.split('/') {
            if fragment == "**" {
                weight += 1;
            } else if fragment == "*" {
                weight += 10;
            } else if fragment.contains('*') || fragment.contains('?') {
                weight += 50;
            } else if !fragment.is_empty() {
                weight += 100;
            }
        }

        weight
    }

    fn apply_patterns(&self, resource: &URI) -> Option<String> {
        let root = self.workspace_context_service.get_workspace_folder(resource);
        let mut relative_path: Option<String> = None;

        let patterns = self.patterns.borrow().clone();
        for pattern in &patterns {
            let relevant_path = match &root {
                Some(root) if !pattern.is_absolute_path => {
                    if relative_path.as_deref().is_none_or(str::is_empty) {
                        let extension_uri = ext_uri();
                        relative_path =
                            Some(extension_uri.relative_path(&extension_uri.dirname(&root.uri), resource).unwrap_or_else(|| resource.path().to_string_lossy()));
                    }
                    relative_path.clone().unwrap_or_default()
                }
                _ => resource.path().to_string_lossy(),
            };

            if pattern.parsed_pattern.matches(&relevant_path, None) {
                return Some(Self::apply_template(&pattern.template, resource, &relevant_path));
            }
        }

        None
    }

    /// `template.replace(_parsedTemplateExpression, ...)`: troca cada `${...}` reconhecido pelo valor que a variável
    /// resolve, e deixa o trecho como está quando a variável não resolve.
    fn apply_template(template: &str, resource: &URI, relevant_path: &str) -> String {
        let mut base_name: Option<String> = None;
        let mut result = String::with_capacity(template.len());

        let mut rest = template;
        while let Some(start) = rest.find("${") {
            result.push_str(&rest[..start]);
            let candidate = &rest[start..];

            let Some((variable, length)) = Self::parse_template_variable(candidate) else {
                // Sem casamento nesta posição, a varredura segue um caractere adiante.
                result.push('$');
                rest = &candidate[1..];
                continue;
            };

            let matched = &candidate[..length];
            let base = base_name.get_or_insert_with(|| native_path().parse(&resource.path().to_string_lossy()).base);
            match Self::resolve_variable(&variable, base, relevant_path) {
                Some(resolved) => result.push_str(&resolved),
                None => result.push_str(matched),
            }
            rest = &candidate[length..];
        }
        result.push_str(rest);

        result
    }

    /// Reconhece `${dirname}`, `${filename}`, `${extname}`, `${extname(N)}` e `${dirname(N)}` no começo do texto e
    /// devolve a variável com o tamanho do trecho.
    fn parse_template_variable(text: &str) -> Option<(TemplateVariable, usize)> {
        let inner = text.strip_prefix("${")?;
        let name_length = inner.find(|c: char| !c.is_ascii_lowercase()).unwrap_or(inner.len());
        let (name, after_name) = inner.split_at(name_length);

        if let Some(after) = after_name.strip_prefix('}') {
            let variable = match name {
                "dirname" => TemplateVariable::Dirname,
                "filename" => TemplateVariable::Filename,
                "extname" => TemplateVariable::Extname,
                _ => return None,
            };
            return Some((variable, text.len() - after.len()));
        }

        let after_open = after_name.strip_prefix('(')?;
        let digits_start = usize::from(after_open.starts_with(['-', '+']));
        let digits_length = after_open[digits_start..].find(|c: char| !c.is_ascii_digit()).unwrap_or(after_open.len() - digits_start);
        if digits_length == 0 {
            return None;
        }
        let (number, after_number) = after_open.split_at(digits_start + digits_length);
        let after = after_number.strip_prefix(")}")?;
        let parsed = number.parse::<i64>().ok();
        let variable = match name {
            "extname" => TemplateVariable::ExtnameN(parsed),
            "dirname" => TemplateVariable::DirnameN(parsed),
            _ => return None,
        };
        Some((variable, text.len() - after.len()))
    }

    /// O corpo do callback do `replace`: o valor da variável, ou `None` quando ela não resolve (o callback devolve o
    /// próprio trecho casado).
    fn resolve_variable(variable: &TemplateVariable, base_name: &str, relevant_path: &str) -> Option<String> {
        match variable {
            TemplateVariable::Filename => {
                // `/(?<filename>^\.*[^.]*)/`
                let leading_dots = base_name.len() - base_name.trim_start_matches('.').len();
                let name_length = base_name[leading_dots..].find('.').unwrap_or(base_name.len() - leading_dots);
                let filename = &base_name[..leading_dots + name_length];
                (!filename.is_empty()).then(|| filename.to_string())
            }
            TemplateVariable::Extname => Some(Self::get_extnames(base_name)).filter(|extension| !extension.is_empty()),
            TemplateVariable::ExtnameN(n) => (*n).and_then(|n| Self::get_nth_extname(base_name, n)),
            TemplateVariable::Dirname => Self::get_nth_dirname(&native_path().dirname(relevant_path), 0),
            TemplateVariable::DirnameN(n) => (*n).and_then(|n| Self::get_nth_dirname(&native_path().dirname(relevant_path), n)),
        }
    }

    fn remove_leading_dot(path: &str) -> &str {
        path.trim_start_matches('.')
    }

    fn get_nth_dirname(path: &str, n: i64) -> Option<String> {
        // grand-parent/parent/filename.ext1.ext2 -> [grand-parent, parent]
        let path = path.strip_prefix('/').unwrap_or(path);
        let path_fragments: Vec<&str> = path.split('/').collect();

        Self::get_nth_fragment(&path_fragments, n)
    }

    fn get_extnames(full_file_name: &str) -> String {
        Self::remove_leading_dot(full_file_name).split('.').skip(1).collect::<Vec<_>>().join(".")
    }

    fn get_nth_extname(full_file_name: &str, n: i64) -> Option<String> {
        // file.ext1.ext2.ext3 -> [file, ext1, ext2, ext3]
        // O primeiro elemento (o nome do arquivo) sai da lista.
        let extension_name_fragments: Vec<&str> = Self::remove_leading_dot(full_file_name).split('.').skip(1).collect();

        Self::get_nth_fragment(&extension_name_fragments, n)
    }

    fn get_nth_fragment(fragments: &[&str], n: i64) -> Option<String> {
        let length = fragments.len() as i64;

        let nth = if n < 0 { i64::try_from(n.unsigned_abs()).ok()? - 1 } else { length - n - 1 };

        let nth_fragment = usize::try_from(nth).ok().and_then(|index| fragments.get(index))?;
        if nth_fragment.is_empty() {
            return None;
        }
        Some((*nth_fragment).to_string())
    }
}

impl ICustomEditorLabelService for CustomEditorLabelService {
    fn on_did_change(&self) -> Event<()> {
        self.on_did_change.event()
    }

    fn get_name(&self, resource: &URI) -> Option<String> {
        if !self.enabled.borrow().is_truthy() || self.patterns.borrow().is_empty() {
            return None;
        }

        let key = resource.to_string(false).to_string_lossy();
        if let Some(cached) = self.cache.get(&key) {
            return cached;
        }

        let result = self.apply_patterns(resource);
        self.cache.set(key, result.clone());

        result
    }
}
