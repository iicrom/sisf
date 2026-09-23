// wasm 引导：`?ex=<示例名>` 决定加载哪个 pkg（缺省 hello_window）。
// `#[wasm_bindgen(start)]` 使 init() 自动跑实现在 `main` 里。
const example = new URLSearchParams(location.search).get("ex") ?? "hello_window";

import(`./pkg/${example}.js`)
  .then((mod) => mod.default())
  .catch((e) => {
    console.error(`SISF wasm 启动失败 (${example}):`, e);
  });
