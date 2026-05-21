# Playground · 工作流编辑器

> Kooix Gate 控制台内嵌的可视化 workflow editor，基于 `@xyflow/svelte`。
>
> 定位：**让用户在不写代码的前提下，把 Kooix Gate 的多模态能力（chat/embeddings/image/audio）串成可演示的链路**——演示 → 调试 → 链路压测 → 上下游字段映射验证。

## 与 LLM 网关定位的关系

| 网关核心 | Playground 定位 |
|---------|-----------------|
| 多租户 + 流式计费 + 渠道路由 | 复用同一套租户隔离、配额、计费链路 |
| OpenAI 兼容 API | 节点底层调用 `/v1/chat/completions` `/v1/embeddings` `/v1/images/generations` `/v1/audio/{speech,transcriptions}` |
| 渠道插件化 | 节点根据 `ProviderCapability` 自动禁用不支持的 channel/model |

Playground 不是另起炉灶。它**走和生产请求完全一致的链路**：route → routing strategy → channel key → upstream → billing outbox → request_events。所以工作流执行也会落 audit、计费、quota。

## 节点类型

| 节点 | 来源 capability | 文件 |
|------|----------------|------|
| TextInput | n/a | `web/src/lib/components/flow/nodes/TextInputNode.svelte` |
| ImageUpload | n/a | `ImageUploadNode.svelte` |
| AudioUpload | n/a | `AudioUploadNode.svelte` |
| LLMChat | `chat` (+ optional `tools` / `streaming` / `vision` / `json_mode`) | `LLMChatNode.svelte` |
| ImageGen | `image` | `ImageGenNode.svelte` |
| STT | `audio` (transcribe) | `STTNode.svelte` |
| TTS | `audio` (speech) | `TTSNode.svelte` |
| Preview | n/a | `PreviewNode.svelte` |

入口路由：`web/src/routes/playground/+page.svelte`（lazy-loaded shell）+ `web/src/lib/components/playground/FlowEditor.svelte`（核心容器）。

## 与 ProviderCapability 的联动

每个产生上游请求的节点（LLMChat / ImageGen / STT / TTS）在打开 channel/model 选择时，会按下表过滤：

| 节点 | 必需 capability | 可选 capability |
|------|----------------|----------------|
| LLMChat | `chat` | `tools`, `streaming`, `vision`, `json_mode` |
| ImageGen | `image` | `image_quality`, `image_size` |
| STT | `audio` | — |
| TTS | `audio` | — |

不满足 capability 的 channel 会显示"该模型不支持此节点"提示，禁止连线。capability 来源：`/v1/models` aggregated capability union。

## Bundle 策略

Playground 启用 lazy load：

- `playground/+page.svelte` 是 38 行轻量 shell，只负责动态 import `FlowEditor`。
- `@xyflow/svelte` (~340 KB gzipped) 仅在用户进入 `/playground` 时下载。
- `web/scripts/check-bundle-budget.mjs` 验证 playground chunk 不进主 bundle。

## 已知限制（v0.2.1）

- 工作流不支持服务端持久化：刷新页面会丢节点状态。计划 v0.3.x 引入 `playground_workflows` 表存储用户工作流。
- 工作流执行不支持并发节点：当前按 DAG 拓扑顺序串行执行。计划 v0.4.x 用 `tokio::join!` 改并发。
- STT/TTS 节点暂不支持流式分片预览，只显示 final 结果。

## 路线（M1.5）

参考 [ROADMAP.md M1.5](../ROADMAP.md#m15-playground-收编为产品线)：

- [ ] Playground 节点共享 `ProviderCapability` 矩阵（前端从 `/v1/models` 拉 capability union）
- [ ] 7 种节点 vitest 覆盖（input/output schema 与 capability gating）
- [ ] 工作流执行链路接入 `request_events`，落 audit
- [ ] 工作流持久化（`playground_workflows` 表）
