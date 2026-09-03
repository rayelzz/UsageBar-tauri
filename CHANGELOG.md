# Changelog

[English](#english) · [中文](#中文)

Notable changes to UsageBar. Dates are UTC+8.

## English

### 0.0.18 — 2026-09-03

- **Transparent style matches the dark bar.** Same large rings, percents, and gear; only the black dock is gone so the desktop shows through. It no longer flashes the full layout and then shrinks to mini icons.
- **Settings apply again.** Snap left / right / top / bottom moves the bar. Lock position blocks dragging. Switching display style keeps the gear panel open, like language and display value.
- **Providers window is only providers.** Display value stays on the gear menu.
- **Bottom-edge hover card.** The detail card’s arrow stays a fixed distance from the bar when switching providers.

### 0.0.17 — 2026-09-03

- **Fix provider edits not applying** ([#2](https://github.com/rayelzz/UsageBar-tauri/issues/2)). Checking, unchecking, or reordering providers in the Providers window updates the bar and `~/.usagebar/prefs.json` immediately again. A race between window placement and full prefs saves had been restoring the previous provider list.

### 0.0.16 — 2026-09-02

- **All quota windows.** The hover card lists every window the vendor API returns: 5-hour, weekly, monthly / billing cycle, scoped weekly, on-demand, per-model. Missing windows are not invented. GLM / ZCode show weekly when the plan includes it.
- **Grok Bot.** Cursor shows **Grok Bot · Weekly limit** when that separate weekly allowance exists.
- **Reset time.** Calendar date and 24-hour clock in the **system timezone**, e.g. `Resets Sep 2, 15:30`.
- **Snappier settings.** Gear-menu chips and rows update immediately. Saving, tray rebuild, and window placement no longer block the click. The bar shape is drawn at its real size so it does not warp while the window resizes.

### 0.0.15 — 2026-09-02

- **Keep settings across restarts and updates** ([#1](https://github.com/rayelzz/UsageBar-tauri/issues/1)). Edge, position, provider count and order, display style, display value, language, refresh, lock, click-through, and update prefs all live in `~/.usagebar/prefs.json`.
- Restore the last screen and coordinates even if the monitor name changes. Incomplete or corrupt prefs no longer wipe the whole file.

### 0.0.14 — 2026-09-01

- **Display value.** Choose **Used quota** (default) or **Remaining quota** in the gear menu, tray, or Providers window.
- Rings, percents, and the detail card follow that choice. Colors still follow remaining: red ≤ 20%, yellow ≤ 40%, green otherwise.

### 0.0.13 — 2026-09-01

- **Providers.** The old Tools window and menu item are now **Providers…** (English) / **提供商…** (Chinese).
- **Updates stay in the gear menu.** The Providers window is only the provider list. Checking for updates, the current version, and auto-check live on the gear menu.
- **Clearer check result.** Reopening the menu clears the last check. If you are already on the latest build, the row shows a muted **Up to date**. If a newer build exists, the version is green and opens the download page.
- **Menu hover.** Every row and chip highlights on hover, even when the menu window is not focused.
- **No menu pointer.** The settings panel no longer draws a triangle toward the gear.
- **Checking, not empty.** While a provider is still querying, the hover card shows **Checking…** instead of **No data**. Each provider fills in as soon as its request returns.

### 0.0.12 — 2026-09-01

- Check for updates from the gear menu: current version, **Check for update**, and the latest version (click to download) sit with auto-check.

### 0.0.11 — 2026-09-01

- Automatic update checks are off by default.
- The Tools window showed the current version and a manual check; clicking the latest version opened the download page.
- Clicking the update bubble skipped that version until a later release.

### 0.0.10 — 2026-08-31

- Detached settings gear past the end of the bar; hover to show, click for the same dark menu as **UB**.
- Green dot on the gear when a newer GitHub release exists.
- Dark settings UI to match the usage tooltip.

### 0.0.9 — 2026-08-31

- Hourly (and on-launch) update reminder: **Update available x.x.x**, click to open the release page.

### 0.0.8 — 2026-08-31

- **1–10** tools; the bar shortens or lengthens with the selection. No empty **—** slots.

### 0.0.7 — 2026-08-31

- Quota reset notice: when a window drops back to **0%**, the icon pulses green and a dismissible tooltip stays up.

### 0.0.6 — 2026-08-31

- Optional **ZCode** ring (Coding Plan key in `~/.zcode/v2/config.json`). Same official GLM mark, separate credential from GLM.

### 0.0.5 — 2026-08-30

- Optional **Claude**, **Copilot**, **Gemini**, and **Antigravity** rings. Tray **Tools…** picked up to 4 tools.

### 0.0.4 — 2026-08-30

- Documented macOS Gatekeeper quarantine (`xattr -cr`) for ad-hoc signed builds.

### 0.0.3 — 2026-08-30

- Fixed Cursor login on newer desktop builds (long JWT / new user-id keys).

### 0.0.2 — 2026-08-30

- English / Chinese UI language switching.

### 0.0.1 — 2026-08-30

- First public build: Codex, Cursor, Grok, GLM edge bar.

## 中文

### 0.0.18 — 2026-09-03

- **透明样式与黑色模式一致。** 同样的大圆环、百分比和齿轮，只去掉黑底托，桌面直接透出来。不再先闪一下完整布局再收成小图标。
- **设置重新生效。** 贴左 / 右 / 上 / 下会真正挪条。锁定位置后不能再拖。切换显示样式时齿轮面板保持打开，和语言、显示值一样。
- **提供商窗口只管提供商。** 显示值只留在齿轮菜单里。
- **贴下边时详情卡对齐。** 换供应商时，箭头到主条的距离保持固定。

### 0.0.17 — 2026-09-03

- **修复修改提供商不生效**（[#2](https://github.com/rayelzz/UsageBar-tauri/issues/2)）。在提供商窗口勾选、取消或调整顺序后，用量条和 `~/.usagebar/prefs.json` 会立刻更新。此前窗口布局写回与完整配置保存存在竞态，会把提供商列表盖回旧值。

### 0.0.16 — 2026-09-02

- **全部窗口。** 悬停卡列出接口返回的每一档：5 小时、周、月 / 账期、模型周额度、按需、按模型。接口没有的不编造。GLM / ZCode 套餐里有周额度就会显示。
- **Grok Bot。** Cursor 账号若有独立的 Bot 周额度，详情里显示 **Grok Bot · 周额度**。
- **重置时间。** 按**本机系统时区**显示具体日期和 24 小时时刻，例如 `重置 9月2日 15:30`。
- **设置更跟手。** 齿轮菜单里的选项一点就亮。写配置、重建托盘、挪窗口都放到点击之后，不再卡住或把条拉变形。

### 0.0.15 — 2026-09-02

- **重启和更新后保留全部设置**（[#1](https://github.com/rayelzz/UsageBar-tauri/issues/1)）。贴边、位置、提供商个数和顺序、显示样式、显示值、语言、刷新、锁定、点穿、更新偏好都写在 `~/.usagebar/prefs.json`。
- 显示器名称变化时仍按上次坐标恢复。配置缺字段或损坏时不再整份回退成默认。

### 0.0.14 — 2026-09-01

- **显示值。** 齿轮菜单、托盘或提供商窗口里可选 **已使用额度**（默认）或 **剩余额度**。
- 圆环、百分比和详情都按这个值显示。颜色仍按剩余额度：剩余 ≤ 20% 红，≤ 40% 黄，其余绿。

### 0.0.13 — 2026-09-01

- **提供商。** 原来的「工具」菜单和窗口改为 **提供商…**（英文仍为 **Providers…**）。
- **更新只放在齿轮菜单。** 提供商窗口只负责勾选和排序。当前版本、检测更新、自动检测都在齿轮菜单里。
- **检测结果更清楚。** 再次打开菜单会清空上次结果。已是最新版时显示灰色「已是最新版」；有新版本时显示绿色版本号，点击跳转下载。
- **菜单悬停高亮。** 每一行和选项在鼠标移上去时都会高亮，窗口未聚焦时也能用。
- **去掉菜单箭头。** 设置面板不再朝齿轮画三角指针。
- **查询中，不是暂无数据。** 某个提供商还在请求时，悬停卡片显示「查询中」。各家结果返回后立刻填上。

### 0.0.12 — 2026-09-01

- 齿轮菜单里可以直接检测更新：当前版本、「检测更新」、最新版本（点击下载）和自动检测在同一处。

### 0.0.11 — 2026-09-01

- 自动检测更新默认关闭。
- 工具窗口显示当前版本，可手动检测；点击最新版本打开下载页。
- 点过更新气泡会跳过该版本，等到下一个版本再提醒。

### 0.0.10 — 2026-08-31

- 条末端外侧增加设置齿轮，悬停出现，点击打开和 **UB** 同一套深色菜单。
- 有新版本时齿轮带绿灯。
- 设置界面改为和用量气泡一样的深色样式。

### 0.0.9 — 2026-08-31

- 启动时以及大约每小时检查一次更新，显示「有新版本 x.x.x」，点击打开发布页。

### 0.0.8 — 2026-08-31

- 可勾选 **1–10** 个工具，条长随数量变化，不再留空的 **—**。

### 0.0.7 — 2026-08-31

- 额度重置提醒：某个窗口回到 **0%** 时图标绿灯闪动，并弹出可关闭的「额度已重置」。

### 0.0.6 — 2026-08-31

- 新增可选 **ZCode**（读 `~/.zcode/v2/config.json` 里的 Coding Plan Key），图标与 GLM 相同，凭证分开。

### 0.0.5 — 2026-08-30

- 可选 **Claude**、**Copilot**、**Gemini**、**Antigravity**。托盘 **工具…** 最多勾 4 个。

### 0.0.4 — 2026-08-30

- 补充 macOS 隔离标记说明（`xattr -cr`）。

### 0.0.3 — 2026-08-30

- 修复新版 Cursor 桌面端登录（超长 JWT / 新的用户 ID 字段）。

### 0.0.2 — 2026-08-30

- 界面支持中英文切换。

### 0.0.1 — 2026-08-30

- 首个公开版本：Codex、Cursor、Grok、GLM 边缘用量条。
