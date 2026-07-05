// SPDX-FileCopyrightText: 2026 Veydan Project
// SPDX-License-Identifier: LicenseRef-PolyForm-Perimeter-1.0.1

use crate::models::Profile;

pub const DEFAULT_UA: &str =
    "Mozilla/5.0 (X11; Linux x86_64; rv:127.0) Gecko/20100101 Firefox/127.0";

/// Builds the CAMOU_CONFIG_1 JSON for a Camoufox profile.
/// `win_size` — сохранённый размер окна из xulstore.json; None → дефолт 1280×760.
pub fn build_camoufox_config(profile: &Profile, win_size: Option<(i64, i64)>) -> serde_json::Value {
    let preset = crate::fingerprint::get_preset(&profile.fingerprint_preset);

    let ua = profile
        .user_agent
        .as_deref()
        .or_else(|| preset.map(|p| p.user_agent))
        .unwrap_or(DEFAULT_UA);

    let app_version = preset
        .map(|p| p.app_version)
        .unwrap_or("5.0 (X11; Linux x86_64)");

    let oscpu = preset.map(|p| p.oscpu).unwrap_or("Linux x86_64");

    let platform = profile
        .platform
        .as_deref()
        .or_else(|| preset.map(|p| p.platform))
        .unwrap_or("Linux x86_64");

    // navigator.language must match the first element of navigator.languages
    let language = profile
        .languages
        .split(',')
        .next()
        .map(str::trim)
        .unwrap_or(profile.locale.as_str());

    let languages: Vec<&str> = profile.languages.split(',').map(str::trim).collect();

    let seed = profile
        .id
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));

    let mut cfg = serde_json::json!({
        "showcursor": false,
        "navigator.userAgent": ua,
        "navigator.appVersion": app_version,
        "navigator.platform": platform,
        "navigator.oscpu": oscpu,
        "navigator.language": language,
        "navigator.languages": languages,
        "canvas:seed": seed,
        "audio:seed": seed.wrapping_add(1),
        "fonts:spacing_seed": seed.wrapping_add(2),
        "screen.width": profile.screen_width,
        "screen.height": profile.screen_height,
        "screen.availWidth": profile.screen_width,
        "screen.availHeight": profile.screen_height - 48,
    });

    if let Some(tz) = &profile.timezone {
        cfg["timezone"] = serde_json::Value::String(tz.clone());
    }

    // Physical window size: read from xulstore (persistence) or safe default (1280×760).
    // Camoufox hardcodes window.resizeTo(1280, 1040) on every launch and ignores xulstore.
    // The only way to override this is via window.outerWidth/outerHeight in CAMOU_CONFIG_1.
    // The inline CSS lock that Camoufox sets alongside is countered by chrome.css overrides.
    let (outer_w, outer_h) = win_size.unwrap_or((1280, 760));
    cfg["window.outerWidth"] = serde_json::Value::Number(outer_w.into());
    cfg["window.outerHeight"] = serde_json::Value::Number(outer_h.into());

    // Always allow native parameters — otherwise WebGL appears blocked
    cfg["webGl:parameters:blockIfNotDefined"] = serde_json::Value::Bool(false);
    cfg["webGl2:parameters:blockIfNotDefined"] = serde_json::Value::Bool(false);
    cfg["webGl:shaderPrecisionFormats:blockIfNotDefined"] = serde_json::Value::Bool(false);
    cfg["webGl2:shaderPrecisionFormats:blockIfNotDefined"] = serde_json::Value::Bool(false);

    let webgl_vendor = profile.webgl_vendor.as_deref().or_else(|| {
        preset
            .map(|p| p.default_webgl_vendor)
            .filter(|s| !s.is_empty())
    });
    let webgl_renderer = profile.webgl_renderer.as_deref().or_else(|| {
        preset
            .map(|p| p.default_webgl_renderer)
            .filter(|s| !s.is_empty())
    });

    if let (Some(v), Some(r)) = (webgl_vendor, webgl_renderer) {
        cfg["webGl:vendor"] = serde_json::Value::String(v.to_string());
        cfg["webGl:renderer"] = serde_json::Value::String(r.to_string());

        let mut gl_params = serde_json::Map::new();
        gl_params.insert("7937".to_string(), serde_json::Value::String(r.to_string()));
        gl_params.insert(
            "37445".to_string(),
            serde_json::Value::String(v.to_string()),
        );
        gl_params.insert(
            "37446".to_string(),
            serde_json::Value::String(r.to_string()),
        );
        cfg["webGl:parameters"] = serde_json::Value::Object(gl_params.clone());
        cfg["webGl2:parameters"] = serde_json::Value::Object(gl_params);
    }

    cfg
}
