use std::collections::VecDeque;

pub struct SortedVecDeque<T: Ord> {
    deque: VecDeque<T>,
}
impl<T: Ord> SortedVecDeque<T> {
    pub fn with_capacity(init_capacity: usize) -> Self {
        Self {
            deque: VecDeque::with_capacity(init_capacity),
        }
    }

    pub fn new() -> Self {
        Self { deque: VecDeque::new() }
    }

    pub fn iter(&self) -> std::collections::vec_deque::Iter<'_, T> {
        self.deque.iter()
    }

    // pub fn insert(&mut self, item: T) {
    //     let insert_idx = self.deque.iter().position(|existing| existing > &item).unwrap_or(self.deque.len());
    //     self.deque.insert(insert_idx, item);
    // }

    pub fn rinsert(&mut self, item: T) {
        let insert_idx = self.deque.iter().rposition(|existing| existing <= &item).map(|idx| idx + 1).unwrap_or(0);
        self.deque.insert(insert_idx, item);
    }

    pub fn remove(&mut self, item: &T) -> Option<T> {
        let idx = self.deque.iter().position(|existing| existing == item)?;
        let removed = self.deque.remove(idx);
        removed
    }

    pub fn len(&self) -> usize {
        self.deque.len()
    }

    pub fn front(&self) -> Option<&T> {
        self.deque.front()
    }

    pub fn pop_front(&mut self) -> Option<T> {
        self.deque.pop_front()
    }
}
