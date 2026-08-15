"""LAV005 in comprehension form. Genuinely quadratic, and invisible.

Compare `tests/fixtures/LAV005_nested_loop_same_collection/positive/
pairwise_checksum_scan.py`, which is the same computation written with two
`for` statements and which LAV005 reports. This file is silent.
"""


def find_duplicates(records):
    """Every record is compared with every record, in one expression."""
    return [(a, b) for a in records for b in records if a is not b and a.checksum == b.checksum]
