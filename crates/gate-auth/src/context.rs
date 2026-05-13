//! AuthContext: 一次性解析好的「我是谁，我能干什么」
//!
//! ## 设计纪律
//!
//! AuthContext 的内部 HashMap 字段是 `pub(crate)`：
//! **外部代码无法读取 raw 角色映射**，只能通过 `can()` / `require()` 做权限决策，
//! 或通过 `org_role()` / `project_role()` 拿到只读拷贝（仅用于 UI 提示，不可用于授权）。
//!
//! 这从语言层面强制所有 handler 走统一权限门面，杜绝
//! `if ctx.org_memberships.contains_key(&org) { ... }` 这种绕路写法。

use crate::error::{AuthError, Result};
use gate_core::id::{ApiKeyId, OrgId, ProjectId, UserId};
use gate_core::identity::{OrgRole, PlatformRole, ProjectRole};
use gate_core::rbac::{
    permissions_of_org_role, permissions_of_platform_role, permissions_of_project_role,
    Permission, Scope,
};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub enum Subject {
    /// 控制台登录用户
    User {
        user_id: UserId,
        session_id: uuid::Uuid,
    },
    /// 通过 API key 调用的程序方
    ApiKey {
        api_key_id: ApiKeyId,
        project_id: ProjectId,
        org_id: OrgId,
    },
    /// 系统内部任务（迁移、后台 worker 等）
    System,
}

impl Subject {
    pub fn is_user(&self) -> bool {
        matches!(self, Self::User { .. })
    }
    pub fn is_api_key(&self) -> bool {
        matches!(self, Self::ApiKey { .. })
    }
    pub fn is_system(&self) -> bool {
        matches!(self, Self::System)
    }
}

/// 一次请求的授权上下文。
///
/// 构造只有三个入口：[`AuthContext::anonymous`] / [`AuthContext::user`] / [`AuthContext::api_key`]。
/// 角色映射字段是 `pub(crate)`，外部访问只能通过 reader 方法。
///
/// **Project 权限始终与 Org 绑定**：`project_memberships` 用 `(OrgId, ProjectId)` 复合 key，
/// 防止跨 Org 重放——即使攻击者拿到了项目 ID，没有对应 Org 上下文也用不上。
#[derive(Debug, Clone, Default)]
pub struct AuthContext {
    pub(crate) subject: Option<Subject>,
    pub(crate) org_memberships: HashMap<OrgId, OrgRole>,
    pub(crate) project_memberships: HashMap<(OrgId, ProjectId), ProjectRole>,
    pub(crate) platform_role: Option<PlatformRole>,
    /// 控制台当前激活的 Org（user 模式下用于切换租户）
    pub(crate) current_org: Option<OrgId>,
}

impl AuthContext {
    // ---- 构造 -------------------------------------------------------------

    pub fn anonymous() -> Self {
        Self::default()
    }

    pub fn user(
        user_id: UserId,
        session_id: uuid::Uuid,
        org_memberships: HashMap<OrgId, OrgRole>,
        project_memberships: HashMap<(OrgId, ProjectId), ProjectRole>,
        platform_role: Option<PlatformRole>,
        current_org: Option<OrgId>,
    ) -> Self {
        Self {
            subject: Some(Subject::User {
                user_id,
                session_id,
            }),
            org_memberships,
            project_memberships,
            platform_role,
            current_org,
        }
    }

    pub fn api_key(api_key_id: ApiKeyId, project_id: ProjectId, org_id: OrgId) -> Self {
        Self {
            subject: Some(Subject::ApiKey {
                api_key_id,
                project_id,
                org_id,
            }),
            current_org: Some(org_id),
            ..Default::default()
        }
    }

    pub fn system() -> Self {
        Self {
            subject: Some(Subject::System),
            platform_role: Some(PlatformRole::SuperAdmin),
            ..Default::default()
        }
    }

    // ---- 只读 accessor（不可用于授权决策） --------------------------------

    pub fn subject(&self) -> Option<&Subject> {
        self.subject.as_ref()
    }

    pub fn user_id(&self) -> Option<UserId> {
        match self.subject {
            Some(Subject::User { user_id, .. }) => Some(user_id),
            _ => None,
        }
    }

    pub fn session_id(&self) -> Option<uuid::Uuid> {
        match self.subject {
            Some(Subject::User { session_id, .. }) => Some(session_id),
            _ => None,
        }
    }

    pub fn api_key_id(&self) -> Option<ApiKeyId> {
        match self.subject {
            Some(Subject::ApiKey { api_key_id, .. }) => Some(api_key_id),
            _ => None,
        }
    }

    pub fn platform_role(&self) -> Option<PlatformRole> {
        self.platform_role
    }

    /// 当前激活的 Org（控制台 + API key 都适用）
    pub fn current_org(&self) -> Option<OrgId> {
        self.current_org.or_else(|| match self.subject {
            Some(Subject::ApiKey { org_id, .. }) => Some(org_id),
            _ => None,
        })
    }

    /// 是否平台 SuperAdmin（短路所有授权检查）
    pub fn is_super_admin(&self) -> bool {
        matches!(self.platform_role, Some(PlatformRole::SuperAdmin))
    }

    /// 在指定 Org 中的角色 — **仅供 UI 展示用，不可用于授权决策**。
    /// 授权请走 `can()` / `require()`。
    pub fn org_role(&self, org: &OrgId) -> Option<OrgRole> {
        self.org_memberships.get(org).copied()
    }

    /// 在指定 Org+Project 中的角色 — **仅供 UI 展示用，不可用于授权决策**。
    pub fn project_role(&self, org: &OrgId, project: &ProjectId) -> Option<ProjectRole> {
        self.project_memberships.get(&(*org, *project)).copied()
    }

    /// 列出当前 user 加入的所有 Org（用于切换租户菜单）
    pub fn accessible_orgs(&self) -> Vec<OrgId> {
        self.org_memberships.keys().copied().collect()
    }

    // ---- 授权决策（唯一入口） ---------------------------------------------

    /// 计算指定 Scope 下的有效权限集（合并平台 + Org + Project）。
    pub fn permissions_at(&self, scope: Scope<'_>) -> HashSet<Permission> {
        let mut set = HashSet::new();

        if let Some(p) = self.platform_role {
            set.extend(permissions_of_platform_role(p));
        }

        match scope {
            Scope::Platform => {}
            Scope::Org(org) => {
                if let Some(role) = self.org_memberships.get(org) {
                    set.extend(permissions_of_org_role(*role));
                }
            }
            Scope::Project { org, project } => {
                if let Some(role) = self.org_memberships.get(org) {
                    set.extend(permissions_of_org_role(*role));
                }
                // 关键：项目权限必须与所声明的 Org 绑定 — 跨 Org 重放无效
                if let Some(role) = self.project_memberships.get(&(*org, *project)) {
                    set.extend(permissions_of_project_role(*role));
                }
            }
        }
        set
    }

    /// 检查是否拥有权限。SuperAdmin 短路。
    pub fn can(&self, perm: Permission, scope: Scope<'_>) -> bool {
        if self.is_super_admin() {
            return true;
        }
        self.permissions_at(scope).contains(&perm)
    }

    /// 强制检查，缺权限则返回 403 错误。
    pub fn require(&self, perm: Permission, scope: Scope<'_>) -> Result<()> {
        if self.can(perm, scope) {
            Ok(())
        } else {
            Err(AuthError::Forbidden {
                action: perm.to_string(),
                resource: match scope {
                    Scope::Platform => "platform".into(),
                    Scope::Org(o) => format!("org:{o}"),
                    Scope::Project { project, .. } => format!("project:{project}"),
                },
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gate_core::id::{OrgId, ProjectId, UserId};
    use gate_core::identity::{OrgRole, ProjectRole};

    fn make_ctx() -> (AuthContext, OrgId, ProjectId) {
        let user = UserId::new();
        let org = OrgId::new();
        let proj = ProjectId::new();
        let mut org_m = HashMap::new();
        org_m.insert(org, OrgRole::Member);
        let mut proj_m = HashMap::new();
        proj_m.insert((org, proj), ProjectRole::Developer);
        let ctx = AuthContext::user(user, uuid::Uuid::now_v7(), org_m, proj_m, None, Some(org));
        (ctx, org, proj)
    }

    #[test]
    fn developer_can_create_apikey() {
        let (ctx, org, proj) = make_ctx();
        assert!(ctx.can(Permission::ApiKeyCreate, Scope::Project { org: &org, project: &proj }));
    }

    #[test]
    fn developer_cannot_change_quota() {
        let (ctx, org, proj) = make_ctx();
        assert!(!ctx.can(Permission::QuotaWrite, Scope::Project { org: &org, project: &proj }));
    }

    #[test]
    fn cross_org_project_id_replay_denied() {
        // 攻击者拿到合法 project_id，但替换 Org 上下文为他不属于的 OrgB
        let (ctx, _, proj) = make_ctx();
        let other_org = OrgId::new();
        assert!(!ctx.can(
            Permission::ProjectRead,
            Scope::Project { org: &other_org, project: &proj }
        ));
        // 即使有 Project::Developer 也不行
        assert!(!ctx.can(
            Permission::ApiKeyCreate,
            Scope::Project { org: &other_org, project: &proj }
        ));
    }

    #[test]
    fn super_admin_short_circuits() {
        let mut ctx = AuthContext::default();
        ctx.platform_role = Some(PlatformRole::SuperAdmin);
        let org = OrgId::new();
        assert!(ctx.can(Permission::OrgDelete, Scope::Org(&org)));
    }

    #[test]
    fn accessor_returns_role_for_display() {
        let (ctx, org, proj) = make_ctx();
        assert_eq!(ctx.org_role(&org), Some(OrgRole::Member));
        assert_eq!(ctx.project_role(&org, &proj), Some(ProjectRole::Developer));
    }
}
