// hello_window 的 wasm 引导。`#[wasm_bindgen(start)]` 使 init() 自动跑 main()。
import init from "./pkg/hello_window.js";

init().catch((e) => {
  console.error("SISF wasm 启动失败:", e);
});
