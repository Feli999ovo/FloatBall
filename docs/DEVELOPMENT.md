# 开发文档

> 面向参与开发的人：构建、技术栈、测试、工程结构和实现注意事项。用户向介绍见 [README](../README.md)。

悬浮球是基于 Tauri 2 的桌面应用，前端使用原生 HTML、CSS 和 JavaScript，后端使用 Rust；数据保存在本机。当前实现包含对话、Agent、技能、定时任务、共享屏幕、划词和语音等功能。

## 技术栈

[Tauri 2](https://tauri.app) · Rust · 原生 HTML / CSS / JavaScript（前端无需打包）· SQLite · 系统钥匙串 · [xcap](https://github.com/nashaofu/xcap) 截屏。

## 从源码构建

### 环境

- macOS：Xcode 命令行工具（`xcode-select --install`）、[Rust](https://rustup.rs)、[Node.js](https://nodejs.org) LTS。
- Windows：[Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（勾选「使用 C++ 的桌面开发」）、[Rust](https://rustup.rs)、[Node.js](https://nodejs.org) LTS；Windows 10/11 通常已预装 WebView2。

### 运行与打包

```bash
npm install
npm run dev      # 开发运行，首次编译需要几分钟
npm run build    # 生成安装包
```

不想装 Node 也可以在 `src-tauri` 目录运行 `cargo run`。云端构建可在 Actions 手动触发，或推送 `v*` 标签生成安装包；推标签还会生成 Release 草稿。

### 测试

```bash
cargo test --manifest-path src-tauri/Cargo.toml --locked
python3 -m pip install playwright pillow pytest pytest-asyncio pytest-random-order
python3 -m playwright install chromium
python3 tools/test_ui.py ./ui-shots
python3 -m pytest tools/ui_tests
```

全量 CI 检查还包括格式、静态检查、前后端接口核对、Windows 安装卸载和打包；不要把浏览器 mock 测试当作桌面真机验证。

---

## 工程结构

README 用的展示图由浏览器 mock 生成，不是真机截图。更新界面后可运行 [`tools/make_readme_shots.py`](../tools/make_readme_shots.py) 重做图片；脚本默认输出到 `docs`，也可用 `--out <目录>` 指定输出目录。脚本从 Git 历史读取原横幅，浅克隆需要先取得原稿提交 `44a95fe`。如系统找不到中文字体，可用 `--font <字体文件>` 指定。

```
floatball/
├─ package.json            npm run dev / build
├─ app-icon.png            应用图标源文件（Logo v6 深色版）
├─ src/                    前端：纯 HTML/CSS/JS（浏览器原生 ES 模块），不需要打包工具
│  ├─ index.html           小球 + 胶囊 + 卡片（对话 / 任务 / 历史 / 交给终端）
│  ├─ main.js              小球窗口入口：按顺序初始化 app/ 里的各个模块
│  ├─ app/                 小球窗口的模块：对话、发送、任务、定时任务、交给终端、共享屏幕、划词、语音等，共享状态在 state.js
│  ├─ settings.html/.css   设置窗口（单独的窗口）
│  ├─ settings-main.js     设置窗口入口
│  ├─ settings/            设置窗口的模块：模型库、Agents、技能、记忆、权限、外观、快捷键、划词、语音
│  ├─ ctxmenu.* / selbar.* 自绘右键菜单、划词工具栏（各自是单独的透明小窗口）
│  ├─ error-message.js     后端错误（kind + message）转成给用户看的话和按钮
│  ├─ ipc.d.ts             从 Rust 生成的前后端接口说明（别手改，见下面 gen_ipc）
│  ├─ style.css            样子和表情动画
│  └─ assets/              玻璃球、胶囊图、服务商 logo
├─ src-tauri/
│  ├─ tauri.conf.json      窗口：96×96、透明、无边框、置顶
│  ├─ tauri.windows.conf.json  只在 Windows 上生效：数据放在 安装目录\data
│  ├─ windows/hooks.nsh    Windows 卸载时按勾选清掉数据和 API Key
│  ├─ windows/installer-*.bmp  安装器左侧和页眉的图，用 tools/make-installer-images.py 从图标生成
│  ├─ capabilities/        前端能调用哪些窗口接口
│  ├─ icons/               应用图标、托盘图标（Mac 用模板图标，自动适配深浅色菜单栏）
│  └─ src/
│     ├─ lib.rs            应用状态、启动、全局快捷键、命令注册
│     ├─ commands/        对话、配置、历史、交接、定时任务、撤回、截屏、划词、窗口和语音命令
│     ├─ llm.rs            OpenAI 兼容流式调用 + SSE 解析
│     ├─ config.rs         设置（config.json）+ 钥匙串
│     ├─ store.rs          历史记录（SQLite）
│     ├─ changes.rs        文件改动备份、整文件撤回、过期清理。`undo_change(id, confirm_changed)` 执行撤回
│     ├─ agent/            Agent 运行循环、工具、授权、交接、计价及相应用例
│     ├─ routine.rs        定时任务触发时间和一次性派发状态
│     ├─ handoff.rs        交给终端：扫描本机编程 CLI、写交接说明、打开终端
│     ├─ selection.rs      划词和读取选中文字
│     ├─ stt.rs            系统自带语音识别
│     ├─ catalog.rs        模型资料（能力、单价）
│     ├─ error.rs          统一错误类型（kind + message）
│     ├─ capture.rs        截屏、窗口列表、读文本文件
│     └─ bin/gen_ipc.rs    生成 src/ipc.d.ts（只在 macOS CI 上跑并比对）
├─ tools/                  只用于测试和构建：浏览器里模拟 Tauri 的脚本（mock-tauri.js）、界面测试（test_ui.py 入口 + ui_tests/）、接口核对（check-ipc.mjs）、Windows 安装和截图脚本、README 截图生成（make_readme_shots.py）
├─ docs/decisions/         架构决策记录：为什么这么定、什么时候该改
└─ design-reference/       网页原型，做后续功能时照着它的设计
```

数据位置：
- 设置：Mac `~/Library/Application Support/com.floatball.desktop/config.json`，Windows `安装目录\data\config.json`
- 历史：同目录下的 `history.db`
- Windows 上界面（WebView2）的缓存也在 `安装目录\data` 里。这是 `src-tauri/tauri.windows.conf.json` 里的 `appDirectoriesOverride` 决定的；开发时（`npm run dev`）对应 `src-tauri\target\debug\data`。
- Windows 的卸载：卸载界面勾选「删除应用程序数据」时（静默卸载用 `uninstall.exe /S /CLEAN`），`src-tauri/windows/hooks.nsh` 会删掉 `data` 和凭据管理器里的 API Key；不勾选、升级和重装时都不删。`tools/check-windows-install.ps1` 会真装、重装、卸载一遍来检查，云端每次构建都跑；它会删数据，所以在已经装了悬浮球的电脑上会直接拒绝运行。

---

## 当前限制

- 系统材质（Windows Mica / macOS Liquid Glass）尚未实现。Windows 使用窗口后方的桌面模糊底图。
- 小球窗口是 96×96 的方块，透明角也会拦截鼠标点击。
- macOS 尚未在真机上完整验证；高分屏和多显示器也尚未验证。
- PDF 与图片附件、自动更新、安装包签名和设置导入导出尚未实现。

## 划词与剪贴板

划词默认关闭。启用后，优先通过系统辅助功能接口读取选中文字（Windows UI Automation，macOS 辅助功能），不接触剪贴板。少数应用不支持时会模拟复制，再尝试恢复原剪贴板内容；这种回退可能无法恢复文件或富文本，也可能让 Windows 剪贴板历史记录选中的文字。读取的内容只在用户触发工具栏操作或发送后才会交给模型。`Alt+C` 主动喂入选中文字后，按发送才会发出。

## 交给终端的数据流

扫描阶段只检查本机 PATH，不运行扫描到的 CLI。执行交接时，悬浮球将交接说明写入所选项目目录的 `.floatball/handoff.md`；关闭项目内保存时，则写入应用数据目录。随后在选定终端和项目目录打开所选 CLI。CLI 会读取交接说明及其中用户指定的上下文。交接内容不会发给悬浮球服务器（悬浮球没有服务器）；所选 CLI 是否向其服务端发送内容，取决于该工具及其配置。
## 验证情况

每个 PR 和版本标签都在 GitHub Actions 上跑全套检查（`.github/workflows/build.yml`）：

- **代码检查**：`cargo fmt --check`、`cargo clippy -D warnings`（Windows 和 macOS），前端 `biome ci`。
- **Rust 测试**：Windows 和 macOS 各跑一遍 `cargo test --locked`。
- **前后端接口**：macOS 上用 `gen_ipc` 重新生成 `src/ipc.d.ts` 并和仓库里的比对，再用 `tools/check-ipc.mjs` 核对前端和 mock 用到的命令都存在。
- **界面测试**（Ubuntu + Chromium，浏览器里模拟 Tauri）：`tools/ui_tests/` 里的独立用例，每个从干净状态开始。
- **Windows 真机**：真装、重装、卸载一遍（`tools/check-windows-install.ps1`），并在真实桌面上截图（`tools/shoot-windows.ps1`，产物「真机截图-windows」）。
- **打包**：Windows 安装包和 macOS dmg（临时签名，未公证）。

具体项数以每次 CI 的结果为准。

**还没有自动化、需要真机手动确认的**：macOS 上的系统授权（屏幕录制、辅助功能、麦克风）；高分屏和多显示器；真实模型接口（界面测试和 Agent 测试用的都是模拟接口）；FAT32 / exFAT 的 U 盘。Windows CI 的桌面是 1024×768、100% 缩放。

遇到报错，把终端里的输出和日志 `floatball.log` 发给维护者（Windows 在 `安装目录\data\logs\`；Mac 在系统的应用日志目录，启动时终端会打印完整路径）；前端的报错也会以「[前端]」开头写进日志。
