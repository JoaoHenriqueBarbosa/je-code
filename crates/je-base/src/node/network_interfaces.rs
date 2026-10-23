//! Interfaces de rede, o `networkInterfaces()` do módulo `os` do Node que `macAddress.ts` e `id.ts` usam.
//!
//! No Linux a lista vem de `/sys/class/net` (`address`, `ifindex` e `flags` de cada interface), sem `unsafe`
//! e sem `getifaddrs`. Em Windows e macOS o Node usa `GetAdaptersAddresses` e `getifaddrs`, chamadas que
//! exigem `unsafe` ou uma dependência com C; aqui as duas plataformas devolvem a lista vazia, e
//! [`super::mac_address::get_mac`] lança o erro "Unable to retrieve mac address".
//!
//! Divergências do `os.networkInterfaces()` do Node, sem efeito para `getMac` e `getMachineId`:
//! - o Node devolve uma entrada por ENDEREÇO IP de cada interface (e nenhuma para a interface sem IP); aqui é
//!   uma entrada por interface, com ou sem IP. Só a razão de [`super::id::VirtualMachineHint::value`] sente
//!   isso (uma interface com IPv4 e IPv6 pesa duas vezes lá);
//! - o endereço que não tem seis octetos (túneis `sit`, `ipip`, `gre`) vira `00:00:00:00:00:00`, que é o que o
//!   `libuv` entrega, pois copia só seis bytes do endereço de camada de enlace.

/// Uma entrada de `networkInterfaces()`: o `mac` e o `internal` (interface de loopback).
#[derive(Clone, Debug, PartialEq)]
pub struct NetworkInterface {
    pub name: String,
    pub mac: String,
    pub internal: bool,
}

/// `IFF_LOOPBACK` de `linux/if.h`.
#[cfg(target_os = "linux")]
const IFF_LOOPBACK: u32 = 0x8;

#[cfg(target_os = "linux")]
const ZERO_MAC: &str = "00:00:00:00:00:00";

/// `os.networkInterfaces()`, na ordem do índice da interface no kernel.
#[cfg(target_os = "linux")]
pub fn network_interfaces() -> Vec<NetworkInterface> {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return Vec::new();
    };
    let mut found: Vec<(u32, NetworkInterface)> = entries
        .flatten()
        .filter_map(|entry| {
            let read = |file: &str| std::fs::read_to_string(entry.path().join(file)).ok().map(|text| text.trim().to_owned());
            let address = read("address")?;
            let index = read("ifindex").and_then(|text| text.parse().ok()).unwrap_or(u32::MAX);
            let flags = read("flags").and_then(|text| u32::from_str_radix(text.trim_start_matches("0x"), 16).ok()).unwrap_or(0);
            let mac = if address.split(':').count() == 6 { address } else { ZERO_MAC.to_owned() };
            Some((index, NetworkInterface { name: entry.file_name().to_string_lossy().into_owned(), mac, internal: flags & IFF_LOOPBACK != 0 }))
        })
        .collect();
    found.sort_by_key(|(index, _)| *index);
    found.into_iter().map(|(_, interface)| interface).collect()
}

/// Sem leitura segura de interfaces fora do Linux (ver o topo do módulo).
#[cfg(not(target_os = "linux"))]
pub fn network_interfaces() -> Vec<NetworkInterface> {
    Vec::new()
}
