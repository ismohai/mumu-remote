# MuMu 局域网远控实现计划 (Windows Rust + Android)

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 构建一套在局域网内用安卓手机以低延迟远程控制 Windows 上 MuMu 模拟器的系统，包含桌面端 Rust 服务和安卓客户端。

**Architecture:** Windows 端 Rust 程序捕获 MuMu 窗口并用 GPU 硬编推送 H.264 视频帧到安卓，安卓用 MediaCodec 硬解并渲染，同时安卓触控事件通过自定义协议传回 PC，由 Rust 转成 adb 触控/按键注入 MuMu。

**Tech Stack:** Rust（Windows 桌面 & 服务）、Windows DXGI/屏幕捕获 API、硬件视频编码（Media Foundation 或第三方库）、自定义 UDP 协议、adb、Android（Kotlin/Java）、MediaCodec、二维码扫描。

---

### Task 1: 初始化项目结构

**Files:**
- Create: `pc/mumu-remote/Cargo.toml`
- Create: `pc/mumu-remote/src/main.rs`
- Create: `android/app`（Android 工程根目录）

**Step 1: 初始化 Rust 项目**

- 命令（在 `pc` 目录下）：`cargo new mumu-remote --bin`
- 结果：生成基础 Rust 二进制项目。

**Step 2: 初始化 Android 项目**

- 使用 Android Studio 或命令行在 `android` 目录下创建新项目，包名自定义，语言选 Kotlin（或 Java）。
- 配置最小 SDK 版本（例如 24+），启用相机权限（用于二维码）和网络权限。

### Task 2: Rust 桌面应用基本框架

**Files:**
- Modify: `pc/mumu-remote/Cargo.toml`
- Modify: `pc/mumu-remote/src/main.rs`

**Step 1: 选择并添加 GUI/托盘库（可选）**

- 在 `Cargo.toml` 中添加：
  - 日志库（如 `log` + `env_logger`）。
  - 配置解析库（如 `serde`, `serde_json`）。
  - 简单窗口/托盘库（例如 `winit` 或其他适用于 Windows 的托盘库）。

**Step 2: 实现最小可运行主函数**

- 在 `main.rs` 中：
  - 初始化日志。
  - 初始化配置（默认监听端口等）。
  - 启动 GUI/托盘窗口，显示“开启控制功能”的按钮占位。

### Task 3: MuMu 窗口发现与绑定

**Files:**
- Create: `pc/mumu-remote/src/mumu.rs`
- Modify: `pc/mumu-remote/src/main.rs`

**Step 1: 接入 Windows API**

- 在 `Cargo.toml` 中添加 Windows FFI 库，如 `windows` crate。

**Step 2: 枚举窗口**

- 在 `mumu.rs` 中实现：
  - 函数 `find_mumu_window()`：使用 Win32 API 枚举顶层窗口，通过窗口标题/类名匹配 MuMu。
  - 返回窗口句柄和尺寸信息。

**Step 3: 在主程序中调用**

- 在 `main.rs` 中：
  - 调用 `find_mumu_window()` 并在日志/GUI 中显示找到的 MuMu 窗口信息。

### Task 4: 窗口采集与基础帧循环

**Files:**
- Create: `pc/mumu-remote/src/capture.rs`
- Modify: `pc/mumu-remote/src/main.rs`

**Step 1: 封装 DXGI/屏幕捕获**

- 在 `capture.rs` 中：
  - 封装对指定窗口区域进行捕获的逻辑（可先用简单 GDI BitBlt 验证，后续替换 DXGI/Desktop Duplication 优化性能）。

**Step 2: 建立采集循环**

- 在 `main.rs` 或独立模块中：
  - 启动一个线程，以目标帧率（初期 60fps）定时调用捕获函数。
  - 将捕获的原始帧（例如 BGRA buffer）传给后续编码模块（暂时只统计 FPS 日志）。

### Task 5: 视频编码模块（H.264）

**Files:**
- Create: `pc/mumu-remote/src/encoder.rs`
- Modify: `pc/mumu-remote/src/capture.rs`
- Modify: `pc/mumu-remote/src/main.rs`

**Step 1: 选定编码库**

- 方案 A：使用 Windows Media Foundation 的 H.264 硬编（通过 FFI 调用）。
- 方案 B：使用已存在的 Rust/FFI 封装（如基于 ffmpeg 的 crate）。
- 初版选择实现更快的方案，哪种依赖在当前环境更容易接入就用哪种。

**Step 2: 编写编码接口**

- 在 `encoder.rs` 中实现：
  - 初始化编码器（分辨率、帧率、码率）。
  - 接收原始帧 buffer，输出编码好的 NAL 单元/完整帧字节流。

**Step 3: 打通采集→编码**

- 修改采集循环：
  - 将捕获到的帧传入 encoder，获取编码帧。
  - 暂时将编码帧长度/时间戳打印日志，验证性能与稳定性。

### Task 6: UDP 视频传输协议

**Files:**
- Create: `pc/mumu-remote/src/net.rs`
- Modify: `pc/mumu-remote/src/main.rs`
- Modify: `pc/mumu-remote/src/encoder.rs`

**Step 1: 定义帧头结构**

- 在 `net.rs` 中定义：
  - 会话 ID、帧序号、分片序号/总数、时间戳等字段。

**Step 2: 实现 UDP 发送**

- 使用 `tokio` 或标准库 UDP socket：
  - 将编码帧按最大包大小拆分成多个 UDP 包。
  - 每个包加上帧头，发送到指定 IP/端口（后续由握手确定）。

**Step 3: 集成到编码流程**

- 修改 encoder 或主循环：
  - 在编码完成后调用网络模块发送帧。

### Task 7: 安卓端基础工程与设备列表 UI

**Files:**
- Modify: `android/app/src/main/AndroidManifest.xml`
- Modify: `android/app/src/main/java/.../MainActivity.kt`

**Step 1: Android 权限与基本 Activity**

- 在 Manifest 中声明：`INTERNET`、`ACCESS_WIFI_STATE`、`CAMERA`（用于二维码）。

**Step 2: 设备列表界面**

- 在 `MainActivity` 实现：
  - 显示已配对电脑列表（初始为空）。
  - 提供“扫描二维码添加电脑”的按钮。

### Task 8: 二维码配对流程

**Files:**
- Modify: `pc/mumu-remote/src/main.rs`
- Create: `pc/mumu-remote/src/pairing.rs`
- Modify: `android/app/src/main/java/.../MainActivity.kt`

**Step 1: 生成配对二维码（PC）**

- 在 `pairing.rs` 中：
  - 生成随机 token，拼接 IP+端口形成 JSON 或简单字符串。
  - 使用二维码库生成二维码图像，并在桌面窗口中显示。

**Step 2: 安卓扫码与保存设备**

- 在 Android 端集成二维码扫描库。
- 扫描二维码后：
  - 解析 IP、端口、token。
  - 向 PC 发送配对请求（可用 HTTP/UDP）。
  - 成功后将设备保存到本地存储（名称、IP、端口、deviceId）。

### Task 9: 握手与连接管理

**Files:**
- Modify: `pc/mumu-remote/src/net.rs`
- Modify: `pc/mumu-remote/src/main.rs`
- Modify: `android/app/src/main/java/.../MainActivity.kt`

**Step 1: 实现 PC 端握手逻辑**

- 在 `net.rs` 中添加：
  - 处理来自安卓的配对和连接请求。
  - 验证 token 和设备 ID，建立会话（分配会话 ID）。

**Step 2: 安卓端连接与断开**

- 在 Android 设备列表中：
  - 点击某个电脑发起连接请求。
  - 获取会话 ID 后进入游戏控制界面。
  - 在设置菜单中提供“断开连接”，发送断开消息。

### Task 10: 安卓端视频接收与解码

**Files:**
- Create: `android/app/src/main/java/.../VideoActivity.kt`
- Modify: `android/app/src/main/AndroidManifest.xml`

**Step 1: 视频接收线程**

- 在 `VideoActivity` 中：
  - 建立 UDP socket，接收视频帧数据包。
  - 按帧头重组成完整编码帧，加入解码队列。

**Step 2: MediaCodec 解码与渲染**

- 使用 `MediaCodec` 创建 H.264 解码器：
  - 配置分辨率、格式等。
  - 将重组好的帧写入 input buffer。
  - 使用 SurfaceView/TextureView 显示输出。

### Task 11: 输入协议与 Rust 端解包

**Files:**
- Modify: `pc/mumu-remote/src/net.rs`
- Create: `pc/mumu-remote/src/input.rs`
- Modify: `android/app/src/main/java/.../VideoActivity.kt`

**Step 1: 定义输入事件结构**

- 类型：
  - TOUCH_DOWN / TOUCH_MOVE / TOUCH_UP（含 pointer id 和坐标）。
  - KEY_EVENT（返回、主页、多任务等）。

**Step 2: 安卓端发送输入**

- 在 `VideoActivity` 的触摸事件回调中：
  - 采集多点触控事件，转换为自定义输入消息，通过 UDP 发送到 PC。

**Step 3: Rust 端解析输入消息**

- 在 `input.rs` 中实现：
  - 解析输入报文，转换为内部统一结构，交给 adb 模块处理。

### Task 12: adb 注入与坐标映射

**Files:**
- Create: `pc/mumu-remote/src/adb.rs`
- Modify: `pc/mumu-remote/src/input.rs`

**Step 1: adb 调用封装**

- 封装与 MuMu adb 的交互：
  - 启动 adb 客户端（连接到 MuMu 对应端口）。
  - 发送触控和按键事件（可使用 `adb shell input` 或更底层接口）。

**Step 2: 坐标映射实现**

- 根据 MuMu 当前分辨率（通过 adb 读出）将安卓屏幕坐标映射到模拟器坐标。

**Step 3: 集成输入链路**

- 在 `input.rs` 中：
  - 对解析后的事件调用 adb 模块，完成注入。

### Task 13: 安卓右侧导航栏与手势

**Files:**
- Modify: `android/app/src/main/java/.../VideoActivity.kt`
- Create: `android/app/src/main/res/layout/view_side_nav.xml`

**Step 1: 实现右侧边缘手势区**

- 在 `VideoActivity` 布局中：
  - 增加一个覆盖在画面之上的透明右侧边缘 View 作为手势触发区域。

**Step 2: 导航栏 UI 与按钮**

- 新建垂直导航栏布局，包含：返回、主页、多任务、设置、断开。
- 在边缘滑动时显示导航栏，点击按钮发送对应输入事件或打开设置面板。

**Step 3: 拦截系统返回键**

- 在 `VideoActivity` 中重写 `onBackPressed` 或相关回调，将返回键事件转换为远程“返回”输入，而不是退出 Activity。

### Task 14: 全局设置菜单（分辨率/刷新率）

**Files:**
- Create: `android/app/src/main/res/layout/dialog_settings.xml`
- Modify: `android/app/src/main/java/.../VideoActivity.kt`
- Modify: `pc/mumu-remote/src/net.rs`
- Modify: `pc/mumu-remote/src/capture.rs`

**Step 1: 安卓端设置 UI**

- 在设置弹窗中提供：分辨率选项（默认/跟随/720p/1080p/2K）、刷新率选项（60/75/90）。

**Step 2: 设置同步到 PC**

- 点击保存时，将选项通过控制通道发送给 PC。

**Step 3: PC 调整采集参数**

- 在 `capture.rs` 中根据设置调整目标分辨率与帧率。
- 初版只缩放编码输出尺寸，不强制修改 MuMu 内部分辨率。

### Task 15: 连接管理与状态显示

**Files:**
- Modify: `pc/mumu-remote/src/main.rs`
- Modify: `pc/mumu-remote/src/net.rs`
- Modify: `android/app/src/main/java/.../VideoActivity.kt`

**Step 1: PC 端状态显示**

- 在桌面界面/托盘提示中显示当前连接状态、帧率、分辨率等基础信息。

**Step 2: 安卓端连接状态处理**

- 在 `VideoActivity` 中：
  - 处理网络中断或 PC 端断开消息，显示提示并返回设备列表。

### Task 16: 性能调优与基础测试

**Files:**
- Modify: `pc/mumu-remote` 源码（根据需要）
- Modify: `android/app` 源码（根据需要）

**Step 1: 局域网延迟测试**

- 通过简单时间戳日志或在屏幕上显示本地/远程时间差，估算端到端延迟。

**Step 2: 调整编码与缓冲参数**

- 尝试不同码率、帧率、编码参数，以保证在典型环境下接近或低于 20ms 延迟。

**Step 3: 稳定性测试**

- 在多款游戏和不同时间跨度下运行，观察是否存在崩溃、花屏、输入丢失等问题。
