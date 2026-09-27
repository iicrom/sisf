---
name: sisf-gates
description: 在 D:\Projects\sisf 运行或解释验证闸门（check-all / clippy / fmt / test / golden / wasm-test）。当需要"跑闸门、验收、确认绿"时使用。
---

# SISF 验证闸门

## 判据纪律（先读这条）

- 闸门证据必须是下面这些 `cargo xtask …` 命令**本身实跑绿**的输出，不接受"等价 raw cargo 命令"顶替——那是降级证据。
- 写证据时记真实数字（测试条数、通过数、fps、误差上限）与命令原文，让下一条会话能照抄复现。
- 0 个用例通过**不是绿**。发现计数对不上时，在后续阶段点名修正，不回改历史条目。

## 闸门命令

在工作区根目录逐条实跑，任一非零退出即整段失败：

| 命令 | 覆盖内容 | 判据 |
|---|---|---|
| `cargo xtask check-all` | 4 个 feature 组合：宿主 `--all-features`、wasm32 `web`、android `native`、wasm32 音频全开（`web,ogg,mp3,wav,midi`） | 四腿退出码全 0，无 warning |
| `cargo xtask clippy` | workspace `-D warnings` + wasm 腿（含 `--all-targets`，示例/测试也进闸门） | 两腿 0 warning |
| `cargo xtask fmt --check` | `cargo fmt --all --check` | 净 |
| `cargo test --workspace --all-features` | 全部单测 + GPU 读回集成测试 | 全绿，记下总条数 |
| `cargo xtask golden` | `sisf-sprite/golden_sprite`、`sisf-text/golden_text` 离屏逐通道比对 | 见下方 golden 陷阱 |
| `cargo xtask wasm-test` | 纯逻辑套件真跑在 wasm32（node driver） | 每腿 `passed > 0` |

`cargo xtask android`（无参数）只是工具链体检，不是闸门。

## 已知陷阱

- **xtask.exe 被锁**：本机若有正在运行的 `cargo xtask web …`（静态服务占 :8080）会锁住
  `target/debug/xtask.exe`，改过 xtask 源码后所有 xtask 闸门在链接期报
  `failed to remove ... Access is denied. (os error 5)`。
  排查：`tasklist //FI "IMAGENAME eq xtask.exe"` + 查命令行确认是哪个示例；
  **杀掉前先向用户确认**（服务可能是用户正开着的复验页面），清掉后跑真闸门，不要改用 raw cargo 绕过。
- **golden 只在 Windows 真比对**：测试整个 `#![cfg(windows)]`，其它平台编译成 0 用例且通过。
  基线刷新用 `cargo xtask golden --bless`（首次落图也用 --bless）。
- **wasm-test 的 0 通过检查**：套件能被跑到要求其 `mod tests` 里有
  `#[cfg(target_arch = "wasm32")] use wasm_bindgen_test::wasm_bindgen_test as test;`，
  否则 `#[test]` 没人执行、退出码却是 0。带透传参数时该检查关闭（0 可能只是过滤没命中）。
- **sisf-app 拆腿是已知的事**：全量 25 用例跑完后 node（24, Windows）退出时撞 libuv 断言、
  退出码 `0xC0000409`——用例全过但 harness 崩。xtask 已把 app 拆成两腿绕开；
  **不要**因此把退出码当噪音吞掉。
- 只跑 `cargo xtask web` 不带示例名时缺省只构建 `hello_window`，与闸门无关但易误判"重跑刷了产物"。
