use arc_swap::ArcSwap;
use std::sync::Arc;

// a hashmap whose keys are cheap, and whose values can be shared among threads (Arc<V>)
// values are immutable but can be changed by adding a new key-val pair
// there is no awaiting in reads and writes, they just take O(log n) time

// after inserting, `get`s on the same thread always read the latest value. `get`s on other threads can take a little while.

// if you want O(1) get and O(n) insert, use a regular std::HashMap.

// typically, wrap this in Arc<>
pub struct MultiThreadHashMap<K, V> {
    map_swap: ArcSwap<im::HashMap<K, Arc<V>>>,
}

impl<K, V> MultiThreadHashMap<K, V>
where
    K: std::hash::Hash + Eq + Clone,
{
    pub fn new() -> Self {
        let map = im::HashMap::new();
        let map_swap = ArcSwap::from_pointee(map);
        Self { map_swap }
    }

    // O(log n)
    pub fn get(&self, key: &K) -> Option<Arc<V>> {
        let map = self.map_swap.load(); // O(1)
        let val_arc = map.get(key); // O(log n)
        return val_arc.cloned();
    }

    // O(log n)
    pub fn insert(&self, key: K, val: V) {
        let val_arc = Arc::new(val);
        self.map_swap.rcu(|map| {
            let mut map = im::HashMap::clone(map); // O(1)
            map.insert(key.clone(), val_arc.clone()); // O(log n)
            map
        });
    }

    // O(log n)
    pub fn remove(&self, key: &K) {
        self.map_swap.rcu(|map| {
            let mut map = im::HashMap::clone(map); // O(1)
            map.remove(key); // O(log n)
            map
        });
    }
}
