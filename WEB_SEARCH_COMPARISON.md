# Web Search 能力对比分析：WorkBuddy vs DeepSeek-TUI

## 1. WorkBuddy 的 Web Search 能力

### 1.1 内置 web_search 工具（Brave Search API）

**实现方式**：系统内置工具，由平台提供
**搜索引擎**：Brave Search API
**特点**：
- 无需配置，开箱即用
- 结构化 JSON 结果
- 支持高级搜索操作符
- 有免费额度限制

**调用方式**：
```json
{
  "tool": "web_search",
  "arguments": {
    "query": "搜索关键词",
    "max_results": 5
  }
}
```

### 1.2 Tavily 搜索（Skill 集成）

**实现方式**：通过 `openclaw-tavily-search` skill 集成
**搜索引擎**：Tavily API
**特点**：
- 需要 API Key（环境变量或配置文件）
- 支持 AI 生成的答案
- 每月 1000 次免费额度
- 结构化 JSON 结果

**调用方式**：
```bash
python3 scripts/tavily_search.py --query "搜索关键词" --max-results 5 --include-answer
```

### 1.3 Multi Search Engine（17 个引擎）

**实现方式**：通过 `multi-search-engine` skill 集成
**搜索引擎**：17 个（8 国内 + 9 国际）
**特点**：
- 无需 API Key
- 支持网页爬虫
- 覆盖国内外主流搜索引擎
- 支持高级搜索操作符

**支持的引擎**：
| 类型 | 引擎 |
|------|------|
| 国内 | 百度、Bing CN、360、搜狗、微信、头条、集思录 |
| 国际 | Google、Google HK、DuckDuckGo、Yahoo、Startpage、Brave、Ecosia、Qwant、WolframAlpha |

**调用方式**：
```javascript
web_fetch({"url": "https://www.google.com/search?q=搜索关键词"})
```

## 2. DeepSeek-TUI 的 Web Search 能力

### 2.1 当前实现

**搜索引擎**：
- DuckDuckGo（默认）
- Bing（备选）
- Tavily（已集成，需要配置）

**特点**：
- 网络策略默认是 `Prompt`（需要用户确认）
- 支持多搜索引擎切换
- 结构化 JSON 结果

### 2.2 已集成的 Tavily 支持

**配置方式**：
```toml
[web_search]
engine = "tavily"

[tavily]
api_key = "tvly-dev-..."
search_depth = "basic"
include_answer = true
max_results = 5
```

## 3. 移植可行性分析

### 3.1 可以直接移植的能力

| 能力 | 可行性 | 实现难度 | 说明 |
|------|--------|----------|------|
| Tavily 搜索 | ✅ 已完成 | 低 | 已集成到 DeepSeek-TUI |
| Brave Search API | ⚠️ 需要 API Key | 中 | 需要注册 Brave Search API |
| Multi Search Engine | ⚠️ 需要爬虫支持 | 高 | 需要实现网页爬虫和解析 |

### 3.2 详细分析

#### A. Tavily 搜索（已移植）

**状态**：✅ 已完成
**实现**：已添加到 `crates/tui/src/tools/web_search.rs`
**配置**：支持环境变量和配置文件

**优势**：
- API 可访问（在网络隔离环境下）
- 结构化结果
- AI 生成的答案

**限制**：
- 需要 API Key
- 每月 1000 次免费额度

#### B. Brave Search API

**可行性**：⚠️ 需要 API Key
**实现难度**：中等

**实现步骤**：
1. 注册 Brave Search API（https://brave.com/search/api/）
2. 获取 API Key
3. 在 DeepSeek-TUI 中实现 Brave Search API 调用
4. 添加配置支持

**代码示例**：
```rust
async fn execute_brave(&self, query: &str, api_key: &str) -> Result<ToolResult, ToolError> {
    let url = format!(
        "https://api.search.brave.com/res/v1/web/search?q={}&count={}",
        url_encode(query), 10
    );
    
    let resp = client
        .get(&url)
        .header("Accept", "application/json")
        .header("X-Subscription-Token", api_key)
        .send()
        .await?;
    
    // 解析响应...
}
```

#### C. Multi Search Engine（网页爬虫）

**可行性**：⚠️ 需要爬虫支持
**实现难度**：高

**挑战**：
1. 需要实现网页爬虫
2. 需要解析不同搜索引擎的 HTML
3. 需要处理反爬虫机制
4. 需要维护多个搜索引擎的解析器

**优势**：
- 无需 API Key
- 覆盖面广
- 支持国内搜索引擎

**实现建议**：
- 优先实现国内搜索引擎（百度、Bing CN）
- 使用现有的 HTML 解析库
- 添加缓存机制减少请求

## 4. 推荐方案

### 4.1 短期方案（立即可用）

**使用 Tavily 搜索**（已集成）

```toml
[network]
default = "allow"

[web_search]
engine = "tavily"

[tavily]
api_key = "tvly-dev-YOUR_API_KEY"
```

**优势**：
- 已经集成，无需额外开发
- API 可访问
- 结构化结果

### 4.2 中期方案（1-2 周）

**添加 Brave Search API 支持**

1. 注册 Brave Search API
2. 实现 API 调用
3. 添加配置支持
4. 测试和优化

### 4.3 长期方案（1-2 月）

**实现 Multi Search Engine**

1. 实现网页爬虫框架
2. 添加国内搜索引擎支持
3. 添加国际搜索引擎支持
4. 实现智能引擎选择

## 5. 总结

### 当前状态

| 搜索引擎 | WorkBuddy | DeepSeek-TUI | 移植状态 |
|----------|-----------|--------------|----------|
| Brave Search API | ✅ 内置 | ❌ 未集成 | 可移植 |
| Tavily | ✅ Skill | ✅ 已集成 | 已完成 |
| Multi Search Engine | ✅ Skill | ❌ 未集成 | 可移植 |
| DuckDuckGo | ❌ | ✅ 默认 | - |
| Bing | ❌ | ✅ 备选 | - |

### 推荐优先级

1. **立即使用**：Tavily 搜索（已集成）
2. **短期移植**：Brave Search API（需要 API Key）
3. **中期实现**：国内搜索引擎（百度、Bing CN）
4. **长期实现**：Multi Search Engine（17 个引擎）

### 关键结论

**"WorkBuddy 的 web search 能力主要来自三个层面：内置 Brave Search API、Tavily Skill、Multi Search Engine Skill。其中 Tavily 已经成功移植到 DeepSeek-TUI，Brave Search API 和 Multi Search Engine 可以在未来版本中逐步移植。"**

---

## 6. Multi Search Engine 实现可行性确认

### 实现方式

**核心原理**：直接爬取搜索引擎 HTML 页面，解析提取结果

```javascript
// 示例：爬取 Google 搜索结果
web_fetch({"url": "https://www.google.com/search?q=搜索关键词"})
```

### 技术栈需求

| 组件 | DeepSeek-TUI 现状 | 需要添加 |
|------|-------------------|----------|
| HTTP 请求 | ✅ `reqwest` 已有 | - |
| HTML 解析 | ❌ 无 | `scraper` 或 `select` 库 |
| URL 编码 | ✅ 已有 | - |
| 异步运行时 | ✅ `tokio` 已有 | - |

### 实现难度评估

| 难度等级 | 说明 |
|----------|------|
| **低** | HTTP 请求、URL 编码 |
| **中** | HTML 解析库集成、基础解析器 |
| **高** | 17 个搜索引擎的解析器、反爬虫处理 |

### 关键挑战

1. **HTML 解析器**：每个搜索引擎的 HTML 结构不同，需要为每个引擎编写解析器
2. **反爬虫机制**：User-Agent、Cookie、验证码、IP 限制
3. **维护成本**：搜索引擎会更新 HTML 结构，需要定期维护

### 可行性结论

**✅ 可实现，但需要以下条件：**

1. **添加 HTML 解析库**：如 `scraper`（Rust 生态最成熟）
2. **实现解析器**：为 17 个搜索引擎编写 HTML 解析器
3. **处理反爬虫**：添加 User-Agent 轮换、请求间隔、代理支持
4. **定期维护**：监控搜索引擎 HTML 结构变化

### 推荐实现路径

**阶段 1（1-2 天）**：实现基础框架
- 添加 `scraper` 依赖
- 实现 HTTP 请求 + HTML 解析基础架构
- 实现 2-3 个简单引擎（DuckDuckGo、Brave、Startpage）

**阶段 2（3-5 天）**：扩展引擎支持
- 实现国内引擎（百度、Bing CN）
- 实现更多国际引擎
- 添加反爬虫机制

**阶段 3（1-2 周）**：完善和优化
- 实现全部 17 个引擎
- 添加智能引擎选择
- 性能优化和缓存

### 最终确认

**"Multi Search Engine（17 个引擎）实现难度高但可实现。核心挑战是 HTML 解析器编写和反爬虫处理，不是技术可行性问题。推荐分阶段实现，先实现简单引擎，再逐步扩展。"**
