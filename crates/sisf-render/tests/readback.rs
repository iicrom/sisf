//! GPU 清屏读回：证明 `Renderer::readback_clear` 的颜色换算与呈现通路正确（无需窗口）。
//! wasm 无 headless adapter，故仅 native 运行。

#[cfg(not(target_arch = "wasm32"))]
#[test]
fn readback_clear_matches_srgb_bytes() {
    // 0.35 -> 89, 0.6 -> 153（sRGB 数值域直接量化到 8bit）。
    let expected: [u8; 4] = [0, 89, 153, 255];
    let got = pollster::block_on(sisf_render::Renderer::readback_clear([0.0, 0.35, 0.6, 1.0]))
        .expect("readback_clear 需要可用 adapter");
    assert!(
        expected
            .iter()
            .zip(&got)
            .all(|(e, a)| (*e as i32 - *a as i32).abs() <= 1),
        "清屏读回 {got:?} != 期望 {expected:?}"
    );
}
