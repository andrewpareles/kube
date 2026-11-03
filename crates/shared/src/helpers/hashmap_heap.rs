use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::hash::Hash;

pub struct HashMapHeap<K, V> {
    map: HashMap<K, V>,           // always accurate
    heap: BinaryHeap<Reverse<K>>, // lazily remove elements when look for min
}

impl<K: Ord + Eq + Hash + Copy, V> HashMapHeap<K, V> {
    pub fn new() -> Self {
        Self {
            heap: BinaryHeap::new(),
            map: HashMap::new(),
        }
    }

    pub fn insert(&mut self, key: K, val: V) -> bool {
        let old_val = self.map.insert(key, val);
        let is_new = old_val.is_none();
        if is_new {
            self.heap.push(Reverse(key));
        }
        is_new
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        self.map.remove(key)
    }

    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn get_min_key(&mut self) -> Option<(K, &V)> {
        loop {
            // if empty, return None
            let Some(&Reverse(top_key)) = self.heap.peek() else {
                return None;
            };

            // if top_key is not in map it was removed, pop it and continue
            if !self.map.contains_key(&top_key) {
                self.heap.pop();
                continue;
            }

            let Some(val) = self.map.get(&top_key) else {
                return None;
            };

            // if top_val is in set, return it
            return Some((top_key, val));
        }
    }
}
