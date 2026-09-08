//! Pagination utilities for list endpoints
//!
//! Provides reusable pagination helpers for extracting cursor IDs from collections.

/// Pagination utilities for list endpoints
pub struct Paginator;

impl Paginator {
    /// Get first and last IDs from items using an ID extractor function
    ///
    /// # Arguments
    /// * `items` - The collection to extract IDs from
    /// * `id_extractor` - Function to extract ID from an item
    ///
    /// # Returns
    /// Tuple of (first_id, last_id) where both are Option<ID>
    ///
    /// # Example
    /// ```ignore
    /// let (first_id, last_id) = Paginator::get_cursor_ids(&items, |item| item.id);
    /// ```
    pub fn get_cursor_ids<T, ID, F>(items: &[T], id_extractor: F) -> (Option<ID>, Option<ID>)
    where
        F: Fn(&T) -> ID,
    {
        let first_id = items.first().map(&id_extractor);
        let last_id = items.last().map(&id_extractor);
        (first_id, last_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_cursor_ids() {
        struct Item {
            id: i32,
        }
        let items = vec![Item { id: 1 }, Item { id: 2 }, Item { id: 3 }];
        let (first, last) = Paginator::get_cursor_ids(&items, |item| item.id);
        assert_eq!(first, Some(1));
        assert_eq!(last, Some(3));
    }

    #[test]
    fn test_get_cursor_ids_empty() {
        let items: Vec<i32> = vec![];
        let (first, last) = Paginator::get_cursor_ids(&items, |item| *item);
        assert_eq!(first, None);
        assert_eq!(last, None);
    }
}
