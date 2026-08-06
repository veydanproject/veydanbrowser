// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::models::{Proxy, ProxyCheckResult};
use anyhow::Result;
use serde::Deserialize;

#[derive(Deserialize)]
struct IpApiResponse {
    // ip-api.com returns the requester's IP in the `query` field
    #[serde(rename = "query")]
    ip: Option<String>,
    #[serde(rename = "countryCode")]
    country_code: Option<String>,
    city: Option<String>,
}

pub async fn check_proxy(proxy: &Proxy) -> Result<ProxyCheckResult> {
    if proxy.proxy_type == "ssh" {
        return check_ssh(proxy).await;
    }

    let proxy_url = build_proxy_url(proxy);

    let reqwest_proxy = reqwest::Proxy::all(&proxy_url)?;
    let client = reqwest::Client::builder()
        .proxy(reqwest_proxy)
        .timeout(std::time::Duration::from_secs(15))
        .build()?;

    let resp = client
        .get("http://ip-api.com/json?fields=query,countryCode,city")
        .send()
        .await?
        .json::<IpApiResponse>()
        .await?;

    let ip = resp.ip.unwrap_or_default();
    Ok(ProxyCheckResult {
        ok: !ip.is_empty(),
        ip,
        country: resp.country_code,
        city: resp.city,
        ssh_fingerprint: None,
        ssh_fingerprint_is_new: None,
    })
}

/// Для SSH: подключаемся, авторизуемся, открываем direct-tcpip канал к ip-api.com
async fn check_ssh(proxy: &Proxy) -> Result<ProxyCheckResult> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let auth = if let Some(key) = &proxy.private_key {
        if !key.is_empty() {
            crate::proxy::ssh::SshAuth::PrivateKey(key.clone())
        } else {
            crate::proxy::ssh::SshAuth::Password(proxy.password.clone().unwrap_or_default())
        }
    } else {
        crate::proxy::ssh::SshAuth::Password(proxy.password.clone().unwrap_or_default())
    };

    let username = proxy.username.clone().unwrap_or_default();
    let result = crate::proxy::ssh::SshSession::connect(
        &proxy.host,
        proxy.port as u16,
        &username,
        auth,
        proxy.server_fingerprint.clone(),
    )
    .await?;

    let channel = result.session.open_channel("ip-api.com", 80).await?;
    let stream = channel.into_stream();
    let (mut r, mut w) = tokio::io::split(stream);

    w.write_all(b"GET /json?fields=query,countryCode,city HTTP/1.0\r\nHost: ip-api.com\r\nConnection: close\r\n\r\n").await?;

    let mut response = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match tokio::time::timeout(std::time::Duration::from_secs(10), r.read(&mut buf)).await {
            Ok(Ok(0)) | Err(_) => break,
            Ok(Ok(n)) => response.extend_from_slice(&buf[..n]),
            Ok(Err(_)) => break,
        }
    }

    let text = String::from_utf8_lossy(&response);
    // HTTP response: skip headers, find JSON body
    let body = if let Some(pos) = text.find("\r\n\r\n") {
        &text[pos + 4..]
    } else {
        &text
    };

    let parsed: IpApiResponse = serde_json::from_str(body.trim()).unwrap_or(IpApiResponse {
        ip: None,
        country_code: None,
        city: None,
    });

    let ip = parsed.ip.unwrap_or_default();
    Ok(ProxyCheckResult {
        ok: !ip.is_empty(),
        ip,
        country: parsed.country_code,
        city: parsed.city,
        ssh_fingerprint: Some(result.fingerprint),
        ssh_fingerprint_is_new: Some(result.is_new),
    })
}

pub fn build_proxy_url(proxy: &Proxy) -> String {
    let scheme = match proxy.proxy_type.as_str() {
        // socks5h:// = DNS resolves via proxy (consistent with Firefox socks_remote_dns=true)
        "socks5" => "socks5h",
        "https" => "https",
        _ => "http",
    };

    match (&proxy.username, &proxy.password) {
        (Some(user), Some(pass)) if !user.is_empty() => {
            use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
            let user = utf8_percent_encode(user, NON_ALPHANUMERIC);
            let pass = utf8_percent_encode(pass, NON_ALPHANUMERIC);
            format!("{scheme}://{}:{}@{}:{}", user, pass, proxy.host, proxy.port)
        }
        _ => format!("{scheme}://{}:{}", proxy.host, proxy.port),
    }
}
