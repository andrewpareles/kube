use std::collections::{HashMap, HashSet};

/// A HashMap that lets you efficiently lookup, insert, and remove entries with both keys *and* values.
pub struct HashMapWithReverse<K, V> {
    map: HashMap<K, V>,
    reverse_map: HashMap<V, HashSet<K>>,
}
impl<K, V> HashMapWithReverse<K, V>
where
    K: Eq + std::hash::Hash + Copy,
    V: Eq + std::hash::Hash + Clone,
{
    /// Create a new HashMapWithReverse
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            reverse_map: HashMap::new(),
        }
    }

    /// Insert a key-value pair into the map, returning the old value if this key moved to a different value.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        let prev_val = self.map.insert(key, value.clone());
        self.reverse_map.entry(value.clone()).or_insert_with(HashSet::new).insert(key);

        // if there was no prev value, we're done
        let Some(prev_val) = prev_val else {
            return None;
        };

        // if the value is the same, we're done
        if prev_val == value {
            return None;
        }

        // remove the old value from the reverse map
        self._removeFromReverseMap(&prev_val, key);
        Some(prev_val)
    }

    /// Remove a key (and its reverse entry) from the map
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let Some(removed_val) = self.map.remove(key) else {
            return None;
        };
        self._removeFromReverseMap(&removed_val, *key);
        Some(removed_val)
    }

    /// Completely remove a value (and all associated keys) from the map
    pub fn remove_all_with_value(&mut self, value: V) {
        let prev_keys = self.reverse_map.remove(&value);
        let Some(prev_keys) = prev_keys else {
            return;
        };
        for key in prev_keys {
            self.map.remove(&key);
        }
    }

    /// Check if the map contains a key
    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// Check if the map contains any occurrence of a value
    pub fn contains_value(&self, value: &V) -> bool {
        self.reverse_map.contains_key(value)
    }

    /// Get the value associated with a key
    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    /// Get the keys associated with a value
    pub fn get_keys(&self, value: &V) -> Option<&HashSet<K>> {
        self.reverse_map.get(value)
    }

    /// Get the number of key-value pairs in the map
    pub fn len(&self) -> usize {
        self.map.len()
    }

    fn _removeFromReverseMap(&mut self, value: &V, key: K) {
        let prev_keys = self.reverse_map.get_mut(value);
        let Some(prev_keys) = prev_keys else {
            return;
        };
        prev_keys.remove(&key);
        if prev_keys.is_empty() {
            self.reverse_map.remove(value);
        }
    }
}

// --------------------------------------------------

pub struct HashMapWithReverse1To1<K, V> {
    map: HashMap<K, V>,
    reverse_map: HashMap<V, K>,
}
impl<K, V> HashMapWithReverse1To1<K, V>
where
    K: Eq + std::hash::Hash + Clone,
    V: Eq + std::hash::Hash + Clone,
{
    /// Create a new HashMapWithReverse
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
            reverse_map: HashMap::new(),
        }
    }

    /// Insert a key-value pair into the map, returning previous key/val pair if it exists
    pub fn insert(&mut self, key: K, value: V) -> Option<(K, V)> {
        let mut prev_pair = None;

        // check for pre-existing value
        let prev_val = self.map.remove_entry(&key);
        if let Some((prev_key, prev_val)) = prev_val {
            prev_pair = Some((prev_key, prev_val));
        }

        self.map.insert(key.clone(), value.clone());
        self.reverse_map.insert(value, key);

        return prev_pair;
    }

    pub fn remove(&mut self, key: &K) -> Option<V> {
        if !self.contains_key(key) {
            return None;
        }
        let Some(removed_value) = self.map.remove(key) else {
            return None;
        };
        self.reverse_map.remove(&removed_value);
        Some(removed_value)
    }

    pub fn remove_value(&mut self, value: &V) -> Option<K> {
        if !self.contains_value(value) {
            return None;
        }
        let Some(removed_key) = self.reverse_map.remove(value) else {
            return None;
        };
        self.map.remove(&removed_key);
        Some(removed_key)
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    pub fn contains_value(&self, value: &V) -> bool {
        self.reverse_map.contains_key(value)
    }

    /// Get the value associated with a key
    pub fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub fn get_value(&self, value: &V) -> Option<&K> {
        self.reverse_map.get(value)
    }

    /// Get the number of key-value pairs in the map
    pub fn len(&self) -> usize {
        self.map.len()
    }
}
