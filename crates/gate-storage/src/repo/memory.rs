//! 内存版 Repo —— 用于测试与 dev 模式（KOOIX_DEV_INMEMORY=1）。
//!
//! 不追求一致性也不做锁优化，行为契约和 Pg 实现保持一致。

use crate::error::{DbError, DbResult};
use crate::repo::{
    api_key::{ApiKeyRecord, ApiKeyRepo, ApiKeySummaryRecord},
    membership::{MembershipRepo, UserMemberships},
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
use std::collections::HashMap;
use std::sync::RwLock;
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
        let mut g = self.inner.write().unwrap();
        g.by_email.insert(user.email.to_lowercase(), user.id);
        g.users.insert(user.id, (user, password_hash));
    }
}

#[async_trait]
impl UserRepo for InMemoryUserRepo {
    async fn find_by_id(&self, id: UserId) -> DbResult<User> {
        self.inner
            .read()
            .unwrap()
            .users
            .get(&id)
            .map(|(u, _)| u.clone())
            .ok_or(DbError::NotFound)
    }

    async fn find_by_email(&self, email: &str) -> DbResult<User> {
        let g = self.inner.read().unwrap();
        g.by_email
            .get(&email.to_lowercase())
            .and_then(|id| g.users.get(id))
            .map(|(u, _)| u.clone())
            .ok_or(DbError::NotFound)
    }

    async fn find_credentials(&self, email: &str) -> DbResult<(User, Option<String>)> {
        let g = self.inner.read().unwrap();
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
        let mut g = self.inner.write().unwrap();
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
        let mut g = self.inner.write().unwrap();
        if let Some((u, _)) = g.users.get_mut(&id) {
            u.last_login_at = Some(at);
        }
        g.failed.insert(id, 0);
        Ok(())
    }

    async fn bump_failed_login(&self, id: UserId) -> DbResult<i32> {
        let mut g = self.inner.write().unwrap();
        if !g.users.contains_key(&id) {
            return Err(DbError::NotFound);
        }
        let n = g.failed.entry(id).or_insert(0);
        *n += 1;
        Ok(*n)
    }

    async fn reset_failed_login(&self, id: UserId) -> DbResult<()> {
        self.inner.write().unwrap().failed.insert(id, 0);
        Ok(())
    }

    async fn has_any_admin(&self) -> DbResult<bool> {
        Ok(false)
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
        let mut g = self.inner.write().unwrap();
        g.by_slug.insert(org.slug.to_lowercase(), org.id);
        g.orgs.insert(org.id, org);
    }
}

#[async_trait]
impl OrgRepo for InMemoryOrgRepo {
    async fn find_by_id(&self, id: OrgId) -> DbResult<Organization> {
        self.inner
            .read()
            .unwrap()
            .orgs
            .get(&id)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn find_by_slug(&self, slug: &str) -> DbResult<Organization> {
        let g = self.inner.read().unwrap();
        g.by_slug
            .get(&slug.to_lowercase())
            .and_then(|id| g.orgs.get(id))
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn list_for_user(&self, _user_id: UserId) -> DbResult<Vec<Organization>> {
        // 简化：测试场景手动 seed，这里返回全部
        Ok(self.inner.read().unwrap().orgs.values().cloned().collect())
    }

    async fn create(&self, name: &str, slug: &str, owner: UserId) -> DbResult<Organization> {
        let mut g = self.inner.write().unwrap();
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
        self.inner.write().unwrap().insert(project.id, project);
    }
}

#[async_trait]
impl ProjectRepo for InMemoryProjectRepo {
    async fn find_by_id(&self, id: ProjectId) -> DbResult<Project> {
        self.inner
            .read()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or(DbError::NotFound)
    }

    async fn list_in_org(&self, org_id: OrgId) -> DbResult<Vec<Project>> {
        Ok(self
            .inner
            .read()
            .unwrap()
            .values()
            .filter(|p| p.org_id == org_id)
            .cloned()
            .collect())
    }

    async fn create(&self, org_id: OrgId, name: &str, slug: &str) -> DbResult<Project> {
        let mut g = self.inner.write().unwrap();
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
        self.inner.write().unwrap().platform.insert(user, role);
    }
    pub fn seed_project(&self, org: OrgId, project: ProjectId, user: UserId, role: ProjectRole) {
        self.inner
            .write()
            .unwrap()
            .projects
            .insert((project, user), (org, role));
    }
}

#[async_trait]
impl MembershipRepo for InMemoryMembershipRepo {
    async fn load_for_user(&self, user_id: UserId) -> DbResult<UserMemberships> {
        let g = self.inner.read().unwrap();
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
        self.inner.write().unwrap().orgs.insert((org, user), role);
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
            .unwrap()
            .projects
            .iter()
            .find(|((p, _), _)| *p == project)
            .map(|(_, (o, _))| *o)
            .unwrap_or_else(|| OrgId::from(Uuid::nil()));
        self.inner
            .write()
            .unwrap()
            .projects
            .insert((project, user), (org, role));
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
        let mut g = self.inner.write().unwrap();
        g.by_id.insert(record.api_key_id, key_hash.to_string());
        g.by_hash.insert(key_hash.to_string(), record);
    }
}

#[async_trait]
impl ApiKeyRepo for InMemoryApiKeyRepo {
    async fn find_by_hash(&self, hash: &str) -> DbResult<ApiKeyRecord> {
        self.inner
            .read()
            .unwrap()
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
        let mut g = self.inner.write().unwrap();
        g.by_id.insert(id, key_hash.to_string());
        g.by_hash.insert(key_hash.to_string(), record);
        Ok(id)
    }

    async fn list_in_project(&self, project_id: ProjectId) -> DbResult<Vec<ApiKeyRecord>> {
        Ok(self
            .inner
            .read()
            .unwrap()
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
            .unwrap()
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
        let mut g = self.inner.write().unwrap();
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
