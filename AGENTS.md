# AGENTS.md

给在这个仓库里干活的 agent 的说明书。只写"从代码里读不出来、而且会决定下一步做法"的东西。
模块语义去 `docs/`，别在这里重复。

## 计划的主档是 todo.md

`todo.md` 是设计文档的执行拆解：T0–T12 十二个阶段，**每条子任务后面附最小可证明判据**
（通常是具体单测名，或具体命令 + 具体数字）。开工前先读该阶段的全部子任务和依赖链：

```
T1→T2→T3→T4→{T5,T7}；T5→T6；{T5,T7}→T8；T2→T9；{T6,T8,T9}→T10→T11→T12
```

每阶段末尾的"验收（整段）"通过才提交一次，一次提交 = 一个阶段。证据（命令原文 + 真实数字）
写回 `todo.md` 该阶段的验收条目，并在其下追加"### T<n> 期间发现并修正的事实"。那些事实段是
本仓库最有价值的部分：它们记录的几乎全是"设计文档写的和上游 API 实际行为不一致"的地方。

## 闸门

```sh
cargo xtask fmt --check
cargo xtask clippy            # 宿主 + wasm32 两条 -D warnings
cargo xtask check-all         # 四组 feature 组合，见 xtask/src/main.rs
cargo test --workspace --all-features
cargo xtask wasm-test         # 七条腿，纯逻辑套件
cargo xtask golden            # 离屏像素基线；--bless 重写
```

判据纪律，这几条都是被违反过、然后在账里留下的：

- **闸门证据必须是上面这些 `cargo xtask …` 命令本身实跑绿的输出**，不接受"等价的 raw cargo 命令"
  顶替 —— 那是降级证据。
- **写证据要记真实数字**（测试条数、fps、误差上限）和命令原文，让下一轮能照抄复现。
- **0 个用例通过不是绿**。`cargo xtask wasm-test` 已经在合计为 0 时返回 exit 7，别绕过它。
- 某一腿"环境决定"的红（ubuntu 无 GPU adapter、node 退出断言）要在命令旁边写清它红了意味着什么，
  不许因为"反正是环境问题"就把退出码当噪音吞掉。

`cargo doc --no-deps --workspace --all-features` 输出里没有 `warning:` 是 T12 定的判据，
但它**没有**接进 CI/xtask；攒了 rustdoc 警告就顺手清掉，别等闸门。

## 工具链（动它之前必须先问）

- **动工具链（`rustup install/update/default/target add` 等）之前先征求同意**，别自作主张装/换。
- **本项目不考虑旧工具链兼容。** 判定基准永远跟着本地最新走：
  - 本地工具链 **低于** 项目需要 → 把**本地更新到最新**（`rustup update`）。
  - 项目声明的工具链/MSRV **低于** 本地 → **更新项目**去适配最新（改 `rust-version`、修新 lint/新
    告警），**绝不**为图省事把本地退回旧 nightly 或钉一个 dated nightly 来"复现旧绿"。
  - 后果：新版 clippy/rustc 给既有代码带来的新 lint（如某版本新加的 `chunks_exact_to_as_chunks`、
    新的 cargo manifest 告警）算**项目要修的账**，不是"环境漂移、与我无关"可以跳过的红。
- 这条是被教训出来的：曾为了跑绿擅自装了一堆 dated nightly（1.92/1.93/1.95/…），方向正好反了。

## 定案，不要重开

- **没有 `inspector` feature**。T12 定案不引 egui 调试面板：它当时已经是"开了什么都不发生"的空
  feature，比没有更糟。要做面板属于宿主（`FrameCtx` + `BatchList` 的只读视图够画）。
- **`pump_bookkeeping.mjs --transit 3` 按设计就是 FAIL、exit 1**，不是回归，不可当闸门步骤；
  默认那条 exit 0 才是。
- **wasm 中文字形：运行期从 CDN fetch 全量整包，不入库、不进 `web/assets/manifest.txt`**。
  字形来源归宿主不归引擎；引擎侧只负责取不到时显式报错并退回方块。
- **引擎没有"战斗系统"这一层，也不该有** —— 战斗/菜单是游戏侧 FSM（见 `examples/rpg_demo.rs`）。
- **平台不支持的能力一律 feature 关掉 + 文档标注**，不留空实现（DoD-4）。唯一例外是 T11 显式声明的视频桩。

## 依赖与环境

- 第三方版本全在根 `[workspace.dependencies]` 统一钉，子 crate 一律 `{ workspace = true }`。
- **不引入清单外的依赖**（DoD-3）；确需新增时先在 `todo.md` 的"附：钉版增补"登记理由，再改 `Cargo.toml`。
- 两条上游坑已经写死在注释里，别"顺手清理"：`lofty` 必须保留默认 feature
  `id3v2_compression_support`（0.25.4 关掉它会编译失败），`midly` 用 `default-features=false, ["std"]`。
- `wasm-bindgen-test` 只出现在六个 crate 的 wasm32 `dev-dependencies` 段 —— 移到公共段会让发布产物变大。

## 会咬人的地方

- **wasm32-unknown-unknown 上 `std::time::Instant::now()` 直接 panic**。计时统一走 `platform::now_secs()`
  （wasm = `performance.now()`）。
- **`Instance` 是 80 字节不是 64 字节**，`FrameU::proj` 是 `[[[f32;4];4];8]`（512B）。有单测钉住，改字段前先看它们。
- **winit 0.30 的 `RedrawRequested` 不会因 `ControlFlow::Poll` 自动送达**，三端都用 `Wait` + 帧末 `request_redraw()` 续帧。
- **`App::draw` 里 `push_viewport` 只对下一帧生效**，跟着插值位置走的相机必须放 `App::prepare`，否则每拍拽一次像素。
- **`dep:gilrs` 不生成同名 feature**：只写 `dep:gilrs` 时 `cfg(feature = "gilrs")` 恒假、手柄代码整块蒸发而三端 check 全绿。加依赖时确认它要不要显式出现在 feature 面里。
- **`--no-default-features` 的含义是"不开 native"，不等于"能编 wasm"**：`sisf-app` 的 wasm 腿必须同时带 `--features web`。
- **node 24 on Windows 跑 `sisf-app` 的 wasm 套件会在用例全 ok 之后以 `0xC0000409` 退出**，处理方式是拆成两条腿，不是吞退出码。
- 能透传给 wasm harness 的只有 filter / `--skip` / `--list` / `--exact` / `--nocapture` / `--format`；`--quiet`、`--test-threads=1` 会被判 exit 1。
- **正在跑的 `cargo xtask web` 锁住 `target/debug/xtask.exe`**：只要 xtask 源码有改动，之后的
  `golden|check-all|clippy` 都在链接阶段报 `failed to remove ... Access is denied. (os error 5)`。
  用 `tasklist //FI "IMAGENAME eq xtask.exe"` 查到 PID 再 `taskkill //PID <pid> //F` —— **杀之前先问**，
  那个静态服务很可能是用户开着的 wasm 复验页面。

## 复验 wasm

`cargo xtask web <示例名>` 起在 `127.0.0.1:8080`，`?ex=<示例>&wgl` 走 WebGL2。复验要拿证据而不是
"页面看着在动"：console + 截图 + 逐帧像素探针；探针要过滤空缓冲，并且**确认画面真的在变**
（panic 在浏览器里会被吞成"画面不动"）。细节见 `skills` 里的 `sisf-wasm-verify`。

## Skills

`.qoder/skills/` 下有七份按主题切的速查，比从这里猜更快：
`sisf-app-anatomy`（新 App 骨架/feature 矩阵）、`sisf-sprites-text`、`sisf-assets-audio`、
`sisf-events-input`、`sisf-gates`、`sisf-wasm-verify`、`sisf-stage-close`（阶段推进与验收证据格式）。

## 文档写作的一条硬规矩

写文档最容易犯的错是**发明一个更顺手的 API 名**。复核方式不是扫一眼，是：抽出文档里每个反引号
标识符 → 按最后一段 grep `crates/**/*.rs` → 未命中的必须逐个解释。"看着像"不算证据。
同理：**只在一次输出里见过、重跑不复现的失败，不能当事实写下来**。
