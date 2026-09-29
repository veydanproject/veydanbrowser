//! What VPush and its clients exchange over HTTP.
//!
//! Only serde lives here, so a client can depend on this crate without
//! pulling in the server.

use serde::{Deserialize, Serialize};

/// Answer of `GET /healthz`, and of `vpush ctl health`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    pub status: HealthStatus,
    pub version: String,
    pub git_sha: String,
    /// Unix seconds.
    pub built_at: u64,
    pub uptime_secs: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HealthStatus {
    Ok,
}

/// Machine-readable reason of a failed request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    BadRequest,
    NotFound,
    PayloadTooLarge,
    Internal,
}

/// Body of every non-2xx answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    pub error: ErrorDetail,
    pub request_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorDetail {
    pub code: ErrorCode,
    pub message: String,
}

/// What a push is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PushType {
    /// A direct message arrived.
    Dm,
    /// A message arrived in a group.
    Group,
    /// Something was missed; the device should look at its relays.
    Sync,
    /// Asked for by the user or the operator, to see that pushes arrive.
    Test,
    /// A message of the operator to everyone.
    Broadcast,
    /// The project manifest changed. Silent.
    ManifestUpdate,
}

impl PushType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Dm => "dm",
            Self::Group => "group",
            Self::Sync => "sync",
            Self::Test => "test",
            Self::Broadcast => "broadcast",
            Self::ManifestUpdate => "manifest_update",
        }
    }
}

/// Version of [`Payload`]. A device that meets a higher one should ask the
/// user to update the app instead of guessing.
pub const PAYLOAD_VERSION: u32 = 1;

/// What arrives on the device, the same through every provider.
///
/// Providers that carry only strings (FCM) get it through [`Payload::to_data`]:
/// the same keys, every value a string.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Payload {
    pub v: u32,
    #[serde(rename = "type")]
    pub kind: PushType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event_id: Option<String>,
    /// The relay the event was seen on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub relay: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    /// The name this device gave the group when it registered.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_name: Option<String>,
    /// Text to show when the device cannot make a better one. Absent in
    /// silent pushes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// The event itself, as JSON, when it fits. Otherwise the device takes
    /// it from `relay` by `event_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub event: Option<String>,
    /// How many events this push stands for, when more than one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<u32>,
    /// Payload of a service push, as JSON; its meaning depends on `type`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// Id of this push in the server's log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trace: Option<String>,
}

impl Payload {
    pub fn new(kind: PushType) -> Self {
        Self {
            v: PAYLOAD_VERSION,
            kind,
            event_id: None,
            relay: None,
            group_id: None,
            group_name: None,
            title: None,
            body: None,
            event: None,
            count: None,
            data: None,
            trace: None,
        }
    }

    /// Shown to the user by the device, or handled without a sound.
    pub fn is_silent(&self) -> bool {
        self.title.is_none() && self.body.is_none()
    }

    /// The same keys as in the JSON form, every value a string.
    pub fn to_data(&self) -> std::collections::BTreeMap<String, String> {
        let mut out = std::collections::BTreeMap::new();
        out.insert("v".to_string(), self.v.to_string());
        out.insert("type".to_string(), self.kind.as_str().to_string());
        let mut put = |key: &str, value: &Option<String>| {
            if let Some(v) = value {
                out.insert(key.to_string(), v.clone());
            }
        };
        put("event_id", &self.event_id);
        put("relay", &self.relay);
        put("group_id", &self.group_id);
        put("group_name", &self.group_name);
        put("title", &self.title);
        put("body", &self.body);
        put("event", &self.event);
        put("data", &self.data);
        put("trace", &self.trace);
        if let Some(count) = self.count {
            out.insert("count".to_string(), count.to_string());
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_and_string_forms_have_the_same_keys() {
        let mut p = Payload::new(PushType::Group);
        p.event_id = Some("e1".into());
        p.group_name = Some("Team".into());
        p.title = Some("Group: Team".into());
        p.count = Some(3);

        let json = serde_json::to_value(&p).unwrap();
        let data = p.to_data();
        let json_keys: Vec<_> = json.as_object().unwrap().keys().cloned().collect();
        let data_keys: Vec<_> = data.keys().cloned().collect();
        assert_eq!(json_keys, data_keys);
        assert_eq!(data["type"], "group");
        assert_eq!(data["count"], "3");
        assert_eq!(data["v"], "1");
    }

    #[test]
    fn absent_fields_are_not_sent() {
        let p = Payload::new(PushType::ManifestUpdate);
        assert!(p.is_silent());
        assert_eq!(
            serde_json::to_string(&p).unwrap(),
            r#"{"v":1,"type":"manifest_update"}"#
        );
        assert_eq!(p.to_data().len(), 2);
    }

    #[test]
    fn payload_reads_back() {
        let mut p = Payload::new(PushType::Dm);
        p.event = Some(r#"{"kind":1059}"#.into());
        let back: Payload = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }
}
