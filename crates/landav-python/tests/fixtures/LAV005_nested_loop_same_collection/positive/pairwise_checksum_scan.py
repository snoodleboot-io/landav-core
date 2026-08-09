"""Find records that share a checksum."""


def find_duplicates(records):
    """Every record is compared with every record."""
    dupes = []
    for left in records:
        for right in records:  # LANDAV: LAV005 anchor=for right in records:
            if left is not right and left.checksum == right.checksum:
                dupes.append((left, right))
    return dupes
