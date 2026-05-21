#!/usr/bin/env bash
# scripts/cargo-sweep-helper.sh
# 清理 cargo target/ 缓存：sweep stale fingerprint + 可选 deep clean。
#
# 使用：
#   bash scripts/cargo-sweep-helper.sh           # 列出预计释放空间，不删
#   bash scripts/cargo-sweep-helper.sh --apply   # 真删
#   bash scripts/cargo-sweep-helper.sh --deep    # 强 cargo clean -p gate-storage 后再 sweep
#
# 背景：Kooix Gate 跨 crate integration test 多，target/debug 容易膨胀到 100GB+。
# cargo-sweep 按访问时间清理 30 天前 fingerprint，单次能砍 30-60% 体积。
#
# 安装：cargo install cargo-sweep

set -euo pipefail

THRESHOLD_DAYS=${KOOIX_SWEEP_DAYS:-30}
APPLY=0
DEEP=0

for arg in "$@"; do
  case "$arg" in
    --apply) APPLY=1 ;;
    --deep) DEEP=1 ;;
    --help|-h)
      sed -n '2,15p' "$0"; exit 0 ;;
  esac
done

if ! command -v cargo-sweep >/dev/null 2>&1; then
  echo "❄ cargo-sweep 未安装。请跑：cargo install cargo-sweep" >&2
  exit 1
fi

ROOT=$(git rev-parse --show-toplevel)
cd "$ROOT"

before=$(du -sh target 2>/dev/null | awk '{print $1}')
echo "[sweep] target/ before: ${before:-(missing)}"

if [ "$DEEP" = 1 ]; then
  echo "[sweep] deep clean: cargo clean -p gate-storage（migration cache 跨 crate 失效）"
  cargo clean -p gate-storage
fi

if [ "$APPLY" = 1 ]; then
  echo "[sweep] cargo sweep --time $THRESHOLD_DAYS"
  cargo sweep --time "$THRESHOLD_DAYS"
else
  echo "[sweep] dry-run（加 --apply 真删）"
  cargo sweep --time "$THRESHOLD_DAYS" --dry-run || true
fi

after=$(du -sh target 2>/dev/null | awk '{print $1}')
echo "[sweep] target/ after:  ${after:-(missing)}"
