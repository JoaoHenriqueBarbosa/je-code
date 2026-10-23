//! Porte de `src/vs/base/node/macAddress.ts`: o endereço MAC da primeira interface de rede válida.
//!
//! Divergências: as interfaces vêm de [`network_interfaces`], que só lê o Linux (ver
//! `node/network_interfaces.rs`); o `getMac` lança o `Error` do original quando não acha nenhuma, como o
//! pânico de [`errors::throw`] (use [`errors::catch`] para capturar).

use crate::errors::{self, Error};
use crate::node::network_interfaces::network_interfaces;

const INVALID_MAC_ADDRESSES: [&str; 3] = ["00:00:00:00:00:00", "ff:ff:ff:ff:ff:ff", "ac:de:48:00:11:22"];

fn validate_mac_address(candidate: &str) -> bool {
    let temp_candidate = candidate.replace('-', ":").to_lowercase();
    !INVALID_MAC_ADDRESSES.contains(&temp_candidate.as_str())
}

/// `getMac`.
pub fn get_mac() -> String {
    for interface in network_interfaces() {
        if validate_mac_address(&interface.mac) {
            return interface.mac;
        }
    }

    errors::throw(Error::new("Unable to retrieve mac address (unexpected format)"))
}
