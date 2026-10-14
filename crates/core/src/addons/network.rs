use std::{
    collections::{BTreeMap, HashSet},
    net::{IpAddr, SocketAddr},
    time::Duration,
};

use reqwest::{
    header::{HeaderName, AUTHORIZATION},
    Method,
};
use serde::{Deserialize, Serialize};
use tokio::net::lookup_host;
use url::Url;

use super::validate_addon_id;
use crate::secrets::{addon_secret_service_id, legacy_addon_secret_service_id, SecretStore};

const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;
const MAX_RESPONSE_BODY_BYTES: usize = 2 * 1024 * 1024;
const REQUEST_TIMEOUT_SECS: u64 = 10;
const MAX_REQUEST_TIMEOUT_SECS: u64 = 120;

/// Operator setting listing hostnames whose addon requests may reach private-range
/// addresses. Read once at startup by the server and desktop runtimes.
pub const ADDON_NETWORK_PRIVATE_HOSTS_ENV: &str = "WF_ADDON_NETWORK_PRIVATE_HOSTS";

/// Parses the comma-separated private-host allowlist into exact, normalised hostnames.
///
/// Entries are normalised like manifest `allowedHosts`. IP literals and wildcards are
/// dropped: the exemption is granted to a name the operator trusts, never to an address
/// or a pattern.
pub fn parse_addon_network_private_hosts(raw: &str) -> HashSet<String> {
    let mut hosts = HashSet::new();
    for entry in raw.split(',') {
        let host = entry.trim().trim_end_matches('.').to_ascii_lowercase();
        if host.is_empty() {
            continue;
        }
        if parse_ip_host(&host).is_ok() || host.contains('*') {
            log::warn!("Ignoring {ADDON_NETWORK_PRIVATE_HOSTS_ENV} entry '{host}': only exact hostnames are supported");
            continue;
        }
        hosts.insert(host);
    }
    hosts
}

fn validate_addon_runtime_id(addon_id: &str) -> Result<(), String> {
    validate_addon_id(&addon_id.to_ascii_lowercase())
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonNetworkAuth {
    #[serde(rename = "type")]
    pub auth_type: String,
    pub secret_key: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonNetworkRequest {
    pub url: String,
    pub method: Option<String>,
    pub headers: Option<BTreeMap<String, String>>,
    pub body: Option<String>,
    pub auth: Option<AddonNetworkAuth>,
    /// HTTP timeout through response-body completion, excluding the preceding DNS lookup.
    /// Defaults to 10 seconds; positive values are capped at 120 seconds.
    pub timeout_secs: Option<u64>,
    #[serde(skip)]
    pub injected_authorization: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddonNetworkResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: String,
}

pub async fn perform_addon_network_request(
    addon_id: &str,
    allowed_hosts: &[String],
    private_hosts: &HashSet<String>,
    request: AddonNetworkRequest,
) -> Result<AddonNetworkResponse, String> {
    validate_addon_runtime_id(addon_id)?;
    let timeout_secs = resolve_request_timeout_secs(request.timeout_secs)?;
    let url = validate_url(&request.url, allowed_hosts)?;
    let host = url
        .host_str()
        .ok_or_else(|| "Addon network URL must include a host".to_string())?
        .to_string();
    let resolved_addresses = resolve_public_addresses(&url, private_hosts).await?;
    let method = validate_method(request.method.as_deref().unwrap_or("GET"))?;
    let body = request.body.unwrap_or_default();
    if body.len() > MAX_REQUEST_BODY_BYTES {
        return Err("Addon network request body is too large".to_string());
    }

    let client = addon_network_client_builder(&host, &resolved_addresses, timeout_secs)
        .build()
        .map_err(|e| e.to_string())?;

    let mut builder = client.request(method, url);
    for (name, value) in request.headers.unwrap_or_default() {
        if is_authorization_header(&name) {
            return Err(
                "Addon network Authorization header must use request.auth.secretKey".to_string(),
            );
        }
        if is_blocked_request_header(&name) {
            continue;
        }
        let header_name = HeaderName::from_bytes(name.as_bytes())
            .map_err(|_| format!("Invalid request header '{name}'"))?;
        builder = builder.header(header_name, value);
    }
    if let Some(authorization) = request.injected_authorization {
        builder = builder.header(AUTHORIZATION, authorization);
    }

    if !body.is_empty() {
        builder = builder.body(body);
    }

    let response = builder.send().await.map_err(|e| e.to_string())?;
    let status = response.status().as_u16();
    let mut headers = BTreeMap::new();
    for (name, value) in response.headers() {
        let name = name.as_str();
        if is_blocked_response_header(name) {
            continue;
        }
        if let Ok(value) = value.to_str() {
            headers.insert(name.to_string(), value.to_string());
        }
    }

    if response.content_length().unwrap_or_default() > MAX_RESPONSE_BODY_BYTES as u64 {
        return Err("Addon network response body is too large".to_string());
    }
    let mut response = response;
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BODY_BYTES {
            return Err("Addon network response body is too large".to_string());
        }
        body.extend_from_slice(&chunk);
    }

    Ok(AddonNetworkResponse {
        status,
        headers,
        body: String::from_utf8_lossy(&body).to_string(),
    })
}

pub fn resolve_addon_network_auth_header(
    addon_id: &str,
    auth: Option<&AddonNetworkAuth>,
    secret_store: &dyn SecretStore,
) -> Result<Option<String>, String> {
    validate_addon_runtime_id(addon_id)?;
    let Some(auth) = auth else {
        return Ok(None);
    };
    let scheme = match auth.auth_type.as_str() {
        "bearer" => "Bearer",
        "basic" => "Basic",
        _ => return Err("Addon network auth type is not supported".to_string()),
    };
    let service_id = addon_secret_service_id(addon_id, &auth.secret_key)?;
    let legacy_service_id = legacy_addon_secret_service_id(addon_id, &auth.secret_key)?;
    let secret = match secret_store
        .get_secret(&service_id)
        .map_err(|e| e.to_string())?
    {
        Some(secret) => secret,
        None => secret_store
            .get_secret(&legacy_service_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "Addon network auth secret was not found".to_string())?,
    };
    if secret.trim().is_empty() {
        return Err("Addon network auth secret is empty".to_string());
    }
    Ok(Some(format!("{} {}", scheme, secret)))
}

fn addon_network_client_builder(
    host: &str,
    resolved_addresses: &[SocketAddr],
    timeout_secs: u64,
) -> reqwest::ClientBuilder {
    wealthfolio_http::client_builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(timeout_secs))
        .resolve_to_addrs(host, resolved_addresses)
}

fn resolve_request_timeout_secs(timeout_secs: Option<u64>) -> Result<u64, String> {
    match timeout_secs {
        Some(0) => Err("Addon network timeoutSecs must be a positive integer".to_string()),
        value => Ok(value
            .unwrap_or(REQUEST_TIMEOUT_SECS)
            .min(MAX_REQUEST_TIMEOUT_SECS)),
    }
}

fn validate_url(url: &str, allowed_hosts: &[String]) -> Result<Url, String> {
    let parsed = Url::parse(url).map_err(|_| "Invalid addon network URL".to_string())?;
    if parsed.scheme() != "https" {
        return Err("Addon network requests must use HTTPS".to_string());
    }

    if parsed.username() != "" || parsed.password().is_some() {
        return Err("Addon network URLs cannot include credentials".to_string());
    }

    let host = parsed
        .host_str()
        .ok_or_else(|| "Addon network URL must include a host".to_string())?
        .to_ascii_lowercase();

    if is_blocked_host(&host) {
        return Err("Addon network host is not allowed".to_string());
    }

    if !allowed_hosts
        .iter()
        .any(|allowed| host_matches_allowed(&host, allowed))
    {
        return Err(format!("Addon network host '{host}' is not approved"));
    }

    Ok(parsed)
}

fn validate_method(method: &str) -> Result<Method, String> {
    match method.to_ascii_uppercase().as_str() {
        "GET" => Ok(Method::GET),
        "POST" => Ok(Method::POST),
        "PUT" => Ok(Method::PUT),
        "PATCH" => Ok(Method::PATCH),
        "DELETE" => Ok(Method::DELETE),
        "HEAD" => Ok(Method::HEAD),
        _ => Err("Addon network method is not allowed".to_string()),
    }
}

fn host_matches_allowed(host: &str, allowed: &str) -> bool {
    let allowed = allowed.trim().trim_end_matches('.').to_ascii_lowercase();
    if let Some(suffix) = allowed.strip_prefix("*.") {
        host.ends_with(&format!(".{suffix}")) && host != suffix
    } else {
        host == allowed
    }
}

fn is_blocked_host(host: &str) -> bool {
    if matches!(host, "localhost" | "ip6-localhost" | "ip6-loopback")
        || host.ends_with(".localhost")
        || host.ends_with(".local")
    {
        return true;
    }

    match parse_ip_host(host) {
        Ok(ip) => is_blocked_ip(ip),
        Err(_) => false,
    }
}

fn parse_ip_host(host: &str) -> Result<IpAddr, std::net::AddrParseError> {
    host.trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
}

fn is_blocked_ip(ip: IpAddr) -> bool {
    is_always_blocked_ip(ip) || is_private_range_ip(ip)
}

/// Addresses no addon request may reach, even for an operator-listed private host.
fn is_always_blocked_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_loopback()
                || ip.is_link_local()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.is_unspecified()
                || ip.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(ip) => ip.is_loopback() || ip.is_unspecified() || ip.is_unicast_link_local(),
    }
}

/// LAN ranges, reachable only through hosts listed in `WF_ADDON_NETWORK_PRIVATE_HOSTS`.
fn is_private_range_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => ip.is_private(),
        IpAddr::V6(ip) => ip.is_unique_local(),
    }
}

async fn resolve_public_addresses(
    url: &Url,
    private_hosts: &HashSet<String>,
) -> Result<Vec<SocketAddr>, String> {
    let host = url
        .host_str()
        .ok_or_else(|| "Addon network URL must include a host".to_string())?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| "Addon network URL must include a port".to_string())?;

    if let Ok(ip) = parse_ip_host(host) {
        if is_blocked_ip(ip) {
            return Err("Addon network host resolves to a private address".to_string());
        }
        return Ok(vec![SocketAddr::new(ip, port)]);
    }

    let addresses = lookup_host((host, port))
        .await
        .map_err(|_| "Addon network host could not be resolved".to_string())?;

    // The exemption follows the requested name, never the resolved address.
    let private_host_allowed =
        private_hosts.contains(&host.trim_end_matches('.').to_ascii_lowercase());
    check_resolved_addresses(addresses, private_host_allowed)
}

fn check_resolved_addresses(
    addresses: impl IntoIterator<Item = SocketAddr>,
    private_host_allowed: bool,
) -> Result<Vec<SocketAddr>, String> {
    let mut resolved = Vec::new();
    for address in addresses {
        let ip = address.ip();
        if is_always_blocked_ip(ip) || (!private_host_allowed && is_private_range_ip(ip)) {
            return Err("Addon network host resolves to a private address".to_string());
        }
        resolved.push(address);
    }

    if resolved.is_empty() {
        return Err("Addon network host could not be resolved".to_string());
    }

    Ok(resolved)
}

fn is_blocked_request_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "content-length"
            | "cookie"
            | "host"
            | "origin"
            | "proxy-authorization"
            | "referer"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn is_authorization_header(name: &str) -> bool {
    name.eq_ignore_ascii_case("authorization")
}

fn is_blocked_response_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "set-cookie" | "set-cookie2" | "transfer-encoding"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Result;
    use std::collections::BTreeMap;

    struct TestSecretStore {
        secrets: BTreeMap<String, String>,
    }

    impl SecretStore for TestSecretStore {
        fn set_secret(&self, _service: &str, _secret: &str) -> Result<()> {
            Ok(())
        }

        fn get_secret(&self, service: &str) -> Result<Option<String>> {
            Ok(self.secrets.get(service).cloned())
        }

        fn delete_secret(&self, _service: &str) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn resolves_request_timeout() {
        for (requested, expected) in [
            (None, 10),
            (Some(1), 1),
            (Some(30), 30),
            (Some(120), 120),
            (Some(121), 120),
            (Some(u64::MAX), 120),
        ] {
            assert_eq!(resolve_request_timeout_secs(requested).unwrap(), expected);
        }
        assert!(resolve_request_timeout_secs(Some(0)).is_err());
    }

    #[test]
    fn deserializes_request_timeout() {
        let request: AddonNetworkRequest = serde_json::from_value(serde_json::json!({
            "url": "https://api.example.com/v1"
        }))
        .unwrap();
        assert_eq!(request.timeout_secs, None);

        let request: AddonNetworkRequest = serde_json::from_value(serde_json::json!({
            "url": "https://api.example.com/v1", "timeoutSecs": 30
        }))
        .unwrap();
        assert_eq!(request.timeout_secs, Some(30));
        assert_eq!(serde_json::to_value(request).unwrap()["timeoutSecs"], 30);

        for invalid in [
            serde_json::json!(-1),
            serde_json::json!(1.5),
            serde_json::json!("30"),
        ] {
            assert!(
                serde_json::from_value::<AddonNetworkRequest>(serde_json::json!({
                    "url": "https://api.example.com/v1", "timeoutSecs": invalid
                }))
                .is_err()
            );
        }
    }

    #[tokio::test]
    async fn rejects_zero_timeout_before_resolving_host() {
        let request = serde_json::from_value(serde_json::json!({
            "url": "https://api.example.invalid/v1", "timeoutSecs": 0
        }))
        .unwrap();
        assert_eq!(
            perform_addon_network_request(
                "example-addon",
                &["api.example.invalid".into()],
                &HashSet::new(),
                request
            )
            .await
            .unwrap_err(),
            "Addon network timeoutSecs must be a positive integer"
        );
    }

    #[tokio::test]
    async fn timeout_override_allows_a_body_slower_than_the_default() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let mut connections = tokio::task::JoinSet::new();
            for _ in 0..2 {
                let (mut socket, _) = listener.accept().await.unwrap();
                connections.spawn(async move {
                    let mut request = [0; 1024];
                    let mut received = 0;
                    while !request[..received]
                        .windows(4)
                        .any(|bytes| bytes == b"\r\n\r\n")
                    {
                        let count = socket.read(&mut request[received..]).await.unwrap();
                        assert!(count > 0, "expected complete request headers");
                        received += count;
                    }
                    socket
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\n",
                        )
                        .await
                        .unwrap();
                    tokio::time::sleep(Duration::from_secs(REQUEST_TIMEOUT_SECS + 1)).await;
                    // The default-timeout client will already have disconnected.
                    let _ = socket.write_all(b"ok").await;
                });
            }
            while let Some(result) = connections.join_next().await {
                result.unwrap();
            }
        });
        let fetch_body = |timeout| async move {
            // Exercise the production client configuration with a local transport fixture.
            // Public URL/IP validation remains enforced by perform_addon_network_request.
            addon_network_client_builder(
                "127.0.0.1",
                &[address],
                resolve_request_timeout_secs(timeout).unwrap(),
            )
            .no_proxy()
            .build()
            .unwrap()
            .get(format!("http://{address}"))
            .send()
            .await?
            .text()
            .await
        };
        let (default_result, override_result) =
            tokio::join!(fetch_body(None), fetch_body(Some(30)));
        assert!(default_result.unwrap_err().is_timeout());
        assert_eq!(override_result.unwrap(), "ok");
        server.await.unwrap();
    }

    #[test]
    fn validates_allowed_hosts() {
        assert!(validate_url("https://api.example.com/v1", &["api.example.com".into()]).is_ok());
        assert!(validate_url("https://x.example.com/v1", &["*.example.com".into()]).is_ok());
        assert!(validate_url("https://example.com/v1", &["*.example.com".into()]).is_err());
        assert!(validate_url("http://api.example.com/v1", &["api.example.com".into()]).is_err());
        assert!(validate_url("https://127.0.0.1/v1", &["127.0.0.1".into()]).is_err());
        assert!(validate_url("https://[::1]/v1", &["::1".into()]).is_err());
        assert!(validate_url("https://localhost/v1", &["localhost".into()]).is_err());
    }

    #[test]
    fn blocks_private_and_metadata_addresses() {
        assert!(is_blocked_ip("127.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("10.0.0.1".parse().unwrap()));
        assert!(is_blocked_ip("169.254.169.254".parse().unwrap()));
        assert!(is_blocked_ip("::1".parse().unwrap()));
        assert!(is_blocked_ip("fd00::1".parse().unwrap()));
        assert!(!is_blocked_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_blocked_ip("2606:4700:4700::1111".parse().unwrap()));
    }

    fn answers(ips: &[&str]) -> Vec<SocketAddr> {
        ips.iter()
            .map(|ip| SocketAddr::new(ip.parse().unwrap(), 443))
            .collect()
    }

    fn private_request(url: &str) -> AddonNetworkRequest {
        serde_json::from_value(serde_json::json!({ "url": url })).unwrap()
    }

    #[test]
    fn splits_blocked_addresses_into_always_and_private_ranges() {
        for ip in ["10.0.0.1", "172.16.0.1", "192.168.1.10", "fd00::1"] {
            let ip = ip.parse().unwrap();
            assert!(is_private_range_ip(ip) && !is_always_blocked_ip(ip));
        }
        for ip in [
            "127.0.0.1",
            "169.254.169.254",
            "169.254.1.1",
            "0.0.0.0",
            "255.255.255.255",
            "192.0.2.1",
            "::1",
            "::",
            "fe80::1",
        ] {
            let ip = ip.parse().unwrap();
            assert!(is_always_blocked_ip(ip) && !is_private_range_ip(ip));
        }
        for ip in ["8.8.8.8", "2606:4700:4700::1111"] {
            let ip = ip.parse().unwrap();
            assert!(!is_always_blocked_ip(ip) && !is_private_range_ip(ip));
        }
    }

    #[test]
    fn rejects_unlisted_host_resolving_to_private_address() {
        assert_eq!(
            check_resolved_addresses(answers(&["192.168.10.200"]), false).unwrap_err(),
            "Addon network host resolves to a private address"
        );
        assert!(check_resolved_addresses(answers(&["8.8.8.8"]), false).is_ok());
    }

    #[test]
    fn allows_listed_host_resolving_to_private_ranges() {
        for ip in ["192.168.10.200", "10.1.2.3", "172.16.0.5", "fd00::1"] {
            assert_eq!(
                check_resolved_addresses(answers(&[ip]), true).unwrap(),
                answers(&[ip])
            );
        }
    }

    #[test]
    fn rejects_listed_host_resolving_to_always_blocked_addresses() {
        for ip in ["127.0.0.1", "::1", "169.254.169.254", "fe80::1"] {
            assert_eq!(
                check_resolved_addresses(answers(&[ip]), true).unwrap_err(),
                "Addon network host resolves to a private address"
            );
        }
    }

    #[test]
    fn rejects_listed_host_with_mixed_private_and_loopback_answers() {
        assert!(check_resolved_addresses(answers(&["192.168.10.200", "127.0.0.1"]), true).is_err());
        assert!(check_resolved_addresses(answers(&["127.0.0.1", "192.168.10.200"]), true).is_err());
    }

    #[test]
    fn parses_private_hosts_like_allowed_hosts() {
        let hosts = parse_addon_network_private_hosts(
            " Bank-Broker.Home.Example. ,, other.lan.example ,192.168.1.10,[fd00::1],fd00::2,*.lan.example",
        );
        assert_eq!(
            hosts,
            HashSet::from([
                "bank-broker.home.example".to_string(),
                "other.lan.example".to_string(),
            ])
        );
        assert!(parse_addon_network_private_hosts("").is_empty());
        assert!(parse_addon_network_private_hosts(" , ").is_empty());
    }

    #[tokio::test]
    async fn ignores_ip_literals_in_private_hosts() {
        let private_hosts = parse_addon_network_private_hosts("192.168.1.10");
        let url = Url::parse("https://192.168.1.10/health").unwrap();
        assert_eq!(
            resolve_public_addresses(&url, &private_hosts)
                .await
                .unwrap_err(),
            "Addon network host resolves to a private address"
        );
        assert_eq!(
            perform_addon_network_request(
                "example-addon",
                &["192.168.1.10".into()],
                &private_hosts,
                private_request("https://192.168.1.10/health"),
            )
            .await
            .unwrap_err(),
            "Addon network host is not allowed"
        );
    }

    #[tokio::test]
    async fn listed_private_host_still_requires_approval_and_https() {
        let private_hosts = parse_addon_network_private_hosts("bank-broker.home.example");
        assert_eq!(
            perform_addon_network_request(
                "example-addon",
                &["api.example.com".into()],
                &private_hosts,
                private_request("https://bank-broker.home.example/health"),
            )
            .await
            .unwrap_err(),
            "Addon network host 'bank-broker.home.example' is not approved"
        );
        assert_eq!(
            perform_addon_network_request(
                "example-addon",
                &["bank-broker.home.example".into()],
                &private_hosts,
                private_request("http://bank-broker.home.example/health"),
            )
            .await
            .unwrap_err(),
            "Addon network requests must use HTTPS"
        );
    }

    #[test]
    fn unset_or_empty_private_hosts_block_every_private_address() {
        for raw in [None, Some(""), Some("  ")] {
            let private_hosts = raw
                .map(parse_addon_network_private_hosts)
                .unwrap_or_default();
            assert!(private_hosts.is_empty());
            for ip in [
                "192.168.10.200",
                "10.1.2.3",
                "172.16.0.5",
                "fd00::1",
                "127.0.0.1",
            ] {
                let ip: IpAddr = ip.parse().unwrap();
                assert_eq!(
                    check_resolved_addresses(answers(&[&ip.to_string()]), false).is_err(),
                    is_blocked_ip(ip)
                );
            }
        }
    }

    #[test]
    fn resolves_bearer_auth_from_addon_scoped_secret() {
        let store = TestSecretStore {
            secrets: BTreeMap::from([(
                "addon:example-addon:api-token".to_string(),
                "secret-token".to_string(),
            )]),
        };
        let header = resolve_addon_network_auth_header(
            "example-addon",
            Some(&AddonNetworkAuth {
                auth_type: "bearer".to_string(),
                secret_key: "api-token".to_string(),
            }),
            &store,
        )
        .unwrap();

        assert_eq!(header, Some("Bearer secret-token".to_string()));
    }

    #[test]
    fn resolves_bearer_auth_from_legacy_addon_secret() {
        let store = TestSecretStore {
            secrets: BTreeMap::from([(
                "addon_example-addon_apitoken".to_string(),
                "legacy-token".to_string(),
            )]),
        };
        let header = resolve_addon_network_auth_header(
            "example-addon",
            Some(&AddonNetworkAuth {
                auth_type: "bearer".to_string(),
                secret_key: "ApiToken".to_string(),
            }),
            &store,
        )
        .unwrap();

        assert_eq!(header, Some("Bearer legacy-token".to_string()));
    }

    #[test]
    fn resolves_basic_auth_from_addon_scoped_secret() {
        let store = TestSecretStore {
            secrets: BTreeMap::from([(
                "addon:example-addon:simplefin-access".to_string(),
                "dXNlcjpwYXNz".to_string(),
            )]),
        };
        let header = resolve_addon_network_auth_header(
            "example-addon",
            Some(&AddonNetworkAuth {
                auth_type: "basic".to_string(),
                secret_key: "simplefin-access".to_string(),
            }),
            &store,
        )
        .unwrap();

        assert_eq!(header, Some("Basic dXNlcjpwYXNz".to_string()));
    }

    #[test]
    fn rejects_invalid_network_auth_requests() {
        let store = TestSecretStore {
            secrets: BTreeMap::new(),
        };

        assert!(resolve_addon_network_auth_header(
            "example-addon",
            Some(&AddonNetworkAuth {
                auth_type: "digest".to_string(),
                secret_key: "api-token".to_string(),
            }),
            &store,
        )
        .is_err());
        assert!(resolve_addon_network_auth_header(
            "example-addon",
            Some(&AddonNetworkAuth {
                auth_type: "bearer".to_string(),
                secret_key: "ApiToken".to_string(),
            }),
            &store,
        )
        .is_err());
    }

    #[test]
    fn recognizes_raw_authorization_headers() {
        assert!(is_authorization_header("Authorization"));
        assert!(is_authorization_header("authorization"));
        assert!(!is_authorization_header("X-Authorization"));
    }
}
