//! [`ResourceDescriptor`] - one row of the `--resource` surface.

use crate::{resource_id::ResourceId, semiring_id::SemiringId};

/// Everything the CLI needs to know about one registered resource.
///
/// Produced only by the registry macro, so `--help`, the unknown-value error
/// message and the dispatch all read the same list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceDescriptor {
    id: ResourceId,
    semiring: SemiringId,
    unit: &'static str,
    summary: &'static str,
}

impl ResourceDescriptor {
    /// Builds a descriptor. `const` so the registry can build a `static`
    /// table.
    #[must_use]
    pub const fn new(
        id: ResourceId,
        semiring: SemiringId,
        unit: &'static str,
        summary: &'static str,
    ) -> Self {
        Self {
            id,
            semiring,
            unit,
            summary,
        }
    }

    /// The `--resource` value, and the cache key component.
    #[must_use]
    pub const fn id(self) -> ResourceId {
        self.id
    }

    /// The algebra this resource instantiates. Shared between resources; not a
    /// cache key.
    #[must_use]
    pub const fn semiring(self) -> SemiringId {
        self.semiring
    }

    /// The unit the reported number is in.
    #[must_use]
    pub const fn unit(self) -> &'static str {
        self.unit
    }

    /// One line for `--help`.
    #[must_use]
    pub const fn summary(self) -> &'static str {
        self.summary
    }
}

#[cfg(test)]
mod accessors {
    use super::ResourceDescriptor;
    use crate::{registry::ResourceKind, resource_id::ResourceId, semiring_id::SemiringId};

    /// Four fields, four accessors, and two of them are `&'static str` of the
    /// same type - so an accessor returning the wrong field compiles, passes
    /// the type checker, and renames every `--help` line.
    #[test]
    fn every_accessor_returns_its_own_field() {
        let descriptor = ResourceDescriptor::new(
            ResourceId::new("ops"),
            SemiringId::new("additive"),
            "operations",
            "abstract operation count",
        );
        assert_eq!(descriptor.id(), ResourceId::new("ops"));
        assert_eq!(descriptor.semiring(), SemiringId::new("additive"));
        assert_eq!(descriptor.unit(), "operations");
        assert_eq!(descriptor.summary(), "abstract operation count");
    }

    /// The two `&'static str` fields must not be swappable without a test
    /// failing, so they are pinned distinctly on a descriptor whose values
    /// share no prefix.
    #[test]
    fn the_unit_and_the_summary_do_not_alias() {
        let descriptor = ResourceDescriptor::new(
            ResourceId::new("peak-mem"),
            SemiringId::new("peak"),
            "bytes",
            "peak live memory",
        );
        assert_ne!(descriptor.unit(), descriptor.summary());
        assert_eq!(descriptor.unit(), "bytes");
        assert_eq!(descriptor.summary(), "peak live memory");
    }

    /// `new` is `const`, which is what lets the registry build a `static`
    /// table. Checked by building one, so the property cannot regress to a
    /// runtime constructor without a compile error here.
    #[test]
    fn new_is_usable_in_a_const_context() {
        static TABLE: &[ResourceDescriptor] = &[ResourceDescriptor::new(
            ResourceId::new("queries"),
            SemiringId::new("additive"),
            "queries",
            "external calls issued",
        )];
        assert_eq!(TABLE.len(), 1);
        assert_eq!(TABLE[0].id().as_str(), "queries");
    }

    /// The registry's descriptors read back exactly what was registered.
    /// Three resources share one algebra, which is the whole reason
    /// `SemiringId` may not be a cache key.
    #[test]
    fn the_registered_descriptors_agree_with_the_registry() {
        let peak = ResourceKind::PeakMem.descriptor();
        assert_eq!(peak.id(), ResourceId::new("peak-mem"));
        assert_eq!(peak.semiring(), SemiringId::new("peak"));
        assert_eq!(peak.unit(), "bytes");
        assert_eq!(peak.summary(), "peak live memory");

        let ops = ResourceKind::Ops.descriptor();
        let queries = ResourceKind::Queries.descriptor();
        assert_eq!(ops.semiring(), queries.semiring());
        assert_ne!(ops.unit(), queries.unit());
    }
}
