"""Fixture for the SLP150 comment/blank padding e2e test."""


def build_report(records):
    # set up the lines accumulator
    lines = []

    # walk over each record we received
    for record in records:
        # format this record into a line
        line = f"{record.id}: {record.value}"

        # append it to the running list
        lines.append(line)

    # join everything together and return it
    return "\n".join(lines)


def tidy(records):
    return [r for r in records if r.value is not None]
