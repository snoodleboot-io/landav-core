"""Group a stream of lines into blank-line-separated paragraphs."""


def read_paragraphs(lines):
    """The inner loop *consumes* the iterator; it does not restart it.

    ``stream`` is a single iterator, so the two loops together advance it once
    from beginning to end. Every line is visited exactly once and the whole
    function is linear, even though both headers name the same object.
    """
    stream = iter(lines)
    paragraphs = []
    for first in stream:
        if not first.strip():
            continue
        block = [first]
        for line in stream:
            if not line.strip():
                break
            block.append(line)
        paragraphs.append(block)
    return paragraphs
