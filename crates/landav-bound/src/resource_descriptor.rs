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
