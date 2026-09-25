//! 应用配置。

use sisf_event::ActionMap;
use sisf_render::Rgba;

/// 虚拟分辨率到物理窗口的映射方式。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScaleMode {
    /// 等比缩放并留边（letterbox）。
    Fit,
    /// 拉伸铺满（可能变形）。
    Fill,
    /// 整数倍缩放，居中留边；窗口太小时退化为 1:1。
    IntegerFit,
}

#[derive(Clone, Debug)]
pub struct AppConfig {
    pub title: String,
    /// 逻辑分辨率（RPG Maker 惯例默认 544×416）。
    pub virtual_size: (u32, u32),
    pub scale_mode: ScaleMode,
    /// letterbox 边条颜色。
    pub letterbox: Rgba,
    /// 逻辑 tick 频率（T8 的固定步长累加器使用）。
    pub tick_hz: f64,
    /// 每帧清屏色。
    pub clear: Rgba,
    /// 命名动作表（T9）。游戏侧读输入只认这里的动作名，不认裸按键。
    ///
    /// 缺省是 RPG Maker 惯例键位；T11 会把工程 `System.json::keyMapping` 翻译成同一种
    /// [`ActionMap`] 盖上来，玩家自定义键位则走 `ActionMap::from_ron`。
    pub actions: ActionMap,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            title: "SISF".to_owned(),
            virtual_size: (544, 416),
            scale_mode: ScaleMode::Fit,
            letterbox: [0.0, 0.0, 0.0, 1.0],
            tick_hz: 60.0,
            clear: [0.0, 0.35, 0.6, 1.0],
            actions: ActionMap::rpg_maker(),
        }
    }
}
