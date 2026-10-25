//! Porte de `src/vs/platform/tunnel/common/tunnelProxy.ts`: as credenciais e os metadados do proxy de túnel.

/// As credenciais Basic do `Proxy-Authorization` (`credentials` do `ITunnelProxyInfo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITunnelProxyCredentials {
    pub username: String,
    pub password: String,
}

/// `ITunnelProxyInfo`: credentials and metadata for the tunnel proxy.
/// This is the single type consumers need to configure an Electron session
/// (proxy URL, credentials for Basic auth, certificate fingerprint for
/// pinning the self-signed TLS certificate).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ITunnelProxyInfo {
    /// Proxy URL for `session.setProxy()` (e.g. `https://127.0.0.1:PORT`).
    pub url: String,
    pub host: String,
    pub port: u16,
    /// Basic auth credentials for `Proxy-Authorization`.
    pub credentials: ITunnelProxyCredentials,
    /// SHA-256 fingerprint of the self-signed certificate (`sha256/<base64>`).
    pub cert_fingerprint: String,
}
