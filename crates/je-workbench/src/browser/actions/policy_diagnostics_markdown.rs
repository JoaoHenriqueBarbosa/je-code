//! Porte de `src/vs/workbench/browser/actions/policyDiagnosticsMarkdown.ts`: o Markdown do relatório de
//! diagnóstico de políticas (tabelas escapadas, blocos `<details>` recolhidos e a prontidão do Agent Host).
//!
//! Divergências:
//! - Os textos são `&str`/`String` em vez de `JsString` (UTF-16): as funções de escape do `je-base` recebem
//!   unidades UTF-16, e a conversão acontece nas bordas. Um surrogate isolado não chega por aqui, porque
//!   `&str` não o representa.
//! - `IAgentHostPolicyReadinessGap extends IAgentHostPolicyGap` vira composição
//!   ([`AgentHostPolicyReadinessGap::gap`] mais `source`), já que o Rust não tem herança de interface de dados.
//! - `throw new Error` de `markdownTable` é [`errors::throw`] (pânico carregando o `Error`).
//! - `markdownJsonBlock(value: unknown)` recebe um [`Dynamic`]; `JSON.stringify(value, null, 2)` é
//!   [`json_stringify_indented`].
//! - Cada `localize` mantém a chave e a mensagem originais.

use je_base::dynamic::{Dynamic, json_stringify_indented};
use je_base::errors::{self, Error};
use je_base::html_content::{append_escaped_markdown_code_block_fence, escape_markdown_syntax_tokens};
use je_base::js_string::{JsString, trim_end};
use je_base::nls::localize;
use je_base::strings::escape;
use je_platform::agent_host::common::agent_host_policy_readiness::{AgentHostPolicyGap, get_agent_host_policy_gap_impact};
use je_platform::agent_host::common::agent_host_policy_support::AgentHostPolicySupportStatus;

/// `IAgentHostPolicyReadinessGap`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentHostPolicyReadinessGap {
    pub gap: AgentHostPolicyGap,
    pub source: String,
}

/// `agentHostPolicyReadiness`.
pub fn agent_host_policy_readiness(gaps: &[AgentHostPolicyReadinessGap], managed_sandbox_enforced: bool) -> String {
    let mut content = format!("## {}\n\n", localize("policyReadiness.title", "Agent Host Policy Readiness", &[]));
    content += &localize("policyReadiness.scope", "Only requirements from your currently applied enterprise policies are shown here. Unconfigured policies and known permissive values are excluded. This check covers Copilot Agent Host on this machine, not remote hosts, Claude, or Codex.", &[]);
    content += "\n\n";
    let not_enforced: Vec<&AgentHostPolicyReadinessGap> = gaps.iter().filter(|gap| gap.gap.status == AgentHostPolicySupportStatus::NotEnforced).collect();
    let needs_verification: Vec<&AgentHostPolicyReadinessGap> = gaps.iter().filter(|gap| gap.gap.status != AgentHostPolicySupportStatus::NotEnforced).collect();
    let gap_table = |entries: &[&AgentHostPolicyReadinessGap]| -> String {
        markdown_table(
            &[
                localize("policyReadiness.policy", "Policy", &[]),
                localize("policyReadiness.setting", "Setting", &[]),
                localize("policyReadiness.source", "Applied Source", &[]),
                localize("policyReadiness.impact", "Impact / What to Verify", &[]),
            ],
            &entries
                .iter()
                .map(|gap| vec![gap.gap.policy_name.clone(), gap.gap.setting_id.clone(), gap.source.clone(), get_agent_host_policy_gap_impact(&gap.gap.policy_name)])
                .collect::<Vec<_>>(),
        )
    };
    if gaps.is_empty() {
        content += &localize("policyReadiness.noGaps", "No known unsupported requirements were found in the applied VS Code policies. This does not establish full runtime policy parity; user settings and unprojected runtime managed settings are outside this check.", &[]);
        content += "\n\n";
    }
    if !not_enforced.is_empty() {
        content += &format!("### {} ({})\n\n", localize("policyReadiness.notEnforced", "Applied requirements not enforced by Agent Host", &[]), not_enforced.len());
        content += &gap_table(&not_enforced);
    }
    if !needs_verification.is_empty() {
        content += &format!("### {} ({})\n\n", localize("policyReadiness.needsVerification", "Applied requirements needing verification", &[]), needs_verification.len());
        content += &localize("policyReadiness.conditional", "These policies have incomplete coverage for some paths or delivery sources. This is not a finding that your current source or session fails to enforce them. Check the conditions below; a runtime-managed value may already be enforced even when its VS Code-only equivalent is not.", &[]);
        content += "\n\n";
        content += &gap_table(&needs_verification);
    }
    content += &format!("### {}\n\n", localize("policyReadiness.rollout", "Rollout and harness selection", &[]));
    content += &localize("policyReadiness.reportOnly", "These findings are report-only: they do not hold users on Local, change experiment enrollment or harness selection, block messages, or add Chat banners. Existing rollout, preferences, explicit choices, and sessions are unchanged. Opening this report has no effect on selection. Supported bridge restrictions remain enforced.", &[]);
    content += "\n\n";
    content += &if managed_sandbox_enforced {
        localize("policyReadiness.runtimeSandboxRequired", "An enterprise-required Agent Host sandbox is active. Local cannot satisfy this runtime sandbox requirement.", &[])
    } else {
        localize("policyReadiness.runtimeSandboxNotRequired", "No enterprise-required Agent Host sandbox is active.", &[])
    };
    content += "\n\n";
    content += &localize("policyReadiness.harnessPolicy", "ChatEditorPreferCopilotHarness controls chat.editor.preferCopilotHarness: enabling it prefers Copilot when a new chat would otherwise use Local. It is a preference, not a harness lock; disabling it does not require Local or prohibit explicit or remembered Copilot choices. Existing sessions and the enterprise-required sandbox behavior are unchanged. Requiring a particular harness needs a separate enforcement contract, not an inference from this readiness report.", &[]);
    content += "\n\n";
    content
}

/// `policyDiagnosticsReport`.
pub fn policy_diagnostics_report(readiness: &str, diagnostic_sections: &str) -> String {
    format!("# VS Code Policy Diagnostics\n\n*WARNING: This file may contain sensitive information.*\n\n{diagnostic_sections}{readiness}")
}

/// `escapeMarkdownText`: `escapeMarkdownSyntaxTokens(escape(value))`.
fn escape_markdown_text(value: &str) -> String {
    escape_markdown_syntax_tokens(escape(JsString::from(value).units()).units()).to_string_lossy()
}

/// O `.replace(/\r?\n/g, replacement)` do original.
fn replace_line_breaks(value: &str, replacement: &str) -> String {
    value.replace("\r\n", replacement).replace('\n', replacement)
}

/// `markdownText`.
pub fn markdown_text(value: &str) -> String {
    replace_line_breaks(&escape_markdown_text(value), " ")
}

fn markdown_table_cell(value: &str) -> String {
    replace_line_breaks(&escape_markdown_text(value), "<br>").replace('|', "\\|")
}

/// `markdownTable`: levanta um erro se alguma linha não tem tantas células quanto os cabeçalhos.
pub fn markdown_table(headers: &[impl AsRef<str>], rows: &[Vec<String>]) -> String {
    for row in rows {
        if row.len() != headers.len() {
            errors::throw(Error::new(format!("Markdown table row has {} cells; expected {}.", row.len(), headers.len())));
        }
    }

    let mut lines = vec![
        format!("| {} |", headers.iter().map(|header| markdown_table_cell(header.as_ref())).collect::<Vec<_>>().join(" | ")),
        format!("| {} |", headers.iter().map(|_| "---").collect::<Vec<_>>().join(" | ")),
    ];
    lines.extend(rows.iter().map(|row| format!("| {} |", row.iter().map(|cell| markdown_table_cell(cell)).collect::<Vec<_>>().join(" | "))));
    format!("{}\n\n", lines.join("\n"))
}

/// `markdownJsonBlock`: `null` e `undefined` viram `{}`; o que o `JSON.stringify` não serializa vira `null`.
pub fn markdown_json_block(value: &Dynamic) -> String {
    let empty = Dynamic::object(Vec::<(&str, Dynamic)>::new());
    let serialized = json_stringify_indented(if value.is_nullish() { &empty } else { value }, 2).unwrap_or_else(|| "null".to_owned());
    format!("{}\n\n", append_escaped_markdown_code_block_fence(JsString::from(serialized).units(), JsString::from("json").units()).to_string_lossy())
}

/// `markdownDetails`.
pub fn markdown_details(summary: &str, content: &str) -> String {
    let safe_summary = replace_line_breaks(&escape(JsString::from(summary).units()).to_string_lossy(), " ");
    let content = JsString::from(content);
    let trimmed = JsString::from(trim_end(content.units())).to_string_lossy();
    format!("<details>\n<summary>{safe_summary}</summary>\n\n{trimmed}\n\n</details>\n\n")
}
