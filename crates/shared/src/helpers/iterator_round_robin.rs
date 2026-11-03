// Round-robins across N iterators, yielding (index, item) until all are exhausted.

pub struct IteratorRoundRobin<'a, const N: usize, T> {
    iterator_of_idx: [Box<dyn Iterator<Item = T> + 'a>; N],
    curr_val_of_idx: [Option<T>; N],
    next_idx: usize,
}

impl<'a, const N: usize, T> IteratorRoundRobin<'a, N, T> {
    pub fn new(mut iterators: [Box<dyn Iterator<Item = T> + 'a>; N]) -> Self {
        let curr_val_of_idx = std::array::from_fn(|index| iterators[index].next());
        Self {
            iterator_of_idx: iterators,
            curr_val_of_idx,
            next_idx: 0,
        }
    }
}

impl<'a, const N: usize, T> Iterator for IteratorRoundRobin<'a, N, T> {
    type Item = (usize, T);

    fn next(&mut self) -> Option<(usize, T)> {
        for _ in 0..N {
            let index = self.next_idx;
            self.next_idx = (self.next_idx + 1) % N;

            if let Some(value) = self.curr_val_of_idx[index].take() {
                self.curr_val_of_idx[index] = self.iterator_of_idx[index].next();
                return Some((index, value));
            }
        }
        None
    }
}
