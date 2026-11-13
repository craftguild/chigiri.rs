//! Source-ordered descriptor arrays stored in fixed-width chunks.

use std::fmt;

/// Large descriptor collection stored as a fixed-width, non-cyclic chunk table.
pub struct DescriptorList<T: 'static> {
    chunks: &'static [&'static [T]],
}

impl<T> Copy for DescriptorList<T> {}

impl<T> Clone for DescriptorList<T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> DescriptorList<T> {
    /// Creates an empty descriptor list.
    pub const fn empty() -> Self {
        Self { chunks: &[] }
    }

    /// Creates a descriptor list from source-ordered, non-cyclic chunks.
    pub const fn new(chunks: &'static [&'static [T]]) -> Self {
        Self { chunks }
    }

    /// Returns an iterator in source order.
    pub fn iter(&self) -> DescriptorListIter<'_, T> {
        DescriptorListIter {
            chunks: self.chunks.iter(),
            current: [].iter(),
        }
    }

    /// Returns the total number of descriptors.
    pub fn len(&self) -> usize {
        self.chunks
            .iter()
            .fold(0usize, |count, chunk| count.saturating_add(chunk.len()))
    }

    /// If the descriptor is empty, it returns true.
    pub fn is_empty(&self) -> bool {
        self.iter().next().is_none()
    }
}

impl<T: PartialEq> PartialEq for DescriptorList<T> {
    fn eq(&self, other: &Self) -> bool {
        self.iter().eq(other.iter())
    }
}

impl<T: Eq> Eq for DescriptorList<T> {}

impl<T: fmt::Debug> fmt::Debug for DescriptorList<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.debug_list().entries(self.iter()).finish()
    }
}

impl<'a, T> IntoIterator for &'a DescriptorList<T> {
    type Item = &'a T;
    type IntoIter = DescriptorListIter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// Iterator over a [`DescriptorList`] in source order.
pub struct DescriptorListIter<'a, T: 'static> {
    chunks: std::slice::Iter<'a, &'static [T]>,
    current: std::slice::Iter<'a, T>,
}

impl<'a, T> Iterator for DescriptorListIter<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(value) = self.current.next() {
                return Some(value);
            }
            let next = self.chunks.next()?;
            self.current = next.iter();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DescriptorList;

    static FIRST: &[u8] = &[1, 2];
    static SECOND: &[u8] = &[3, 4];
    static HEAD: DescriptorList<u8> = DescriptorList::new(&[FIRST, SECOND]);

    #[test]
    fn visits_multiple_chunks_in_source_order() {
        assert_eq!(HEAD.iter().copied().collect::<Vec<_>>(), vec![1, 2, 3, 4]);
        assert_eq!(HEAD.len(), 4);
        assert!(!HEAD.is_empty());
        assert!(DescriptorList::<u8>::empty().is_empty());
    }

    #[test]
    fn treats_chunks_as_one_logical_slice() {
        static ONE: DescriptorList<u8> = DescriptorList::new(&[&[1, 2, 3]]);
        static MANY: DescriptorList<u8> = DescriptorList::new(&[&[1], &[2, 3]]);

        assert_eq!(ONE, MANY);
        assert_eq!(format!("{ONE:?}"), "[1, 2, 3]");
    }
}
