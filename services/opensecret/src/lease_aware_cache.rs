use clru::CLruCache;
use std::collections::hash_map::RandomState;
use std::hash::Hash;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InsertOutcome {
    Inserted,
    ReclaimedExpired,
    EvictedLeastRecentlyUsed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum InsertError {
    DuplicateKey,
    AllEntriesLeased,
}

struct LeaseSlot<V> {
    value: V,
    leases: AtomicUsize,
}

impl<V> LeaseSlot<V> {
    fn new(value: V) -> Self {
        Self {
            value,
            leases: AtomicUsize::new(0),
        }
    }

    fn is_leased(&self) -> bool {
        self.leases.load(Ordering::Acquire) != 0
    }

    fn try_acquire(&self) -> bool {
        self.leases
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |leases| {
                leases.checked_add(1)
            })
            .is_ok()
    }
}

struct CachedValue<V> {
    slot: Arc<LeaseSlot<V>>,
    expires_at: Instant,
}

impl<V> CachedValue<V> {
    fn has_external_lease(&self) -> bool {
        self.slot.is_leased() || Arc::strong_count(&self.slot) != 1
    }
}

/// Cache-owned slots detached for destruction after the caller releases the
/// session-cache mutex.
#[must_use = "keep retired values alive until after releasing the cache lock"]
pub(crate) struct RetiredLeaseValues<V> {
    slots: Vec<Arc<LeaseSlot<V>>>,
}

impl<V> RetiredLeaseValues<V> {
    pub(crate) fn removed_count(&self) -> usize {
        self.slots.len()
    }
}

/// An RAII lease that prevents its cached value from being expired or evicted.
pub(crate) struct CacheLease<V> {
    slot: Arc<LeaseSlot<V>>,
}

impl<V> CacheLease<V> {
    pub(crate) fn value(&self) -> &V {
        &self.slot.value
    }
}

impl<V> Drop for CacheLease<V> {
    fn drop(&mut self) {
        let previous = self.slot.leases.fetch_sub(1, Ordering::AcqRel);
        debug_assert!(previous > 0, "cache lease count underflow");
    }
}

/// A private, fixed-capacity sliding-TTL LRU cache whose in-flight values stay
/// resident until their final request lease is dropped.
///
/// As with `BoundedTtlCache`, this deliberately exposes neither resizing nor
/// mutable iteration. Values live behind an `Arc`, so LRU reordering moves only
/// the handle rather than secret key bytes.
pub(crate) struct LeaseAwareTtlCache<K, V> {
    entries: CLruCache<K, CachedValue<V>, RandomState>,
    ttl: Duration,
}

impl<K, V> LeaseAwareTtlCache<K, V>
where
    K: Clone + Eq + Hash,
{
    pub(crate) fn new(capacity: NonZeroUsize, ttl: Duration) -> Self {
        Self {
            entries: CLruCache::with_memory(capacity, capacity.get()),
            ttl,
        }
    }

    /// Inserts a value and evicts the least-recently-used unleased value when
    /// full. The only capacity rejection is the true-overload case where every
    /// resident value is concurrently leased by an in-flight response.
    pub(crate) fn insert_evicting(
        &mut self,
        key: K,
        value: V,
    ) -> Result<InsertOutcome, InsertError> {
        self.insert_evicting_at(key, value, Instant::now())
    }

    fn insert_evicting_at(
        &mut self,
        key: K,
        value: V,
        now: Instant,
    ) -> Result<InsertOutcome, InsertError> {
        if self.entries.peek(&key).is_some() {
            drop(value);
            return Err(InsertError::DuplicateKey);
        }

        let expires_at = self.expiry_at(now);
        if !self.entries.is_full() {
            self.insert_new(key, value, expires_at);
            return Ok(InsertOutcome::Inserted);
        }

        // Scan from LRU toward MRU without changing recency. In particular,
        // merely being pinned must not refresh idle TTL; only authenticated
        // success (or an admitted bodyless request) calls `touch`.
        let eviction_key = self
            .entries
            .iter()
            .rev()
            .find(|(_, entry)| !entry.has_external_lease())
            .map(|(candidate_key, _)| candidate_key.clone());
        let Some(eviction_key) = eviction_key else {
            drop(value);
            return Err(InsertError::AllEntriesLeased);
        };

        let evicted = self
            .entries
            .pop(&eviction_key)
            .expect("selected unleased entry must remain present");
        debug_assert_eq!(Arc::strong_count(&evicted.slot), 1);
        let outcome = if evicted.expires_at <= now {
            InsertOutcome::ReclaimedExpired
        } else {
            InsertOutcome::EvictedLeastRecentlyUsed
        };
        drop(evicted);
        self.insert_new(key, value, expires_at);
        Ok(outcome)
    }

    /// Acquires a request-lifetime lease without updating recency or idle TTL.
    /// Call `touch` only after successful authenticated use (or immediately for
    /// a valid bodyless request).
    pub(crate) fn acquire(&mut self, key: &K) -> Option<CacheLease<V>> {
        self.acquire_at(key, Instant::now())
    }

    fn acquire_at(&mut self, key: &K, now: Instant) -> Option<CacheLease<V>> {
        let expired_and_unleased = {
            let entry = self.entries.peek(key)?;
            entry.expires_at <= now && !entry.has_external_lease()
        };
        if expired_and_unleased {
            let expired = self.entries.pop(key);
            debug_assert!(expired
                .as_ref()
                .is_none_or(|entry| Arc::strong_count(&entry.slot) == 1));
            drop(expired);
            return None;
        }

        let slot = Arc::clone(&self.entries.peek(key)?.slot);
        if !slot.try_acquire() {
            return None;
        }
        Some(CacheLease { slot })
    }

    /// Refreshes a value's sliding idle TTL and moves it to the MRU position.
    pub(crate) fn touch(&mut self, key: &K) -> bool {
        self.touch_at(key, Instant::now())
    }

    fn touch_at(&mut self, key: &K, now: Instant) -> bool {
        let expired_and_unleased = match self.entries.peek(key) {
            Some(entry) => entry.expires_at <= now && !entry.has_external_lease(),
            None => return false,
        };
        if expired_and_unleased {
            let expired = self.entries.pop(key);
            debug_assert!(expired
                .as_ref()
                .is_none_or(|entry| Arc::strong_count(&entry.slot) == 1));
            drop(expired);
            return false;
        }

        let expires_at = self.expiry_at(now);
        self.entries
            .get_mut(key)
            .expect("peeked cache entry must remain present")
            .expires_at = expires_at;
        true
    }

    fn expiry_at(&self, now: Instant) -> Instant {
        now.checked_add(self.ttl)
            .expect("lease-aware cache TTL must fit in Instant")
    }

    fn insert_new(&mut self, key: K, value: V, expires_at: Instant) {
        let previous = self.entries.put(
            key,
            CachedValue {
                slot: Arc::new(LeaseSlot::new(value)),
                expires_at,
            },
        );
        debug_assert!(previous.is_none());
        debug_assert!(self.entries.len() <= self.entries.capacity());
    }

    /// Takes one LRU-to-MRU snapshot of expired, currently unleased keys.
    ///
    /// A fresh deadline always moves an entry to MRU, while acquisition alone
    /// changes neither deadline nor recency. Expired entries therefore form an
    /// LRU suffix and this scan can stop at the first live entry. The caller can
    /// process the snapshot in bounded batches without repeatedly rescanning
    /// expired leased entries.
    pub(crate) fn expired_unleased_keys_at(&self, now: Instant) -> Vec<K> {
        let mut keys = Vec::new();
        for (key, entry) in self.entries.iter().rev() {
            if entry.expires_at > now {
                break;
            }
            if !entry.has_external_lease() {
                keys.push(key.clone());
            }
        }
        keys
    }

    /// Revalidates and detaches a bounded candidate batch.
    ///
    /// Revalidation protects a newly inserted value if another operation
    /// removed an expired candidate while the mutex was released. Returned
    /// slots retain their values so secret destruction occurs outside the
    /// mutex.
    pub(crate) fn retire_expired_candidates_at(
        &mut self,
        keys: &[K],
        now: Instant,
    ) -> RetiredLeaseValues<V> {
        let mut slots = Vec::with_capacity(keys.len());
        for key in keys {
            let should_retire = self
                .entries
                .peek(key)
                .is_some_and(|entry| entry.expires_at <= now && !entry.has_external_lease());
            if !should_retire {
                continue;
            }

            let retired = self
                .entries
                .pop(key)
                .expect("revalidated cache entry must remain present");
            debug_assert_eq!(Arc::strong_count(&retired.slot), 1);
            slots.push(retired.slot);
        }
        RetiredLeaseValues { slots }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }

    #[cfg(test)]
    fn lease_count(&self, key: &K) -> Option<usize> {
        self.entries
            .peek(key)
            .map(|entry| entry.slot.leases.load(Ordering::Acquire))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::web::attestation_routes::SessionState;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn cache<K, V>(capacity: usize, ttl: Duration) -> LeaseAwareTtlCache<K, V>
    where
        K: Clone + Eq + Hash,
    {
        LeaseAwareTtlCache::new(NonZeroUsize::new(capacity).unwrap(), ttl)
    }

    #[test]
    fn configured_policy_and_layout_fit_the_aggregate_budget() {
        assert_eq!(crate::MAX_ENCRYPTION_SESSIONS, 2_097_152);
        assert_eq!(
            crate::ENCRYPTION_SESSION_IDLE_TTL,
            Duration::from_secs(65 * 60)
        );

        let accounted_bytes_per_entry = 192;
        let accounted_bytes = (crate::MAX_PENDING_ATTESTATIONS + crate::MAX_ENCRYPTION_SESSIONS)
            * accounted_bytes_per_entry;
        assert!(accounted_bytes <= 500 * 1024 * 1024);
        assert_eq!(500 * 1024 * 1024 - accounted_bytes, 109_051_904);

        #[cfg(target_pointer_width = "64")]
        {
            assert_eq!(std::mem::size_of::<SessionState>(), 32);
            assert_eq!(std::mem::size_of::<LeaseSlot<SessionState>>(), 40);
            assert_eq!(std::mem::size_of::<CachedValue<SessionState>>(), 24);
            assert_eq!(std::mem::size_of::<CacheLease<SessionState>>(), 8);

            // Cleanup can temporarily hold a UUID candidate snapshot while the
            // fixed CLru backing allocation stays reserved. Budget for twice
            // the final Vec payload to cover geometric growth/reallocation,
            // plus one detached batch of Arc handles.
            assert_eq!(std::mem::size_of::<uuid::Uuid>(), 16);
            let cleanup_snapshot_peak =
                crate::MAX_ENCRYPTION_SESSIONS * std::mem::size_of::<uuid::Uuid>() * 2;
            let detached_batch = crate::secret_cache_maintenance::SESSION_RETIREMENT_BATCH_SIZE
                * std::mem::size_of::<Arc<LeaseSlot<SessionState>>>();
            let peak_accounted_bytes = accounted_bytes + cleanup_snapshot_peak + detached_batch;
            assert!(peak_accounted_bytes <= 500 * 1024 * 1024);
            assert_eq!(500 * 1024 * 1024 - peak_accounted_bytes, 41_910_272);
        }
    }

    #[test]
    fn evicts_lru_and_always_admits_when_an_unleased_entry_exists() {
        let start = Instant::now();
        let mut cache = cache(2, Duration::from_secs(60));
        cache.insert_evicting_at(1, "one", start).unwrap();
        cache
            .insert_evicting_at(2, "two", start + Duration::from_secs(1))
            .unwrap();

        assert_eq!(
            cache.insert_evicting_at(3, "three", start + Duration::from_secs(2)),
            Ok(InsertOutcome::EvictedLeastRecentlyUsed)
        );
        assert!(cache
            .acquire_at(&1, start + Duration::from_secs(3))
            .is_none());
        assert_eq!(
            cache
                .acquire_at(&3, start + Duration::from_secs(3))
                .unwrap()
                .value(),
            &"three"
        );
        assert_eq!(cache.len(), 2);
    }

    #[test]
    fn acquire_without_successful_touch_does_not_change_lru_order() {
        let start = Instant::now();
        let mut cache = cache(2, Duration::from_secs(60));
        cache.insert_evicting_at(1, "one", start).unwrap();
        cache
            .insert_evicting_at(2, "two", start + Duration::from_secs(1))
            .unwrap();

        drop(
            cache
                .acquire_at(&1, start + Duration::from_secs(2))
                .unwrap(),
        );
        cache
            .insert_evicting_at(3, "three", start + Duration::from_secs(3))
            .unwrap();
        assert!(cache
            .acquire_at(&1, start + Duration::from_secs(4))
            .is_none());
        assert!(cache
            .acquire_at(&2, start + Duration::from_secs(4))
            .is_some());
    }

    #[test]
    fn successful_touch_refreshes_ttl_and_promotes_to_mru() {
        let start = Instant::now();
        let mut cache = cache(2, Duration::from_secs(10));
        cache.insert_evicting_at(1, "one", start).unwrap();
        cache
            .insert_evicting_at(2, "two", start + Duration::from_secs(1))
            .unwrap();

        let lease = cache
            .acquire_at(&1, start + Duration::from_secs(2))
            .unwrap();
        assert!(cache.touch_at(&1, start + Duration::from_secs(2)));
        drop(lease);
        cache
            .insert_evicting_at(3, "three", start + Duration::from_secs(3))
            .unwrap();

        assert!(cache
            .acquire_at(&2, start + Duration::from_secs(4))
            .is_none());
        assert!(cache
            .acquire_at(&1, start + Duration::from_secs(11))
            .is_some());
    }

    #[test]
    fn exact_idle_ttl_boundary_is_expired_when_unleased() {
        let start = Instant::now();
        let mut cache = cache(1, Duration::from_secs(5));
        cache.insert_evicting_at(1, "one", start).unwrap();
        assert!(cache
            .acquire_at(&1, start + Duration::from_secs(5))
            .is_none());
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn retires_untouched_expired_values_below_capacity_and_drops_outside_cache() {
        struct DropSpy(Arc<AtomicUsize>);
        impl Drop for DropSpy {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let start = Instant::now();
        let drops = Arc::new(AtomicUsize::new(0));
        let mut cache = cache(4, Duration::from_secs(5));
        cache
            .insert_evicting_at(1, DropSpy(drops.clone()), start)
            .unwrap();
        cache
            .insert_evicting_at(2, DropSpy(drops.clone()), start + Duration::from_secs(4))
            .unwrap();

        let candidates = cache.expired_unleased_keys_at(start + Duration::from_secs(5));
        assert_eq!(candidates, vec![1]);
        let retired =
            cache.retire_expired_candidates_at(&candidates, start + Duration::from_secs(5));
        assert_eq!(retired.removed_count(), 1);
        assert_eq!(cache.len(), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 0);

        drop(retired);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        assert!(cache
            .acquire_at(&2, start + Duration::from_secs(5))
            .is_some());
    }

    #[test]
    fn touched_value_survives_its_original_expiry_snapshot() {
        let start = Instant::now();
        let mut cache = cache(2, Duration::from_secs(5));
        cache.insert_evicting_at(1, "one", start).unwrap();
        assert!(cache.touch_at(&1, start + Duration::from_secs(4)));

        assert!(cache
            .expired_unleased_keys_at(start + Duration::from_secs(5))
            .is_empty());
        assert_eq!(
            cache.expired_unleased_keys_at(start + Duration::from_secs(9)),
            vec![1]
        );
    }

    #[test]
    fn leased_expired_value_waits_for_a_later_cleanup_pass() {
        struct DropSpy(Arc<AtomicUsize>);
        impl Drop for DropSpy {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let start = Instant::now();
        let drops = Arc::new(AtomicUsize::new(0));
        let mut cache = cache(2, Duration::from_secs(5));
        cache
            .insert_evicting_at(1, DropSpy(drops.clone()), start)
            .unwrap();
        let lease = cache.acquire_at(&1, start).unwrap();

        let expired_at = start + Duration::from_secs(5);
        assert!(cache.expired_unleased_keys_at(expired_at).is_empty());
        assert_eq!(cache.len(), 1);
        drop(lease);

        let candidates = cache.expired_unleased_keys_at(expired_at);
        let retired = cache.retire_expired_candidates_at(&candidates, expired_at);
        assert_eq!(retired.removed_count(), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 0);
        drop(retired);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn cleanup_skips_leased_expiry_holes_and_preserves_survivor_order() {
        let start = Instant::now();
        let mut cache = cache(4, Duration::from_secs(5));
        cache.insert_evicting_at(1, "leased", start).unwrap();
        cache
            .insert_evicting_at(2, "expired", start + Duration::from_secs(1))
            .unwrap();
        cache
            .insert_evicting_at(3, "live", start + Duration::from_secs(2))
            .unwrap();
        let lease = cache
            .acquire_at(&1, start + Duration::from_secs(2))
            .unwrap();

        let cutoff = start + Duration::from_secs(6);
        let candidates = cache.expired_unleased_keys_at(cutoff);
        assert_eq!(candidates, vec![2]);
        drop(cache.retire_expired_candidates_at(&candidates, cutoff));

        cache.insert_evicting_at(4, "new", cutoff).unwrap();
        cache.insert_evicting_at(5, "newest", cutoff).unwrap();
        assert_eq!(
            cache.insert_evicting_at(6, "overflow", cutoff),
            Ok(InsertOutcome::EvictedLeastRecentlyUsed)
        );
        assert!(cache.acquire_at(&3, cutoff).is_none());
        assert_eq!(lease.value(), &"leased");
        drop(lease);
    }

    #[test]
    fn stale_cleanup_candidate_cannot_remove_a_replacement() {
        let start = Instant::now();
        let cutoff = start + Duration::from_secs(5);
        let mut cache = cache(1, Duration::from_secs(5));
        cache.insert_evicting_at(1, "old", start).unwrap();
        let candidates = cache.expired_unleased_keys_at(cutoff);

        assert!(cache.acquire_at(&1, cutoff).is_none());
        cache.insert_evicting_at(1, "replacement", cutoff).unwrap();
        let retired = cache.retire_expired_candidates_at(&candidates, cutoff);
        assert_eq!(retired.removed_count(), 0);
        assert_eq!(
            cache.acquire_at(&1, cutoff).unwrap().value(),
            &"replacement"
        );
    }

    #[test]
    fn lingering_arc_excludes_expired_value_from_cleanup_snapshot() {
        let start = Instant::now();
        let cutoff = start + Duration::from_secs(5);
        let mut cache = cache(1, Duration::from_secs(5));
        cache.insert_evicting_at(1, "one", start).unwrap();
        let lingering_arc = Arc::clone(&cache.entries.peek(&1).unwrap().slot);

        assert!(cache.expired_unleased_keys_at(cutoff).is_empty());
        drop(lingering_arc);
        assert_eq!(cache.expired_unleased_keys_at(cutoff), vec![1]);
    }

    #[test]
    fn pinned_lru_is_preserved_and_next_unleased_entry_is_evicted() {
        let start = Instant::now();
        let mut cache = cache(2, Duration::from_secs(60));
        cache.insert_evicting_at(1, "one", start).unwrap();
        cache
            .insert_evicting_at(2, "two", start + Duration::from_secs(1))
            .unwrap();
        let lease = cache
            .acquire_at(&1, start + Duration::from_secs(2))
            .unwrap();

        assert_eq!(
            cache.insert_evicting_at(3, "three", start + Duration::from_secs(3)),
            Ok(InsertOutcome::EvictedLeastRecentlyUsed)
        );
        assert!(cache
            .acquire_at(&1, start + Duration::from_secs(4))
            .is_some());
        assert!(cache
            .acquire_at(&2, start + Duration::from_secs(4))
            .is_none());
        drop(lease);
    }

    #[test]
    fn all_pinned_is_bounded_true_overload_and_drops_rejected_value() {
        struct DropSpy(Arc<AtomicUsize>);
        impl Drop for DropSpy {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let start = Instant::now();
        let drops = Arc::new(AtomicUsize::new(0));
        let mut cache = cache(2, Duration::from_secs(60));
        cache
            .insert_evicting_at(1, DropSpy(drops.clone()), start)
            .unwrap();
        cache
            .insert_evicting_at(2, DropSpy(drops.clone()), start)
            .unwrap();
        let first = cache.acquire_at(&1, start).unwrap();
        let second = cache.acquire_at(&2, start).unwrap();

        assert_eq!(
            cache.insert_evicting_at(3, DropSpy(drops.clone()), start),
            Err(InsertError::AllEntriesLeased)
        );
        assert_eq!(cache.len(), 2);
        assert_eq!(drops.load(Ordering::SeqCst), 1);

        drop(first);
        assert!(cache
            .insert_evicting_at(3, DropSpy(drops.clone()), start)
            .is_ok());
        assert_eq!(cache.len(), 2);
        assert_eq!(drops.load(Ordering::SeqCst), 2);
        drop(second);
    }

    #[test]
    fn expired_pinned_entry_survives_until_final_lease_drops() {
        let start = Instant::now();
        let mut cache = cache(1, Duration::from_secs(5));
        cache.insert_evicting_at(1, "one", start).unwrap();
        let first = cache.acquire_at(&1, start).unwrap();

        let second = cache
            .acquire_at(&1, start + Duration::from_secs(6))
            .expect("an in-flight session remains usable");
        assert_eq!(cache.lease_count(&1), Some(2));
        assert_eq!(
            cache.insert_evicting_at(2, "two", start + Duration::from_secs(6)),
            Err(InsertError::AllEntriesLeased)
        );

        drop(first);
        drop(second);
        assert!(!cache.touch_at(&1, start + Duration::from_secs(12)));
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn lease_drop_decrements_exactly_once() {
        let start = Instant::now();
        let mut cache = cache(1, Duration::from_secs(60));
        cache.insert_evicting_at(1, "one", start).unwrap();
        let lease = cache.acquire_at(&1, start).unwrap();
        assert_eq!(cache.lease_count(&1), Some(1));
        drop(lease);
        assert_eq!(cache.lease_count(&1), Some(0));
    }

    #[test]
    fn final_drop_gap_cannot_detach_a_still_referenced_value() {
        let start = Instant::now();
        let mut cache = cache(1, Duration::from_secs(60));
        cache.insert_evicting_at(1, "one", start).unwrap();

        // Models the narrow interval after CacheLease::drop decrements the
        // explicit count but before Rust drops that lease's Arc field.
        let lingering_arc = Arc::clone(&cache.entries.peek(&1).unwrap().slot);
        assert_eq!(cache.lease_count(&1), Some(0));
        assert_eq!(Arc::strong_count(&lingering_arc), 2);
        assert_eq!(
            cache.insert_evicting_at(2, "two", start),
            Err(InsertError::AllEntriesLeased)
        );
        assert_eq!(lingering_arc.value, "one");

        drop(lingering_arc);
        assert!(cache.insert_evicting_at(2, "two", start).is_ok());
    }

    #[test]
    fn duplicate_key_never_replaces_a_live_or_leased_session() {
        let start = Instant::now();
        let mut cache = cache(1, Duration::from_secs(60));
        cache.insert_evicting_at(1, "original", start).unwrap();
        let lease = cache.acquire_at(&1, start).unwrap();

        assert_eq!(
            cache.insert_evicting_at(1, "replacement", start),
            Err(InsertError::DuplicateKey)
        );
        assert_eq!(lease.value(), &"original");
    }
}
