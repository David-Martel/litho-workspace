use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher, Hash, Hasher};

#[derive(Debug)]
enum Slot<K, V> {
    Empty,
    Occupied(K, V),
}

#[derive(Debug)]
pub struct FastHashTable<K, V, S = RandomState> {
    slots: Vec<Slot<K, V>>,
    len: usize,
    build_hasher: S,
}

#[derive(Debug)]
pub struct FastHashSet<K, S = RandomState> {
    table: FastHashTable<K, (), S>,
}

impl<K: Eq + Hash, V> FastHashTable<K, V, RandomState> {
    pub fn new() -> Self {
        Self::with_capacity(16)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        let cap = capacity.next_power_of_two().max(8);
        Self {
            slots: empty_slots(cap),
            len: 0,
            build_hasher: RandomState::new(),
        }
    }
}

impl<K: Eq + Hash, V, S: BuildHasher> FastHashTable<K, V, S> {
    pub fn contains_key(&self, key: &K) -> bool {
        self.get(key).is_some()
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        let mut idx = self.bucket(key);
        loop {
            match &self.slots[idx] {
                Slot::Empty => return None,
                Slot::Occupied(existing_key, value) => {
                    if existing_key == key {
                        return Some(value);
                    }
                }
            }
            idx = (idx + 1) & (self.slots.len() - 1);
        }
    }

    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        if self.should_grow() {
            self.rehash(self.slots.len() * 2);
        }
        self.insert_no_grow(key, value)
    }

    fn insert_no_grow(&mut self, key: K, value: V) -> Option<V> {
        let mut idx = self.bucket(&key);

        loop {
            match &mut self.slots[idx] {
                Slot::Empty => {
                    self.slots[idx] = Slot::Occupied(key, value);
                    self.len += 1;
                    return None;
                }
                Slot::Occupied(existing_key, existing_value) => {
                    if existing_key == &key {
                        return Some(std::mem::replace(existing_value, value));
                    }
                }
            }
            idx = (idx + 1) & (self.slots.len() - 1);
        }
    }

    fn should_grow(&self) -> bool {
        ((self.len + 1) * 10) >= (self.slots.len() * 7)
    }

    fn rehash(&mut self, capacity: usize) {
        let cap = capacity.next_power_of_two().max(8);
        let old_slots = std::mem::replace(&mut self.slots, empty_slots(cap));
        self.len = 0;

        for slot in old_slots {
            if let Slot::Occupied(key, value) = slot {
                let _ = self.insert_no_grow(key, value);
            }
        }
    }

    fn bucket(&self, key: &K) -> usize {
        let mut hasher = self.build_hasher.build_hasher();
        key.hash(&mut hasher);
        (hasher.finish() as usize) & (self.slots.len() - 1)
    }
}

impl<K: Eq + Hash, V> Default for FastHashTable<K, V, RandomState> {
    fn default() -> Self {
        Self::new()
    }
}

impl<K: Eq + Hash> FastHashSet<K, RandomState> {
    pub fn new() -> Self {
        Self::with_capacity(16)
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            table: FastHashTable::with_capacity(capacity),
        }
    }
}

impl<K: Eq + Hash, S: BuildHasher> FastHashSet<K, S> {
    pub fn insert(&mut self, key: K) -> bool {
        self.table.insert(key, ()).is_none()
    }

    pub fn contains(&self, key: &K) -> bool {
        self.table.contains_key(key)
    }
}

impl<K: Eq + Hash> Default for FastHashSet<K, RandomState> {
    fn default() -> Self {
        Self::new()
    }
}

fn empty_slots<K, V>(capacity: usize) -> Vec<Slot<K, V>> {
    std::iter::repeat_with(|| Slot::Empty)
        .take(capacity.max(8))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{FastHashSet, FastHashTable};

    #[test]
    fn table_insert_get_and_replace() {
        let mut table = FastHashTable::with_capacity(4);
        assert!(table.insert("a".to_string(), 1).is_none());
        assert_eq!(table.get(&"a".to_string()), Some(&1));

        assert_eq!(table.insert("a".to_string(), 2), Some(1));
        assert_eq!(table.get(&"a".to_string()), Some(&2));
    }

    #[test]
    fn table_handles_growth() {
        let mut table = FastHashTable::with_capacity(8);
        for i in 0..2000usize {
            let _ = table.insert(format!("k{i}"), i);
        }
        for i in 0..2000usize {
            assert_eq!(table.get(&format!("k{i}")), Some(&i));
        }
    }

    #[test]
    fn set_insert_and_contains() {
        let mut set = FastHashSet::with_capacity(4);
        assert!(set.insert("one".to_string()));
        assert!(!set.insert("one".to_string()));
        assert!(set.contains(&"one".to_string()));
        assert!(!set.contains(&"two".to_string()));
    }
}
