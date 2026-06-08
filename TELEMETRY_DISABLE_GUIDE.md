# 禁用遥测配置指南

本文档说明如何完全禁用 Codex 的遥测功能。

---

## 方法一：修改源码默认禁用 (已应用)

**状态**: ✅ 已完成

**修改文件**: `codex-rs/otel/src/config.rs`

**改动内容**:
```rust
// 修改前：仅在 debug 构建中禁用
if cfg!(debug_assertions) {
    return OtelExporter::None;
}

// 修改后：所有构建都禁用
OtelExporter::None
```

**生效范围**: 所有用户，无需额外配置

---

## 方法二：配置文件禁用 (推荐用户侧配置)

### 步骤

1. 创建或编辑配置文件 `~/.codex/config.toml`

2. 添加以下配置:

```toml
# 完全禁用遥测
[otel]
# 禁用所有导出器 (metrics, traces, logs)
exporter = "none"
trace-exporter = "none"
metrics-exporter = "none"

# 可选：明确禁用用户提示日志
log-user-prompt = false

# 可选：设置环境标识为本地
environment = "local"
```

### 完整配置示例

```toml
# ~/.codex/config.toml

# 基础模型配置
model = "gpt-5.1"

# 禁用遥测
[otel]
exporter = "none"
trace-exporter = "none"
metrics-exporter = "none"
log-user-prompt = false
environment = "local"

# 沙箱配置
sandbox-mode = "workspace-write"

[sandbox-workspace-write]
writable-roots = ["."]
network-access = false  # 可选：禁用网络访问增强隐私

# MCP 服务器配置
[mcp-servers]
# 按需配置...
```

---

## 方法三：环境变量禁用 (临时)

在启动 Codex 前设置环境变量:

```bash
# Linux/macOS
export CODEX_OTEL_EXPORTER=none
export CODEX_OTEL_TRACE_EXPORTER=none
export CODEX_OTEL_METRICS_EXPORTER=none
codex

# Windows PowerShell
$env:CODEX_OTEL_EXPORTER="none"
$env:CODEX_OTEL_TRACE_EXPORTER="none"
$env:CODEX_OTEL_METRICS_EXPORTER="none"
codex
```

---

## 验证遥测已禁用

### 方法 1: 检查网络流量

```bash
# 监控是否有发往 chatgpt.com 的流量
sudo tcpdump -i any host ab.chatgpt.com

# 或使用 Wireshark 过滤
# 过滤器: ip.dst == <ab.chatgpt.com 的 IP>
```

### 方法 2: 检查日志输出

运行 Codex 时观察日志，不应出现:
- `Statsig` 相关日志
- `OTLP` 导出相关日志
- `telemetry` 相关日志

### 方法 3: 防火墙规则阻断

设置防火墙规则测试，如果遥测已禁用，阻断规则不会触发:

```bash
# Linux iptables 示例
sudo iptables -A OUTPUT -d ab.chatgpt.com -j DROP

# 然后运行 Codex，观察是否有连接被阻断
sudo iptables -L OUTPUT -v | grep ab.chatgpt.com
```

---

## 高级：完全移除遥测依赖

如需彻底移除遥测代码，执行以下步骤:

### 1. 移除 Cargo.toml 中的依赖

```toml
# codex-rs/otel/Cargo.toml
# 注释或删除以下依赖:
# opentelemetry = "..."
# opentelemetry-otlp = "..."
# opentelemetry_sdk = "..."
```

### 2. 移除代码中的遥测初始化

```rust
// codex-rs/otel/src/lib.rs
// 注释掉整个初始化函数或返回 Ok(()) 直接跳过
```

### 3. 重新编译

```bash
./build-fast.sh
```

---

## 隐私增强建议

### 1. 禁用网络访问

```toml
[sandbox-workspace-write]
network-access = false
```

### 2. 排除敏感环境变量

```toml
[shell-environment-policy]
exclude = ["*SECRET*", "*PASSWORD*", "*CREDENTIAL*", "*API_KEY*", "*TOKEN*"]
```

### 3. 禁用历史记录

```toml
[history]
persistence = "none"
```

### 4. 使用离线模式

```bash
# 如果支持离线模式
codex --offline
```

---

## 故障排除

### 问题：配置不生效

**解决**:
1. 确认配置文件位置：`~/.codex/config.toml`
2. 验证 TOML 语法正确
3. 检查是否有多个配置文件冲突
4. 重启 Codex 应用

### 问题：仍有网络请求

**解决**:
1. 检查是否有其他遥测插件
2. 确认第三方 MCP 服务器没有独立遥测
3. 使用网络监控工具定位请求来源

### 问题：编译失败

**解决**:
```bash
# 清理构建缓存
cd codex-rs
cargo clean

# 重新构建
./build-fast.sh
```

---

## 配置参考

### 完整 otel 配置选项

```toml
[otel]
# 导出器类型: none | statsig | otlp-http | otlp-grpc
exporter = "none"

# Trace 导出器
trace-exporter = "none"

# Metrics 导出器
metrics-exporter = "none"

# 是否记录用户提示词 (默认 false)
log-user-prompt = false

# 环境标识 (dev | staging | prod | test | local)
environment = "local"

# 自定义 Span 属性
[otel.span-attributes]
# team = "backend"
# project = "my-app"

# Tracestate 配置
[otel.tracestate]
# vendor = { key1 = "value1", key2 = "value2" }
```

---

## 安全基线检查清单

- [x] 源码默认禁用遥测
- [ ] 用户配置文件设置 `exporter = "none"`
- [ ] 禁用网络访问 (可选)
- [ ] 排除敏感环境变量
- [ ] 验证无外传流量
- [ ] 禁用历史记录 (可选)
- [ ] 审查第三方依赖

---

## 更新日志

- **2026-06-08**: 修改源码默认禁用 Statsig 遥测
- **2026-06-08**: 创建配置禁用指南文档

---

## 相关文档

- [SECURITY.md](./SECURITY.md) - 安全政策
- [config.toml.example](./config.toml.example) - 配置示例
- [Agent approvals & security](https://developers.openai.com/codex/agent-approvals-security) - 官方安全文档
