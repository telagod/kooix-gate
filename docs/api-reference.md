# API Reference

Kooix Gate 提供三种 API 文档形式：

## 1. OpenAPI 3.1 Spec

- 位置：[examples/openapi/kooix-gate.openapi.json](../examples/openapi/kooix-gate.openapi.json)
- 17 个 paths / 22 个 schemas
- 0.4.x 起包含 `security.wasm.*` manifest 字段（ADR-0003 v0）

可用工具：

```bash
# Swagger UI 本地预览
docker run --rm -p 8081:8080 \
  -e SWAGGER_JSON=/spec/kooix-gate.openapi.json \
  -v $(pwd)/examples/openapi:/spec \
  swaggerapi/swagger-ui

# Redocly 静态文档
npx @redocly/cli build-docs examples/openapi/kooix-gate.openapi.json
```

## 2. Postman Collection

- 位置：[examples/collections/postman/kooix-gate.postman_collection.json](../examples/collections/postman/kooix-gate.postman_collection.json)
- 三大段：01_Auth / 02_Admin / 03_Data_Plane

导入：

```bash
# Postman 界面：Import → File → 选 kooix-gate.postman_collection.json
# 设置 Environment 变量：base_url / admin_jwt / api_key
```

## 3. Bruno Collection

- 位置：[examples/collections/bruno/Kooix_Gate/](../examples/collections/bruno/Kooix_Gate/)
- 与 Postman 同步，文件夹结构对应 Auth / Admin / Data_Plane

启动：

```bash
cd examples/collections/bruno/Kooix_Gate
bruno open .  # 或 GUI 选 environments.json
```

## 关键 API 索引

### OpenAI-compatible (Data Plane)

| Path | Method | 说明 |
|------|--------|------|
| `/v1/chat/completions` | POST | Chat completions（streaming/non-streaming） |
| `/v1/embeddings` | POST | Embedding |
| `/v1/images/generations` | POST | 图像生成 |
| `/v1/audio/speech` | POST | TTS |
| `/v1/audio/transcriptions` | POST | STT |
| `/v1/responses` | POST | Responses API thin adapter |
| `/v1/models` | GET | 模型聚合 |

### Admin / Control Plane

| Path | Method | 说明 |
|------|--------|------|
| `/v1/admin/channels` | GET/POST | Channel CRUD |
| `/v1/admin/channels/{id}/keys` | POST | Key 创建 |
| `/v1/admin/pricing-rules` | GET/POST | Pricing rule CRUD |
| `/v1/orgs/{orgId}/quotas` | GET/POST | Quota CRUD |
| `/v1/orgs/{orgId}/billing` | GET | 月账单 |
| `/v1/admin/audit-logs` | GET | Audit log |
| `/v1/admin/plugin-manifest/schema` | GET | manifest JSON Schema（含 0.4.x 新增 wasm.* 字段） |

## 错误码

统一 error shape：

```json
{
  "error": {
    "type": "authentication_error" | "rate_limit_error" | "quota_exceeded" | "model_not_found" | "no_healthy_channel" | ...,
    "message": "human-readable description",
    "code": "<vendor-original-code>",
    "retry_after_ms": 30000
  }
}
```

完整错误映射见 [docs/plugin-manifest.md](./plugin-manifest.md) § Error mapping。
