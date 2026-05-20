# Release Assets Checklist

Status: active
Scope: 每次 GitHub Release、README 更新、官网/社媒演示前的截图与短视频素材清单。
Last verified: 2026-05-20

## 目标

发布资产必须让外部用户在 3 分钟内看懂：Kooix Gate 能接入 Provider、配置路由、定价计费、审计请求，并用 Playground 验证模型能力。

## 录制前准备

1. 跑通 demo 环境：

   ```bash
   export UPSTREAM_BASE_URL="https://api.openai.com/v1"
   export UPSTREAM_API_KEY="<provider-key>"
   examples/demo/quickstart.sh
   ```

2. 使用专门的 demo Org / Project / API Key，不复用生产租户。
3. 浏览器开启干净 profile，缩放 100%，窗口建议 `1440x960`。
4. 遮挡或重建所有 secret：Project API key、Channel key、OIDC secret、JWT、refresh token。
5. 检查 UI 使用中文主文案，保留 Provider / Channel / API Key / Playground 等术语。

## 必备截图

| 资产 | 页面 | 必须露出的信息 | 禁止露出的信息 |
| --- | --- | --- | --- |
| Dashboard | `/dashboard` | 请求量、成本、最近趋势、快速入口 | 真实租户名、真实成本截图 |
| Channel wizard | `/channels` 创建抽屉 | Provider preset、Auth strategy、Probe / Save 步骤 | 上游 API key 明文 |
| Pricing rules | `/admin/pricing` | Model、dimension、rate、usage cost preview | 真实合同价或客户名 |
| Request logs | `/admin/requests` 或 `/admin/audit` | request_id、status、latency、before/after audit detail | Authorization、API key、raw secret |
| Playground | `/playground` | 节点编排、输入/输出预览、成功结果 | 私有 prompt、客户数据 |

## 短视频脚本

建议 60-90 秒，按下列节奏录制：

1. **0-10s**：Dashboard 展示当前 demo 环境健康状态。
2. **10-25s**：Channel wizard 选择 OpenAI-compatible preset，填 base URL，展示 secret slot 不进入 manifest。
3. **25-40s**：Probe 成功后保存 Channel，加入 Group / Project default route。
4. **40-55s**：Pricing rules 增加 input/output tokens 规则，展示 usage cost preview。
5. **55-75s**：用 Playground 或 curl 发一条 chat，切到 Request logs 看 request_id、latency、status。
6. **75-90s**：打开 Billing / Usage，确认成本和 token 聚合出现。

## 文件命名

```text
release-assets/
  vX.Y.Z-dashboard.png
  vX.Y.Z-channel-wizard.png
  vX.Y.Z-pricing-rules.png
  vX.Y.Z-request-logs.png
  vX.Y.Z-playground.png
  vX.Y.Z-demo.mp4
```

## 发布前复核

- [ ] 所有截图使用 demo 数据。
- [ ] 无 `sk-`、Bearer token、cookie、JWT、refresh token、OIDC secret、数据库 URL 密码。
- [ ] Release notes 已由 `scripts/render-release-notes.mjs` 生成并包含 migration notes / known limitations / post-smoke。
- [ ] `examples/demo/quickstart.sh` 的命令能复现截图主链路。
- [ ] 若素材进入仓库，先跑 gitleaks 双扫。
