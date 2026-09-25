//! `post_custom`：宿主 / JS / 移植层 → 引擎总线的单向通道（T9）。
//!
//! 存在的理由不是"现在有人要发消息"，而是**给那条消息留一个已经定型的口子**：T10 的事件
//! 解释器有 `Command::Custom`，QuickJS 与 RPG Maker 移植层都要往引擎里塞结构化消息，而它
//! 们的调用点都不在 `Runner` 手里（JS 那边连 Rust 对象都拿不到）。所以这里用一条全局队列，
//! `Runner` 每次派发前取空、按到达顺序追加到总线尾部（见 [`EngineBus`](sisf_event::EngineBus)）。
//!
//! 载荷是 `serde_json::Value` 而不是 `Vec<u8>`：跨 wasm 边界时它对应的就是 JS 侧那个对象，
//! 不必在两边各抄一套编解码。
//!
//! 本阶段只提供通道与顺序保证，**不**订阅、不路由、不去重 —— 那些是游戏侧的事。

use std::collections::VecDeque;
use std::sync::Mutex;

use sisf_event::{EngineBus, EngineEvent};

/// 一条消息：主题号 + 任意 JSON 载荷。
type Message = (u32, serde_json::Value);

/// 可单测的队列本体（全局那份见 [`GLOBAL`]，包一层是为了能在测试里造互不干扰的实例）。
#[derive(Default)]
pub struct CustomInbox {
    queue: VecDeque<Message>,
}

impl CustomInbox {
    pub fn post(&mut self, channel: u32, payload: serde_json::Value) {
        self.queue.push_back((channel, payload));
    }

    /// 按到达顺序取空并追加进总线（总线已有内容在前 —— 队列只认进队顺序，不认来源）。
    pub fn drain_into(&mut self, bus: &mut EngineBus) {
        while let Some((channel, payload)) = self.queue.pop_front() {
            bus.push(EngineEvent::Custom(channel, payload));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.queue.is_empty()
    }
}

static GLOBAL: Mutex<CustomInbox> = Mutex::new(CustomInbox {
    queue: VecDeque::new(),
});

/// 从任意线程 / JS 胶水投一条消息。锁定失败（别的线程 panic 在队列里）时把消息丢掉并回报，
/// 不 panic —— 丢一条宿主消息总比把整个引擎带崩好。
pub fn post_custom(channel: u32, payload: serde_json::Value) -> Result<(), String> {
    let mut inbox = GLOBAL
        .lock()
        .map_err(|e| format!("custom 通道锁不可用: {e}"))?;
    inbox.post(channel, payload);
    Ok(())
}

/// `Runner` 每次派发前调用。锁不住就当这批还没到（下一条消息会一起带出来）。
pub(crate) fn drain_global_into(bus: &mut EngineBus) {
    if let Ok(mut inbox) = GLOBAL.lock() {
        inbox.drain_into(bus);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 验收点名的顺序：先进先出，且真的落到总线里。
    #[test]
    fn posts_drain_in_arrival_order() {
        let mut inbox = CustomInbox::default();
        assert!(inbox.is_empty());
        inbox.post(1, json!("first"));
        inbox.post(2, json!({"scene": "title"}));
        let mut bus = EngineBus::new();
        bus.push(EngineEvent::key("A", sisf_event::KeyKind::Press, None));
        inbox.drain_into(&mut bus);

        let events = bus.drain();
        assert_eq!(
            events,
            vec![
                EngineEvent::key("A", sisf_event::KeyKind::Press, None),
                EngineEvent::Custom(1, json!("first")),
                EngineEvent::Custom(2, json!({"scene": "title"})),
            ],
            "已排队的裸事件在前，消息按到达顺序追加在后"
        );
        assert!(inbox.is_empty());
    }

    /// 取空之后再投要成为新的一批，不许串回上一批。
    #[test]
    fn a_second_post_round_starts_a_fresh_batch() {
        let mut inbox = CustomInbox::default();
        let mut bus = EngineBus::new();
        inbox.post(1, json!("a"));
        inbox.drain_into(&mut bus);
        assert_eq!(bus.len(), 1);
        inbox.post(1, json!("b"));
        inbox.drain_into(&mut bus);
        assert_eq!(bus.len(), 2);
        assert_eq!(bus.peek()[1], EngineEvent::Custom(1, json!("b")));
    }

    /// 全局那份要真的能投出去（`post_custom` 是宿主唯一的入口，坏了没人发现）。
    #[test]
    fn the_global_channel_round_trips() {
        post_custom(9, json!({"from": "test"})).unwrap();
        let mut bus = EngineBus::new();
        drain_global_into(&mut bus);
        assert!(
            bus.peek()
                .contains(&EngineEvent::Custom(9, json!({"from": "test"}))),
            "全局通道没把消息带出来"
        );
        // 别的测试可能同时在用全局通道，这里只保证不残留自己那条。
        let mut empty = EngineBus::new();
        drain_global_into(&mut empty);
        assert!(
            !empty
                .peek()
                .contains(&EngineEvent::Custom(9, json!({"from": "test"})))
        );
    }
}
