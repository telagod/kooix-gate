//! 强类型 ID — 防止把 OrgId 当 UserId 传
//!
//! Display / Serialize: `{prefix}_{uuid_simple}` (如 `org_019e2c2126597042bc2ebb9dce0aa638`)
//! Deserialize / FromStr: 接受带前缀或裸 UUID 两种格式
//! sqlx Encode/Decode: 裸 UUID（数据库层不变）

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use uuid::Uuid;

fn serialize_typed_id<S: serde::Serializer>(
    prefix: &str,
    uuid: &Uuid,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_str(&format!("{}_{}", prefix, uuid.simple()))
}

fn deserialize_typed_id<'de, D: serde::Deserializer<'de>>(
    prefix: &str,
    deserializer: D,
) -> Result<Uuid, D::Error> {
    let s = String::deserialize(deserializer)?;
    parse_typed_or_raw(prefix, &s).map_err(serde::de::Error::custom)
}

fn parse_typed_or_raw(prefix: &str, s: &str) -> Result<Uuid, String> {
    let expected_prefix = format!("{}_", prefix);
    if let Some(hex) = s.strip_prefix(&expected_prefix) {
        Uuid::parse_str(hex).map_err(|e| format!("invalid {prefix} id: {e}"))
    } else {
        Uuid::parse_str(s).map_err(|e| format!("invalid id (expected {prefix}_... or raw UUID): {e}"))
    }
}

macro_rules! typed_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub struct $name(pub Uuid);

        impl $name {
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }
            pub fn as_uuid(&self) -> &Uuid {
                &self.0
            }
            pub const PREFIX: &'static str = $prefix;
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, self.0.simple())
            }
        }

        impl FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                parse_typed_or_raw($prefix, s).map(Self)
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
            }
        }

        impl Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serialize_typed_id($prefix, &self.0, serializer)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserialize_typed_id($prefix, deserializer).map(Self)
            }
        }
    };
}

typed_id!(OrgId, "org");
typed_id!(UserId, "usr");
typed_id!(ProjectId, "proj");
typed_id!(ApiKeyId, "key");
typed_id!(ChannelId, "ch");
typed_id!(ChannelKeyId, "ck");
typed_id!(ChannelGroupId, "grp");
typed_id!(ModelAliasId, "alias");
typed_id!(QuotaId, "q");
typed_id!(RoleId, "role");
typed_id!(AuditLogId, "audit");

/// A flexible UUID wrapper used as path parameter.
/// Accepts `{prefix}_{hex}` or raw UUID during deserialization.
/// Re-exported here so route handlers can reference it without importing gate-server internals.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlexUuid(pub Uuid);

impl std::ops::Deref for FlexUuid {
    type Target = Uuid;
    fn deref(&self) -> &Uuid { &self.0 }
}

impl fmt::Display for FlexUuid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { self.0.fmt(f) }
}

impl From<FlexUuid> for Uuid {
    fn from(f: FlexUuid) -> Self { f.0 }
}

impl PartialEq<Uuid> for FlexUuid {
    fn eq(&self, other: &Uuid) -> bool { self.0 == *other }
}

impl<'de> serde::Deserialize<'de> for FlexUuid {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        let stripped = match s.find('_') {
            Some(idx) => &s[idx + 1..],
            None => &s,
        };
        Uuid::parse_str(stripped).map(FlexUuid).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_format() {
        let id = OrgId::from(Uuid::parse_str("019e2c1b-a7d1-7162-8422-07e4b24f5f98").unwrap());
        assert_eq!(id.to_string(), "org_019e2c1ba7d17162842207e4b24f5f98");
    }

    #[test]
    fn from_str_prefixed() {
        let id: OrgId = "org_019e2c1ba7d17162842207e4b24f5f98".parse().unwrap();
        assert_eq!(id.0, Uuid::parse_str("019e2c1b-a7d1-7162-8422-07e4b24f5f98").unwrap());
    }

    #[test]
    fn from_str_raw_uuid() {
        let id: OrgId = "019e2c1b-a7d1-7162-8422-07e4b24f5f98".parse().unwrap();
        assert_eq!(id.0, Uuid::parse_str("019e2c1b-a7d1-7162-8422-07e4b24f5f98").unwrap());
    }

    #[test]
    fn serde_roundtrip() {
        let id = ProjectId::from(Uuid::parse_str("019e2c1b-a7d1-7162-8422-07e4b24f5f98").unwrap());
        let json = serde_json::to_string(&id).unwrap();
        assert!(json.starts_with("\"proj_"), "got: {json}");
        let parsed: ProjectId = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, id);
    }

    #[test]
    fn deserialize_raw_uuid_compat() {
        let parsed: OrgId = serde_json::from_str("\"019e2c1b-a7d1-7162-8422-07e4b24f5f98\"").unwrap();
        assert_eq!(*parsed.as_uuid(), Uuid::parse_str("019e2c1b-a7d1-7162-8422-07e4b24f5f98").unwrap());
    }
}
