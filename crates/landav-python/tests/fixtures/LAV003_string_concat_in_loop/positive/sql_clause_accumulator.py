"""Build a WHERE clause from a list of predicates."""


def build_query(clauses):
    """The explicit ``sql = sql + ...`` spelling of the same quadratic."""
    sql = "SELECT * FROM events WHERE 1 = 1"
    for clause in clauses:
        sql = sql + " AND " + clause  # LANDAV: LAV003 anchor=sql = sql + " AND " + clause
    return sql
