---
name: sisf-gates
description: 在 sisf 工作区根目录运行或解释验证闸门（check-all / clippy / fmt / test / golden / wasm-test）。当需要"跑闸门、验收、确认绿"时使用。
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
| `cargo xtask clippy` | 四条腿：宿主 `--workspace --all-targets`、宿主 `--workspace --all-targets --all-features`、wasm32 `web_features()`、wasm32 `web`（都是 `-D warnings`） | 四腿 0 warning |
| `cargo xtask fmt --check` | `cargo fmt --all --check` | 净 |
| `cargo test --workspace --all-features` | 全部单测 + GPU 读回集成测试 | 全绿，记下总条数。**非 Windows 要加 `-- --skip golden`**（见下方 golden 陷阱） |
| `cargo xtask golden` | `sisf-sprite/golden_sprite`、`sisf-text/golden_text` 离屏逐通道比对 | 见下方 golden 陷阱 |
| `cargo xtask wasm-test` | 纯逻辑套件真跑在 wasm32（node driver） | 十条腿（`sisf-app` 与 `sisf-event` 各占两条），本轮合计 473：asset 5·audio 19·render 34·sprite 51·app 15+10·event 61·event(js) 11·rpgmk 27·rpgmkmv 240 |

`cargo xtask android`（无参数）只是工具链体检，不是闸门。

## 已知陷阱

- **xtask.exe 被锁**：本机若有正在运行的 `cargo xtask web …`（静态服务占 :8080）会锁住
  `target/debug/xtask.exe`，改过 xtask 源码后所有 xtask 闸门在链接期报
  `failed to remove ... Access is denied. (os error 5)`。
  排查：`tasklist //FI "IMAGENAME eq xtask.exe"` + 查命令行确认是哪个示例；
  **杀掉前先向用户确认**（服务可能是用户正开着的复验页面），清掉后跑真闸门，不要改用 raw cargo 绕过。
- **golden：两条基线的口径不一致，而基线只属于 Windows**。`sisf-text/tests/golden_text.rs` 整个文件挂了
  `#![cfg(windows)]`（非 Windows ⇒ 0 用例），而 `sisf-sprite/tests/golden_sprite.rs` **没挂**（它的文件头
  那句"所以整个文件 `#![cfg(windows)]`"与实现不符，账在 `todo.md`「T11 续 事实 4」与「M 系列补账 事实 2」），
  所以在 macOS/Linux 上它会真编译、真找 adapter、真比对。后果：
  ① macOS（本轮实测）红在跨后端取整——`sprite_scene_golden` 报单通道误差 2 > TOL 1，首处 `(35,226)`；
  **不许 `--bless`**（会把 Windows 基线覆盖成 mac 输出、反过来弄坏 CI 的 golden job）。
  ② ubuntu CI 的 test 腿会红在 `.expect("离屏渲染器需要可用 adapter（DX12/Vulkan）")` ⇒ 那里的
  `test_args` 已经带 `--skip golden`。③ 非 Windows 上的
  `cargo test --workspace --all-features`（不带 skip）会被同一条顶成 exit 101。
  基线刷新只应在 Windows 上做：`cargo xtask golden --bless`。
- **`cargo xtask check-all` 盖不到 `sisf-rpgmkmv`**：四组组合全是 `-p sisf`，而门面不依赖那颗 crate
  ⇒ 它的覆盖来自 clippy 的两条 `--workspace` 腿、`cargo test --workspace --all-features`，以及
  wasm-test 里那条 `--features js,web` 腿。别把"check-all 绿"当兼容层被编过了。
- **wasm-test 的 0 通过检查**：套件能被跑到要求其 `mod tests` 里有
  `#[cfg(target_arch = "wasm32")] use wasm_bindgen_test::wasm_bindgen_test as test;`，
  否则 `#[test]` 没人执行、退出码却是 0。带透传参数时该检查关闭（0 可能只是过滤没命中）。
- **sisf-app 拆腿是已知的事**：全量 25 用例跑完后 node（24, Windows）退出时撞 libuv 断言、
  退出码 `0xC0000409`——用例全过但 harness 崩。xtask 已把 app 拆成两腿绕开；
  **不要**因此把退出码当噪音吞掉。
- 只跑 `cargo xtask web` 不带示例名时缺省只构建 `hello_window`，与闸门无关但易误判"重跑刷了产物"。
