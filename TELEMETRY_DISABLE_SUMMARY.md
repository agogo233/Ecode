# 禁用遥测方案 - 执行总结

## 执行状态

| 步骤 | 操作 | 状态 |
|------|------|------|
| 1 | 修改源码默认禁用遥测 | ✅ 已完成 |
| 2 | 创建配置禁用指南 | ✅ 已完成 |
| 3 | 创建快速配置脚本 | ✅ 已完成 |
| 4 | 更新推荐配置模板 | ✅ 已完成 |

---

## 改动详情

### 1. 源码修改

**文件**: `codex-rs/otel/src/config.rs`

**修改前**:
```rust
pub(crate) fn resolve_exporter(exporter: &OtelExporter) -> OtelExporter {
    match exporter {
        OtelExporter::Statsig => {
            if cfg!(debug_assertions) {
                return OtelExporter::None;
            }
            // 构建 Statsig OTLP 导出器配置...
        }
        _ => exporter.clone(),
    }
}
```

**修改后**:
```rust
pub(crate) fn resolve_exporter(exporter: &OtelExporter) -> OtelExporter {
    match exporter {
        OtelExporter::Statsig => {
            // Telemetry disabled by default for privacy.
            OtelExporter::None
        }
        _ => exporter.clone(),
    }
}
```

**影响**:
- ✅ Debug 构建：原本已禁用 → 保持禁用
- ✅ Release 构建：原本启用 → 现在禁用
- ✅ 用户无需配置即生效
- ✅ 需要遥测的用户需在配置文件中显式启用

---

### 2. 文档与工具

#### 创建的文件

1. **TELEMETRY_DISABLE_GUIDE.md** - 完整禁用指南
   - 三种禁用方法
   - 验证步骤
   - 故障排除
   - 隐私增强建议

2. **scripts/disable_telemetry.sh** - 自动配置脚本
   - 一键禁用
   - 自动备份
   - 智能合并

3. **.github/codex/home/config.toml** - 安全基线配置
   - 禁用遥测
   - 限制网络访问
   - 排除敏感环境变量

---

## 用户侧操作指南

### 方案 A：使用自动脚本 (推荐)

```bash
# 方式 1: 本地执行
bash /workspace/scripts/disable_telemetry.sh

# 方式 2: 远程执行 (脚本发布后)
curl -sSL <script-url> | bash
```

### 方案 B：手动配置

1. 创建/编辑 `~/.codex/config.toml`

2. 添加配置:
```toml
[otel]
exporter = "none"
trace-exporter = "none"
metrics-exporter = "none"
environment = "local"
```

### 方案 C：环境变量 (临时)

```bash
export CODEX_OTEL_EXPORTER=none
export CODEX_OTEL_METRICS_EXPORTER=none
codex
```

---

## 验证步骤

### 1. 确认配置生效

```bash
# 检查配置文件
grep -A5 '\[otel\]' ~/.codex/config.toml

# 应输出:
# [otel]
# exporter = "none"
# trace-exporter = "none"
# metrics-exporter = "none"
```

### 2. 监控网络流量

```bash
# 方法 1: tcpdump
sudo tcpdump -i any host ab.chatgpt.com -vv

# 方法 2: Wireshark
# 过滤器: ip.dst == <ab.chatgpt.com 的 IP>

# 方法 3: netstat/lsof
lsof -i -n | grep -i chatgpt
```

### 3. 检查日志

运行 Codex 后，日志中不应出现:
- `Statsig`
- `OTLP export`
- `telemetry/metrics/traces`

---

## 恢复遥测 (如需)

### 恢复方法 1: 修改配置文件

```toml
[otel]
exporter = "statsig"
metrics-exporter = "statsig"
```

### 恢复方法 2: 删除配置

```bash
# 删除配置文件
rm ~/.codex/config.toml

# 或使用备份
mv ~/.codex/config.toml.backup.* ~/.codex/config.toml
```

### 恢复方法 3: 回滚源码

```bash
# 如果使用 Git
cd codex-rs
git checkout codex-rs/otel/src/config.rs
```

---

## 安全增强建议

### 立即实施

```toml
# 禁用网络访问
[sandbox-workspace-write]
network-access = false

# 排除敏感环境变量
[shell-environment-policy]
exclude = ["*SECRET*", "*PASSWORD*", "*CREDENTIAL*", "*API_KEY*", "*TOKEN*"]
```

### 可选实施

```toml
# 禁用历史记录
[history]
persistence = "none"

# 使用只读沙箱模式
sandbox-mode = "read-only"
```

---

## 影响评估

### 正面影响

✅ **隐私保护**
- 无数据外传
- 无使用行为追踪
- 无代码/项目信息泄露风险

✅ **网络流量减少**
- 无后台遥测请求
- 降低带宽消耗
- 减少 DNS 查询

✅ **启动速度提升**
- 省去初始化遥测的时间
- 减少第三方依赖调用

### 潜在影响

⚠️ **问题诊断困难**
- OpenAI 无法收集崩溃报告
- 难以远程分析用户体验

⚠️ **功能限制**
- 可能影响基于遥测的智能功能
- 无法参与产品改进计划

⚠️ **统计偏差**
- 用户行为数据不完整
- 活跃度统计偏低

---

## 合规性说明

### 遵循的原则

✅ **知情同意**
- 默认禁用确保用户明确选择
- 符合 GDPR 隐私设计原则

✅ **最小化数据收集**
- 不主动收集用户数据
- 符合数据保护最佳实践

✅ **用户控制**
- 提供显式启用选项
- 配置透明可审查

---

## 技术细节

### 遥测数据流 (禁用前)

```
Codex 运行 → OpenTelemetry SDK → OTLP 导出 → ab.chatgpt.com/otlp/v1/metrics
                                      ↓
                              Statsig API Key 认证
                                      ↓
                               数据收集与分析
```

### 禁用后的行为

```
Codex 运行 → OpenTelemetry SDK → None Exporter → 立即丢弃
```

### 代码路径

```
codex-otel::resolve_exporter()
  ↓
OtelExporter::Statsig 匹配
  ↓
返回 OtelExporter::None (而非构建 OTLP 配置)
  ↓
初始化时跳过导出器创建
```

---

## 测试验证

### 单元测试

现有测试 `statsig_default_metrics_exporter_is_disabled_in_debug_builds` 需要更新:

```rust
#[test]
fn telemetry_is_disabled_in_all_builds() {
    assert!(matches!(
        resolve_exporter(&OtelExporter::Statsig),
        OtelExporter::None
    ));
}
```

### 集成测试

建议添加:
1. 网络流量监控测试
2. 配置文件优先级测试
3. 环境变量覆盖测试

---

## 后续行动

### 短期 (1 周内)

- [ ] 更新 CHANGELOG.md
- [ ] 发布版本文本说明
- [ ] 用户通知邮件/公告

### 中期 (1 个月内)

- [ ] 移除硬编码的 Statsig API Key
- [ ] 完全移除遥测依赖 (可选)
- [ ] 添加遥测开关 UI (如适用)

### 长期

- [ ] 审核其他数据收集机制
- [ ] 建立隐私审查流程
- [ ] 定期安全审计

---

## 联系与支持

### 问题反馈

- GitHub Issues: 提交禁用遥测相关问题
- 文档 PR: 改进 TELEMETRY_DISABLE_GUIDE.md

### 参考资源

- [OpenTelemetry 官方文档](https://opentelemetry.io/docs/)
- [GDPR 隐私设计指南](https://gdpr.eu/principles/)
- [Mozilla 隐私承诺](https://www.mozilla.org/en-US/privacy/)

---

**文档生成时间**: 2026-06-08  
**版本**: 1.0  
**状态**: 已实施
