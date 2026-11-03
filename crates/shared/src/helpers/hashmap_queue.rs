// A HashMap that also remembers the order its keys were *first* inserted (not subsequently), so callers can walk it
// oldest-first instead of in HashMap order. Updating a key that is already in the map keeps the place
// in line it already had, so a key that keeps being updated can never push itself back to the front
// nor be pushed to the back by others.
//
// This is what you want whenever a map is drained a few entries at a time under a budget: draining in
// HashMap order means the same entries win every time and the rest wait forever.

use std::collections::{BTreeMap, HashMap};
use std::hash::Hash;

// A key's place in line. It only ever increases, so a smaller number means the key has been waiting longer.
type QueueNum = u64;

pub struct HashMapQueue<K, V> {
    map: HashMap<K, (QueueNum, V)>,          // always accurate
    key_of_queue_num: BTreeMap<QueueNum, K>, // the same keys, ordered by how long they have been in the map
    next_queue_num: QueueNum,
}

impl<K: Ord + Eq + Hash + Copy, V> HashMapQueue<K, V> {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            key_of_queue_num: BTreeMap::new(),
            next_queue_num: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key).map(|(_queue_num, val)| val)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        self.map.get_mut(key).map(|(_queue_num, val)| val)
    }

    // Get the value for a key, first adding it to the back of the line if it is not in the map yet.
    pub fn get_or_insert_with(&mut self, key: K, make_val: impl FnOnce() -> V) -> &mut V {
        let new_queue_num = self.next_queue_num;
        let mut did_insert = false;
        let (_queue_num, val) = self.map.entry(key).or_insert_with(|| {
            did_insert = true;
            (new_queue_num, make_val())
        });
        // only a key that was not in the map yet takes a queue number and joins the back of the line
        if did_insert {
            self.next_queue_num += 1;
            self.key_of_queue_num.insert(new_queue_num, key);
        }
        val
    }

    // Set the value for a key. A key already in the map keeps its place in line.
    pub fn insert(&mut self, key: K, val: V) {
        match self.map.get_mut(&key) {
            Some((_queue_num, current_val)) => *current_val = val,
            None => {
                self.get_or_insert_with(key, || val);
            }
        }
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        let (queue_num, val) = self.map.remove(key)?;
        self.key_of_queue_num.remove(&queue_num);
        Some(val)
    }

    // The keys that have been waiting the longest come first.
    pub fn keys_oldest_first(&self) -> impl Iterator<Item = &K> {
        self.key_of_queue_num.values()
    }
}
