"""Split a token stream at a marker."""


def split_at_marker(tokens, marker):
    """One linear scan, outside any loop."""
    cut = tokens.index(marker)
    return tokens[:cut], tokens[cut + 1:]
