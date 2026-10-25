//! Porte de `src/vs/platform/assignment/common/assignment.ts`: o contrato do serviço de atribuição (experimentos),
//! os filtros que o ExP/TAS recebe e a decisão de conta interna.
//!
//! Divergências:
//! - `IAssignmentService.getTreatment<T extends string | number | boolean>` é [`IAssignmentService::get_treatment`], que
//!   devolve o [`TreatmentValue`] (a união `string | number | boolean`) e deixa o chamador escolher a variante.
//! - `IExperimentationFilterProvider` (de `tas-client`) é o trait [`ExperimentationFilterProvider`]: `getFilterValue` e
//!   `getFilters` com os mesmos nomes em `snake_case`. O `Map<string, unknown>` de `getFilters` é um `IndexMap`, que
//!   guarda a ordem de inserção como o `Map` do JavaScript.
//! - As constantes `platform.language` e `platform.platform` do `platform.ts` são dado detectado: os provedores
//!   recebem o [`Platform`] no construtor, no lugar de lê-lo de um global.
//! - `private version: string` etc. viram campos privados dos provedores.

use indexmap::IndexMap;
use je_base::event::Event;
use je_base::platform::Platform;
use je_base::promise::Promise;

pub const ASSIGNMENT_STORAGE_KEY: &str = "VSCode.ABExp.FeatureData";
/// 1 hour.
pub const ASSIGNMENT_REFETCH_INTERVAL: u64 = 60 * 60 * 1000;

/// O `T extends string | number | boolean` de `getTreatment`.
#[derive(Clone, Debug, PartialEq)]
pub enum TreatmentValue {
    String(String),
    Number(f64),
    Boolean(bool),
}

/// `IAssignmentService`.
pub trait IAssignmentService {
    fn on_did_refetch_assignments(&self) -> Event<()>;
    fn get_treatment(&self, name: &str) -> Promise<Option<TreatmentValue>>;
}

/// `TargetPopulation`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetPopulation {
    Insiders,
    Public,
    Exploration,
}

impl TargetPopulation {
    /// O valor de texto (`'insider'`, `'public'` ou `'exploration'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Insiders => "insider",
            Self::Public => "public",
            Self::Exploration => "exploration",
        }
    }
}

/// `Filters`: baseado nos filtros do ExP para o cluster do VS Code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filters {
    /// The market in which the extension is distributed.
    Market,
    /// The corporation network.
    CorpNet,
    /// Version of the application which uses experimentation service.
    ApplicationVersion,
    /// Insiders vs Stable.
    Build,
    /// Client Id which is used as primary unit for the experimentation.
    ClientId,
    /// Developer Device Id which can be used as an alternate unit for experimentation.
    DeveloperDeviceId,
    /// Extension header.
    ExtensionName,
    /// The version of the extension.
    ExtensionVersion,
    /// The language in use by VS Code.
    Language,
    /// The target population. This is used to separate internal, early preview, GA, etc.
    TargetPopulation,
    /// The platform (OS) on which VS Code is running.
    Platform,
    /// The release/build date of VS Code (UTC) in the format yyyymmddHH.
    ReleaseDate,
    /// The kind of window VS Code is running in (`editor` or `agents`).
    WindowKind,
}

impl Filters {
    /// Todos os filtros, na ordem de declaração (`Object.values(Filters)`).
    pub const ALL: [Filters; 13] = [
        Self::Market,
        Self::CorpNet,
        Self::ApplicationVersion,
        Self::Build,
        Self::ClientId,
        Self::DeveloperDeviceId,
        Self::ExtensionName,
        Self::ExtensionVersion,
        Self::Language,
        Self::TargetPopulation,
        Self::Platform,
        Self::ReleaseDate,
        Self::WindowKind,
    ];

    /// O valor de texto do filtro (o nome do cabeçalho).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Market => "X-MSEdge-Market",
            Self::CorpNet => "X-FD-Corpnet",
            Self::ApplicationVersion => "X-VSCode-AppVersion",
            Self::Build => "X-VSCode-Build",
            Self::ClientId => "X-MSEdge-ClientId",
            Self::DeveloperDeviceId => "X-VSCode-DevDeviceId",
            Self::ExtensionName => "X-VSCode-ExtensionName",
            Self::ExtensionVersion => "X-VSCode-ExtensionVersion",
            Self::Language => "X-VSCode-Language",
            Self::TargetPopulation => "X-VSCode-TargetPopulation",
            Self::Platform => "X-VSCode-Platform",
            Self::ReleaseDate => "X-VSCode-ReleaseDate",
            Self::WindowKind => "X-VSCode-WindowKind",
        }
    }

    fn from_str(value: &str) -> Option<Filters> {
        Self::ALL.into_iter().find(|filter| filter.as_str() == value)
    }
}

/// `WindowKind`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowKind {
    Editor,
    Agents,
}

impl WindowKind {
    /// O valor de texto (`'editor'` ou `'agents'`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Editor => "editor",
            Self::Agents => "agents",
        }
    }
}

/// `IExperimentationFilterProvider` (de `tas-client`).
pub trait ExperimentationFilterProvider {
    fn get_filter_value(&self, filter: &str) -> Option<String>;
    fn get_filters(&self) -> IndexMap<String, Option<String>>;
}

/// `AssignmentFilterProvider`.
pub struct AssignmentFilterProvider {
    version: String,
    app_name: String,
    machine_id: String,
    dev_device_id: String,
    target_population: TargetPopulation,
    release_date: String,
    window_kind: WindowKind,
    platform: Platform,
}

impl AssignmentFilterProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        version: &str,
        app_name: &str,
        machine_id: &str,
        dev_device_id: &str,
        target_population: TargetPopulation,
        release_date: &str,
        window_kind: WindowKind,
        platform: Platform,
    ) -> Self {
        Self {
            version: version.to_owned(),
            app_name: app_name.to_owned(),
            machine_id: machine_id.to_owned(),
            dev_device_id: dev_device_id.to_owned(),
            target_population,
            release_date: release_date.to_owned(),
            window_kind,
            platform,
        }
    }
}

impl ExperimentationFilterProvider for AssignmentFilterProvider {
    fn get_filter_value(&self, filter: &str) -> Option<String> {
        let Some(filter) = Filters::from_str(filter) else {
            return Some(String::new());
        };
        match filter {
            Filters::ApplicationVersion => Some(trim_version_suffix(&self.version)), // productService.version
            Filters::Build => Some(self.app_name.clone()),                           // productService.nameLong
            Filters::ClientId => Some(self.machine_id.clone()),
            Filters::DeveloperDeviceId => Some(self.dev_device_id.clone()),
            Filters::Language => Some(self.platform.language.clone()),
            Filters::ExtensionName => Some("vscode-core".to_owned()), // always return vscode-core for exp service
            Filters::ExtensionVersion => Some("999999.0".to_owned()), // always return a very large number for cross-extension experimentation
            Filters::TargetPopulation => Some(self.target_population.as_str().to_owned()),
            Filters::Platform => Some(self.platform.kind.name().to_owned()),
            Filters::ReleaseDate => Some(format_release_date(&self.release_date)),
            Filters::WindowKind => Some(self.window_kind.as_str().to_owned()),
            Filters::Market | Filters::CorpNet => Some(String::new()),
        }
    }

    fn get_filters(&self) -> IndexMap<String, Option<String>> {
        Filters::ALL.into_iter().map(|filter| (filter.as_str().to_owned(), self.get_filter_value(filter.as_str()))).collect()
    }
}

/// Trims a TAS-incompatible version suffix (e.g. `-insider`) so the value can be parsed into a .NET Build object by
/// the experimentation backend. É o `version.split(/\-[a-zA-Z0-9]+$/)[0]`.
fn trim_version_suffix(version: &str) -> String {
    if let Some(dash) = version.rfind('-') {
        let suffix = &version[dash + 1..];
        if !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
            return version[..dash].to_owned();
        }
    }
    version.to_owned()
}

/// Formats an ISO release date into the `yyyymmddHH` form the experimentation backend expects (10 digits). Returns an
/// empty string when unavailable. É o `/^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2})/`.
fn format_release_date(iso: &str) -> String {
    let bytes = iso.as_bytes();
    let digits_at = |start: usize, len: usize| bytes.get(start..start + len).is_some_and(|slice| slice.iter().all(u8::is_ascii_digit));
    let well_formed = digits_at(0, 4)
        && bytes.get(4) == Some(&b'-')
        && digits_at(5, 2)
        && bytes.get(7) == Some(&b'-')
        && digits_at(8, 2)
        && bytes.get(10) == Some(&b'T')
        && digits_at(11, 2);
    if !well_formed {
        return String::new();
    }
    format!("{}{}{}{}", &iso[0..4], &iso[5..7], &iso[8..10], &iso[11..13])
}

/// `AssignmentsFilters`: userParam names for the new TAS assignments API (`POST /api/v1/assignments`), emitted by
/// [`VSCodeCoreAssignmentsFilterProvider`]. These replace the legacy header keys and are sent only to the new endpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssignmentsFilters {
    ApplicationVersion,
    Build,
    DeveloperDeviceId,
    ExtensionName,
    ExtensionNameShort,
    TargetPopulation,
    Platform,
    ReleaseDate,
    WindowKind,
}

impl AssignmentsFilters {
    /// Todos os filtros, na ordem de declaração (`Object.values(AssignmentsFilters)`).
    pub const ALL: [AssignmentsFilters; 9] = [
        Self::ApplicationVersion,
        Self::Build,
        Self::DeveloperDeviceId,
        Self::ExtensionName,
        Self::ExtensionNameShort,
        Self::TargetPopulation,
        Self::Platform,
        Self::ReleaseDate,
        Self::WindowKind,
    ];

    /// O valor de texto do filtro (o nome do userParam).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ApplicationVersion => "vscode_core_appversion",
            Self::Build => "vscode_core_build",
            Self::DeveloperDeviceId => "devdeviceid",
            Self::ExtensionName => "vscode_core_extensionname",
            Self::ExtensionNameShort => "extensionname",
            Self::TargetPopulation => "vscode_core_targetpopulation",
            Self::Platform => "vscode_core_platform",
            Self::ReleaseDate => "vscode_core_releasedate",
            Self::WindowKind => "vscode_core_windowkind",
        }
    }

    fn from_str(value: &str) -> Option<AssignmentsFilters> {
        Self::ALL.into_iter().find(|filter| filter.as_str() == value)
    }
}

/// `VSCodeCoreAssignmentsFilterProvider`: emits the generic VS Code core filters for the new TAS assignments API using
/// the new userParam key names, so the assignments endpoint never receives the legacy header keys.
pub struct VSCodeCoreAssignmentsFilterProvider {
    version: String,
    app_name: String,
    dev_device_id: String,
    target_population: TargetPopulation,
    release_date: String,
    window_kind: WindowKind,
    platform: Platform,
}

impl VSCodeCoreAssignmentsFilterProvider {
    pub fn new(
        version: &str,
        app_name: &str,
        dev_device_id: &str,
        target_population: TargetPopulation,
        release_date: &str,
        window_kind: WindowKind,
        platform: Platform,
    ) -> Self {
        Self {
            version: version.to_owned(),
            app_name: app_name.to_owned(),
            dev_device_id: dev_device_id.to_owned(),
            target_population,
            release_date: release_date.to_owned(),
            window_kind,
            platform,
        }
    }
}

impl ExperimentationFilterProvider for VSCodeCoreAssignmentsFilterProvider {
    fn get_filter_value(&self, filter: &str) -> Option<String> {
        match AssignmentsFilters::from_str(filter)? {
            AssignmentsFilters::ApplicationVersion => Some(trim_version_suffix(&self.version)),
            AssignmentsFilters::Build => Some(self.app_name.clone()),
            AssignmentsFilters::DeveloperDeviceId => Some(self.dev_device_id.clone()),
            AssignmentsFilters::ExtensionName | AssignmentsFilters::ExtensionNameShort => Some("vscode-core".to_owned()),
            AssignmentsFilters::TargetPopulation => Some(self.target_population.as_str().to_owned()),
            AssignmentsFilters::Platform => Some(self.platform.kind.name().to_owned()),
            AssignmentsFilters::ReleaseDate => Some(format_release_date(&self.release_date)),
            AssignmentsFilters::WindowKind => Some(self.window_kind.as_str().to_owned()),
        }
    }

    fn get_filters(&self) -> IndexMap<String, Option<String>> {
        AssignmentsFilters::ALL.into_iter().map(|filter| (filter.as_str().to_owned(), self.get_filter_value(filter.as_str()))).collect()
    }
}

/// `getInternalOrg`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InternalOrg {
    VsCode,
    GitHub,
    Microsoft,
}

/// `getInternalOrg`.
pub fn get_internal_org(organisations: Option<&[&str]>) -> Option<InternalOrg> {
    let includes = |name: &str| organisations.is_some_and(|orgs| orgs.contains(&name));
    let is_vscode_internal = includes("Visual-Studio-Code");
    let is_github_internal = includes("github");
    let is_microsoft_internal = includes("microsoft") || includes("ms-copilot") || includes("MicrosoftCopilot");
    if is_vscode_internal {
        Some(InternalOrg::VsCode)
    } else if is_github_internal {
        Some(InternalOrg::GitHub)
    } else if is_microsoft_internal {
        Some(InternalOrg::Microsoft)
    } else {
        None
    }
}

/// Whether the account is staff or belongs to an internal org; the machine half is `verifyMicrosoftInternalDomain`.
pub fn is_internal_account(is_staff: Option<bool>, organisations: Option<&[&str]>) -> bool {
    is_staff == Some(true) || get_internal_org(organisations).is_some()
}
