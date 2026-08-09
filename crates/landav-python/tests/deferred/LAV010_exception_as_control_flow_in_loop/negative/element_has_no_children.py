"""Take the first child of every ``<record>`` element."""


def first_child_tags(root):
    """``Element.get`` reads an *attribute*, so the total-lookup rewrite is a
    different operation entirely.

    ``element[0]`` indexes the children; ``element.get(0)`` looks up an XML
    attribute named ``0`` and returns ``None`` for every element in the
    document. There is no ``.get`` on the child sequence to move to.
    """
    tags = []
    for element in root.iter("record"):
        try:
            tags.append(element[0].tag)
        except IndexError:
            continue
    return tags
