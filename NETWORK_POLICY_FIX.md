# DeepSeek-TUI 网络策略修复指南

## 问题描述

在 DeepSeek-TUI 中使用 `web_search` 工具时，出现以下错误：

```
网络完全被隔离了——无法解析任何外部 DNS，没有配置代理。
```

## 根本原因

**不是网络隔离，而是网络策略配置问题！**

DeepSeek-TUI 有一个安全的网络策略系统（#135），默认行为是 `Prompt`（提示用户确认）：

```rust
// crates/tui/src/network_policy.rs
fn default_decision() -> DecisionToml {
    DecisionToml::Prompt  // 默认是"提示确认"，不是"允许"
}
```

这意味着：
1. 每次网络请求都会弹出确认对话框
2. 在 TUI 界面中，如果用户没有及时响应或忽略提示
3. 网络请求就会被阻止
4. 看起来像"网络被隔离"

## 解决方案

### 方法 1：创建配置文件（推荐）

创建或编辑 `~/.udas/config.toml`：

```toml
# 网络策略配置 - 允许所有网络请求
[network]
default = "allow"     # allow | deny | prompt
allow = []            # 允许的域名列表（为空表示允许所有）
deny = []             # 拒绝的域名列表
audit = true          # 记录网络请求日志

# Web Search 配置 - 使用 Tavily
[web_search]
engine = "tavily"     # duckduckgo | bing | tavily

# Tavily 搜索配置
[tavily]
api_key = "tvly-dev-YOUR_API_KEY"
search_depth = "basic"
include_answer = true
max_results = 5
```

### 方法 2：环境变量配置

```powershell
# 设置网络策略环境变量
$env:DEEPSEEK_NETWORK_DEFAULT = "allow"

# 设置 Tavily API Key
$env:TAVILY_API_KEY = "tvly-dev-YOUR_API_KEY"

# 启动 DeepSeek-TUI
.\deepseek.exe
```

### 方法 3：交互式配置

在 DeepSeek-TUI 中运行：

```
/network allow
```

这会将所有域名添加到允许列表。

## 配置文件位置

| 操作系统 | 配置文件路径 |
|---------|-------------|
| Windows | `%USERPROFILE%\.deepseek\config.toml` |
| macOS/Linux | `~/.udas/config.toml` |

## 验证配置

1. 启动 DeepSeek-TUI
2. 运行 `web_search` 工具
3. 如果不再弹出确认对话框，说明配置成功

## 技术细节

### 网络策略优先级

```
deny 列表 > allow 列表 > default 策略
```

- 如果域名在 `deny` 列表中 → 拒绝（即使在 `allow` 中）
- 如果域名在 `allow` 列表中 → 允许
- 否则 → 使用 `default` 策略

### 主机匹配规则

- **精确匹配**：`api.deepseek.com` 只匹配 `api.deepseek.com`
- **子域名匹配**：`.example.com` 匹配 `api.example.com`、`a.b.example.com`
- **通配符**：`*.example.com` 也支持

## 相关文件

- `crates/tui/src/network_policy.rs` - 网络策略实现
- `crates/config/src/lib.rs` - 配置结构定义
- `config.example.toml` - 配置文件示例

## 注意事项

1. **安全考虑**：设置 `default = "allow"` 会允许所有网络请求，可能带来安全风险
2. **审计日志**：建议保持 `audit = true`，记录所有网络请求
3. **Tavily 优势**：Tavily API 在网络隔离环境下可访问，是可靠的搜索方案
