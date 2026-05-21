//! Plugin HTTP outbound sandbox — SSRF guard、DNS rebinding 防护、metadata host 拒绝。
//!
//! 这层是 plugin runtime 的安全边界：
//! - `OutboundAllow` 是 manifest `security.outbound_allowlist` 解析后的运行时形式
//! - `PluginHttpSandbox` 在每次发请求前校验 URL，绝对 URL 默认拒绝
//! - `SandboxDnsResolver` 包一层 reqwest dns，二次阻断 metadata IP

use crate::error::{ProviderError, ProviderResult};
use crate::plugin_manifest::{AuthStrategy, PluginManifest};
use reqwest::Url;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub(super) struct PluginHttpSandbox {
    allowlist: Vec<OutboundAllow>,
    redacted_headers: Vec<HeaderName>,
    allow_absolute_urls: bool,
}

#[derive(Debug, Clone)]
pub(super) struct OutboundAllow {
    scheme: Option<String>,
    host: String,
    port: Option<u16>,
}

#[derive(Debug, Clone, Copy)]
pub(super) enum EndpointKind {
    BaseUrl,
    AbsolutePath,
    OauthToken,
}

impl EndpointKind {
    pub(super) fn label(self) -> &'static str {
        match self {
            EndpointKind::BaseUrl => "base_url",
            EndpointKind::AbsolutePath => "absolute URL",
            EndpointKind::OauthToken => "oauth token_url",
        }
    }

    fn enforce_internal_host(self) -> bool {
        matches!(self, EndpointKind::AbsolutePath | EndpointKind::OauthToken)
    }
}

impl PluginHttpSandbox {
    pub(super) fn new(manifest: &PluginManifest) -> ProviderResult<Self> {
        let allowlist = manifest
            .security
            .outbound_allowlist
            .iter()
            .map(|entry| OutboundAllow::parse(entry))
            .collect::<ProviderResult<Vec<_>>>()?;
        let redacted_headers = redacted_header_names(manifest)?;
        Ok(Self {
            allowlist,
            redacted_headers,
            allow_absolute_urls: manifest.security.allow_absolute_chat_path,
        })
    }

    pub(super) fn validate_endpoint(
        &self,
        endpoint: &str,
        kind: EndpointKind,
    ) -> ProviderResult<()> {
        let parsed = reqwest::Url::parse(endpoint)
            .map_err(|e| ProviderError::Config(format!("invalid plugin endpoint URL: {e}")))?;
        if matches!(kind, EndpointKind::AbsolutePath) && !self.allow_absolute_urls {
            return Err(ProviderError::Config(
                "plugin request.chat_path must be relative; absolute URLs are disabled by default"
                    .into(),
            ));
        }
        match parsed.scheme() {
            "http" | "https" => {}
            other => {
                return Err(ProviderError::Config(format!(
                    "plugin endpoint scheme must be http/https, got {other}"
                )));
            }
        }
        let host = parsed
            .host_str()
            .ok_or_else(|| ProviderError::Config("plugin endpoint URL missing host".into()))?;
        if is_metadata_host(host) {
            return Err(ProviderError::Config(format!(
                "plugin {} targets forbidden host {host}",
                kind.label()
            )));
        }
        let allow_local_sandbox =
            cfg!(test) || std::env::var_os("KOOIX_PLUGIN_ALLOW_LOCALHOST").is_some();
        let is_allowed_localhost =
            allow_localhost_loopback(allow_local_sandbox, kind, parsed.scheme(), host);
        if kind.enforce_internal_host()
            && !is_allowed_localhost
            && is_internal_or_metadata_host(host)
        {
            return Err(ProviderError::Config(format!(
                "plugin {} targets forbidden host {host}",
                kind.label()
            )));
        }
        if !self.allowlist.is_empty() && !self.allowed_url(&parsed) {
            return Err(ProviderError::Config(format!(
                "plugin {} target {} is not in security.outbound_allowlist",
                kind.label(),
                url_origin(&parsed)
            )));
        }
        Ok(())
    }

    fn allowed_url(&self, url: &Url) -> bool {
        self.allowlist.iter().any(|entry| entry.matches(url))
    }

    pub(super) fn validate_resolved_addrs(
        &self,
        host: &str,
        addrs: &[SocketAddr],
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if is_explicit_internal_host(host) && !is_metadata_host(host) {
            return Ok(());
        }
        for addr in addrs {
            if is_internal_or_metadata_ip(addr.ip()) {
                return Err(format!(
                    "plugin DNS rebind guard blocked {host} resolved to {}",
                    addr.ip()
                )
                .into());
            }
        }
        Ok(())
    }

    pub(super) fn validate_response_peer(&self, resp: &reqwest::Response) -> ProviderResult<()> {
        let Some(remote_addr) = resp.remote_addr() else {
            return Ok(());
        };
        let allow_local_sandbox =
            cfg!(test) || std::env::var_os("KOOIX_PLUGIN_ALLOW_LOCALHOST").is_some();
        if let Some(host) = resp.url().host_str() {
            if allow_localhost_loopback(
                allow_local_sandbox,
                EndpointKind::OauthToken,
                resp.url().scheme(),
                host,
            ) || (is_explicit_internal_host(host) && !is_metadata_host(host))
            {
                return Ok(());
            }
            if is_metadata_host(host) {
                return Err(ProviderError::Network(format!(
                    "plugin DNS rebind guard blocked response peer {} for {}",
                    remote_addr.ip(),
                    self.redact_url(resp.url().as_str())
                )));
            }
        }
        if is_internal_or_metadata_ip(remote_addr.ip()) {
            return Err(ProviderError::Network(format!(
                "plugin DNS rebind guard blocked response peer {} for {}",
                remote_addr.ip(),
                self.redact_url(resp.url().as_str())
            )));
        }
        Ok(())
    }

    pub(super) fn redact_url(&self, url: &str) -> String {
        let Ok(mut parsed) = Url::parse(url) else {
            return url.to_string();
        };
        let mut redacted = Vec::new();
        for (key, value) in parsed.query_pairs() {
            if should_redact_query_key(&key) {
                redacted.push((key.into_owned(), "[REDACTED]".to_string()));
            } else {
                redacted.push((key.into_owned(), value.into_owned()));
            }
        }
        if redacted
            .iter()
            .any(|(_, value)| value.as_str() == "[REDACTED]")
        {
            parsed.query_pairs_mut().clear().extend_pairs(redacted);
        }
        parsed.to_string()
    }

    pub(super) fn redact_headers(&self, headers: &HeaderMap) -> HeaderMap {
        let mut redacted = HeaderMap::new();
        for (name, value) in headers {
            if self
                .redacted_headers
                .iter()
                .any(|sensitive| sensitive == name)
            {
                redacted.insert(name.clone(), HeaderValue::from_static("[REDACTED]"));
            } else {
                redacted.insert(name.clone(), value.clone());
            }
        }
        redacted
    }

    pub(super) fn reqwest_error(&self, error: reqwest::Error) -> ProviderError {
        let redacted = error
            .url()
            .map(|url| self.redact_url(url.as_str()))
            .unwrap_or_default();
        let error = error.without_url();
        if !redacted.is_empty() && (error.is_connect() || error.is_timeout() || error.is_request())
        {
            return ProviderError::Network(format!("{error}; url={redacted}"));
        }
        ProviderError::from(error)
    }
}

impl OutboundAllow {
    pub(super) fn parse(entry: &str) -> ProviderResult<Self> {
        let entry = entry.trim();
        let normalized = if entry.contains("://") {
            entry.to_string()
        } else {
            format!("https://{entry}")
        };
        let parsed = Url::parse(&normalized).map_err(|e| {
            ProviderError::Config(format!(
                "invalid plugin outbound_allowlist entry {entry:?}: {e}"
            ))
        })?;
        let host = parsed
            .host_str()
            .ok_or_else(|| {
                ProviderError::Config(format!(
                    "invalid plugin outbound_allowlist entry {entry:?}: missing host"
                ))
            })?
            .trim_matches(['[', ']'])
            .to_ascii_lowercase();
        Ok(Self {
            scheme: Some(parsed.scheme().to_string()),
            host,
            port: parsed.port(),
        })
    }

    pub(super) fn matches(&self, url: &Url) -> bool {
        if self
            .scheme
            .as_deref()
            .is_some_and(|scheme| scheme != url.scheme())
        {
            return false;
        }
        let Some(host) = url.host_str() else {
            return false;
        };
        if host.trim_matches(['[', ']']).to_ascii_lowercase().as_str() != self.host {
            return false;
        }
        self.port == url.port()
    }
}

#[derive(Clone)]
pub(super) struct SandboxDnsResolver {
    sandbox: Arc<PluginHttpSandbox>,
}

impl SandboxDnsResolver {
    pub(super) fn new(sandbox: Arc<PluginHttpSandbox>) -> Self {
        Self { sandbox }
    }
}

impl reqwest::dns::Resolve for SandboxDnsResolver {
    fn resolve(&self, name: reqwest::dns::Name) -> reqwest::dns::Resolving {
        let host = name.as_str().to_string();
        let sandbox = self.sandbox.clone();
        Box::pin(async move {
            let addrs = tokio::net::lookup_host((host.as_str(), 0))
                .await
                .map_err(|err| Box::new(err) as Box<dyn std::error::Error + Send + Sync>)?;
            let addrs = addrs.collect::<Vec<_>>();
            sandbox.validate_resolved_addrs(&host, &addrs)?;
            Ok(Box::new(addrs.into_iter()) as reqwest::dns::Addrs)
        })
    }
}

pub(super) fn redacted_header_names(manifest: &PluginManifest) -> ProviderResult<Vec<HeaderName>> {
    let mut names = Vec::new();
    for name in [
        "authorization",
        "api-key",
        "x-api-key",
        "cookie",
        "set-cookie",
        "x-amz-security-token",
    ] {
        push_header_name(&mut names, name)?;
    }
    if let Some(name) = manifest.auth.header_name() {
        push_header_name(&mut names, name)?;
    }
    match manifest.auth.strategy {
        AuthStrategy::Hmac => {
            push_header_name(&mut names, &manifest.auth.hmac.signature_header)?;
        }
        AuthStrategy::AwsSigv4 => {
            push_header_name(&mut names, "x-amz-date")?;
            push_header_name(&mut names, "x-amz-content-sha256")?;
        }
        _ => {}
    }
    for name in &manifest.security.header_redaction {
        push_header_name(&mut names, name)?;
    }
    Ok(names)
}

pub(super) fn push_header_name(names: &mut Vec<HeaderName>, name: &str) -> ProviderResult<()> {
    let name = HeaderName::from_bytes(name.as_bytes())
        .map_err(|e| ProviderError::Config(format!("invalid plugin header {name:?}: {e}")))?;
    if !names.contains(&name) {
        names.push(name);
    }
    Ok(())
}

pub(super) fn is_internal_or_metadata_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    if matches!(
        host.as_str(),
        "localhost" | "metadata" | "metadata.google.internal"
    ) {
        return true;
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        return is_internal_or_metadata_ip(ip);
    }
    false
}

pub(super) fn is_metadata_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    matches!(host.as_str(), "metadata" | "metadata.google.internal")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|ip| matches!(ip, IpAddr::V4(ip) if ip.octets() == [169, 254, 169, 254]))
}

pub(super) fn is_explicit_internal_host(host: &str) -> bool {
    let host = host.trim_matches(['[', ']']).to_ascii_lowercase();
    host == "localhost" || host.parse::<IpAddr>().is_ok_and(is_internal_or_metadata_ip)
}

pub(super) fn allow_localhost_loopback(
    enabled: bool,
    kind: EndpointKind,
    scheme: &str,
    host: &str,
) -> bool {
    enabled
        && matches!(kind, EndpointKind::OauthToken)
        && scheme == "http"
        && matches!(host, "127.0.0.1" | "localhost" | "::1" | "[::1]")
}

pub(super) fn is_internal_or_metadata_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_broadcast()
                || ip.octets() == [169, 254, 169, 254]
        }
        IpAddr::V6(ip) => {
            ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_unique_local()
                || ip.is_unicast_link_local()
        }
    }
}

pub(super) fn url_origin(url: &Url) -> String {
    let Some(host) = url.host_str() else {
        return url.as_str().to_string();
    };
    match url.port() {
        Some(port) => format!("{}://{}:{port}", url.scheme(), host),
        None => format!("{}://{host}", url.scheme()),
    }
}

pub(super) fn should_redact_query_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("key")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key == "access_token"
}
