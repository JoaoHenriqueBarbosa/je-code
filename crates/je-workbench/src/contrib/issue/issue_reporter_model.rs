//! Porte de `src/vs/workbench/contrib/issue/browser/issueReporterModel.ts`: o modelo de dados do relator de
//! problemas e o texto (markdown) do relatório que ele serializa.
//!
//! Divergências:
//! - **`undefined` interpolado.** Onde o original interpola um valor que pode faltar num template literal
//!   (`${this._data.issueDescription}`), o relatório traz o texto `undefined`. Os campos ausentes são `Option` e a
//!   serialização escreve `undefined` por eles, byte a byte como o original.
//! - **`Partial<IssueReporterData>`.** O construtor recebe os dados iniciais já completos (`Option<IssueReporterData>`):
//!   quem chama preenche o que quer e deixa o resto em [`IssueReporterData::default`] (os padrões do original),
//!   o que dá o mesmo que o `Object.assign(defaultData, initialData)`. O `update(Partial)` vira um fecho que
//!   altera só os campos desejados.
//! - **Escuta de `message` na janela.** O construtor do original registra um `addDisposableListener(mainWindow,
//!   'message', ...)`, que responde a `vscode:triggerIssueData` com um `postMessage`. O je-code não tem DOM nem
//!   `window`: a lógica da resposta é [`IssueReporterModel::handle_window_message`], que a camada de janela chama a
//!   cada mensagem e que devolve o que seria postado.
//! - **Herança de `Disposable`** vira o campo `base`.
//! - `Object.keys(gpuStatus)` é a lista de pares do [`SystemInfo`] (ver `je_platform::diagnostics`).
//! - Os nomes `number_of_theme_extesions` e `enabled_non_theme_extesions` mantêm a grafia do original, para
//!   o campo ser encontrado pelo nome nos dois lados.

use std::cell::{Ref, RefCell};
use std::rc::Rc;

use je_base::dynamic::number_to_js_string;
use je_base::lifecycle::{Disposable, IDisposable};
use je_platform::diagnostics::{RemoteDiagnostic, SystemInfo};

use super::issue_common::{IssueReporterExtensionData, IssueSource, IssueType, SettingSearchResult};

/// `VersionInfo`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub vscode_version: String,
    pub os: String,
}

/// `IssueReporterData` (a do modelo, não a do serviço em `issue_common`).
#[derive(Debug, Clone, PartialEq)]
pub struct IssueReporterData {
    pub issue_type: IssueType,
    pub issue_source: Option<IssueSource>,
    pub issue_description: Option<String>,
    pub issue_title: Option<String>,
    pub extension_data: Option<String>,

    pub version_info: Option<VersionInfo>,
    pub system_info: Option<SystemInfo>,
    pub system_info_web: Option<String>,
    pub process_info: Option<String>,
    pub workspace_info: Option<String>,

    pub include_system_info: bool,
    pub include_workspace_info: bool,
    pub include_process_info: bool,
    pub include_extensions: bool,
    pub include_experiments: bool,
    pub include_extension_data: bool,

    pub number_of_theme_extesions: Option<f64>,
    pub all_extensions: Vec<IssueReporterExtensionData>,
    pub enabled_non_theme_extesions: Option<Vec<IssueReporterExtensionData>>,
    pub extensions_disabled: Option<bool>,
    pub file_on_extension: Option<bool>,
    pub file_on_marketplace: Option<bool>,
    pub file_on_product: Option<bool>,
    pub selected_extension: Option<IssueReporterExtensionData>,
    pub actual_search_results: Option<Vec<SettingSearchResult>>,
    pub query: Option<String>,
    pub filter_result_count: Option<f64>,
    pub experiment_info: Option<String>,
    pub restricted_mode: Option<bool>,
    pub is_installation_pure: Option<bool>,
    pub is_sessions_window: Option<bool>,
}

impl Default for IssueReporterData {
    /// O `defaultData` do construtor do original: o tipo `Bug` e tudo incluído.
    fn default() -> Self {
        Self {
            issue_type: IssueType::Bug,
            issue_source: None,
            issue_description: None,
            issue_title: None,
            extension_data: None,
            version_info: None,
            system_info: None,
            system_info_web: None,
            process_info: None,
            workspace_info: None,
            include_system_info: true,
            include_workspace_info: true,
            include_process_info: true,
            include_extensions: true,
            include_experiments: true,
            include_extension_data: true,
            number_of_theme_extesions: None,
            all_extensions: Vec::new(),
            enabled_non_theme_extesions: None,
            extensions_disabled: None,
            file_on_extension: None,
            file_on_marketplace: None,
            file_on_product: None,
            selected_extension: None,
            actual_search_results: None,
            query: None,
            filter_result_count: None,
            experiment_info: None,
            restricted_mode: None,
            is_installation_pure: None,
            is_sessions_window: None,
        }
    }
}

/// O canal que pede os dados da issue (`'vscode:triggerIssueData'`).
pub const TRIGGER_ISSUE_DATA_CHANNEL: &str = "vscode:triggerIssueData";
/// O canal da resposta (`'vscode:triggerIssueDataResponse'`).
pub const TRIGGER_ISSUE_DATA_RESPONSE_CHANNEL: &str = "vscode:triggerIssueDataResponse";

/// A mensagem que o original posta na janela em resposta a [`TRIGGER_ISSUE_DATA_CHANNEL`]:
/// `{ data: { issueBody, issueTitle }, replyChannel }`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueDataReply {
    pub issue_body: Option<String>,
    pub issue_title: Option<String>,
    pub reply_channel: &'static str,
}

/// `IssueReporterModel`.
pub struct IssueReporterModel {
    base: Rc<Disposable>,
    data: RefCell<IssueReporterData>,
}

/// O `${valor}` de um template literal quando o valor pode ser `undefined`.
fn or_undefined(value: Option<&str>) -> &str {
    value.unwrap_or("undefined")
}

/// A verdade de um número em JavaScript: nem `undefined`, nem `0`, nem `NaN`.
fn is_truthy_number(value: Option<f64>) -> bool {
    value.is_some_and(|v| v != 0.0 && !v.is_nan())
}

/// A verdade de um texto em JavaScript: nem `undefined`, nem vazio.
fn is_truthy_text(value: Option<&String>) -> bool {
    value.is_some_and(|text| !text.is_empty())
}

/// `Number.prototype.toFixed(2)`. O `toFixed` escolhe o maior `n` num empate exato (o `{:.2}` do Rust escolhe o
/// par); como só há empate em múltiplos de 1/8 que não são de 1/4, esse caso é arredondado para cima à mão.
fn to_fixed_2(x: f64) -> String {
    if x.is_nan() {
        return "NaN".to_string();
    }
    if x.is_infinite() {
        return if x > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if x.abs() >= 1e21 {
        return number_to_js_string(x);
    }
    let magnitude = x.abs();
    let is_tie = (magnitude * 8.0).fract() == 0.0 && (magnitude * 4.0).fract() != 0.0;
    let sign = if x < 0.0 { "-" } else { "" };
    if !is_tie {
        return format!("{}{:.2}", sign, magnitude);
    }
    let thousandths: u128 = format!("{magnitude:.3}").replace('.', "").parse().expect("dígitos de um número finito");
    let hundredths = (thousandths + 5) / 10;
    format!("{sign}{}.{:02}", hundredths / 100, hundredths % 100)
}

impl IssueReporterModel {
    /// `new IssueReporterModel(initialData)`.
    pub fn new(initial_data: Option<IssueReporterData>) -> Rc<Self> {
        Rc::new(Self { base: Disposable::new(), data: RefCell::new(initial_data.unwrap_or_default()) })
    }

    /// A resposta à mensagem `event.data.sendChannel` que chegou na janela, ou `None` se a mensagem não pede os
    /// dados da issue (o corpo do `addDisposableListener(mainWindow, 'message', ...)` do construtor).
    pub fn handle_window_message(&self, send_channel: Option<&str>) -> Option<IssueDataReply> {
        if send_channel != Some(TRIGGER_ISSUE_DATA_CHANNEL) {
            return None;
        }
        let data = self.data.borrow();
        Some(IssueDataReply {
            issue_body: data.issue_description.clone(),
            issue_title: data.issue_title.clone(),
            reply_channel: TRIGGER_ISSUE_DATA_RESPONSE_CHANNEL,
        })
    }

    /// `getData()`.
    pub fn get_data(&self) -> Ref<'_, IssueReporterData> {
        self.data.borrow()
    }

    /// `update(newData)`: o fecho altera só os campos que o `Partial` traria.
    pub fn update(&self, apply: impl FnOnce(&mut IssueReporterData)) {
        apply(&mut self.data.borrow_mut());
    }

    /// `serialize()`.
    pub fn serialize(&self) -> String {
        let data = self.data.borrow();
        let mut modes: Vec<&str> = Vec::new();
        if data.restricted_mode == Some(true) {
            modes.push("Restricted");
        }
        if data.is_installation_pure == Some(false) {
            modes.push("Unsupported");
        }
        let window = if data.is_sessions_window == Some(true) { "\nWindow: Agents\n" } else { "" };
        let version_info = data.version_info.as_ref();
        let modes_text = if modes.is_empty() { String::new() } else { format!(" {}", modes.join(", ")) };
        format!(
            "\nType: <b>{}</b>\n{}\n{}\n{}\nVS Code version: {}\nOS version: {}\nModes:{}\n{}\n{}\n<!-- generated by issue reporter -->",
            self.issue_type_title(&data),
            window,
            or_undefined(data.issue_description.as_deref()),
            self.extension_version(&data),
            or_undefined(version_info.map(|info| info.vscode_version.as_str())),
            or_undefined(version_info.map(|info| info.os.as_str())),
            modes_text,
            self.remote_oses(&data),
            self.infos(&data),
        )
    }

    fn remote_oses(&self, data: &IssueReporterData) -> String {
        if let Some(system_info) = &data.system_info
            && !system_info.remote_data.is_empty()
        {
            let lines: Vec<String> = system_info
                .remote_data
                .iter()
                .map(|remote| match remote {
                    RemoteDiagnostic::Error(error) => error.error_message.clone(),
                    RemoteDiagnostic::Info(info) => format!("Remote OS version: {}", info.info.machine_info.os),
                })
                .collect();
            return lines.join("\n") + "\n";
        }

        String::new()
    }

    /// `fileOnExtension()`: só os tipos Bug, PerformanceIssue e FeatureRequest suportam arquivar numa extensão;
    /// nos demais o resultado é `false`, e nos suportados é o valor do dado (que pode faltar).
    pub fn file_on_extension(&self) -> Option<bool> {
        Self::file_on_extension_of(&self.data.borrow())
    }

    fn file_on_extension_of(data: &IssueReporterData) -> Option<bool> {
        let file_on_extension_supported = matches!(
            data.issue_type,
            IssueType::Bug | IssueType::PerformanceIssue | IssueType::FeatureRequest
        );

        if file_on_extension_supported { data.file_on_extension } else { Some(false) }
    }

    fn extension_version(&self, data: &IssueReporterData) -> String {
        if Self::file_on_extension_of(data) == Some(true)
            && let Some(extension) = &data.selected_extension
        {
            format!("\nExtension version: {}", extension.version)
        } else {
            String::new()
        }
    }

    fn issue_type_title(&self, data: &IssueReporterData) -> &'static str {
        match data.issue_type {
            IssueType::Bug => "Bug",
            IssueType::PerformanceIssue => "Performance Issue",
            IssueType::FeatureRequest => "Feature Request",
        }
    }

    fn infos(&self, data: &IssueReporterData) -> String {
        let mut info = String::new();

        if data.file_on_marketplace == Some(true) {
            return info;
        }

        if data.include_extension_data && is_truthy_text(data.extension_data.as_ref()) {
            info += &self.extension_data(data);
        }

        if data.include_system_info
            && let Some(system_info) = &data.system_info
        {
            info += &self.system_info_md(data, system_info);
        }

        if data.issue_type == IssueType::PerformanceIssue {
            if data.include_process_info {
                info += &self.process_info_md(data);
            }

            if data.include_workspace_info {
                info += &self.workspace_info_md(data);
            }
        }

        if data.file_on_extension != Some(true) && data.include_extensions {
            info += &self.extensions_md(data);
        }

        if data.include_experiments && is_truthy_text(data.experiment_info.as_ref()) {
            info += &self.experiments_info_md(data);
        }

        info
    }

    fn extension_data(&self, data: &IssueReporterData) -> String {
        data.extension_data.clone().unwrap_or_default()
    }

    fn system_info_md(&self, data: &IssueReporterData, system_info: &SystemInfo) -> String {
        let mut md = String::from("<details>\n<summary>System Info</summary>\n\n|Item|Value|\n|---|---|\n");

        let gpu_status: Vec<String> = system_info.gpu_status.iter().map(|(key, value)| format!("{key}: {value}")).collect();
        md += &format!(
            "|CPUs|{}|\n|GPU Status|{}|\n|Load (avg)|{}|\n|Memory (System)|{}|\n|Process Argv|{}|\n|Screen Reader|{}|\n|VM|{}|",
            or_undefined(system_info.machine_info.cpus.as_deref()),
            gpu_status.join("<br>"),
            or_undefined(system_info.load.as_deref()),
            system_info.machine_info.memory,
            system_info.process_args.replace('\\', "\\\\"),
            system_info.screen_reader,
            system_info.machine_info.vm_hint,
        );

        if is_truthy_text(data.system_info_web.as_ref()) {
            md += &format!("\n|User Agent|{}|", data.system_info_web.as_deref().unwrap_or_default());
        }

        if let Some(linux_env) = &system_info.machine_info.linux_env {
            md += &format!(
                "\n|DESKTOP_SESSION|{}|\n|XDG_CURRENT_DESKTOP|{}|\n|XDG_SESSION_DESKTOP|{}|\n|XDG_SESSION_TYPE|{}|",
                or_undefined(linux_env.desktop_session.as_deref()),
                or_undefined(linux_env.xdg_current_desktop.as_deref()),
                or_undefined(linux_env.xdg_session_desktop.as_deref()),
                or_undefined(linux_env.xdg_session_type.as_deref()),
            );
        }

        for remote in &system_info.remote_data {
            match remote {
                RemoteDiagnostic::Error(error) => md += &format!("\n\n{}", error.error_message),
                RemoteDiagnostic::Info(remote) => {
                    let remote_label = match &remote.latency {
                        Some(latency) => format!(
                            "{} (latency: {}ms last, {}ms average)",
                            remote.host_name,
                            to_fixed_2(latency.current),
                            to_fixed_2(latency.average)
                        ),
                        None => remote.host_name.clone(),
                    };
                    let machine = &remote.info.machine_info;
                    md += &format!(
                        "\n\n|Item|Value|\n|---|---|\n|Remote|{}|\n|OS|{}|\n|CPUs|{}|\n|Memory (System)|{}|\n|VM|{}|",
                        remote_label,
                        machine.os,
                        or_undefined(machine.cpus.as_deref()),
                        machine.memory,
                        machine.vm_hint,
                    );
                }
            }
        }

        md += "\n</details>";

        md
    }

    fn process_info_md(&self, data: &IssueReporterData) -> String {
        format!(
            "<details>\n<summary>Process Info</summary>\n\n```\n{}\n```\n\n</details>\n",
            or_undefined(data.process_info.as_deref())
        )
    }

    fn workspace_info_md(&self, data: &IssueReporterData) -> String {
        format!(
            "<details>\n<summary>Workspace Info</summary>\n\n```\n{};\n```\n\n</details>\n",
            or_undefined(data.workspace_info.as_deref())
        )
    }

    fn experiments_info_md(&self, data: &IssueReporterData) -> String {
        format!(
            "<details>\n<summary>A/B Experiments</summary>\n\n```\n{}\n```\n\n</details>\n",
            or_undefined(data.experiment_info.as_deref())
        )
    }

    fn extensions_md(&self, data: &IssueReporterData) -> String {
        if data.extensions_disabled == Some(true) {
            return "Extensions disabled".to_string();
        }

        let enabled = data.enabled_non_theme_extesions.as_deref().unwrap_or_default();
        if enabled.is_empty() && !is_truthy_number(data.number_of_theme_extesions) {
            return "Extensions: none".to_string();
        }

        let mut md = String::new();
        let table_header = "Name|Identifier|Author|Version\n---|---|---|---";

        if !enabled.is_empty() {
            let table: Vec<String> = enabled
                .iter()
                .map(|extension| {
                    let name = match extension.display_name.as_deref() {
                        Some(display_name) if !display_name.is_empty() => display_name,
                        _ => extension.name.as_str(),
                    };
                    format!(
                        "{}|{}|{}|{}",
                        name,
                        extension.id,
                        extension.publisher.as_deref().unwrap_or("N/A"),
                        extension.version
                    )
                })
                .collect();

            md += &format!(
                "<details><summary>Extensions ({})</summary>\n\n{}\n{}\n\n</details>",
                enabled.len(),
                table_header,
                table.join("\n")
            );
        }

        md
    }
}

impl IDisposable for IssueReporterModel {
    fn dispose(&self) {
        self.base.dispose();
    }
}
