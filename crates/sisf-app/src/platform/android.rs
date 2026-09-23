//! Android 接入：`android-activity` 0.6（native-activity）+ winit `EventLoopBuilderExtAndroid`。
//!
//! 真机构建/安装步骤见 `docs/android.md`。surface 生命周期规则：
//! `Resumed`（winit→`Runner::resumed`）建 surface；`Suspended` 释放 `Surface` 与
//! GPU 资源（`Runner::sync_suspend` 在 android 上把 renderer 置 `None`）。

use std::error::Error;

use android_activity::AndroidApp;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::android::EventLoopBuilderExtAndroid;

use crate::app::App;
use crate::config::AppConfig;
use crate::runner::Runner;

/// 由 cdylib 的 `android_main` 调用（模板见 `docs/android.md`）。
pub fn run_with_android_app<A: App + 'static>(
    app: A,
    config: AppConfig,
    android_app: AndroidApp,
) -> Result<(), Box<dyn Error>> {
    let event_loop = EventLoop::builder().with_android_app(android_app).build()?;
    event_loop.set_control_flow(ControlFlow::Wait);
    let mut runner = Runner::new(app, config);
    let result = event_loop.run_app(&mut runner);
    drop(runner);
    result?;
    Ok(())
}
