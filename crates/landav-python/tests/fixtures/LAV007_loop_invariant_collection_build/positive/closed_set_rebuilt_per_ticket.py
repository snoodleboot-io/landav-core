"""Filter tickets against a list of closed ids."""


def unresolved(tickets, closed_ids):
    """``set(closed_ids)`` is loop-invariant and costs a full copy each pass."""
    out = []
    for ticket in tickets:
        closed = set(closed_ids)  # LANDAV: LAV007 anchor=set(closed_ids)
        if ticket.id not in closed:
            out.append(ticket)
    return out
