# 开发文档

> 面向参与开发的人：完整功能说明、工程结构、和原型的差别、测试与验证记录。用户向介绍见 [README](../README.md)。

一个住在桌面上的液态玻璃小球。点一下展开成胶囊输入框和对话卡片，接你自己的模型（任何 OpenAI 兼容接口）。

这是**能真正跑起来的第一版（v0.1）**，不是网页原型。原型里的大部分 Agent 功能（技能库、定时任务、审批、多 Agent、共享屏幕、划词、语音）还没有做，见文末「还没做的」。

---

## 已经做好的

| 功能 | 说明 |
|---|---|
| 透明置顶小球 | 无边框、透明、永远在最上层、不占任务栏 / Dock；所有桌面空间都显示 |
| 拖动、贴边、记位置 | 按住拖动；松手靠近屏幕左右边 24px 内自动贴边；下次启动回到原位；外接屏拔掉后自动回到主屏 |
| 展开方向 | 小球在屏幕右半边就向左展开，在下半边就向上展开，不会出屏幕 |
| 表情 | 待机眨眼、鼠标靠近好奇、输入时聆听、等第一个字时思考（小光点绕圈）、输出时工作、完成眯眼笑、出错歪头、拖文件进来张嘴、10 分钟没动静睡觉、点一下会被戳扁 |
| 对话 | 流式输出、停止生成、重试、复制、Markdown（标题、列表、代码块、粗体、行内代码）、推理模型的思考过程单独显示、显示 token 用量 |
| 接你自己的模型 | 任何 OpenAI 兼容 `/chat/completions`。设置里有 OpenAI、DeepSeek、通义千问、Kimi、OpenRouter、Ollama 本地 的一键预设；「测试连接」 |
| API Key 安全 | 存在系统钥匙串（macOS 钥匙串 / Windows 凭据管理器），不写进任何文件 |
| 历史记录 | 本地 SQLite，可打开、删除单条、全部删除（二次确认） |
| 截屏提问 | 截鼠标所在的那块屏幕（先把小球藏起来），缩到最长边 1600px，作为图片发给支持图片的模型；不支持图片的模型会提示 |
| 拖文件 | 把文本文件（≤200KB，代码、Markdown、txt、csv 等）拖到小球上，内容作为上下文 |
| 托盘 / 菜单栏 | 显示/隐藏小球、新对话、截屏提问、历史、设置、开机启动、退出 |
| 右键小球 | 系统原生菜单，同上 |
| 全局快捷键 | 默认 `Alt+Space` 呼出/收起（Mac 上是 ⌥Space），`Alt+Shift+S` 截屏提问；设置里可改，被占用会提示 |
| 开机启动 | 设置里或菜单里勾选 |
| 单实例 | 重复打开只会唤出已有的小球 |
| 点别处收起 | 输入框有内容、正在生成、在设置页或点了「固定」时不收 |
| 第一次打开 | 没配 Key 时自动打开设置 |

---

## 怎么跑起来

### 1. 装环境（只需一次）

**Mac**
```bash
xcode-select --install                                    # 命令行工具
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh   # Rust
brew install node                                          # 或去 nodejs.org 下载 LTS
```

**Windows**
1. 安装 [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)，勾选「使用 C++ 的桌面开发」
2. 安装 [Rust](https://www.rust-lang.org/tools/install)（rustup-init.exe）
3. 安装 [Node.js LTS](https://nodejs.org/)
4. WebView2：Windows 10/11 一般已自带

### 2. 运行

```bash
cd floatball
npm install
npm run dev          # 第一次会编译几分钟，之后很快
```

小球会出现在屏幕右侧中间，同时自动打开设置。选一个预设、填 Key、点「测试连接」、保存，就能聊了。

> Mac 第一次截屏提问时会弹「屏幕录制」权限：系统设置 → 隐私与安全性 → 屏幕与系统录音，打开本应用（开发时是你的终端），然后**重启应用**。

### 3. 打包安装包

```bash
npm run build
```
- Mac：`src-tauri/target/release/bundle/dmg/悬浮球_0.1.0_aarch64.dmg`
- Windows：`src-tauri\target\release\bundle\msi\` 和 `nsis\` 里的安装程序

自己用不需要签名。要发给别人：
- **Mac**：需要 Apple 开发者账号（99 美元/年）做签名和公证，否则别人打开会提示「无法验证开发者」。因为透明窗口用了私有接口（`macOSPrivateApi`），只能官网下载分发，不能上 App Store。
- **Windows**：没有代码签名证书会被 SmartScreen 拦一下（可点「仍要运行」）。

### 4. 跑测试

```bash
cd src-tauri && cargo test          # Rust：流式解析、HTTP、历史、设置、截图编码
```
界面测试（需要 Python 和 Playwright）：
```bash
pip install playwright pillow && playwright install chromium
python3 tools/test_ui.py /tmp/ui-shots
```

---

## 工程结构

```
floatball/
├─ package.json            npm run dev / build
├─ app-icon.png            应用图标源文件（Logo v6 深色版）
├─ src/                    前端：纯 HTML/CSS/JS，不需要打包工具
│  ├─ index.html           小球 + 胶囊 + 卡片（对话 / 历史 / 设置）
│  ├─ style.css            样子和表情动画
│  ├─ app.js               交互、窗口展开收起、流式渲染、设置
│  └─ assets/orb.png       玻璃球（从原型的球转成透明 PNG）
├─ src-tauri/
│  ├─ tauri.conf.json      窗口：96×96、透明、无边框、置顶
│  ├─ tauri.windows.conf.json  只在 Windows 上生效：数据放在 安装目录\data
│  ├─ windows/hooks.nsh    Windows 卸载时按勾选清掉数据和 API Key
│  ├─ windows/installer-*.bmp  安装器左侧和页眉的图，用 tools/make-installer-images.py 从图标生成
│  ├─ capabilities/        前端能调用哪些窗口接口
│  ├─ icons/               应用图标、托盘图标（Mac 用模板图标，自动适配深浅色菜单栏）
│  └─ src/
│     ├─ lib.rs            托盘、菜单、快捷键、开机启动、所有命令
│     ├─ llm.rs            OpenAI 兼容流式调用 + SSE 解析
│     ├─ config.rs         设置（config.json）+ 钥匙串
│     ├─ store.rs          历史记录（SQLite）
│     └─ capture.rs        截屏、读文本文件
├─ tools/                  只用于测试：浏览器里模拟 Tauri 的脚本、界面测试
└─ design-reference/       网页原型，做后续功能时照着它的设计
```

数据位置：
- 设置：Mac `~/Library/Application Support/com.floatball.desktop/config.json`，Windows `安装目录\data\config.json`
- 历史：同目录下的 `history.db`
- Windows 上界面（WebView2）的缓存也在 `安装目录\data` 里。这是 `src-tauri/tauri.windows.conf.json` 里的 `appDirectoriesOverride` 决定的；开发时（`npm run dev`）对应 `src-tauri\target\debug\data`。
- Windows 的卸载：卸载界面勾选「删除应用程序数据」时（静默卸载用 `uninstall.exe /S /CLEAN`），`src-tauri/windows/hooks.nsh` 会删掉 `data` 和凭据管理器里的 API Key；不勾选、升级和重装时都不删。`tools/check-windows-install.ps1` 会真装、重装、卸载一遍来检查，云端每次构建都跑；它会删数据，所以在已经装了悬浮球的电脑上会直接拒绝运行。

---

## 和原型的差别（要知道的）

- **玻璃不会透出背后的桌面。** 网页没法读到窗口背后的画面，小球和卡片现在是半透明深色玻璃的「画」出来的样子。真正的毛玻璃要用系统材质：Mac 的 `NSVisualEffectView`、Windows 的 Mica/Acrylic（`window-vibrancy` 库），但它们只能铺满整个矩形窗口，圆形小球要另外处理。这是下一步要做的视觉工作。
- 小球窗口是 96×96 的方块，四个透明角也会挡住鼠标点击（很小，一般感觉不到）。
- 展开时窗口会瞬间变大再出现内容，没有原型里那种小球「变形」成胶囊的连续动画。

---

## 还没做的（按建议顺序）

1. 系统毛玻璃材质（见上）
2. 划词：选中文字后浮出快捷操作（Mac 用辅助功能接口，Windows 用 UI Automation，最难）
3. 语音输入（先用模型的语音转写接口）
4. Agent：工具调用、审批（允许一次 / 修改后允许 / 拒绝并说明）、步数和预算上限
5. 技能库、定时任务、多 Agent
6. 共享屏幕（Mac ScreenCaptureKit 选择器、Windows GraphicsCapturePicker）
7. PDF、图片等非文本文件
8. 自动更新（Tauri updater）

## 验证情况

这边没有 Mac 和 Windows，所以在 Linux（arm64）上把**同一份代码**编译成真程序来验证：

- **Rust**：编译无警告；`cargo test` 16 项全部通过（流式解析含汉字被切开、本地 HTTP 流式/401 报错/非流式回退/连不上、测试连接、历史增删、设置读写、截图缩放编码、文本文件规则、不支持图片时去掉截图、本机地址绕过代理）。
- **真程序端到端**（虚拟屏幕 + 本地假模型服务 `tools/mock_openai.py`，用 xdotool 真点真按）：小球出现在上次的位置；点击展开、打字回车、请求真的发到接口、流式回复和 Markdown 显示正常；Esc 收起回原位；右键弹出系统菜单（7 项中文）；菜单打开历史记录（SQLite 里有这条对话）；全局快捷键呼出和收起；截屏快捷键真的截了屏并出现在输入框；菜单「隐藏小球」后用快捷键唤回，位置不变；拖动后位置写进 config.json。
- **界面测试** `tools/test_ui.py`（Chromium）：48 项全部通过。

测试中发现并已修好的问题：显示器信息接口用法写错导致点了不展开；展开时窗口贴屏幕边缩回会错误地改掉收起位置；有的窗口管理器隐藏再显示会把小球挪走；开着代理软件时连不上本地模型（Ollama）；有消息时空状态引导没消失；移除文件后输入框提示没恢复。

**没有在 Mac 和 Windows 真机上跑过。** 第一次在你电脑上运行，最可能需要调的是：Windows 上透明窗口的边缘、Mac 菜单栏图标大小、`Alt+Space` 是否和输入法或 Raycast/Alfred 冲突（冲突就在设置里换一个）。Linux 上窗口最小只能到 200×200（GTK 限制，Mac/Windows 没有这个问题）。遇到报错，把终端里的输出发给我；前端的报错也会以「[前端]」开头打印在终端里。
