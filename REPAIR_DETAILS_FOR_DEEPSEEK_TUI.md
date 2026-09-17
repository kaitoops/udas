# DEEPSEEK-TUI 修复细节与认知提示词

**生成时间**: 2026-05-20 21:56  
**用途**: 为 DEEPSEEK-TUI 提供修复过程中的关键细节和认知，确保未来能正确处理类似问题。

---

## ⚠️ 关键认知：编译阻塞问题的根本原因

### 现象
当尝试编译 `deepseek-tui` 时，出现错误：
```
error: failed to remove file target/debug/deepseek-tui.exe (拒绝访问)
```

### 根本原因
**当 DEEPSEEK-TUI 在运行时，它自身的可执行文件会被锁定，导致无法被覆盖或删除。**

### 重要认知
> **DEEPSEEK-TUI 在改动自身时才会出现此锁定问题。**
> 
> 具体场景：
> 1. 当你（DEEPSEEK-TUI）正在运行时，你的 `.exe` 文件被操作系统锁定
> 2. 此时如果尝试重新编译你自己，新生成的 `.exe` 无法覆盖正在运行的旧版本
> 3. 这是 Windows 操作系统的文件锁定机制，不是代码错误

### 解决方案
**必须从外部终端进行编译，而不是在运行中的 TUI 内部编译。**

#### 正确流程：
```bash
# 1. 退出当前 DEEPSEEK-TUI 会话
# 2. 在外部终端（PowerShell/CMD）执行：
cd C:\v4pro
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cargo build --package deepseek-tui

# 3. 或者使用 build.bat 脚本（已包含 vcvars 初始化）
.\build.bat
```

#### 错误做法：
- ❌ 在运行中的 TUI 内部执行编译命令
- ❌ 尝试强制删除正在运行的 `.exe` 文件
- ❌ 使用 `cargo watch` 等自动重编译工具（会持续冲突）

### 预防措施
1. **编译前检查**：如果需要修改 DEEPSEEK-TUI 自身代码，先完全退出程序
2. **开发模式**：使用 `cargo check` 进行快速语法检查（不生成新 exe）
3. **外部编译**：始终从外部终端执行完整编译

---

## 🔧 多终端显示系统（Sub-Agent Display Relay）细节

### 架构概述
当子代理通过 `agent_open` 启动时，系统会自动弹出独立终端窗口实时显示输出（含 thinking 块）。

### 关键组件
1. **`crates/tui/src/tools/subagent/display_relay.rs`**
   - `SubAgentDisplayRelay` 结构体
   - 文件输出捕获：`~/.udas/agent-display/<agent-id>/output.log`
   - 平台自适应终端启动（Windows/macOS/Linux）
   - 只读显示，不接收 stdin 输入

2. **`crates/tui/src/tools/subagent/mod.rs`**
   - `SubAgentTask` 新增 `display_relay` 字段
   - `run_subagent` 新增 `relay` 参数
   - `spawn_background_with_assignment_options` 创建 relay + 启动终端

3. **`crates/tui/src/main.rs`**
   - `Commands::AgentDisplay` 枚举变体
   - `run_agent_display()` 函数：读文件、轮询、只读显示

### 集成点
- **子代理启动时**：自动创建 relay 并启动终端窗口
- **输出转发**：thinking/text/tool blocks 转发到 relay
- **完成/失败时**：写入状态到 relay

### 测试验证
```bash
# 1. 创建测试目录和日志文件
mkdir "%USERPROFILE%\.deepseek\agent-display\test-agent"
echo [STATUS] test > "%USERPROFILE%\.deepseek\agent-display\test-agent\output.log"

# 2. 测试显示客户端
target\debug\deepseek-tui.exe agent-display test-agent
# 应显示 "Agent Display: test-agent" 和测试内容

# 3. 验证子代理默认模型
启动子代理时，API 调用应使用 `deepseek-v4-flash` 而非 `deepseek-v4-pro`
```

---

## 🎯 模型变更细节

### 变更内容
所有子代理默认使用 `deepseek-v4-flash` 替代默认的 PRO，以节省 API 费用。

### 修改位置
`crates/tui/src/tools/subagent/mod.rs`，在 `configured_model` 解析后添加：
```rust
if configured_model.is_none() {
    configured_model = Some("deepseek-v4-flash".to_string());
}
```

### 验证方法
1. 启动子代理，观察 API 调用日志
2. 检查网络请求中的 model 字段应为 `deepseek-v4-flash`
3. 在 TUI 界面中确认模型显示

### 影响范围
- 所有自动创建的子代理
- 路由管理器（`SUBAGENT_ROUTER_SYSTEM_PROMPT`）中的模型选择逻辑
- 错误消息中的示例模型名称（仅作为示例，不影响实际选择）

---

## 📋 编译与测试检查清单

### 编译前检查
- [ ] 确保 DEEPSEEK-TUI 未运行（检查任务管理器）
- [ ] 确保在外部终端中（非 TUI 内部）
- [ ] 确保 Visual Studio Build Tools 已安装

### 编译命令
```bash
# 方式 1：使用 build.bat（推荐）
cd C:\v4pro
.\build.bat

# 方式 2：手动编译
cd C:\v4pro
call "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
cargo build --package deepseek-tui
```

### 验证清单
1. **编译通过**：`cargo check --package deepseek-tui` 无错误
2. **生成 exe**：`target/debug/deepseek-tui.exe` 存在且可执行
3. **显示功能**：`deepseek-tui agent-display test-agent` 正常显示
4. **模型验证**：子代理默认使用 `deepseek-v4-flash`

---

## 🚨 常见错误与解决方案

### 错误 1：编译时 exe 被锁定
**现象**：`error: failed to remove file target/debug/deepseek-tui.exe`
**原因**：DEEPSEEK-TUI 正在运行，锁定自身 exe 文件
**解决**：
1. 完全退出 DEEPSEEK-TUI
2. 从外部终端重新编译
3. 或者重启计算机后编译

### 错误 2：cargo check 通过但 cargo build 失败
**现象**：语法检查通过，但链接或依赖错误
**原因**：可能是依赖库版本冲突或环境配置问题
**解决**：
1. 清理构建缓存：`cargo clean`
2. 更新依赖：`cargo update`
3. 重新编译：`cargo build --package deepseek-tui`

### 错误 3：agent-display 无法启动
**现象**：运行 `deepseek-tui agent-display <id>` 无响应
**原因**：日志文件不存在或路径错误
**解决**：
1. 检查 `~/.udas/agent-display/<id>/output.log` 是否存在
2. 确认子代理已启动且 ID 正确
3. 手动创建测试日志文件验证

---

## 💡 最佳实践

1. **开发流程**：
   - 先用 `cargo check` 进行快速语法检查
   - 确认无错误后，再执行完整编译
   - 编译前确保程序未运行

2. **调试技巧**：
   - 使用 `cargo check` 代替 `cargo build` 进行快速验证
   - 查看 `target/debug/deps/` 目录了解编译产物
   - 使用 `cargo clean` 清理后重新编译

3. **版本管理**：
   - 编译前提交代码变更
   - 使用 `cargo build --release` 生成优化版本
   - 保留多个版本的 exe 文件用于回滚

---

## 📞 技术支持信息

**项目路径**：`C:\v4pro`  
**构建工具**：Visual Studio Build Tools 2022  
**Rust 版本**：1.88+  
**关键文件**：
- 编译脚本：`C:\v4pro\build.bat`
- 工作空间配置：`C:\v4pro\Cargo.toml`
- 主程序入口：`C:\v4pro\crates\tui\src\main.rs`

**编译环境要求**：
- Windows 10/11
- Visual Studio Build Tools 2022（含 MSVC）
- Rust 工具链 1.88+
- 网络连接（下载依赖）

---

**最后更新**：2026-05-20  
**维护者**：WorkBuddy Claw Agent  
**版本**：1.0