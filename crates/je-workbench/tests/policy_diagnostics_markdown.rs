//! Porte de `src/vs/workbench/test/browser/actions/policyDiagnosticsMarkdown.test.ts` (suíte `Policy
//! diagnostics Markdown`), caso a caso e na mesma ordem.
//!
//! Notas de tradução:
//! - `ensureNoDisposablesAreLeakedInTestSuite()` vira o guarda criado no começo de cada caso.
//! - Os `assert.deepStrictEqual` de objetos de booleanos viram a comparação de vetores de pares
//!   `(nome, valor)`, o que mantém o nome do campo na mensagem de falha.
//! - `IAgentHostPolicyReadinessGap` é composição (`gap` mais `source`), então [`readiness_gap`] monta o
//!   literal `{ policyName, settingId, status, source }` do original.
//! - No caso `readiness escapes applied-source labels and never renders policy values` o original passa um
//!   objeto com um campo extra `policyValue`, que o TypeScript aceita por estar numa variável. O tipo
//!   Rust não tem esse campo (o relatório não tem como recebê-lo), então o literal fica sem ele e a
//!   conferência de que o valor sensível não aparece no relatório continua igual.

#[path = "../../je-base/tests/common/mod.rs"]
mod common;

use common::ensure_no_disposables_are_leaked_in_test_suite;
use je_base::dynamic::Dynamic;
use je_platform::agent_host::common::agent_host_policy_readiness::AgentHostPolicyGap;
use je_platform::agent_host::common::agent_host_policy_support::AgentHostPolicySupportStatus;
use je_workbench::browser::actions::policy_diagnostics_markdown::{
    AgentHostPolicyReadinessGap, agent_host_policy_readiness, markdown_details, markdown_json_block, markdown_table,
    policy_diagnostics_report,
};

fn readiness_gap(policy_name: &str, setting_id: &str, status: AgentHostPolicySupportStatus, source: &str) -> AgentHostPolicyReadinessGap {
    AgentHostPolicyReadinessGap {
        gap: AgentHostPolicyGap { policy_name: policy_name.to_owned(), setting_id: setting_id.to_owned(), status },
        source: source.to_owned(),
    }
}

fn row(cells: &[&str]) -> Vec<String> {
    cells.iter().map(|cell| (*cell).to_owned()).collect()
}

#[test]
fn readiness_names_applied_concerns_and_explains_the_existing_harness_policy_without_changing_it() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let output = agent_host_policy_readiness(
        &[
            readiness_gap("ChatMCP", "chat.mcp.access", AgentHostPolicySupportStatus::Partial, "Device"),
            readiness_gap("CopilotOtelHeaders", "chat.agentHost.otel.headers", AgentHostPolicySupportStatus::Partial, "Managed Settings: Server"),
        ],
        true,
    );
    assert_eq!(
        [
            ("heading", output.contains("## Agent Host Policy Readiness")),
            ("policy", output.contains("| ChatMCP | chat.mcp.access | Device |")),
            ("impact", output.contains("discover and run MCP servers")),
            ("headers", output.contains("Export requiring those headers may fail")),
            ("preference", output.contains("ChatEditorPreferCopilotHarness") && output.contains("disabling it does not require Local")),
            ("sandbox", output.contains("Local cannot satisfy this runtime sandbox requirement")),
            ("diagnosticOnly", output.contains("Opening this report has no effect on selection")),
        ],
        [
            ("heading", true),
            ("policy", true),
            ("impact", true),
            ("headers", true),
            ("preference", true),
            ("sandbox", true),
            ("diagnosticOnly", true),
        ]
    );
}

#[test]
fn an_empty_readiness_result_does_not_claim_complete_runtime_parity() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let output = agent_host_policy_readiness(&[], false);
    assert_eq!(
        [
            ("empty", output.contains("No known unsupported requirements")),
            ("limits", output.contains("user settings and unprojected runtime managed settings are outside this check")),
            ("table", output.contains("| Impact / What to Verify |")),
        ],
        [("empty", true), ("limits", true), ("table", false)]
    );
}

#[test]
fn partial_coverage_is_not_presented_as_a_confirmed_failure_for_the_applied_source() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let output = agent_host_policy_readiness(
        &[readiness_gap("ChatAllowManagedMcpServersOnly", "chat.mcp.allowManagedServersOnly", AgentHostPolicySupportStatus::Partial, "Managed Settings: Server")],
        false,
    );
    assert!(output.contains("### Applied requirements needing verification (1)"));
    assert!(output.contains("not a finding that your current source or session fails"));
    assert!(output.contains("| ChatAllowManagedMcpServersOnly | chat.mcp.allowManagedServersOnly | Managed Settings: Server |"));
    assert!(output.contains("preference, not a harness lock"));
    assert!(!output.contains("### Applied requirements not enforced"));
    assert!(!output.contains("CopilotOtel"));
}

#[test]
fn unsupported_requirements_are_separated_from_conditional_coverage() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let output = agent_host_policy_readiness(
        &[
            readiness_gap("ChatHooks", "chat.useHooks", AgentHostPolicySupportStatus::NotEnforced, "Device"),
            readiness_gap("CopilotOtelEnabled", "chat.agentHost.otel.enabled", AgentHostPolicySupportStatus::Partial, "Device"),
        ],
        false,
    );
    let unsupported = &output[output.find("### Applied requirements not enforced").unwrap()..output.find("### Applied requirements needing verification").unwrap()];
    assert!(unsupported.contains("| ChatHooks |"));
    assert!(!unsupported.contains("CopilotOtelEnabled"));
    assert!(output.contains("### Applied requirements not enforced by Agent Host (1)"));
    assert!(output.contains("### Applied requirements needing verification (1)"));
}

#[test]
fn readiness_preserves_runtime_sandbox_requirements_and_identity_verification_without_legacy_sandbox_gaps() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let output = agent_host_policy_readiness(
        &[readiness_gap("CopilotOtelCaptureIdentity", "chat.agentHost.otel.captureIdentity", AgentHostPolicySupportStatus::Partial, "Device")],
        true,
    );
    assert_eq!(
        [
            ("verification", output.contains("### Applied requirements needing verification (1)")),
            ("unsupported", output.contains("### Applied requirements not enforced")),
            ("legacySandbox", output.contains("ChatAgentSandbox")),
            ("runtimeSandbox", output.contains("Local cannot satisfy this runtime sandbox requirement")),
            ("hostIdentity", output.contains("The Agent Host pipeline honors identity capture and suppression.")),
            ("runtimeAttribution", output.contains("requires a runtime update and end-to-end verification")),
        ],
        [
            ("verification", true),
            ("unsupported", false),
            ("legacySandbox", false),
            ("runtimeSandbox", true),
            ("hostIdentity", true),
            ("runtimeAttribution", true),
        ]
    );
}

#[test]
fn readiness_follows_all_existing_diagnostic_sections_without_changing_their_layout() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let readiness = agent_host_policy_readiness(&[readiness_gap("ChatMCP", "chat.mcp.access", AgentHostPolicySupportStatus::Partial, "Device")], false);
    let headings = ["Summary", "System Information", "Account Information", "Account Policy Gate", "Managed Settings", "Agent Runtime Resolution", "Authentication Information"];
    let details = headings.iter().map(|heading| format!("## {heading}\n\n")).collect::<String>()
        + &markdown_details("Non-applied policy inventory", "CopilotOtelHeaders")
        + &markdown_details("Raw values", &markdown_json_block(&Dynamic::object([("rawPolicyValue", Dynamic::string("sensitive-placeholder"))])));
    let report = policy_diagnostics_report(&readiness, &details);
    let visible = &report[..report.find("<details>").unwrap()];
    assert!(report.starts_with("# VS Code Policy Diagnostics\n\n*WARNING: This file may contain sensitive information.*\n\n## Summary\n\n"));
    assert_eq!(&report[report.find("## Summary").unwrap()..report.find("## Agent Host Policy Readiness").unwrap()], details);
    for heading in headings {
        assert!(visible.contains(&format!("## {heading}")), "{heading}");
    }
    assert!(!visible.contains("CopilotOtel"));
    assert!(!visible.contains("sensitive-placeholder"));
    assert!(report.contains("<details>\n<summary>Non-applied policy inventory"));
    assert!(!report.contains("<details open"));
    assert!(report.ends_with(&format!("</details>\n\n{readiness}")));
    assert!(readiness.contains("ChatMCP"));
}

#[test]
fn readiness_escapes_applied_source_labels_and_never_renders_policy_values() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let gap = readiness_gap("ChatMCP", "chat.mcp.access", AgentHostPolicySupportStatus::Partial, "<script>|source");
    let output = agent_host_policy_readiness(&[gap], false);
    assert!(output.contains("&lt;script&gt;\\|source"));
    assert!(!output.contains("<script>"));
    assert!(!output.contains("sensitive-placeholder"));
}

#[test]
fn all_gaps_remain_report_only_regardless_of_the_runtime_sandbox_requirement() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let gaps = [
        readiness_gap("ChatHooks", "chat.useHooks", AgentHostPolicySupportStatus::NotEnforced, "Device"),
        readiness_gap("ChatAllowManagedMcpServersOnly", "chat.mcp.allowManagedServersOnly", AgentHostPolicySupportStatus::Partial, "Managed Settings: Server"),
    ];
    for sandbox_required in [false, true] {
        let output = agent_host_policy_readiness(&gaps, sandbox_required);
        assert_eq!(
            [
                ("hooks", output.contains("| ChatHooks |")),
                ("mcp", output.contains("| ChatAllowManagedMcpServersOnly |")),
                ("reportOnly", output.contains("report-only: they do not hold users on Local")),
                ("selection", output.contains("Existing rollout, preferences, explicit choices, and sessions are unchanged")),
                ("enforcement", output.contains("Supported bridge restrictions remain enforced")),
                ("requiredSandbox", output.contains("Local cannot satisfy this runtime sandbox requirement")),
                ("migrationRole", output.contains("Migration Role")),
                ("acceptance", output.contains("acceptPolicyGaps")),
                ("blocker", output.contains("Blocker")),
            ],
            [
                ("hooks", true),
                ("mcp", true),
                ("reportOnly", true),
                ("selection", true),
                ("enforcement", true),
                ("requiredSandbox", sandbox_required),
                ("migrationRole", false),
                ("acceptance", false),
                ("blocker", false),
            ]
        );
    }
}

#[test]
fn renders_escaped_tables_and_collapsed_details() {
    let _guard = ensure_no_disposables_are_leaked_in_test_suite();
    let output = markdown_table(
        &["Property", "Value"],
        &[row(&["pipe|name", "line 1\n- line 2 <script>*"]), row(&["bracket[one]", "A & B"])],
    ) + &markdown_details("Raw <settings> & values", &markdown_json_block(&Dynamic::object([("value", Dynamic::string("raw"))])));

    assert_eq!(
        output.split('\n').collect::<Vec<_>>(),
        [
            "| Property | Value |",
            "| --- | --- |",
            "| pipe\\|name | line 1<br>\\- line 2 &lt;script&gt;\\* |",
            "| bracket\\[one\\] | A &amp; B |",
            "",
            "<details>",
            "<summary>Raw &lt;settings&gt; &amp; values</summary>",
            "",
            "```json",
            "{",
            "  \"value\": \"raw\"",
            "}",
            "```",
            "",
            "</details>",
            "",
            ""
        ]
    );
}
