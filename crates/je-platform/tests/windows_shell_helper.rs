//! Casos do `WindowsShellHelper` (`src/vs/platform/terminal/node/windowsShellHelper.ts`) que não dependem do Windows: a
//! travessia da árvore de processos e a classificação do executável. O VS Code não tem suíte própria para o arquivo; os
//! casos seguem o comportamento do original, linha a linha.
#![cfg(not(target_arch = "wasm32"))]

use je_base::processes::ProcessItem;
use je_platform::terminal::terminal::{GeneralShellType, ShellType, WindowsShellType};
use je_platform::terminal::windows_shell_helper::{get_shell_type, python_exe_matches, traverse_tree};

fn node(pid: u32, cmd: &str, children: Vec<ProcessItem>) -> ProcessItem {
    ProcessItem {
        name: cmd.to_owned(),
        cmd: cmd.to_owned(),
        pid: f64::from(pid),
        ppid: 0.0,
        load: 0.0,
        mem: 0.0,
        children: if children.is_empty() { None } else { Some(children) },
    }
}

#[test]
fn shell_types_by_executable() {
    assert_eq!(get_shell_type("cmd.exe"), Some(ShellType::from(WindowsShellType::CommandPrompt)));
    assert_eq!(get_shell_type("PowerShell.exe"), Some(ShellType::from(GeneralShellType::PowerShell)));
    assert_eq!(get_shell_type("pwsh.exe"), Some(ShellType::from(GeneralShellType::PowerShell)));
    assert_eq!(get_shell_type("git-cmd.exe"), Some(ShellType::from(WindowsShellType::GitBash)));
    assert_eq!(get_shell_type("ubuntu1804.exe"), Some(ShellType::from(WindowsShellType::Wsl)));
    assert_eq!(get_shell_type("python3.12.exe"), Some(ShellType::from(GeneralShellType::Python)));
    assert_eq!(get_shell_type("notepad.exe"), None);
}

#[test]
fn python_executable_patterns() {
    assert!(python_exe_matches("python.exe", true));
    assert!(python_exe_matches("python3.exe", true));
    assert!(python_exe_matches("python3.12.exe", true));
    assert!(!python_exe_matches("python3.123.exe", true));
    assert!(!python_exe_matches("xpython.exe", true));
    assert!(python_exe_matches("xpython.exe", false));
    assert!(!python_exe_matches("pythonw.exe", false));
}

#[test]
fn traverses_to_the_innermost_shell() {
    let tree = node(1, r"C:\Windows\System32\cmd.exe", vec![node(2, r"C:\Windows\System32\powershell.exe", vec![])]);
    assert_eq!(traverse_tree(Some(&tree)), "powershell.exe");
}

#[test]
fn skips_children_whose_first_child_is_conhost() {
    // O primeiro filho do `cmd.exe` é um processo que tem o `conhost.exe` como filho (um subprocesso ligado a outro console):
    // vale o segundo.
    let noise = node(2, r"C:\tools\helper.exe", vec![node(3, r"\??\C:\Windows\system32\conhost.exe 0x4", vec![])]);
    let tree = node(1, "cmd.exe", vec![noise, node(4, r"C:\Windows\System32\pwsh.exe", vec![])]);
    assert_eq!(traverse_tree(Some(&tree)), "pwsh.exe");
}

#[test]
fn stops_at_a_non_shell_executable_and_on_an_empty_tree() {
    let tree = node(1, r#""C:\Program Files\App\app.exe" --flag"#, vec![node(2, "cmd.exe", vec![])]);
    assert_eq!(traverse_tree(Some(&tree)), "app.exe");
    assert_eq!(traverse_tree(None), "");
}

#[test]
fn detects_agent_clis_running_inside_node() {
    let tree = node(1, r"C:\Program Files\nodejs\node.exe C:\Users\x\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\cli.js", vec![]);
    assert_eq!(traverse_tree(Some(&tree)), "claude.exe");
    let plain = node(1, r"C:\Program Files\nodejs\node.exe server.js", vec![]);
    assert_eq!(traverse_tree(Some(&plain)), "node.exe");
}
