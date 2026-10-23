//! Porte de `src/vs/base/node/id.ts`: `virtualMachineHint`, `getMachineId`, `getSqmMachineId` e
//! `getDevDeviceId`.
//!
//! Divergências:
//! - as funções `async` do original devolvem [`Promise`] já resolvida: o trabalho é síncrono (não há laço de
//!   eventos a esperar), e `errorLogger` é `&dyn Fn(&Error)`;
//! - as interfaces de rede vêm de [`network_interfaces`], que só lê o Linux (ver
//!   `node/network_interfaces.rs`: Windows e macOS usariam `GetAdaptersAddresses` e `getifaddrs`). Fora do Linux
//!   `getMac` lança, e `getMachineId` registra o erro e cai no UUID, o ramo de fallback do original;
//! - `getSqmMachineId` lê `HKEY_LOCAL_MACHINE\Software\Microsoft\SQMClient` com `reg query` (o original usa o
//!   módulo nativo `@vscode/windows-registry`, que exige `unsafe` ou C); fora do Windows devolve `''`;
//! - `getDevDeviceId` reimplementa o comportamento do `@vscode/deviceid`, que não está em `ref/` (só o
//!   `package.json` o declara): o identificador mora no valor `deviceid` de `HKCU\SOFTWARE\Microsoft\DeveloperTools`
//!   no Windows e no arquivo `Microsoft/DeveloperTools/deviceid` sob `~/Library/Application Support` (macOS) ou
//!   `$XDG_CACHE_HOME` (padrão `~/.cache`, Linux); se falta ou não é um UUID, gera um UUID e o grava. O
//!   comportamento foi reconstruído de memória e NÃO foi conferido contra o pacote;
//! - `virtualMachineHint` guarda o valor no processo (`OnceLock`), como a variável de módulo do original.

use std::cell::RefCell;
use std::path::PathBuf;
use std::process::Command;
use std::sync::OnceLock;

use sha2::{Digest, Sha256};

use crate::buffer::{VSBuffer, encode_hex};
use crate::errors::{self, Error};
use crate::node::mac_address::get_mac;
use crate::node::network_interfaces::network_interfaces;
use crate::promise::Promise;
use crate::strings::strip_utf8_bom;
use crate::ternary_search_tree::TernarySearchTree;
use crate::uuid::{generate_uuid, is_uuid};

/// Os OUIs (três primeiros octetos) de MAC de máquina virtual, separados por dois pontos. O original lista
/// também a forma com hífen, que [`VirtualMachineHint::value`] deriva. A tabela vem de
/// <http://www.techrepublic.com/blog/data-center/mac-address-scorecard-for-common-virtual-machine-platforms/>:
/// VMware ESX 3, Server, Workstation, Player 00-50-56, 00-0C-29, 00-05-69;
/// Microsoft Hyper-V, Virtual Server, Virtual PC 00-03-FF;
/// Parallels Desktop, Workstation, Server, Virtuozzo 00-1C-42;
/// Virtual Iron 4 00-0F-4B; Red Hat Xen, Oracle VM, XenSource, Novell Xen 00-16-3E;
/// Sun xVM VirtualBox 08-00-27.
const VIRTUAL_MACHINE_OUIS: [&str; 7] = ["00:50:56", "00:0C:29", "00:05:69", "00:03:FF", "00:1C:42", "00:16:3E", "08:00:27"];

/// `virtualMachineHint`: `value()` é a fração das interfaces externas cujo MAC é de máquina virtual.
pub struct VirtualMachineHint;

pub static VIRTUAL_MACHINE_HINT: VirtualMachineHint = VirtualMachineHint;

impl VirtualMachineHint {
    pub fn value(&self) -> f64 {
        static VALUE: OnceLock<f64> = OnceLock::new();
        *VALUE.get_or_init(|| {
            let mut virtual_machine_ouis = TernarySearchTree::<String, bool>::for_strings();
            for oui in VIRTUAL_MACHINE_OUIS {
                // dash-separated e colon-separated
                virtual_machine_ouis.set(oui.replace(':', "-"), true);
                virtual_machine_ouis.set(oui.to_owned(), true);
            }

            let mut vm_oui = 0;
            let mut interface_count = 0;
            for interface in network_interfaces() {
                if !interface.internal {
                    interface_count += 1;
                    if virtual_machine_ouis.find_substr(&interface.mac.to_uppercase()).is_some() {
                        vm_oui += 1;
                    }
                }
            }
            if interface_count > 0 { f64::from(vm_oui) / f64::from(interface_count) } else { 0.0 }
        })
    }
}

thread_local! {
    static MACHINE_ID: RefCell<Option<Promise<String>>> = const { RefCell::new(None) };
}

/// `getMachineId`: o SHA-256 do MAC, ou um UUID se ele não existe. Fica guardado depois da primeira chamada.
pub fn get_machine_id(error_logger: &dyn Fn(&Error)) -> Promise<String> {
    MACHINE_ID.with(|machine_id| {
        machine_id
            .borrow_mut()
            .get_or_insert_with(|| {
                let id = get_mac_machine_id(error_logger);

                // fallback, generate a UUID
                Promise::resolved(id.filter(|id| !id.is_empty()).unwrap_or_else(generate_uuid))
            })
            .clone()
    })
}

fn get_mac_machine_id(error_logger: &dyn Fn(&Error)) -> Option<String> {
    match errors::catch(get_mac) {
        Ok(mac_address) => Some(encode_hex(&VSBuffer::wrap(Sha256::digest(mac_address.as_bytes()).to_vec()))),
        Err(err) => {
            error_logger(&err);
            None
        }
    }
}

const SQM_KEY: &str = "HKEY_LOCAL_MACHINE\\Software\\Microsoft\\SQMClient";

/// `getSqmMachineId`: o `MachineId` do SQMClient no Windows, `''` nas outras plataformas.
pub fn get_sqm_machine_id(error_logger: &dyn Fn(&Error)) -> Promise<String> {
    if !cfg!(windows) {
        return Promise::resolved(String::new());
    }
    Promise::resolved(match read_registry_string(SQM_KEY, "MachineId") {
        Ok(id) => id,
        Err(err) => {
            error_logger(&err);
            String::new()
        }
    })
}

/// `getDevDeviceId`: o identificador de dispositivo de desenvolvimento, ou um UUID se não dá para lê-lo.
pub fn get_dev_device_id(error_logger: &dyn Fn(&Error)) -> Promise<String> {
    Promise::resolved(match read_or_create_device_id() {
        Ok(id) => String::from_utf16_lossy(strip_utf8_bom(&id.encode_utf16().collect::<Vec<u16>>())),
        Err(err) => {
            error_logger(&err);
            generate_uuid()
        }
    })
}

const DEVICE_ID_KEY: &str = "HKEY_CURRENT_USER\\SOFTWARE\\Microsoft\\DeveloperTools";
const DEVICE_ID_VALUE: &str = "deviceid";

fn io_error(error: &std::io::Error) -> Error {
    Error::new(error.to_string())
}

fn read_or_create_device_id() -> Result<String, Error> {
    if cfg!(windows) {
        let stored = read_registry_string(DEVICE_ID_KEY, DEVICE_ID_VALUE).ok().map(|id| id.trim().to_owned()).filter(|id| is_uuid(id));
        if let Some(id) = stored {
            return Ok(id);
        }
        let id = generate_uuid();
        run_reg(&["add", DEVICE_ID_KEY, "/v", DEVICE_ID_VALUE, "/t", "REG_SZ", "/d", id.as_str(), "/f"])?;
        return Ok(id);
    }

    let path = device_id_path()?;
    if let Some(content) = std::fs::read_to_string(&path).ok().filter(|content| is_uuid(content.trim())) {
        return Ok(content);
    }
    let id = generate_uuid();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| io_error(&e))?;
    }
    std::fs::write(&path, &id).map_err(|e| io_error(&e))?;
    Ok(id)
}

fn device_id_path() -> Result<PathBuf, Error> {
    let home = || std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| Error::new("HOME is not set"));
    let base = if cfg!(target_os = "macos") {
        home()?.join("Library").join("Application Support")
    } else {
        match std::env::var_os("XDG_CACHE_HOME").filter(|dir| !dir.is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => home()?.join(".cache"),
        }
    };
    Ok(base.join("Microsoft").join("DeveloperTools").join(DEVICE_ID_VALUE))
}

/// Roda `reg` e devolve a saída padrão; falha se o comando não roda ou termina com erro.
fn run_reg(args: &[&str]) -> Result<String, Error> {
    let output = Command::new("reg").args(args).output().map_err(|e| io_error(&e))?;
    if !output.status.success() {
        return Err(Error::new(String::from_utf8_lossy(&output.stderr).trim().to_owned()));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `reg query <key> /v <value>`: o texto do valor `REG_SZ`. A linha de resultado é `    nome    REG_SZ    dado`.
fn read_registry_string(key: &str, value: &str) -> Result<String, Error> {
    let output = run_reg(&["query", key, "/v", value])?;
    output
        .lines()
        .find_map(|line| {
            let line = line.trim();
            let (name, rest) = line.split_once(char::is_whitespace)?;
            let data = rest.trim_start().strip_prefix("REG_SZ")?;
            name.eq_ignore_ascii_case(value).then(|| data.trim().to_owned())
        })
        .ok_or_else(|| Error::new(format!("Unable to read the registry value {value} of {key}")))
}
