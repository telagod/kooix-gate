# Build Hygiene Runbook

> **2026-05-23 真实事故复盘 + 防复发系统性指南**
> 日常操作清单见 [`CONTRIBUTING.md` § 6 Disk usage management](../CONTRIBUTING.md#6-disk-usage-management)。
> 本文档专注**为什么会发生 + 如何预测下一次**。

---

## 1. 事故现场（2026-05-23 17:15 ~ 17:42）

### 1.1 时间线

| 时刻 | 事件 |
|------|------|
| 长期 | `target/` 静默膨胀至 **240G**（debug/incremental 99G + debug/deps 137G） |
| 17:15 | OOM killer 首次触发，`steamwebhelper`、`msedge` 被杀，`npm install` 仍在运行 |
| 17:19 | OOM 第二轮，`steamwebhelper` 再死 |
| 17:23-24 | OOM 第三、四轮，多个 `msedge` 被杀，`total-vm` 达 **1.5TB** 虚拟地址 |
| 17:26 | `kworker/7:0` 与 `WeChatAppEx` 进入 D 状态超 122 秒（hung task） |
| 17:40 | `systemd-journald` watchdog 3min 超时，dump core |
| 17:42 | 系统失去响应能力 |

### 1.2 根因排序

| 根因 | 占比 | 性质 |
|------|------|------|
| Swap 优先级 -2（默认 fstab `sw` 不指定 priority） | 高 | 配置问题，非本仓库责任 |
| `vm.swappiness=10`（旧配置过度保守） | 中 | 配置问题 |
| `target/` 240G 触发文件系统 IO 压力 | 中 | **本仓库责任** |
| `npm install` + 多个 Edge 同时申请大块内存 | 高 | 触发因素 |

**本 runbook 关注**：第 3 项——为什么 Rust workspace 的 `target/` 会静默膨胀到这个规模，以及如何在它再次接近临界值前发现。

---

## 2. 240G 是怎么形成的（数学）

```
target/                       240G
├── debug/                    237G   ← 99% 集中在这里
│   ├── incremental/           99G   ← 增量编译缓存
│   ├── deps/                 137G   ← 依赖编译产物
│   ├── build/                1.4G
│   └── .fingerprint/         180M
├── release/                  2.7G   ← 健康
└── criterion/                7.0M   ← bench 报告
```

### 2.1 incremental 99G 的机制

`[profile.dev] incremental = true` 让 rustc 为每个 crate 维护一份"增量编译数据库"。在 9-crate workspace 下，每次改动会：

1. 生成新的 `.bin`/`.o` 中间文件，标记旧文件为 stale
2. **不删旧文件**——因为可能下次回滚改动还能用上
3. 累积到一定规模触发 garbage collection，但 GC 阈值很宽松

**临界点公式（经验）**：
```
incremental_size ≈ crate数 × 平均每 crate IR 大小 × 月活改动量
                ≈ 9 × 800MB × 多月 = 数十 GB
```

99G 是这个公式的"高活跃 + 长期不清"上限。

### 2.2 deps 137G 的机制

`debug/deps/` 存放每个被编译的 (crate, feature_set, profile) 组合的 `.rlib`/`.rmeta`/`.o`。

放大因子：
- 9 个 workspace member × 各自的 feature 组合
- `cargo build` / `cargo test` / `cargo clippy` / `cargo check` 各自留产物
- 工具链升级（rustc 1.83 → 1.85）后，旧版本编译产物不会自动清
- doctest binary（每个 doctest 一个独立 binary）

每次工具链/feature/profile 切换都是一次"产物 fork"，且 cargo 不主动 reap。

---

## 3. 三层防御

### Layer 1 · 实时门槛（每次开发会话）

```bash
# 每次进项目目录，看一眼 target 大小
du -sh target/ 2>/dev/null
```

**红线**：
- 单次会话开始时 > 30G ⇒ 跑一次 `bash scripts/cargo-sweep-helper.sh --apply`
- > 80G ⇒ 强制 `cargo clean`，本次会话不要先压力测试

> 配 alias 自动化：
> ```fish
> # ~/.config/fish/config.fish
> function cd
>   builtin cd $argv
>   if test -d target
>     set -l size (du -sb target 2>/dev/null | awk '{print $1}')
>     if test "$size" -gt 32212254720  # 30G
>       echo "⚠ target/ size: "(du -sh target | cut -f1)" — consider sweep"
>     end
>   end
> end
> ```

### Layer 2 · 定期巡检（每周）

```bash
# 全机 Rust target 总览
find ~ -type d -name target -not -path '*/node_modules/*' -prune 2>/dev/null \
  | xargs -I{} du -sh {} | sort -rh | head -10
```

**全机预算**：所有 `target/` 合计 < **80G**，超过就巡检最大头。

### Layer 3 · 结构性配置（一劳永逸）

#### 3.1 `CARGO_TARGET_DIR` 共享 target 目录

跨 worktree / 跨 clone 共享一个 target，依赖只编一次：

```fish
set -gx CARGO_TARGET_DIR "$HOME/.cargo-target"
```

**对 kooix-gate 特别有效**——`.codex/worktrees/` 下的 worktree 不再各自开 target/。

⚠ 注意：与 sqlx-macro offline 缓存可能冲突，配合 `SQLX_OFFLINE=true` 使用。

#### 3.2 `sccache` 跨项目缓存

```bash
cargo install sccache
# ~/.cargo/config.toml
[build]
rustc-wrapper = "sccache"
```

sccache 按内容哈希缓存，比 incremental 更稳定，跨项目命中率高。**与 `incremental = true` 互斥**——选一个：
- 单仓库高频改动 ⇒ `incremental`
- 多仓库切换 ⇒ `sccache`

#### 3.3 `cargo-cache` 自动 GC

```bash
cargo install cargo-cache
cargo cache --autoclean   # 智能清理 ~/.cargo/registry
cargo cache -a            # 查看占用
```

注意：cargo-cache 管 `~/.cargo`，不管项目内 `target/`。两者职责不同。

---

## 4. 为什么 cargo-sweep 比 cargo clean 好

CONTRIBUTING.md 推荐的 `cargo-sweep`（包装在 `scripts/cargo-sweep-helper.sh`）的核心优势：

| 维度 | `cargo clean` | `cargo-sweep` |
|------|---------------|---------------|
| 粒度 | 全删，下次冷编译 | 只删 N 天前未访问的 fingerprint，保留热产物 |
| 重编时间 | 5-15 分钟 | 30 秒 ~ 2 分钟（仅过期部分） |
| 工具链切换 | 旧产物全留 | 自动检测并清旧 toolchain 产物 |

**经验阈值**（`KOOIX_SWEEP_DAYS`）：
- 日活开发：7 天
- 周活开发：30 天（默认）
- 长期不动的 crate：直接 `cargo clean -p <crate>`

---

## 5. CI 监控（可选加固）

`.github/workflows/disk-watch.yml`：

```yaml
name: target-size-watch
on: [pull_request]
jobs:
  size:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - run: |
          target_size=$(du -sb target 2>/dev/null | cut -f1 || echo 0)
          max_size=$((20 * 1024 * 1024 * 1024))
          if [[ $target_size -gt $max_size ]]; then
            echo "::warning::target/ exceeds 20G in CI cache"
          fi
```

CI 缓存层面的预警，避免 self-hosted runner 重蹈覆辙。

---

## 6. FAQ（事故相关）

**Q: 240G 是 incremental 的常态吗？**
A: 不是。常态在 5-30G。99G incremental 表明长期未跑 cargo-sweep + 多次 toolchain 切换叠加，是慢性病。

**Q: 我刚 cargo clean，下次编译多久？**
A: kooix-gate 全量 debug ≈ 5-15 分钟（依 CPU），dep cache 保留时 ≈ 2-5 分钟。

**Q: 系统 OOM 跟 target/ 大小有直接关系吗？**
A: **没有直接关系**。target/ 占的是磁盘，不是内存。但本次事故中：
- 大文件系统的 page cache 与 dirty page 占用增加内核内存压力
- `cargo build` 链接阶段 RAM 占用本身就高（rustc 单进程可吃 4-8G）
- 与 npm install + Edge 撞车后，触发 OOM
- 240G target 不是凶手，但是**共谋**

**Q: 跨 crate test 失败 + cargo-sweep 后还失败？**
A: sqlx-macro offline 缓存可能与 sweep 后的状态不同步：
```bash
cargo clean -p gate-storage   # CONTRIBUTING § 6 已记录
cargo sqlx prepare --workspace --check
cargo test -p gate-server
```

---

## 7. 历史事故登记

| 日期 | 现象 | 触发链 | 处置 |
|------|------|--------|------|
| 2026-05-23 | target/ 240G + 系统 OOM 卡死 | 长期未 sweep + npm install + Edge 多进程 | 全量 cargo clean + sysctl 调优 + earlyoom 部署 |

> 维护规则：每次 target/ > 80G、或参与触发系统级问题，登记到本表。

---

## 参考

- [Cargo Book · Build Cache](https://doc.rust-lang.org/cargo/guide/build-cache.html)
- [Rust profile reference](https://doc.rust-lang.org/cargo/reference/profiles.html)
- [cargo-sweep](https://github.com/holmgr/cargo-sweep)
- [sccache](https://github.com/mozilla/sccache)
- [cargo-cache](https://github.com/matthiaskrgr/cargo-cache)
- 配套：[`CONTRIBUTING.md § 6`](../CONTRIBUTING.md#6-disk-usage-management) 日常操作清单
