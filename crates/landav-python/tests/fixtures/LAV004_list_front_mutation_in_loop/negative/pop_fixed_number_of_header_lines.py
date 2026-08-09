"""Peel a three-line header off a file."""


def split_header(lines):
    """Exactly three shifts, whatever the file size."""
    header = []
    for _ in range(3):
        header.append(lines.pop(0))
    return header, lines
