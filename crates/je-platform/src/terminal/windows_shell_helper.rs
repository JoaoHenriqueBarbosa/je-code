//! Porte de `src/vs/platform/terminal/node/windowsShellHelper.ts`: no Windows, descobre o processo em primeiro plano do
//! terminal (o shell mais interno da árvore de processos) e o tipo de shell dele.
//!
//! Divergências:
//! - O `@vscode/windows-process-tree` é nativo; a árvore vem de `list_processes` (`je_base::node::ps`), que no Windows
//!   monta a mesma árvore a partir da lista do `Win32_Process`. Essa lista não traz o nome da imagem (`szExeFile`), então o
//!   nome do nó é o executável da linha de comando (sem diretório), e a comparação com a lista de shells ignora maiúsculas.
//!   A chamada é bloqueante (roda o PowerShell), e a promessa já volta assentada.
//! - `class WindowsShellHelper extends Disposable` é um [`Disposable`] por composição; `@debounce(500) checkShell` é um
//!   [`Debounce`]. O `checkShell` do original não devolve a promessa interna, então um erro dela não aparece: aqui é
//!   descartado do mesmo jeito.
//! - Uma árvore que `list_processes` rejeita (o processo raiz já saiu) vale como `undefined` no original (nome vazio).
//! - As expressões regulares (`/^python(\d(\.\d{0,2})?)?\.exe$/` e a sem âncoras do `getShellType`) são escritas à mão em
//!   [`python_exe_matches`].

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use je_base::async_::{async_fn, timeout};
use je_base::decorators::Debounce;
use je_base::errors::Error;
use je_base::event::{Emitter, Event};
use je_base::lifecycle::{Disposable, IDisposable};
use je_base::network::FileAccess;
use je_base::node::ps::list_processes;
use je_base::platform::Platform;
use je_base::processes::ProcessItem;
use je_base::promise::Promise;

use super::terminal::{GeneralShellType, ShellType, TerminalShellType, WindowsShellType};

/// O tempo que o shell ganha para lançar um processo antes de a árvore ser lida (`await timeout(300)`).
const SHELL_LAUNCH_GRACE_MS: u64 = 300;
/// `@debounce(500)`.
const CHECK_SHELL_DEBOUNCE_MS: u64 = 500;

const SHELL_EXECUTABLES: [&str; 16] = [
    "cmd.exe",
    "powershell.exe",
    "pwsh.exe",
    "bash.exe",
    "git-cmd.exe",
    "wsl.exe",
    "ubuntu.exe",
    "ubuntu1804.exe",
    "kali.exe",
    "debian.exe",
    "opensuse-42.exe",
    "sles-12.exe",
    "julia.exe",
    "nu.exe",
    "node.exe",
    "xonsh.exe",
];

/// `NODE_AGENT_CLI_PATTERNS`: as CLIs de agente instaladas pelo npm aparecem como `node.exe`, então são reconhecidas pela
/// pasta do pacote na linha de comando do Node (`/[\\/]claude-code[\\/]/i` e as outras).
const NODE_AGENT_CLI_PATTERNS: [(&str, &str); 5] = [
    ("claude-code", "claude.exe"),
    ("codex", "codex.exe"),
    ("command-code", "commandcode.exe"),
    ("copilot", "copilot.exe"),
    ("gemini-cli", "gemini.exe"),
];

/// Os comprimentos que `(\d(\.\d{0,2})?)?\.exe` pode consumir no início de `rest` (o que vem depois de `python`).
fn python_tail_lengths(rest: &str) -> Vec<usize> {
    let bytes = rest.as_bytes();
    let mut lengths = Vec::new();
    let mut push_if_exe = |at: usize| {
        if rest.get(at..at + 4) == Some(".exe") {
            lengths.push(at + 4);
        }
    };
    push_if_exe(0);
    if bytes.first().is_some_and(u8::is_ascii_digit) {
        push_if_exe(1);
        if bytes.get(1) == Some(&b'.') {
            let digits = bytes[2..].iter().take(2).take_while(|byte| byte.is_ascii_digit()).count();
            for count in 0..=digits {
                push_if_exe(2 + count);
            }
        }
    }
    lengths
}

/// `/^python(\d(\.\d{0,2})?)?\.exe$/.test(name)` (`anchored`) ou `/python(\d(\.\d{0,2})?)?\.exe/.test(name)`.
pub fn python_exe_matches(name: &str, anchored: bool) -> bool {
    if anchored {
        return name.strip_prefix("python").is_some_and(|rest| python_tail_lengths(rest).contains(&rest.len()));
    }
    name.match_indices("python").any(|(index, _)| !python_tail_lengths(&name[index + "python".len()..]).is_empty())
}

/// O nome do executável de uma linha de comando: o primeiro item, entre aspas ou até o `.exe`, sem o diretório.
fn image_name(item: &ProcessItem) -> String {
    let command_line = item.cmd.trim_start();
    let executable = if let Some(quoted) = command_line.strip_prefix('"') {
        quoted.split('"').next().unwrap_or("")
    } else {
        // Um caminho sem aspas pode ter espaços (`C:\Program Files\...`): vale até o primeiro `.exe`.
        let lower = command_line.to_ascii_lowercase();
        match lower.find(".exe") {
            Some(end) => &command_line[..end + 4],
            None => command_line.split_whitespace().next().unwrap_or(""),
        }
    };
    let name = executable.rsplit(['\\', '/']).next().unwrap_or(executable);
    if name.is_empty() { item.name.clone() } else { name.to_owned() }
}

fn first_child_is_conhost(child: &ProcessItem) -> bool {
    child.children.as_ref().and_then(|children| children.first()).is_some_and(|first| image_name(first).eq_ignore_ascii_case("conhost.exe"))
}

/// `traverseTree`: o nome do processo mais interno que ainda é um shell.
pub fn traverse_tree(tree: Option<&ProcessItem>) -> String {
    let Some(tree) = tree else {
        return String::new();
    };
    let name = image_name(tree);
    let lower = name.to_ascii_lowercase();
    // Detect npm-installed agent CLIs running inside `node.exe` by inspecting the command line passed to Node. Without
    // this we'd treat them as a generic Node shell.
    if lower == "node.exe" && !tree.cmd.is_empty() {
        let command_line = tree.cmd.to_ascii_lowercase();
        for (package, executable) in NODE_AGENT_CLI_PATTERNS {
            if ['\\', '/'].iter().any(|separator| command_line.contains(&format!("{separator}{package}{separator}"))) {
                return (*executable).to_owned();
            }
        }
    }
    if !SHELL_EXECUTABLES.contains(&lower.as_str()) {
        return name;
    }
    if python_exe_matches(&lower, true) {
        return name;
    }
    let Some(children) = tree.children.as_ref().filter(|children| !children.is_empty()) else {
        return name;
    };
    let mut favourite_child = 0;
    while favourite_child < children.len() {
        let child = &children[favourite_child];
        if child.children.as_ref().is_none_or(Vec::is_empty) || !first_child_is_conhost(child) {
            break;
        }
        favourite_child += 1;
    }
    if favourite_child >= children.len() {
        return name;
    }
    traverse_tree(Some(&children[favourite_child]))
}

/// `getShellType`.
pub fn get_shell_type(executable: &str) -> TerminalShellType {
    let shell = match executable.to_lowercase().as_str() {
        "cmd.exe" => ShellType::from(WindowsShellType::CommandPrompt),
        "powershell.exe" | "pwsh.exe" => ShellType::from(GeneralShellType::PowerShell),
        "bash.exe" | "git-cmd.exe" => ShellType::from(WindowsShellType::GitBash),
        "julia.exe" => ShellType::from(GeneralShellType::Julia),
        "node.exe" => ShellType::from(GeneralShellType::Node),
        "nu.exe" => ShellType::from(GeneralShellType::NuShell),
        "xonsh.exe" => ShellType::from(GeneralShellType::Xonsh),
        "claude.exe" => ShellType::from(GeneralShellType::Claude),
        "codex.exe" => ShellType::from(GeneralShellType::Codex),
        "commandcode.exe" => ShellType::from(GeneralShellType::CommandCode),
        "copilot.exe" => ShellType::from(GeneralShellType::Copilot),
        "gemini.exe" => ShellType::from(GeneralShellType::Gemini),
        "wsl.exe" | "ubuntu.exe" | "ubuntu1804.exe" | "kali.exe" | "debian.exe" | "opensuse-42.exe" | "sles-12.exe" => ShellType::from(WindowsShellType::Wsl),
        _ if python_exe_matches(executable, false) => ShellType::from(GeneralShellType::Python),
        _ => return None,
    };
    Some(shell)
}

/// `WindowsShellHelper`.
pub struct WindowsShellHelper {
    me: Weak<WindowsShellHelper>,
    base: Rc<Disposable>,
    root_process_id: u32,
    file_access: Rc<FileAccess>,
    current_request: RefCell<Option<Promise<String>>>,
    shell_type: Cell<TerminalShellType>,
    shell_title: RefCell<String>,
    on_shell_name_changed: Rc<Emitter<String>>,
    on_shell_type_changed: Rc<Emitter<TerminalShellType>>,
    check_shell_debounced: RefCell<Option<Debounce<(), ()>>>,
}

impl WindowsShellHelper {
    /// `new WindowsShellHelper(rootProcessId)`: lança erro fora do Windows, como o original.
    pub fn new(root_process_id: u32, file_access: Rc<FileAccess>) -> Result<Rc<Self>, Error> {
        if !Platform::detect().is_windows {
            return Err(Error::new("WindowsShellHelper cannot be instantiated on this platform"));
        }
        let base = Disposable::new();
        let on_shell_name_changed: Rc<Emitter<String>> = Emitter::new();
        let on_shell_type_changed: Rc<Emitter<TerminalShellType>> = Emitter::new();
        base.register(on_shell_name_changed.clone());
        base.register(on_shell_type_changed.clone());
        let this = Rc::new_cyclic(|me: &Weak<Self>| Self {
            me: me.clone(),
            base,
            root_process_id,
            file_access,
            current_request: RefCell::new(None),
            shell_type: Cell::new(None),
            shell_title: RefCell::new(String::new()),
            on_shell_name_changed,
            on_shell_type_changed,
            check_shell_debounced: RefCell::new(None),
        });
        let weak = this.me.clone();
        *this.check_shell_debounced.borrow_mut() = Some(Debounce::without_reducer(CHECK_SHELL_DEBOUNCE_MS, move |()| {
            if let Some(this) = weak.upgrade() {
                this.run_check_shell();
            }
        }));
        // `_startMonitoringShell`.
        this.check_shell();
        Ok(this)
    }

    /// `get shellType()`.
    pub fn shell_type(&self) -> TerminalShellType {
        self.shell_type.get()
    }

    /// `get shellTitle()`.
    pub fn shell_title(&self) -> String {
        self.shell_title.borrow().clone()
    }

    /// `onShellNameChanged`.
    pub fn on_shell_name_changed(&self) -> Event<String> {
        self.on_shell_name_changed.event()
    }

    /// `onShellTypeChanged`.
    pub fn on_shell_type_changed(&self) -> Event<TerminalShellType> {
        self.on_shell_type_changed.event()
    }

    /// `@debounce(500) checkShell()`.
    pub fn check_shell(&self) {
        if let Some(debounced) = self.check_shell_debounced.borrow().as_ref() {
            debounced.call(());
        }
    }

    fn run_check_shell(&self) {
        let Some(this) = self.me.upgrade() else {
            return;
        };
        drop(async_fn(async move {
            // Wait to give the shell some time to actually launch a process, this could lead to a race condition but it
            // would be recovered from when data stops and should cover the majority of cases.
            timeout(SHELL_LAUNCH_GRACE_MS).await?;
            if this.base.store().is_disposed() {
                return Ok(());
            }
            let title = this.get_shell_name().await?;
            let shell_type = get_shell_type(&title);
            if shell_type != this.shell_type.get() {
                this.on_shell_type_changed.fire(shell_type);
                this.on_shell_name_changed.fire(title.clone());
                this.shell_type.set(shell_type);
                *this.shell_title.borrow_mut() = title;
            }
            Ok(())
        }));
    }

    /// `getShellName()`: the innermost shell executable running in the terminal.
    pub fn get_shell_name(&self) -> Promise<String> {
        if self.base.store().is_disposed() {
            return Promise::resolved(String::new());
        }
        // Prevent multiple requests at once, instead return current request.
        if let Some(request) = self.current_request.borrow().clone() {
            return request;
        }
        let weak = self.me.clone();
        let request = list_processes(f64::from(self.root_process_id), &self.file_access, &Platform::detect()).then_result(move |result| {
            let name = traverse_tree(result.ok().as_ref());
            if let Some(this) = weak.upgrade() {
                *this.current_request.borrow_mut() = None;
            }
            Ok(name)
        });
        // O `then_result` pode ter rodado já: não deixa uma requisição assentada guardada.
        if !request.is_settled() {
            *self.current_request.borrow_mut() = Some(request.clone());
        }
        request
    }

    /// `getShellType(executable)`.
    pub fn get_shell_type(&self, executable: &str) -> TerminalShellType {
        get_shell_type(executable)
    }
}

impl IDisposable for WindowsShellHelper {
    fn dispose(&self) {
        self.base.dispose();
    }
}
