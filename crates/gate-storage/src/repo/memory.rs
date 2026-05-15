//! 内存版 Repo —— 用于测试与 dev 模式（KOOIX_DEV_INMEMORY=1）。
//!
//! 不追求一致性也不做锁优化，行为契约和 Pg 实现保持一致。

use crate::error::{DbError, DbResult};
use crate::repo::{
    api_key::{ApiKeyRecord, ApiKeyRepo, ApiKeySummaryRecord},
    membership::{MembershipRepo, OrgMemberView, UserMemberships},
    org::OrgRepo,
    project::ProjectRepo,
    user::UserRepo,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use gate_core::id::{ApiKeyId, OrgId, ProjectId, UserId};
use gate_core::identity::{
    OrgRole, OrgStatus, Organization, PlatformRole, Project, ProjectRole, ProjectStatus, User,
    UserStatus,
};
use parking_lot::RwLock;
use std::collections::HashMap;
use uuid::Uuid;

// ----------------------------------------------------------------------------
// UserRepo
// ----------------------------------------------------------------------------

#[derive(Default)]
pub struct InMemoryUserRepo {
    inner: RwLock<UsersInner>,
}

#[derive(Default)]
struct UsersInner {
    users: HashMap<UserId, (User, Option<String>)>, // (user, password_hash)
    by_email: HashMap<String, UserId>,
    failed: HashMap<UserId, i32>,
}

impl InMemoryUserRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 测试快捷：直接塞一个完整 User（绕过 create 也行）
    pub fn seed(&self, user: User, password_hash: Option<String>) {
        let mut g = self.inner.write();
        g.by_email.insert(user.email.to_lowercase(), user.id);
        g.users.insert(user.id, (user, password_hash));
    }
}

#[async_trait]
impl UserRepo for InMemoryUserRepo {
    async fn find_by_id(&self, id: UserId) -> DbResult<User> {
        self.inner
            .read()
            .users
            .get(&id)
            .map(|(u, _)| u.clone())
            .ok_or(DbError::NotFound)
    }

    async fn find_by_email(&self, email: &str) -> DbResult<User> {
        let g = self.inner.read();
        g.by_email
            .get(&email.to_lowercase())
            .and_then(|id| g.users.get(id))
            .map(|(u, _)| u.clone())
            .ok_or(DbError::NotFound)
    }

    async fn find_credentials(&self, email: &str) -> DbResult<(User, Option<String>)> {
        let g = self.inner.read();
        g.by_email
            .get(&email.to_lowercase())
            .and_then(|id| g.users.get(id))
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn create(
        &self,
        email: &str,
        password_hash: Option<&str>,
        display_name: Option<&str>,
    ) -> DbResult<User> {
        let mut g = self.inner.write();
        if g.by_email.contains_key(&email.to_lowercase()) {
            return Err(DbError::Conflict(format!("email {email} already in use")));
        }
        let now = Utc::now();
        let id = UserId::new();
        let user = User {
            id,
            email: email.to_string(),
            display_name: display_name.map(String::from),
            status: UserStatus::Active,
            mfa_enabled: false,
            email_verified_at: None,
            last_login_at: None,
            created_at: now,
            updated_at: now,
        };
        g.by_email.insert(email.to_lowercase(), id);
        g.users
            .insert(id, (user.clone(), password_hash.map(String::from)));
        Ok(user)
    }

    async fn mark_last_login(
        &self,
        id: UserId,
        at: DateTime<Utc>,
        _ip: Option<std::net::IpAddr>,
    ) -> DbResult<()> {
        let mut g = self.inner.write();
        if let Some((u, _)) = g.users.get_mut(&id) {
            u.last_login_at = Some(at);
        }
        g.failed.insert(id, 0);
        Ok(())
    }

    async fn bump_failed_login(&self, id: UserId) -> DbResult<i32> {
        let mut g = self.inner.write();
        if !g.users.contains_key(&id) {
            return Err(DbError::NotFound);
        }
        let n = g.failed.entry(id).or_insert(0);
        *n += 1;
        Ok(*n)
    }

    async fn reset_failed_login(&self, id: UserId) -> DbResult<()> {
        self.inner.write().failed.insert(id, 0);
        Ok(())
    }

    async fn has_any_admin(&self) -> DbResult<bool> {
        Ok(false)
    }

    async fn list_all(&self, _limit: i64, _offset: i64) -> DbResult<Vec<User>> {
        Ok(self
            .inner
            .read()
            .users
            .values()
            .map(|(u, _)| u.clone())
            .collect())
    }

    async fn update_status(&self, id: UserId, status: &str) -> DbResult<User> {
        let mut g = self.inner.write();
        let (user, _) = g.users.get_mut(&id).ok_or(DbError::NotFound)?;
        user.status = match status {
            "active" => UserStatus::Active,
            "suspended" => UserStatus::Suspended,
            _ => return Err(DbError::Internal(format!("unknown status: {status}"))),
        };
        user.updated_at = Utc::now();
        Ok(user.clone())
    }

    async fn update_password(&self, id: UserId, password_hash: &str) -> DbResult<()> {
        let mut g = self.inner.write();
        let (_, ph) = g.users.get_mut(&id).ok_or(DbError::NotFound)?;
        *ph = Some(password_hash.to_string());
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// OrgRepo
// ----------------------------------------------------------------------------

#[derive(Default)]
pub struct InMemoryOrgRepo {
    inner: RwLock<OrgsInner>,
}

#[derive(Default)]
struct OrgsInner {
    orgs: HashMap<OrgId, Organization>,
    by_slug: HashMap<String, OrgId>,
}

impl InMemoryOrgRepo {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn seed(&self, org: Organization) {
        let mut g = self.inner.write();
        g.by_slug.insert(org.slug.to_lowercase(), org.id);
        g.orgs.insert(org.id, org);
    }
}

#[async_trait]
impl OrgRepo for InMemoryOrgRepo {
    async fn find_by_id(&self, id: OrgId) -> DbResult<Organization> {
        self.inner
            .read()
            .orgs
            .get(&id)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn find_by_slug(&self, slug: &str) -> DbResult<Organization> {
        let g = self.inner.read();
        g.by_slug
            .get(&slug.to_lowercase())
            .and_then(|id| g.orgs.get(id))
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn list_for_user(&self, _user_id: UserId) -> DbResult<Vec<Organization>> {
        // 简化：测试场景手动 seed，这里返回全部
        Ok(self.inner.read().orgs.values().cloned().collect())
    }

    async fn create(&self, name: &str, slug: &str, owner: UserId) -> DbResult<Organization> {
        let mut g = self.inner.write();
        if g.by_slug.contains_key(&slug.to_lowercase()) {
            return Err(DbError::Conflict(format!("slug {slug} in use")));
        }
        let now = Utc::now();
        let id = OrgId::new();
        let org = Organization {
            id,
            name: name.to_string(),
            slug: slug.to_string(),
            owner_user_id: owner,
            status: OrgStatus::Active,
            billing_email: None,
            created_at: now,
            updated_at: now,
        };
        g.by_slug.insert(slug.to_lowercase(), id);
        g.orgs.insert(id, org.clone());
        Ok(org)
    }

    async fn list_all(&self) -> DbResult<Vec<Organization>> {
        Ok(self.inner.read().orgs.values().cloned().collect())
    }

    async fn update(
        &self,
        id: OrgId,
        name: Option<&str>,
        billing_email: Option<&str>,
    ) -> DbResult<Organization> {
        let mut g = self.inner.write();
        let org = g.orgs.get_mut(&id).ok_or(DbError::NotFound)?;
        if let Some(n) = name {
            org.name = n.to_string();
        }
        if let Some(e) = billing_email {
            org.billing_email = Some(e.to_string());
        }
        org.updated_at = Utc::now();
        Ok(org.clone())
    }
}

// ----------------------------------------------------------------------------
// ProjectRepo
// ----------------------------------------------------------------------------

#[derive(Default)]
pub struct InMemoryProjectRepo {
    inner: RwLock<HashMap<ProjectId, Project>>,
}

impl InMemoryProjectRepo {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn seed(&self, project: Project) {
        self.inner.write().insert(project.id, project);
    }
}

#[async_trait]
impl ProjectRepo for InMemoryProjectRepo {
    async fn find_by_id(&self, id: ProjectId) -> DbResult<Project> {
        self.inner.read().get(&id).cloned().ok_or(DbError::NotFound)
    }

    async fn list_in_org(&self, org_id: OrgId) -> DbResult<Vec<Project>> {
        Ok(self
            .inner
            .read()
            .values()
            .filter(|p| p.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn create(&self, org_id: OrgId, name: &str, slug: &str) -> DbResult<Project> {
        let mut g = self.inner.write();
        let dup = g
            .values()
            .any(|p| p.org_id == org_id && p.slug.eq_ignore_ascii_case(slug));
        if dup {
            return Err(DbError::Conflict(format!("project slug {slug} in use")));
        }
        let now = Utc::now();
        let id = ProjectId::new();
        let project = Project {
            id,
            org_id,
            name: name.to_string(),
            slug: slug.to_string(),
            status: ProjectStatus::Active,
            default_group_id: None,
            created_at: now,
            updated_at: now,
        };
        g.insert(id, project.clone());
        Ok(project)
    }

    async fn update(
        &self,
        id: ProjectId,
        name: Option<&str>,
        status: Option<&str>,
    ) -> DbResult<Project> {
        let mut g = self.inner.write();
        let project = g.get_mut(&id).ok_or(DbError::NotFound)?;
        if let Some(n) = name {
            project.name = n.to_string();
        }
        if let Some(s) = status {
            project.status = match s {
                "active" => ProjectStatus::Active,
                "archived" => ProjectStatus::Archived,
                _ => return Err(DbError::Internal(format!("unknown project status: {s}"))),
            };
        }
        project.updated_at = Utc::now();
        Ok(project.clone())
    }
}

// ----------------------------------------------------------------------------
// MembershipRepo
// ----------------------------------------------------------------------------

#[derive(Default)]
pub struct InMemoryMembershipRepo {
    inner: RwLock<MembershipsInner>,
}

#[derive(Default)]
struct MembershipsInner {
    orgs: HashMap<(OrgId, UserId), OrgRole>,
    // project_memberships 需要带 org_id 才能正确组合 key
    projects: HashMap<(ProjectId, UserId), (OrgId, ProjectRole)>,
    platform: HashMap<UserId, PlatformRole>,
}

impl InMemoryMembershipRepo {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn seed_platform(&self, user: UserId, role: PlatformRole) {
        self.inner.write().platform.insert(user, role);
    }
    pub fn seed_project(&self, org: OrgId, project: ProjectId, user: UserId, role: ProjectRole) {
        self.inner
            .write()
            .projects
            .insert((project, user), (org, role));
    }
}

#[async_trait]
impl MembershipRepo for InMemoryMembershipRepo {
    async fn load_for_user(&self, user_id: UserId) -> DbResult<UserMemberships> {
        let g = self.inner.read();
        let orgs: HashMap<_, _> = g
            .orgs
            .iter()
            .filter(|((_, u), _)| *u == user_id)
            .map(|((o, _), r)| (*o, *r))
            .collect();
        let projects: HashMap<_, _> = g
            .projects
            .iter()
            .filter(|((_, u), _)| *u == user_id)
            .map(|((p, _), (o, r))| ((*o, *p), *r))
            .collect();
        let platform = g.platform.get(&user_id).copied();
        Ok(UserMemberships {
            orgs,
            projects,
            platform,
        })
    }

    async fn add_org_member(&self, org: OrgId, user: UserId, role: OrgRole) -> DbResult<()> {
        self.inner.write().orgs.insert((org, user), role);
        Ok(())
    }

    async fn add_project_member(
        &self,
        project: ProjectId,
        user: UserId,
        role: ProjectRole,
    ) -> DbResult<()> {
        // 需要从已 seed 的项目里推 org_id；如果没 seed 就用全零（测试 happy path）
        let org = self
            .inner
            .read()
            .projects
            .iter()
            .find(|((p, _), _)| *p == project)
            .map(|(_, (o, _))| *o)
            .unwrap_or_else(|| OrgId::from(Uuid::nil()));
        self.inner
            .write()
            .projects
            .insert((project, user), (org, role));
        Ok(())
    }

    async fn list_org_members(&self, _org: OrgId) -> DbResult<Vec<OrgMemberView>> {
        Ok(vec![])
    }

    async fn remove_org_member(&self, org: OrgId, user: UserId) -> DbResult<()> {
        let mut g = self.inner.write();
        if g.orgs.remove(&(org, user)).is_none() {
            return Err(DbError::NotFound);
        }
        Ok(())
    }
}

// ----------------------------------------------------------------------------
// ApiKeyRepo
// ----------------------------------------------------------------------------

#[derive(Default)]
pub struct InMemoryApiKeyRepo {
    inner: RwLock<ApiKeysInner>,
}

#[derive(Default)]
struct ApiKeysInner {
    by_hash: HashMap<String, ApiKeyRecord>,
    by_id: HashMap<ApiKeyId, String>, // id → hash 反查
}

impl InMemoryApiKeyRepo {
    pub fn new() -> Self {
        Self::default()
    }

    /// 用 hash 字符串 + 完整记录直接 seed（测试用）。
    /// 调用方负责算 hash（保持 `gate-storage` 不依赖 `gate-auth`）。
    pub fn seed(&self, key_hash: &str, record: ApiKeyRecord) {
        let mut g = self.inner.write();
        g.by_id.insert(record.api_key_id, key_hash.to_string());
        g.by_hash.insert(key_hash.to_string(), record);
    }
}

#[async_trait]
impl ApiKeyRepo for InMemoryApiKeyRepo {
    async fn find_by_hash(&self, hash: &str) -> DbResult<ApiKeyRecord> {
        self.inner
            .read()
            .by_hash
            .get(hash)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn create(
        &self,
        project_id: ProjectId,
        name: &str,
        key_hash: &str,
        _key_prefix: &str,
        _key_last4: &str,
        _created_by: UserId,
        allowed_models: &[String],
    ) -> DbResult<ApiKeyId> {
        let id = ApiKeyId::new();
        // 内存版没有 org_id 上下文，调用方需要在 seed 阶段补
        let record = ApiKeyRecord {
            api_key_id: id,
            project_id,
            org_id: OrgId::from(Uuid::nil()),
            name: name.to_string(),
            allowed_ips: vec![],
            allowed_models: allowed_models.to_vec(),
            allowed_groups: vec![],
            expires_at: None,
            revoked_at: None,
        };
        let mut g = self.inner.write();
        g.by_id.insert(id, key_hash.to_string());
        g.by_hash.insert(key_hash.to_string(), record);
        Ok(id)
    }

    async fn list_in_project(&self, project_id: ProjectId) -> DbResult<Vec<ApiKeyRecord>> {
        Ok(self
            .inner
            .read()
            .by_hash
            .values()
            .filter(|r| r.project_id == project_id && !r.is_revoked())
            .cloned()
            .collect())
    }

    async fn list_summaries(&self, project_id: ProjectId) -> DbResult<Vec<ApiKeySummaryRecord>> {
        Ok(self
            .inner
            .read()
            .by_hash
            .values()
            .filter(|r| r.project_id == project_id)
            .map(|r| ApiKeySummaryRecord {
                api_key_id: r.api_key_id,
                name: r.name.clone(),
                prefix: String::new(),
                last4: String::new(),
                allowed_models: r.allowed_models.clone(),
                created_at: Utc::now(),
                last_used_at: None,
                revoked_at: r.revoked_at,
            })
            .collect())
    }

    async fn revoke(&self, id: ApiKeyId, _by: UserId, _reason: Option<&str>) -> DbResult<()> {
        let mut g = self.inner.write();
        let hash = g.by_id.get(&id).cloned().ok_or(DbError::NotFound)?;
        if let Some(rec) = g.by_hash.get_mut(&hash) {
            rec.revoked_at = Some(Utc::now());
        }
        Ok(())
    }

    async fn touch_used(
        &self,
        _id: ApiKeyId,
        _at: DateTime<Utc>,
        _ip: Option<std::net::IpAddr>,
    ) -> DbResult<()> {
        Ok(())
    }
}
