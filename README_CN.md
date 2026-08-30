# UsageBar

[English](README.md) · [中文](README_CN.md)

贴在屏幕边缘的 AI 用量圆环条。平时占位很小，悬停某个圆环看明细和重置时间。可拖到任意位置，靠近边缘自动吸附。只读本机登录态，只请求各家官方接口，不会把对话发到第三方服务器。

macOS 12+ · Windows · Linux · MIT License · [下载最新版](https://github.com/rayelzz/UsageBar-tauri/releases/latest)

### 目录

- [功能](#功能)
- [用量和重置时间怎么来的](#用量和重置时间怎么来的)
- [安装](#安装)
- [菜单](#菜单)
- [常见问题](#常见问题)
- [隐私](#隐私)
- [许可证](#许可证)

| Codex | Cursor |
| --- | --- |
| <img src="docs/codex-usage.png" alt="Codex 用量" width="280" /> | <img src="docs/cursor-usage.png" alt="Cursor 用量" width="280" /> |

| Grok | GLM |
| --- | --- |
| <img src="docs/grok-usage.png" alt="Grok 用量" width="280" /> | <img src="docs/glm-usage.png" alt="GLM 用量" width="280" /> |

<p>
  <img src="docs/menu.png" alt="右键菜单" width="240" />
  &nbsp;
  <img src="docs/menu-display.png" alt="显示样式子菜单" width="280" />
  &nbsp;
  <img src="docs/style-icons.png" alt="透明图标样式" width="220" />
</p>

### 功能

- 一条紧凑条上同时看 Codex、Cursor、Grok、GLM
- 悬停显示套餐用量、API 用量、周窗口 / 5 小时窗口和重置时间
- 可拖动，自动贴左 / 右 / 上 / 下
- 两种显示样式：**圆环用量**（完整圆环 + 百分比）或 **透明图标**（迷你圆环，仍贴在屏幕边缘）
- 顶部 / 底部贴边时，百分比显示在圆环右侧
- 鼠标不在条上时默认点穿，不挡后面窗口
- 状态栏 / 托盘是文字 **UB**（macOS）。右键圆环条或点 **UB** 打开同一套菜单
- 默认 60 秒刷新，可改
- 官方品牌图标；剩余 &lt; 20%（已用 ≥ 80%）红，剩余 20%–40%（已用 60%–80%）黄，其余绿

### 用量和重置时间怎么来的

应用**不做估算**。它复用本机已有登录态，直接请求各家官方用量接口。

| 工具 | 本机凭证 | 官方接口 | 圆环 / 气泡 |
| --- | --- | --- | --- |
| **Codex** | `~/.codex/auth.json` | ChatGPT `backend-api/wham/usage` | 周额度（及额外窗口） |
| **Cursor** | Cursor `state.vscdb` 会话 | `cursor.com/api/usage-summary` | Included usage + API usage |
| **Grok** | `~/.grok/auth.json` | `cli-chat-proxy.grok.com/v1/billing` | 周额度 + 额外窗口 |
| **GLM** | `GLM_API_KEY` / `~/.zai/config.json` / cc-switch | `api.z.ai` 或智谱 quota | 5 小时窗口 + 周额度 + MCP |

Cursor 会话文件位置：

- macOS：`~/Library/Application Support/Cursor/User/globalStorage/state.vscdb`
- Windows：`%APPDATA%\Cursor\User\globalStorage\state.vscdb`
- Linux：`~/.config/Cursor/User/globalStorage/state.vscdb`

百分比和重置时间都是接口原样返回。过期 token 会尽量在本地刷新。没装或没登录的工具显示暂无数据。

### 安装

对方电脑必须**自己登录** Codex / Cursor / Grok / GLM。安装包不带任何人的账号。

1. 从 [Releases](https://github.com/rayelzz/UsageBar-tauri/releases/latest) 下载对应系统的包。
   - macOS 苹果芯片（M 系列）：`aarch64.dmg`
   - macOS Intel：`x64.dmg`
   - Windows：`x64-setup.exe`
   - Linux：`.AppImage` 或 `.deb`
2. macOS 包是临时签名。第一次打开用右键 → 打开，或在 **系统设置 → 隐私与安全性** 里允许。
3. 照常登录 Codex CLI / Cursor / Grok CLI / GLM，UsageBar 会读这些登录态。

从源码打包：

```bash
npm install
npm run tauri dev      # 开发
npm run tauri build    # 本机安装包
```

产物目录：

- macOS：`src-tauri/target/release/bundle/dmg`
- Windows：`src-tauri/target/release/bundle/nsis`
- Linux：`src-tauri/target/release/bundle/appimage` 或 `deb`

推送 `v*` 标签会走 GitHub Actions，同时出 macOS（苹果芯片 + Intel）、Windows 和 Linux 的包。

### 菜单

菜单栏 / 托盘 **UB**，或右键圆环条：立即刷新、自动刷新、锁定位置、不阻挡下方点击、贴左 / 右 / 上 / 下、显示样式、语言（默认 English，可切 中文；厂商和模型名保持英文）、登录时打开、退出。

偏好存在 `~/.usagebar/prefs.json`。

### 常见问题

**某个工具显示「暂无数据」？**  
这台电脑上还没登录该工具，或读不到本机凭证。先用官方应用 / CLI 登录，再点 **立即刷新**。

**Cursor 提示「未找到 Cursor 登录信息」？**  
UsageBar 从 Cursor 本机 `state.vscdb` 读 `cursorAuth/accessToken` 和 `cursorAuth/userId`。请在**这台电脑**打开 Cursor 桌面端并登录。把 UsageBar 拷给别人，不会带上你的 Cursor 登录态。

**发给朋友后，对方看不到 Cursor / Codex 用量？**  
这是预期行为。凭证只存在各自电脑上，对方需要自己登录。

**会不会上传对话，或把 token 打进安装包？**  
不会。只读本机会话文件 / 环境变量，只请求官方用量接口。安装包里没有账号。

**点不到条后面的窗口？**  
打开 **不阻挡下方点击**（默认开启）。鼠标不在条上时，点击会穿过。

**点不到圆环条 / 打不开菜单？**  
点穿开启时，点圆环周围的透明区域可能点到后面。请悬停圆环，或用托盘 **UB**。也可以先关掉点穿，再锁定位置。

**macOS 提示无法打开 / 未识别的开发者？**  
发布包未做 Apple 公证。请右键应用 → 打开，或到 **隐私与安全性** 允许。

**圆环颜色？**  
绿：已用 &lt; 60%。黄：60%–80%。红：≥ 80%。

**GLM 的 Key 放哪？**  
在 shell 里导出 `GLM_API_KEY`（或 `ZAI_API_KEY` / `Z_AI_API_KEY`），或写 `~/.zai/config.json`，或在 cc-switch 里配置 GLM / 智谱 / z.ai。

**Windows / Linux 注意。**  
Windows 需要 WebView2（安装器可引导下载）。托盘通常会显示图标，而不是只有标题文字。Linux 需要可用的系统托盘。

### 隐私

只读本机凭证，只请求官方接口，不上传对话，无第三方统计。

### 许可证

[MIT](LICENSE)
