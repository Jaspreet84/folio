"""Draw one diagram at every ladder step, in the prototype's original mode and in fix mode."""
import time

import proto as P
import check as C

FLOW_TD = [('N', 28, 3, 4, 'TD'), ('C', 28, 1, 2, 'TD'), ('L16', 16, 1, 2, 'TD'), ('L8', 8, 1, 2, 'TD')]
FLOW_LR = [('N', 28, 3, 4, 'LR'), ('C', 28, 1, 2, 'LR'), ('L16lr', 16, 1, 2, 'LR'), ('L8lr', 8, 1, 2, 'LR'),
           ('T', 28, 3, 4, 'TD'), ('TC', 28, 1, 2, 'TD'), ('L16', 16, 1, 2, 'TD'), ('L8', 8, 1, 2, 'TD')]
SEQ = [('N', 28, 3, True), ('C', 28, 1, False), ('L16', 16, 1, False), ('L8', 8, 1, False)]
LADDERS = {
    'A': ['N', 'C', 'T', 'TC', 'L16', 'L8'],            # the plan's ladder: turn before narrowing labels
    'B': ['N', 'C', 'L16lr', 'T', 'TC', 'L16', 'L8'],   # try LR with labels 16 before turning
    'C': ['N', 'C', 'L16lr', 'L8lr', 'T', 'TC', 'L16', 'L8'],  # try every LR step before turning
}


def flow_steps(g, prep, fix, cjk=False, want_rows=False):
    P.FIX, P.CJK_BREAK = fix, cjk
    d = 'LR' if g.dir in ('LR', 'RL') else 'TD'
    edges = {k: (e['a'], e['b']) for k, e in enumerate(g.edges) if e['stroke'] != 'invisible'}
    out, rows_by = {}, {}
    for name, lw, gap, chan, dr in (FLOW_LR if d == 'LR' else FLOW_TD):
        rows, viol = P.flow_draw(prep, P.Prof(name, lw, gap, chan, dr))
        cv = P.LAST['cv']
        crossings, bad = C.cell_check(cv)
        falsep = C.flow_check(cv, edges)
        out[name] = [max((P.width(r) for r in rows), default=0), len(rows), viol, falsep, crossings, bad,
                     cv.hard_cycles, cv.dropped]
        if want_rows:
            rows_by[name] = rows
    P.FIX = P.CJK_BREAK = False
    return (out, rows_by) if want_rows else out


def seq_steps(sd, fix, cjk=False, want_rows=False):
    P.FIX, P.CJK_BREAK = fix, cjk
    out, rows_by = {}, {}
    for name, lw, gap, spacer in SEQ:
        rows, viol = P.seq_draw(sd, lw, gap, spacer, name in ('N', 'C'))
        cv = P.LAST['cv']
        crossings, bad = C.cell_check(cv)
        badmsg, framej = C.seq_check(cv)
        out[name] = [max((P.width(r) for r in rows), default=0), len(rows), viol, badmsg, framej, crossings, bad]
        if want_rows:
            rows_by[name] = rows
    P.FIX = P.CJK_BREAK = False
    return (out, rows_by) if want_rows else out


def flow_ok(m, strict):
    """m = [w, h, viol, falsep, crossings, bad, hard, dropped]"""
    if not strict:
        return True
    return m[2] == 0 and m[3] == 0 and m[5] == 0 and m[6] == 0 and m[7] == 0


def seq_ok(m, strict, frames_count=False):
    """m = [w, h, viol, badmsg, framej, crossings, bad]"""
    if not strict:
        return True
    return m[2] == 0 and m[3] == 0 and m[6] == 0 and (not frames_count or m[4] == 0)


def choose(steps, ladder, width, ok):
    for name in ladder:
        if name in steps and steps[name][0] <= width and ok(steps[name]):
            return name
    return 'outline'
