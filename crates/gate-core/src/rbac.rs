//! RBAC: 权限点 + Role 映射 + 检查门面

use crate::identity::{OrgRole, PlatformRole, ProjectRole};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fmt;

/// 资源类型 — 与 audit_logs.resource_type 对齐
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Org,
    Project,
    User,
    Membership,
    ApiKey,
    Channel,
    ChannelKey,
    ChannelGroup,
    ModelAlias,
    Quota,
    Usage,
    AuditLog,
    Billing,
}

/// 权限点 — 字符串字面量，存数据库不丢类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Permission {
    // Org
    OrgRead,
    OrgUpdate,
    OrgDelete,
    OrgBillingRead,
    OrgBillingWrite,
    OrgMemberInvite,
    OrgMemberRemove,
    OrgMemberRoleChange,

    // Project
    ProjectCreate,
    ProjectRead,
    ProjectUpdate,
    ProjectDelete,
    ProjectMemberInvite,
    ProjectMemberRemove,
    ProjectMemberRoleChange,

    // API Keys
    ApiKeyCreate,
    ApiKeyRead,
    ApiKeyUpdate,
    ApiKeyRevoke,
    ApiKeyRotate,

    // Channels (平台级)
    ChannelCreate,
    ChannelRead,
    ChannelUpdate,
    ChannelDelete,
    ChannelKeyManage,

    // Groups & Routing
    GroupCreate,
    GroupRead,
    GroupUpdate,
    GroupDelete,
    GroupBindProject,
    ModelAliasManage,

    // Quota
    QuotaRead,
    QuotaWrite,

    // Usage & Audit
    UsageRead,
    UsageExport,
    AuditRead,

    // Platform
    PlatformAdmin,
}

impl Permission {
    pub fn as_str(&self) -> &'static str {
        use Permission::*;
        match self {
            OrgRead => "org.read",
            OrgUpdate => "org.update",
            OrgDelete => "org.delete",
            OrgBillingRead => "org.billing.read",
            OrgBillingWrite => "org.billing.write",
            OrgMemberInvite => "org.member.invite",
            OrgMemberRemove => "org.member.remove",
            OrgMemberRoleChange => "org.member.role_change",
            ProjectCreate => "project.create",
            ProjectRead => "project.read",
            ProjectUpdate => "project.update",
            ProjectDelete => "project.delete",
            ProjectMemberInvite => "project.member.invite",
            ProjectMemberRemove => "project.member.remove",
            ProjectMemberRoleChange => "project.member.role_change",
            ApiKeyCreate => "apikey.create",
            ApiKeyRead => "apikey.read",
            ApiKeyUpdate => "apikey.update",
            ApiKeyRevoke => "apikey.revoke",
            ApiKeyRotate => "apikey.rotate",
            ChannelCreate => "channel.create",
            ChannelRead => "channel.read",
            ChannelUpdate => "channel.update",
            ChannelDelete => "channel.delete",
            ChannelKeyManage => "channel.key.manage",
            GroupCreate => "group.create",
            GroupRead => "group.read",
            GroupUpdate => "group.update",
            GroupDelete => "group.delete",
            GroupBindProject => "group.bind_project",
            ModelAliasManage => "model_alias.manage",
            QuotaRead => "quota.read",
            QuotaWrite => "quota.write",
            UsageRead => "usage.read",
            UsageExport => "usage.export",
            AuditRead => "audit.read",
            PlatformAdmin => "platform.admin",
        }
    }
}

impl fmt::Display for Permission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 检查的作用域 — 决定 actor 的权限来源
#[derive(Debug, Clone, Copy)]
pub enum Scope<'a> {
    Platform,
    Org(&'a crate::id::OrgId),
    Project {
        org: &'a crate::id::OrgId,
        project: &'a crate::id::ProjectId,
    },
}

/// 角色 → 权限集合的静态映射
pub fn permissions_of_org_role(role: OrgRole) -> HashSet<Permission> {
    use Permission::*;
    match role {
        OrgRole::Owner => HashSet::from([
            OrgRead, OrgUpdate, OrgDelete,
            OrgBillingRead, OrgBillingWrite,
            OrgMemberInvite, OrgMemberRemove, OrgMemberRoleChange,
            ProjectCreate, ProjectRead, ProjectUpdate, ProjectDelete,
            ProjectMemberInvite, ProjectMemberRemove, ProjectMemberRoleChange,
            ApiKeyCreate, ApiKeyRead, ApiKeyUpdate, ApiKeyRevoke, ApiKeyRotate,
            GroupBindProject, ModelAliasManage,
            QuotaRead, QuotaWrite,
            UsageRead, UsageExport, AuditRead,
        ]),
        OrgRole::Admin => HashSet::from([
            OrgRead, OrgUpdate,
            OrgMemberInvite, OrgMemberRemove, OrgMemberRoleChange,
            ProjectCreate, ProjectRead, ProjectUpdate, ProjectDelete,
            ProjectMemberInvite, ProjectMemberRemove, ProjectMemberRoleChange,
            ApiKeyCreate, ApiKeyRead, ApiKeyUpdate, ApiKeyRevoke, ApiKeyRotate,
            GroupBindProject, ModelAliasManage,
            QuotaRead, QuotaWrite,
            UsageRead, UsageExport, AuditRead,
        ]),
        OrgRole::BillingViewer => HashSet::from([
            OrgRead, OrgBillingRead,
            ProjectRead, UsageRead, QuotaRead,
        ]),
        OrgRole::Member => HashSet::from([
            OrgRead, ProjectRead,
        ]),
    }
}

pub fn permissions_of_project_role(role: ProjectRole) -> HashSet<Permission> {
    use Permission::*;
    match role {
        ProjectRole::Owner => HashSet::from([
            ProjectRead, ProjectUpdate, ProjectDelete,
            ProjectMemberInvite, ProjectMemberRemove, ProjectMemberRoleChange,
            ApiKeyCreate, ApiKeyRead, ApiKeyUpdate, ApiKeyRevoke, ApiKeyRotate,
            GroupBindProject, ModelAliasManage,
            QuotaRead, QuotaWrite,
            UsageRead, UsageExport, AuditRead,
        ]),
        ProjectRole::Admin => HashSet::from([
            ProjectRead, ProjectUpdate,
            ProjectMemberInvite, ProjectMemberRemove,
            ApiKeyCreate, ApiKeyRead, ApiKeyUpdate, ApiKeyRevoke, ApiKeyRotate,
            ModelAliasManage,
            QuotaRead, QuotaWrite,
            UsageRead, AuditRead,
        ]),
        ProjectRole::Developer => HashSet::from([
            ProjectRead,
            ApiKeyCreate, ApiKeyRead, ApiKeyUpdate, ApiKeyRevoke,
            UsageRead,
        ]),
        ProjectRole::Viewer => HashSet::from([
            ProjectRead, ApiKeyRead, UsageRead, QuotaRead,
        ]),
    }
}

pub fn permissions_of_platform_role(role: PlatformRole) -> HashSet<Permission> {
    use Permission::*;
    match role {
        PlatformRole::SuperAdmin => HashSet::from([
            PlatformAdmin,
            ChannelCreate, ChannelRead, ChannelUpdate, ChannelDelete, ChannelKeyManage,
            GroupCreate, GroupRead, GroupUpdate, GroupDelete,
            AuditRead,
        ]),
        PlatformRole::Operator => HashSet::from([
            ChannelCreate, ChannelRead, ChannelUpdate, ChannelKeyManage,
            GroupCreate, GroupRead, GroupUpdate,
            AuditRead,
        ]),
        PlatformRole::Support => HashSet::from([
            ChannelRead, GroupRead, AuditRead,
        ]),
    }
}
