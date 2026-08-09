"""Deduplicate within each group, but not across groups."""


def dedupe_per_group(groups):
    """Hoisting ``seen`` would change the result, not just the cost."""
    out = []
    for group in groups:
        seen = set()
        for item in group.items:
            if item.key in seen:
                continue
            seen.add(item.key)
            out.append((group.name, item))
    return out
