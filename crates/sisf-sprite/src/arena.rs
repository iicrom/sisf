//! 精灵集合：`Vec` 槽位 + free list + generation 句柄。
//!
//! 不上 ECS：扁平 z 排序的 2D 批渲染只要一个缓存友好的连续数组，句柄用
//! `(slot, generation)` 保证"槽被复用后旧句柄自动失效"，不需要借用检查器之外的别名跟踪。

use crate::sprite::Sprite;

/// arena 句柄：`(槽号, 代号)`。`spawn` 发出、`get`/`get_mut`/`despawn` 消费。
///
/// 槽位复用时代号递增，所以拿着一个已失效的 `SpriteRef` 去 `get` 只会得到 `None`，
/// 不会摸到复用后的新精灵。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SpriteRef(pub u32, pub u32);

impl SpriteRef {
    pub const fn slot(self) -> u32 {
        self.0
    }

    /// 发出本句柄时代所在的代号（2024 版 `gen` 是保留字，故全名）。
    pub const fn generation(self) -> u32 {
        self.1
    }
}

struct Slot {
    sprite: Sprite,
    /// 本槽当前这一代；与 `SpriteRef.1` 不符即为陈旧句柄。
    generation: u32,
    alive: bool,
}

/// 精灵数组。`Vec` 只增不减（回收靠 free list），所以 `get` 的槽号永远有效。
#[derive(Default)]
pub struct SpriteArena {
    slots: Vec<Slot>,
    /// 空闲槽号栈（后进先出：刚死掉的槽最快被复用，热数据留在缓存里）。
    free: Vec<u32>,
    /// 下一次 `spawn` 使用的代号。
    generation: u32,
    live: usize,
}

impl SpriteArena {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            slots: Vec::with_capacity(capacity),
            free: Vec::with_capacity(capacity),
            ..Default::default()
        }
    }

    /// 放入一个精灵，返回句柄。
    pub fn spawn(&mut self, sprite: Sprite) -> SpriteRef {
        self.generation = self.generation.wrapping_add(1);
        let generation = self.generation;
        let index = match self.free.pop() {
            Some(i) => {
                let slot = &mut self.slots[i as usize];
                slot.sprite = sprite;
                slot.generation = generation;
                slot.alive = true;
                i
            }
            None => {
                self.slots.push(Slot {
                    sprite,
                    generation,
                    alive: true,
                });
                (self.slots.len() - 1) as u32
            }
        };
        self.live += 1;
        SpriteRef(index, generation)
    }

    /// 移除并归还精灵本体；句柄已失效时返回 `None`（幂等）。
    pub fn despawn(&mut self, r: SpriteRef) -> Option<Sprite> {
        // 拆字段借用：先判有效性，再改 `live`/`free`，不必先整块借走 `self`。
        let Self {
            slots, live, free, ..
        } = self;
        let slot = slots.get_mut(r.slot() as usize)?;
        if slot.generation != r.generation() || !slot.alive {
            return None;
        }
        slot.alive = false;
        *live -= 1;
        free.push(r.slot());
        // 留在槽里的 sprite 是"回收站"，等下次 spawn 覆盖。
        Some(std::mem::take(&mut slot.sprite))
    }

    pub fn get(&self, r: SpriteRef) -> Option<&Sprite> {
        self.slot(r).map(|s| &s.sprite)
    }

    pub fn get_mut(&mut self, r: SpriteRef) -> Option<&mut Sprite> {
        self.slot_mut(r).map(|s| &mut s.sprite)
    }

    /// 句柄是否仍指向一个活精灵。
    pub fn is_alive(&self, r: SpriteRef) -> bool {
        self.slot(r).is_some()
    }

    /// 活精灵数。
    pub fn len(&self) -> usize {
        self.live
    }

    pub fn is_empty(&self) -> bool {
        self.live == 0
    }

    /// 槽位总数（含待复用的死槽）：容量增长的直观读数。
    pub fn slots(&self) -> usize {
        self.slots.len()
    }

    /// 按槽号顺序遍历活精灵（槽序即 `insertion` 的相对次序来源）。
    pub fn iter_live(&self) -> impl Iterator<Item = (SpriteRef, &Sprite)> {
        self.slots
            .iter()
            .enumerate()
            .filter(|(_, s)| s.alive)
            .map(|(i, s)| (SpriteRef(i as u32, s.generation), &s.sprite))
    }

    /// 清空（不保留 free list：arena 重建时用，日常增删走 `despawn`）。
    pub fn clear(&mut self) {
        self.slots.clear();
        self.free.clear();
        self.live = 0;
    }

    /// 只在句柄仍指向**活**精灵时给出槽。
    fn slot(&self, r: SpriteRef) -> Option<&Slot> {
        self.slots
            .get(r.slot() as usize)
            .filter(|s| s.generation == r.generation() && s.alive)
    }

    fn slot_mut(&mut self, r: SpriteRef) -> Option<&mut Slot> {
        let slots = &mut self.slots;
        let i = usize::try_from(r.slot()).ok()?;
        let slot = slots.get_mut(i)?;
        (slot.generation == r.generation() && slot.alive).then_some(slot)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprite::{Layer, Vec2};

    fn at(x: f32) -> Sprite {
        Sprite {
            pos: Vec2::new(x, 0.0),
            ..Default::default()
        }
    }

    #[test]
    fn spawn_get_despawn_round_trip() {
        let mut a = SpriteArena::new();
        let r = a.spawn(at(3.0));
        assert_eq!(a.len(), 1);
        assert!(a.is_alive(r));
        assert_eq!(a.get(r).unwrap().pos.x, 3.0);
        a.get_mut(r).unwrap().pos.y = 9.0;
        assert_eq!(a.get(r).unwrap().pos.y, 9.0);
        assert_eq!(a.despawn(r).unwrap().pos.x, 3.0);
        assert_eq!(a.len(), 0);
        assert!(!a.is_alive(r));
        assert!(a.get(r).is_none());
        // 重复 despawn 幂等。
        assert!(a.despawn(r).is_none());
    }

    #[test]
    fn reused_slot_invalidates_old_handle() {
        let mut a = SpriteArena::new();
        let old = a.spawn(at(1.0));
        a.despawn(old);
        let fresh = a.spawn(at(2.0));
        // 槽被复用，代号不同。
        assert_eq!(fresh.slot(), old.slot());
        assert_ne!(fresh.generation(), old.generation());
        assert!(a.get(old).is_none(), "旧句柄必须因 generation 失效");
        assert!(!a.is_alive(old));
        assert_eq!(a.get(fresh).unwrap().pos.x, 2.0);
        // 拿旧句柄写也不得碰到新精灵。
        assert!(a.get_mut(old).is_none());
        assert_eq!(a.len(), 1);
    }

    #[test]
    fn stale_handle_cannot_despawn_new_sprite() {
        let mut a = SpriteArena::new();
        let first = a.spawn(at(1.0));
        a.despawn(first);
        let second = a.spawn(at(2.0));
        assert!(a.despawn(first).is_none());
        assert!(a.is_alive(second));
    }

    #[test]
    fn free_list_recycles_before_growing() {
        let mut a = SpriteArena::with_capacity(4);
        let refs: Vec<_> = (0..4).map(|i| a.spawn(at(i as f32))).collect();
        assert_eq!(a.slots(), 4);
        for r in &refs[0..2] {
            a.despawn(*r);
        }
        let reused = a.spawn(at(9.0));
        assert_eq!(a.slots(), 4, "有死槽就不该长出新槽");
        assert!(refs[0].slot() == reused.slot() || refs[1].slot() == reused.slot());
        assert_eq!(a.len(), 3);
    }

    #[test]
    fn iter_live_skips_dead_slots_in_slot_order() {
        let mut a = SpriteArena::new();
        let mut refs = Vec::new();
        for i in 0..5 {
            let mut s = at(i as f32);
            s.layer = Layer(if i % 2 == 0 { 0 } else { 1 });
            refs.push(a.spawn(s));
        }
        a.despawn(refs[1]);
        a.despawn(refs[3]);
        let live: Vec<u32> = a
            .iter_live()
            .map(|(r, s)| {
                assert!(std::ptr::eq(a.get(r).unwrap(), s));
                s.pos.x as u32
            })
            .collect();
        assert_eq!(live, vec![0, 2, 4]);
        assert_eq!(a.len(), 3);
        a.clear();
        assert!(a.is_empty());
        assert_eq!(a.slots(), 0);
    }
}
