//! `kgctl smoke` — 对已运行的 gate-server 做最小端到端冒烟。
//!
//! 覆盖 ROADMAP P2.4 要求：登录、创建 channel、创建 API key、发 chat、查 usage。
//! 该命令只走 HTTP API，不直接写数据库；适合发布后或部署流水线验证真实运行路径。

use anyhow::{Context, Result, bail};
use reqwest::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub struct SmokeOpts {
    pub base_url: String,
    pub email: String,
    pub password: String,
    pub upstream_base_url: Option<String>,
    pub upstream_api_key: String,
    pub model: String,
}

pub async fn run(opts: SmokeOpts) -> Result<()> {
    let smoke = Smoke::new(opts)?;
    smoke.run().await
}

struct Smoke {
    client: reqwest::Client,
    base_url: String,
    email: String,
    password: String,
    upstream_base_url: Option<String>,
    upstream_api_key: String,
    model: String,
    suffix: String,
}

#[derive(Debug, Deserialize)]
struct LoginResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct MeResponse {
    orgs: Vec<String>,
    is_platform_admin: bool,
}

#[derive(Debug, Deserialize)]
struct IdResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct CreateApiKeyResponse {
    plaintext: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

impl Smoke {
    fn new(opts: SmokeOpts) -> Result<Self> {
        let base_url = opts.base_url.trim().trim_end_matches('/').to_string();
        if base_url.is_empty() {
            bail!("--base-url / KOOIX_PUBLIC_URL 不能为空");
        }
        url::Url::parse(&base_url).with_context(|| format!("invalid base url: {base_url}"))?;
        if opts.email.trim().is_empty() {
            bail!("--email / KOOIX_SMOKE_EMAIL 不能为空");
        }
        if opts.password.is_empty() {
            bail!("--password / KOOIX_SMOKE_PASSWORD 不能为空");
        }
        let upstream_base_url = opts
            .upstream_base_url
            .map(|u| u.trim().trim_end_matches('/').to_string())
            .filter(|u| !u.is_empty());
        if let Some(ref u) = upstream_base_url {
            url::Url::parse(u).with_context(|| format!("invalid upstream base url: {u}"))?;
        }
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .build()?;
        Ok(Self {
            client,
            base_url,
            email: opts.email,
            password: opts.password,
            upstream_base_url,
            upstream_api_key: opts.upstream_api_key,
            model: opts.model,
            suffix: Uuid::now_v7().simple().to_string()[..12].to_string(),
        })
    }

    async fn run(&self) -> Result<()> {
        println!("kgctl smoke: {}", self.base_url);
        let token = self.login().await?;
        let me = self.get_me(&token).await?;
        println!("✓ login: {}", self.email);

        let org_id = self.ensure_org(&token, &me).await?;
        println!("✓ org: {org_id}");

        let project_id = self.create_project(&token, &org_id).await?;
        println!("✓ create project: {project_id}");

        if let Some(channel_id) = self.create_route_if_configured(&token, &project_id).await? {
            println!("✓ create channel/group binding: {channel_id}");
        } else {
            println!("· skip channel create: using existing route or fallback provider");
        }

        let api_key = self.create_api_key(&token, &org_id, &project_id).await?;
        println!(
            "✓ create api key: {}…",
            api_key.chars().take(12).collect::<String>()
        );

        self.chat(&api_key).await?;
        println!("✓ chat completions: {}", self.model);

        self.usage(&token, &org_id).await?;
        println!("✓ usage query");
        println!("smoke ok");
        Ok(())
    }

    async fn login(&self) -> Result<String> {
        let body = self
            .post_json(
                "/v1/auth/login",
                None,
                &json!({
                    "email": self.email,
                    "password": self.password,
                }),
            )
            .await?;
        Ok(serde_json::from_value::<LoginResponse>(body)?.access_token)
    }

    async fn get_me(&self, token: &str) -> Result<MeResponse> {
        let body = self.get_json("/v1/me", Some(token), None).await?;
        Ok(serde_json::from_value(body)?)
    }

    async fn ensure_org(&self, token: &str, me: &MeResponse) -> Result<String> {
        if let Some(id) = me.orgs.first() {
            return Ok(id.clone());
        }
        if !me.is_platform_admin {
            bail!("登录用户无 Org，且不是 platform admin，无法创建 smoke org");
        }
        let body = self
            .post_json(
                "/v1/admin/orgs",
                Some(token),
                &json!({
                    "name": format!("Smoke {}", self.suffix),
                    "slug": format!("smoke-{}", self.suffix),
                }),
            )
            .await?;
        Ok(serde_json::from_value::<IdResponse>(body)?.id)
    }

    async fn create_project(&self, token: &str, org_id: &str) -> Result<String> {
        let path = format!("/v1/orgs/{}/projects", raw_id(org_id));
        let body = self
            .post_json_with_org(
                &path,
                token,
                org_id,
                &json!({
                    "name": format!("Smoke {}", self.suffix),
                    "slug": format!("smoke-{}", self.suffix),
                }),
            )
            .await?;
        Ok(serde_json::from_value::<IdResponse>(body)?.id)
    }

    async fn create_route_if_configured(
        &self,
        token: &str,
        project_id: &str,
    ) -> Result<Option<String>> {
        let Some(base_url) = self.upstream_base_url.as_deref() else {
            return Ok(None);
        };
        let channel = self
            .post_json(
                "/v1/admin/channels",
                Some(token),
                &json!({
                    "code": format!("smoke-{}", self.suffix),
                    "name": format!("Smoke {}", self.suffix),
                    "provider_type": "openai",
                    "base_url": base_url,
                    "enabled": true,
                    "supported_models": [self.model.clone()],
                    "tags": ["kgctl-smoke"],
                    "timeout_ms": 10000,
                    "max_retries": 0,
                }),
            )
            .await?;
        let channel_id = serde_json::from_value::<IdResponse>(channel)?.id;

        self.post_json(
            &format!("/v1/admin/channels/{}/keys", raw_id(&channel_id)),
            Some(token),
            &json!({
                "secret": self.upstream_api_key,
                "alias": "kgctl-smoke",
            }),
        )
        .await?;

        let group = self
            .post_json(
                "/v1/admin/groups",
                Some(token),
                &json!({
                    "name": format!("Smoke {}", self.suffix),
                    "strategy": "priority",
                }),
            )
            .await?;
        let group_id = serde_json::from_value::<IdResponse>(group)?.id;

        self.post_json(
            &format!("/v1/admin/groups/{}/bindings", raw_id(&group_id)),
            Some(token),
            &json!({
                "channel_id": raw_id(&channel_id),
                "priority": 1,
                "weight": 1,
            }),
        )
        .await?;

        self.put_json(
            &format!("/v1/admin/projects/{}/default-group", raw_id(project_id)),
            token,
            &json!({ "group_id": raw_id(&group_id) }),
        )
        .await?;

        Ok(Some(channel_id))
    }

    async fn create_api_key(&self, token: &str, org_id: &str, project_id: &str) -> Result<String> {
        let path = format!(
            "/v1/orgs/{}/projects/{}/api-keys",
            raw_id(org_id),
            raw_id(project_id)
        );
        let body = self
            .post_json_with_org(
                &path,
                token,
                org_id,
                &json!({
                    "name": format!("kgctl-smoke-{}", self.suffix),
                    "allowed_models": [self.model.clone()],
                }),
            )
            .await?;
        Ok(serde_json::from_value::<CreateApiKeyResponse>(body)?.plaintext)
    }

    async fn chat(&self, api_key: &str) -> Result<()> {
        let body = self
            .post_json(
                "/v1/chat/completions",
                Some(api_key),
                &json!({
                    "model": self.model,
                    "messages": [{"role": "user", "content": "kgctl smoke"}],
                }),
            )
            .await?;
        let chat: ChatResponse = serde_json::from_value(body)?;
        let content = chat
            .choices
            .first()
            .map(|c| c.message.content.trim())
            .unwrap_or_default();
        if content.is_empty() {
            bail!("chat response missing assistant content");
        }
        Ok(())
    }

    async fn usage(&self, token: &str, org_id: &str) -> Result<()> {
        let path = "/v1/usage?range=7d&group_by=day";
        let body = self.get_json(path, Some(token), Some(org_id)).await?;
        if body.get("series").and_then(Value::as_array).is_none() {
            bail!("usage response missing series array");
        }
        Ok(())
    }

    async fn get_json(
        &self,
        path: &str,
        bearer: Option<&str>,
        org_id: Option<&str>,
    ) -> Result<Value> {
        let mut req = self.client.get(self.url(path));
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        if let Some(org) = org_id {
            req = req.header("X-Kooix-Org", org);
        }
        self.send_json("GET", path, req).await
    }

    async fn post_json(&self, path: &str, bearer: Option<&str>, body: &Value) -> Result<Value> {
        let mut req = self.client.post(self.url(path)).json(body);
        if let Some(token) = bearer {
            req = req.bearer_auth(token);
        }
        self.send_json("POST", path, req).await
    }

    async fn post_json_with_org(
        &self,
        path: &str,
        bearer: &str,
        org_id: &str,
        body: &Value,
    ) -> Result<Value> {
        let req = self
            .client
            .post(self.url(path))
            .bearer_auth(bearer)
            .header("X-Kooix-Org", org_id)
            .json(body);
        self.send_json("POST", path, req).await
    }

    async fn put_json(&self, path: &str, bearer: &str, body: &Value) -> Result<Value> {
        let req = self
            .client
            .put(self.url(path))
            .bearer_auth(bearer)
            .json(body);
        self.send_json("PUT", path, req).await
    }

    async fn send_json(
        &self,
        method: &'static str,
        path: &str,
        req: reqwest::RequestBuilder,
    ) -> Result<Value> {
        let resp = req
            .send()
            .await
            .with_context(|| format!("{method} {path} failed"))?;
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        if status == StatusCode::NO_CONTENT || text.trim().is_empty() {
            return Ok(json!({}));
        }
        let value: Value = serde_json::from_str(&text)
            .with_context(|| format!("{method} {path} returned non-JSON {status}: {text}"))?;
        if !status.is_success() {
            let message = value
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or(text.as_str());
            bail!("{method} {path} returned {status}: {message}");
        }
        Ok(value)
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }
}

fn raw_id(id: &str) -> String {
    let Some((_, rest)) = id.split_once('_') else {
        return id.to_string();
    };
    if rest.len() == 32 {
        format!(
            "{}-{}-{}-{}-{}",
            &rest[0..8],
            &rest[8..12],
            &rest[12..16],
            &rest[16..20],
            &rest[20..32]
        )
    } else {
        rest.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::raw_id;

    #[test]
    fn raw_id_converts_typed_simple_uuid() {
        assert_eq!(
            raw_id("org_019e2c1ba7d17162842207e4b24f5f98"),
            "019e2c1b-a7d1-7162-8422-07e4b24f5f98"
        );
    }

    #[test]
    fn raw_id_keeps_raw_uuid() {
        assert_eq!(
            raw_id("019e2c1b-a7d1-7162-8422-07e4b24f5f98"),
            "019e2c1b-a7d1-7162-8422-07e4b24f5f98"
        );
    }
}
