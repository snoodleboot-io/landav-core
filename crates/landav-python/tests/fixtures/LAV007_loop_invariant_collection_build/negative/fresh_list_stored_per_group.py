"""Seed one membership list per group, then fill them from assignments."""

_BASE_MEMBERS = ("owner", "auditor")


def build_groups(labels, assignments):
    """Each group needs its *own* list; hoisting would alias them all together.

    ``members`` is stored into two structures and mutated later through those,
    so one shared list would make every group show every assignment. The build
    is loop-invariant in value and emphatically not in identity.
    """
    groups = {}
    roster = []
    for label in labels:
        members = list(_BASE_MEMBERS)
        groups[label] = members
        roster.append((label, members))
    for label, person in assignments:
        groups[label].append(person)
    return groups, roster
