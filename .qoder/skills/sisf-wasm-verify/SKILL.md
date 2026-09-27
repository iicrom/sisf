---
name: sisf-wasm-verify
description: 在浏览器里复验 SISF 的 wasm 产物——构建示例、起本地服务、console/截图/逐帧像素探针取证。当需要验证 wasm 侧行为、验证键盘链路、或确认"画面真的在动"时使用。
---

# SISF wasm 浏览器复验

## 构建 + 起服务

```
cargo xtask web <示例名>
```

- **必须显式传示例名**：缺省只构建 `hello_window`，正在复验的 `<示例>_bg.wasm` 时间戳不会动，
  看着像"改了没生效"。现有示例：`hello_window` `sprite_bench` `tiled_map` `audio_demo`
  `dialog_demo` `windows`（示例自有 URL 参数如 `&sprites=20000`，看对应 `examples/*.rs`）。
- 命令内含 `serve_dir` 死循环（监听 127.0.0.1:8080），**用 Bash 后台方式跑**，或 CI 场景用 `--no-serve`。
- **exit 6（端口被占）不影响验证**：build + wasm-bindgen 已跑完，先起着的静态服务直接读盘上新的
  `web/pkg/*`。确认页面加载的是新产物：`performance.getEntriesByType('resource')` 里 wasm 的
  `transferSize` 与磁盘文件大小对一下（≈ encodedBodySize+头 ⇒ 走网络不是缓存）。
- 改过 JS 胶水（`web/main.js` 或 wasm-bindgen snippets）必须重跑构建 + 页面**硬刷新**
  （`evaluate_script` 里 `location.reload()` 可能仍读缓存胶水——URL 加 `?v=` 或 Ctrl+Shift+R）。

## 访问 URL

```
http://127.0.0.1:8080/index.html?ex=<示例名>&wgl
```

`?ex=` 选 pkg（缺省 hello_window），`&wgl` 强制 WebGL2 回退。本机（RTX 2080 Ti + Chrome）
WebGPU 报 "No available adapters"，**自助复验一律带 `&wgl`**；软件光栅约 42fps，帧时间读数是
被放大的，别当普遍值。WebGPU 腿的结论只能来自用户肉眼开一次页面。

## 取证流程（browser-use MCP，优先自助）

1. `navigate_page` 打开 URL → `list_console_messages` 找启动行与报错。
2. `take_screenshot` 看画面；若返回 `NATIVE_BROWSER_VIEWPORT_UNAVAILABLE`（页面被隐藏），改用像素探针。
3. `evaluate_script` 做逐项断言（见下）。
4. 导航一旦被权限分类器挡下：退回"产物就位 + 精确 URL + 要肉眼确认什么"清单交给用户。
   **任何情况下不要把"未验证"写成"已验证"。**

### console 通道

- wasm 下 `println!` 是**静默丢弃**（不 panic）。新日志必须走 `sisf::app::platform::log_info`
  （桌面 println / wasm console.log）。
- Rust panic 到 JS 只剩 `RuntimeError: unreachable`。`platform/wasm.rs::run` 装了 panic 钩子把
  原文送进 console；若画面黑屏且 console 一行不涨，先在页面里
  `window.addEventListener('error', …)` 再动态 `import('/pkg/x.js?v='+Date.now())` 把 trap 捞出来。

### 键盘链路（无人值守）

向 canvas 与 window 派发合成 `KeyboardEvent('keydown'/'keyup', {code:'ArrowUp'})`，中间用
`setTimeout` 挂住 N 毫秒，即可验证"按住 ⇒ 逐格走 N 格"。

### 逐帧像素探针（量化"在动 / 在抖"）

页面里自注册 rAF，每帧把 canvas `drawImage` 到离屏 2D 上下文再 `getImageData`。判据防假信号：

- **滤空缓冲**：WebGL canvas 未开 `preserveDrawingBuffer`，应用 rAF 之外读到的可能是清空后的
  缓冲。按每帧 distinct 颜色数把空帧滤掉再算（症状："相邻两帧一大半像素变了、任何整数位移都对不齐"）。
- **确认被测对象真的在动**：先确认探针方向落在滚动范围内（例：tiled_map 相机 `cam.x` 到 224 夹边，
  之后画面不滚）。画面冻结且 console 不涨 ⇒ 页面被隐藏过、应用停了帧，先 `location.reload()` 再测。
- 选采样标记先确认它不与隔壁物体撞色（曾被角色上衣/水池同色污染序列）。
- 量"整幅在晃"用相邻两帧在 `dx/dy ∈ -8..8` 最佳整数位移对齐：格点画面对齐后只剩零点几个百分点
  （实测 0.13%），亚像素重采样任何位移都对不齐（实测 17%）。

## 复验收尾

杀掉自己起的 `cargo xtask web`（xtask.exe 会锁 `target/debug/xtask.exe`，卡住后续全部 xtask 闸门）；
杀之前确认不是用户正开着的页面。
