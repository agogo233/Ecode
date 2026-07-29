# 用户指令记忆

本文件记录了用户的指令、偏好和教导，用于在未来的交互中提供参考。

## 条目

### 本地 fork 补丁说明与合并策略
- Date: 2026-07-29
- Context: 将上游 just-every/code v0.6.155 合并到 agogo233/Ecode，并应用安全加固与性能修复
- Category: 工作流协作
- Instructions:
  - 本地 fork: `agogo233/Ecode` (origin)，上游: `just-every/code` (upstream)
  - 当前 master = 上游 v0.6.155 + 以下本地补丁：

  #### 补丁清单（6 个源文件）

  | 文件 | 改动 | 原因 |
  |------|------|------|
  | `code-rs/core/src/config_types.rs` | 新增 `OtelConfigToml.analytics_enabled: Option<bool>` 和 `OtelConfig.analytics_enabled: bool` | 遥测门控数据模型 |
  | `code-rs/core/src/config.rs` | TOML 解析 `analytics_enabled.unwrap_or(true)` | 未配置时默认开启 |
  | `code-rs/core/src/otel_init.rs` | `build_provider()` 开头 `if !analytics_enabled { return Ok(None) }` | 阻断所有 OTEL 导出 |
  | `code-rs/core/src/rollout/recorder.rs` | 移除 `write_line()` 内逐条 flush；`AddItems` 处理完一批后统一 flush；Shutdown 时最终 flush | 减少磁盘 fsync 风暴 |
  | `code-rs/tui/src/session_log.rs` | 移除 `write_json_line()` 内逐事件 flush；新增 `flush()` 方法在 `log_session_end()` 调用 | session 日志仅在结束时 flush |
  | `code-rs/core/src/debug_logger.rs` | `append_usage_entry` 从全文件读取→解析→追加→重写(O(n²))改为 JSONL 追加 + 复用 `BufWriter<File>`；`log_turn_latency` 从每次 reopen 改为复用 `BufWriter<File>` | 消除 O(n²) 磁盘 I/O 和重复 open |

  #### CI 配置

  - `.github/workflows/` 仅保留 `cloud-build.yml`（643 行，从旧提交 61b3221 恢复）
  - 上游所有其他 workflow yml 已删除（release、issue、preview-build 等 11 个）

  #### 下次合并策略

  目标是保留本地补丁的同时跟进上游新版本。

  **策略 A（推荐）：三点 diff 自动合并**

  ```
  BASE=<上次合并的上游 commit，当前为 fdffcd88>
  LOCAL=<我们的 master，当前为 61272a02>
  UPSTREAM=<新的上游 release tag/commit>

  # 仅 diff 我们的 6 个补丁文件，对每个文件应用三方合并
  git diff BASE LOCAL -- code-rs/core/src/config_types.rs code-rs/core/src/config.rs code-rs/core/src/otel_init.rs code-rs/core/src/rollout/recorder.rs code-rs/tui/src/session_log.rs code-rs/core/src/debug_logger.rs > /tmp/local.patch

  # 重置到新上游
  git checkout UPSTREAM -b merge-new-upstream

  # 尝试打补丁
  git apply --3way /tmp/local.patch

  # 如有冲突手动解决，然后提交
  ```

  **策略 B（备选）：手动 re-apply**

  如果上游对同一文件改了太多导致 patch 不适用：
  1. 用 `git diff BASE..LOCAL -- <file>` 查看我们的改动
  2. 在新上游的对应文件上手动重做等价修改
  3. 每次一个文件，逐个验证编译

  **每次合并后必须执行：**
  ```bash
  # 构建验证
  WORKSPACE=code ./build-fast.sh

  # 恢复 cloud-build.yml
  git checkout LOCAL -- .github/workflows/cloud-build.yml

  # 删除上游 CI
  ls .github/workflows/*.yml | grep -v cloud-build.yml | xargs git rm
  ```

  **关键 commit hash：**
  - 上游基准: `fdffcd88` (v0.6.155)
  - 我们的版本: `61272a02` (master)
  - 旧 fork 原始 commit: `61b32215` (已归档，可通过 reflog 访问)

  **注意事项：**
  - 两个仓库是单提交快照，无共享 git 历史，标准 merge/rebase 不可行
  - 推送需要 `--force` 因为历史被替换
  - 上游 `code-rs/` 已移除 Sentry/Statsig/feedback，只需维护 `analytics_enabled` 和磁盘 I/O 补丁
  - `config.toml` 中 `[otel] analytics_enabled = false` 即可完全关闭遥测

  #### 环境配置

  - 构建命令: `WORKSPACE=code ./build-fast.sh`
  - Rust 工具链: 1.90.0（由 `code-rs/rust-toolchain.toml` 指定）
  - 需要预装: `libssl-dev`, `pkg-config`（`apt-get install -y libssl-dev pkg-config`）
