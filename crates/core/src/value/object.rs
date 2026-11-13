use super::Value;
use std::collections::BTreeMap;

/// GraphQL object that keeps the first key insert order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Object {
    entries: Vec<(String, Value)>,
    indices: BTreeMap<String, usize>,
}

impl Object {
    /// Creates an empty object.
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            indices: BTreeMap::new(),
        }
    }

    /// Create an object with preallocated field capacity.
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Vec::with_capacity(capacity),
            indices: BTreeMap::new(),
        }
    }

    /// Returns the number of entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Return whether the object contains no fields.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn capacity(&self) -> usize {
        self.entries.capacity()
    }

    /// Returns the value for `key`.
    pub fn get(&self, key: &str) -> Option<&Value> {
        let index = *self.indices.get(key)?;
        self.entries.get(index).map(|(_, value)| value)
    }

    /// Replaces the value for `key`.
    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        let index = *self.indices.get(key)?;
        self.entries.get_mut(index).map(|(_, value)| value)
    }

    /// Returns whether the object contains `key`.
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// Add value. The existing key is replaced without changing its position.
    pub fn insert(&mut self, key: impl Into<String>, value: impl Into<Value>) -> Option<Value> {
        let key = key.into();
        let value = value.into();
        if let Some(index) = self.indices.get(&key).copied() {
            let current = &mut self.entries[index].1;
            return Some(std::mem::replace(current, value));
        }
        self.indices.insert(key.clone(), self.entries.len());
        self.entries.push((key, value));
        None
    }

    /// Removes `key` while preserving the order of remaining entries.
    pub fn remove(&mut self, key: &str) -> Option<Value> {
        let index = self.indices.remove(key)?;
        let value = self.entries.remove(index).1;
        // Remove is not decode hot path.
        for entry_index in self.indices.values_mut() {
            if *entry_index > index {
                *entry_index -= 1;
            }
        }
        Some(value)
    }

    /// Returns entries in insertion order.
    pub fn iter(&self) -> Iter<'_> {
        Iter(self.entries.iter())
    }
}

/// Borrowed iterator over [`Object`] entries.
#[derive(Clone, Debug)]
pub struct Iter<'a>(std::slice::Iter<'a, (String, Value)>);

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a str, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|(key, value)| (key.as_str(), value))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl ExactSizeIterator for Iter<'_> {}
impl std::iter::FusedIterator for Iter<'_> {}

/// `Object` owned entry iterator.
#[derive(Debug)]
pub struct IntoIter(std::vec::IntoIter<(String, Value)>);

impl Iterator for IntoIter {
    type Item = (String, Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.0.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}

impl ExactSizeIterator for IntoIter {}
impl std::iter::FusedIterator for IntoIter {}

impl<'a> IntoIterator for &'a Object {
    type Item = (&'a str, &'a Value);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

impl IntoIterator for Object {
    type Item = (String, Value);
    type IntoIter = IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter(self.entries.into_iter())
    }
}

impl<K, V> FromIterator<(K, V)> for Object
where
    K: Into<String>,
    V: Into<Value>,
{
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        let mut object = Self::new();
        object.extend(iter);
        object
    }
}

impl<K, V> Extend<(K, V)> for Object
where
    K: Into<String>,
    V: Into<Value>,
{
    fn extend<T: IntoIterator<Item = (K, V)>>(&mut self, iter: T) {
        for (key, value) in iter {
            self.insert(key, value);
        }
    }
}
