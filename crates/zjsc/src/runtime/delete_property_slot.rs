//! Tradução de `runtime/DeletePropertySlot.h`: o que `JSObject::deleteProperty` conta ao chamador
//! (caches de `delete` no `LLInt`/JIT) sobre o resultado.

use crate::runtime::property_offset::{PropertyOffset, INVALID_OFFSET};
use crate::runtime::property_slot::CacheabilityType;

/// `DeletePropertySlot::Type`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DeleteType {
    Uncacheable,
    DeleteHit,
    ConfigurableDeleteMiss,
    Nonconfigurable,
}

/// `class DeletePropertySlot`.
#[derive(Clone, Copy, Debug)]
pub struct DeletePropertySlot {
    offset: PropertyOffset,
    cacheability: CacheabilityType,
    type_: DeleteType,
}

impl Default for DeletePropertySlot {
    /// `DeletePropertySlot()`.
    fn default() -> DeletePropertySlot {
        DeletePropertySlot {
            offset: INVALID_OFFSET,
            cacheability: CacheabilityType::CachingAllowed,
            type_: DeleteType::Uncacheable,
        }
    }
}

impl DeletePropertySlot {
    /// `setConfigurableMiss()`.
    pub fn set_configurable_miss(&mut self) {
        self.type_ = DeleteType::ConfigurableDeleteMiss;
    }

    /// `setNonconfigurable()`.
    pub fn set_nonconfigurable(&mut self) {
        self.type_ = DeleteType::Nonconfigurable;
    }

    /// `setHit(offset)`.
    pub fn set_hit(&mut self, offset: PropertyOffset) {
        self.type_ = DeleteType::DeleteHit;
        self.offset = offset;
    }

    /// `isCacheableDelete()`.
    pub fn is_cacheable_delete(&self) -> bool {
        self.is_cacheable() && self.type_ != DeleteType::Uncacheable
    }

    /// `isDeleteHit()`.
    pub fn is_delete_hit(&self) -> bool {
        self.type_ == DeleteType::DeleteHit
    }

    /// `isConfigurableDeleteMiss()`.
    pub fn is_configurable_delete_miss(&self) -> bool {
        self.type_ == DeleteType::ConfigurableDeleteMiss
    }

    /// `isNonconfigurable()`.
    pub fn is_nonconfigurable(&self) -> bool {
        self.type_ == DeleteType::Nonconfigurable
    }

    /// `cachedOffset()`.
    pub fn cached_offset(&self) -> PropertyOffset {
        self.offset
    }

    /// `disableCaching()`.
    pub fn disable_caching(&mut self) {
        self.cacheability = CacheabilityType::CachingDisallowed;
    }

    /// `isCacheable()`.
    fn is_cacheable(&self) -> bool {
        self.cacheability == CacheabilityType::CachingAllowed
    }
}
