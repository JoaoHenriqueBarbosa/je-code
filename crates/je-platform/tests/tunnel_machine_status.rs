//! Porte de `src/vs/platform/remoteTunnel/test/common/tunnelMachineStatus.test.ts` (suíte `Tunnel machine
//! status`), caso a caso e na mesma ordem.
//!
//! Divergências: `ensureNoDisposablesAreLeakedInTestSuite` não tem o que vigiar (nada aqui é descartável);
//! os literais esperados são as structs do porte, com `None` no lugar dos campos ausentes.

use je_platform::remote_tunnel::tunnel_machine_status::{
    IConnectedTunnelMachineStatus, ITokenErrorTunnelMachineStatus, TUNNEL_MACHINE_STATUS_PREFIX, TunnelMachineStatus,
    parse_tunnel_machine_status,
};

fn connected(tunnel_id: Option<&str>, is_attached: bool, link: Option<&str>, domain: Option<&str>) -> Option<TunnelMachineStatus> {
    Some(TunnelMachineStatus::Connected(IConnectedTunnelMachineStatus {
        tunnel_name: "desktop-oss".to_owned(),
        tunnel_id: tunnel_id.map(str::to_owned),
        is_attached,
        link: link.map(str::to_owned),
        domain: domain.map(str::to_owned),
    }))
}

#[test]
fn parses_valid_status_lines_and_ignores_invalid_output() {
    let p = TUNNEL_MACHINE_STATUS_PREFIX;
    assert_eq!(
        vec![
            parse_tunnel_machine_status(&format!(
                "{p}{{\"type\":\"connected\",\"tunnelName\":\"desktop-oss\",\"isAttached\":false,\"link\":\"https://insiders.vscode.dev/tunnel/desktop-oss/c:/dir\",\"domain\":\"insiders.vscode.dev\"}}"
            )),
            parse_tunnel_machine_status(&format!("{p}{{\"type\":\"connected\",\"tunnelName\":\"desktop-oss\",\"tunnelId\":\"tunnel-id\",\"isAttached\":false}}")),
            parse_tunnel_machine_status(&format!("{p}{{\"type\":\"connected\",\"tunnelName\":\"desktop-oss\",\"isAttached\":true}}")),
            parse_tunnel_machine_status(&format!("{p}{{\"type\":\"tokenError\",\"message\":\"token expired\"}}")),
            parse_tunnel_machine_status(&format!("\u{1b}[32m{p}{{\"type\":\"connected\",\"tunnelName\":\"desktop-oss\",\"isAttached\":false}}\u{1b}[0m")),
            parse_tunnel_machine_status(&format!("{p}{{invalid}}")),
            parse_tunnel_machine_status(&format!("{p}{{\"type\":\"connected\",\"tunnelName\":\"desktop-oss\",\"tunnelId\":1,\"isAttached\":false}}")),
            parse_tunnel_machine_status(&format!("noise {p}{{\"type\":\"tokenError\",\"message\":\"token expired\"}}")),
            parse_tunnel_machine_status("unrelated noise"),
        ],
        vec![
            connected(None, false, Some("https://insiders.vscode.dev/tunnel/desktop-oss/c:/dir"), Some("insiders.vscode.dev")),
            connected(Some("tunnel-id"), false, None, None),
            connected(None, true, None, None),
            Some(TunnelMachineStatus::TokenError(ITokenErrorTunnelMachineStatus { message: "token expired".to_owned() })),
            connected(None, false, None, None),
            None,
            None,
            None,
            None,
        ]
    );
}
