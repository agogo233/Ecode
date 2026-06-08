#!/bin/bash
# 禁用遥测快速配置脚本
# 用法：curl -sSL <script-url> | bash  或  bash disable_telemetry.sh

set -e

echo "=== Codex 禁用遥测配置脚本 ==="
echo ""

# 确定配置目录
CODEX_HOME="${CODEX_HOME:-$HOME/.codex}"
CONFIG_FILE="$CODEX_HOME/config.toml"

echo "配置文件路径：$CONFIG_FILE"
echo ""

# 备份现有配置
if [ -f "$CONFIG_FILE" ]; then
    BACKUP_FILE="$CONFIG_FILE.backup.$(date +%Y%m%d%H%M%S)"
    echo "备份现有配置到：$BACKUP_FILE"
    cp "$CONFIG_FILE" "$BACKUP_FILE"
fi

# 创建配置目录
mkdir -p "$CODEX_HOME"

# 检查是否已存在配置
if [ -f "$CONFIG_FILE" ]; then
    echo "检测到现有配置文件..."
    
    # 检查是否已配置遥测禁用
    if grep -q 'metrics-exporter = "none"' "$CONFIG_FILE" 2>/dev/null; then
        echo "✓ 遥测已禁用，无需修改"
    else
        echo "添加遥测禁用配置..."
        
        # 在文件末尾添加遥测配置
        cat >> "$CONFIG_FILE" << 'EOF'

# 遥测配置 - 已禁用
[otel]
exporter = "none"
trace-exporter = "none"
metrics-exporter = "none"
log-user-prompt = false
environment = "local"

# 隐私增强
[sandbox-workspace-write]
network-access = false
EOF
        
        echo "✓ 配置已更新"
    fi
else
    echo "创建新配置文件..."
    
    # 创建最小化配置
    cat > "$CONFIG_FILE" << 'EOF'
# Codex 配置文件
# 隐私优先配置 - 遥测已禁用

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
network-access = false

# 排除敏感环境变量
[shell-environment-policy]
inherit = "all"
exclude = ["*SECRET*", "*PASSWORD*", "*CREDENTIAL*", "*API_KEY*", "*TOKEN*"]

# 禁用历史记录
[history]
persistence = "none"
EOF
    
    echo "✓ 配置文件已创建"
fi

echo ""
echo "=== 配置完成 ==="
echo ""
echo "验证步骤:"
echo "1. 运行 'codex --version' 确认应用正常启动"
echo "2. 查看日志确认无遥测相关输出"
echo "3. (可选) 使用网络监控工具验证无外传流量"
echo ""
echo "如需回滚，删除 ~/.codex/config.toml 或恢复备份文件"
echo ""
