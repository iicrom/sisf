# 批量 quad 渲染器压测（T3）

被测对象：`examples/sprite_bench.rs`。640×480 虚拟分辨率 + `ScaleMode::Fill`（虚拟像素≈物理像素），
精灵尺寸 6–34px 随机、颜色/透明度随机、10% additive、位置与旋转每帧按随机速度积分推进，
纹理只有内置白图（`WHITE_EPOCH`，图集是 T4 的事）。

测量口径：`App::tick` 收到的帧间隔（`platform::now_secs()` 差值，桌面 = `Instant`、
wasm = `performance.now()`），第一帧含 renderer 异步初始化故剔除。
`batch` = 上一帧 `end_frame` 的分批数（一次 `set_bind_group` + 一次 `draw`），
`slot` = 本帧用到的环形缓冲片数。

## 桌面（≥60fps 验收点）

| 项 | 值 |
| --- | --- |
| 日期 | 2026-09-23 |
| OS | Windows 10.0.21996.1 |
| GPU / 后端 | NVIDIA GeForce RTX 2080 Ti / Vulkan（程序启动时打印，见 `[sisf-app] adapter:`） |
| 构建 | `cargo build --release -p sisf --example sprite_bench`（opt-level=3, lto=thin） |
| 工具链 | rustc 1.95.0-nightly (6a979b3e3 2026-02-26) |
| 窗口 | 800×600 物理像素（winit 默认尺寸） |

命令（`<帧数> <sprite 数>`，缺省 `300 100000`）：

```sh
cargo run --release -p sisf --example sprite_bench -- 300 100000
```

实测（每行 = 一次 `RESULT` 汇总，帧数 240–300）：

| sprites | 平均帧时间 | FPS | min–max | 掉帧(>20ms) | batch | slot |
| --- | --- | --- | --- | --- | --- | --- |
| 10 000 | 6.02 ms | 166.0 | 1.50–6.22 | 0 | 2 | 1 |
| 50 000 | 6.20 ms | 161.2 | 5.39–12.20 | 0 | 3 | 2 |
| **100 000** | **13.27 ms** | **75.4** | 11.38–22.36 | 1/300 | 3 | 2 |
| 250 000 | 37.04 ms | 27.0 | 31.48–49.92 | 239/240 | 3 | 2 |
| 500 000 | 80.34 ms | 12.4 | 70.88–118.78 | 239/240 | 3 | 2 |

100 000 精灵原始输出：

```
[sprite_bench] 启动 sprites=100000 limit=300
[sisf-app] adapter: NVIDIA GeForce RTX 2080 Ti (DiscreteGpu, Vulkan)
[sprite_bench] 周期 帧 60 800x600 sprites=100000 batch=3 slot=2 inst=100000 帧时间 avg=12.86ms(77.7fps) min=11.51 max=22.36 累计掉帧(>20ms)=1
[sprite_bench] 周期 帧 120 800x600 sprites=100000 batch=3 slot=2 inst=100000 帧时间 avg=12.82ms(78.0fps) min=11.38 max=14.76 累计掉帧(>20ms)=1
[sprite_bench] 周期 帧 180 800x600 sprites=100000 batch=3 slot=2 inst=100000 帧时间 avg=12.65ms(79.0fps) min=11.44 max=16.46 累计掉帧(>20ms)=1
[sprite_bench] 周期 帧 240 800x600 sprites=100000 batch=3 slot=2 inst=100000 帧时间 avg=13.81ms(72.4fps) min=12.36 max=17.64 累计掉帧(>20ms)=1
[sprite_bench] 周期 帧 300 800x600 sprites=100000 batch=3 slot=2 inst=100000 帧时间 avg=14.18ms(70.5fps) min=12.66 max=15.82 累计掉帧(>20ms)=1
[sprite_bench] RESULT 帧 300 800x600 sprites=100000 batch=3 slot=2 inst=100000 帧时间 avg=13.27ms(75.4fps) min=11.38 max=22.36 累计掉帧(>20ms)=1
```

结论：**10 万 sprite 平均 13.27ms（75.4fps）≥ 60fps，验收通过**。300 帧里唯一一次
>20ms 落在前 60 帧内（那一段 max=22.36ms），其后各周期 max 都不超过 17.64ms。

### 读数解读

- **10 万实例只切 3 个 batch**：合批键只看 `(layer, epoch, blend)`，所以正常情况下
  是 Normal/Add 两批；这里 100k 实例跨了 2 个环形槽（`slot=2`），跨槽处必须断开
  （顶点缓冲不同），于是 3 批 = 3 次 `draw`。50k 时同理（2 槽），10k 单槽 → 2 批。
- **瓶颈是填充率不是 CPU**：50k→100k→250k→500k 帧时间 6.2→13.3→37.0→80.3ms，
  几乎线性于精灵数（即线性于覆盖像素数：100k 时约 83× 全屏 overdraw）。
  单帧 8MB 实例上传（100k×80B）在 75fps 下约 600MB/s，未成为限制。
- **CPU 侧剩余开销**按顺序是：`(BatchKey, Instance)` 104B 元素的 `sort_unstable_by_key`
  （100k 元素）→ 摊平到 `scratch` → 每槽一次 `write_buffer`。T4 接图集后 batch 数会随
  纹理代号上升，届时再按这条曲线复测。

## wasm（`?wgl` 可跑验收点）

`xtask web` 现在接受示例名（缺省仍是 `hello_window`）：

```sh
cargo xtask web sprite_bench
# 浏览器访问 http://127.0.0.1:8080/?ex=sprite_bench&wgl          （WebGL2 回退）
# 数量可调：      http://127.0.0.1:8080/?ex=sprite_bench&wgl&sprites=50000
```

浏览器控制台输出（自动化浏览器，无独占 GPU）：

```
[sprite_bench] 启动 sprites=20000
[sisf-app] adapter: ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11) (Other, Gl)
[sprite_bench] 周期 avg=71.08ms
```

canvas 上确实是 2 万个旋转、加色的 quad（截图复核通过）。

- **71ms/帧不代表引擎性能**：这台环境的 GL 后端是 `Microsoft Basic Render Driver`
  （软件光栅化），且 `xtask web` 是 debug 构建（未过优化）。同一份 shader 在
  有硬件 WebGPU/WebGL2 的浏览器里需人工复测（与 T2 的 wasm 结论同源）。
- 本阶段 wasm 的判据是"**能跑**"：不 panic、adapter 拿到、帧持续出、`sprite_bench`
  的帧时间统计持续打印。顺带修掉了 T2 遗留的真 bug：
  wasm32-unknown-unknown 的 `std::time::Instant::now()` 会 **panic**
  （std 在该 target 无时钟，`sys/time/unsupported.rs`），而 `Runner::do_frame` 每帧调它——
  现在统一走 `platform::now_secs()`（wasm = `performance.now()`）。
  回归验证：`http://127.0.0.1:8080/?wgl`（缺省 `ex=hello_window`）连跑 2400+ tick，
  控制台打出 `tick 600 dt 10.9ms / tick 1200 dt 13.3ms / tick 1800 dt 8ms / tick 2400 dt 5.4ms`，
  即 T2 的 wasm 帧循环此前从未真正跑到过出帧那一步。
