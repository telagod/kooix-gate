//! 强类型 ID — 防止把 OrgId 当 UserId 传

use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

macro_rules! typed_id {
    ($name:ident, $prefix:literal) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
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

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}_{}", $prefix, self.0.simple())
            }
        }

        impl From<Uuid> for $name {
            fn from(u: Uuid) -> Self {
                Self(u)
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
