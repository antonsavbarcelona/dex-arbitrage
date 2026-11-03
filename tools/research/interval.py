"""The interval mismatch: a 4 h / 8 h venue's settlement T taken with a hedge on a 1 h venue, held over T only.
Open at T - pre (the price at that moment: the close of the 15-min bar ending then), the side that RECEIVES the 4/8 h
payment; close at T + post. PnL = the payment received - the hedge's funding over (open, close] + the basis change
- the round trip. The signal: the rate settled at T (its estimate is public before T: an approximation), or strictly
the previous settlement's rate (no look-ahead at all)."""
import sys, os, json, math, collections, bisect
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
PRE = int(sys.argv[1]) if len(sys.argv) > 1 else 1    # bars before T
POST = int(sys.argv[2]) if len(sys.argv) > 2 else 1   # bars after T
ivs = {}
for m in META:
    f = funding(m['base'], m['venue'])
    if f and len(f[0]) > 3:
        g = sorted(b - a for a, b in zip(f[0], f[0][1:]))
        ivs[(m['base'], m['venue'])] = round(g[len(g) // 2] / 3.6e6)
cache = {}
def bars(a, v):
    if (a, v) not in cache:
        cache[(a, v)] = load_bars(a, v)
    return cache[(a, v)]
rows = []
assets = collections.defaultdict(list)
for (a, v), iv in ivs.items():
    assets[a].append((v, iv))
for a, vs in assets.items():
    for A, ivA in vs:
        if ivA < 4:
            continue
        for B, ivB in vs:
            if ivB != 1 or not same_token(a, A, B):
                continue
            bA, bB = bars(a, A), bars(a, B)
            if not bA or not bB:
                continue
            fA, fB = funding(a, A), funding(a, B)
            rt = 2 * min(MAKER.get(A, 5e-4) + taker(B, a) + spread(B, a) / 2, MAKER.get(B, 5e-4) + taker(A, a) + spread(A, a) / 2) + 2e-4
            rtt = 2 * (taker(A, a) + taker(B, a)) + spread(A, a) + spread(B, a)
            ts, pays = fA
            for n in range(1, len(ts)):
                T = ts[n]
                kT = T // BAR            # the bar starting at T
                k0, k1 = kT - PRE - 1, kT + POST - 1   # closes at T - PRE bars and T + POST bars
                if not all(k in bA and k in bB for k in (k0, k1)):
                    continue
                pay = pays[n]            # + = A's longs pay
                prev = pays[n - 1] / ivs[(a, A)] * ivs[(a, A)]
                sd = -1 if pay > 0 else 1   # -1: short A (receive), long B
                basis = sd * (math.log(bA[k1] / bA[k0]) - math.log(bB[k1] / bB[k0]))
                o_ms, c_ms = k0 * BAR + BAR, k1 * BAR + BAR
                hedge = sd * paid(fB, o_ms, c_ms)   # B's position = -sd; a position p earns -p x paid
                recv = abs(pay)
                rows.append(dict(a=a, A=A, B=B, T=T, pay=pay, prev=prev, recv=recv, hedge=hedge, basis=basis, rt=rt, rtt=rtt, sd=sd,
                                 prev_same=(prev > 0) == (pay > 0)))
print(len(rows), 'settlements x hedges;', len({r['a'] for r in rows}), 'assets; open', PRE * 15, 'min before, close', POST * 15, 'min after')
mid = sorted(r['T'] for r in rows)[len(rows) // 2]
m = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
def table(sig, title):
    print(f'\n{title}')
    print('| payment (bp) | n | received | hedge funding | basis | net m-t | net taker | win m-t | half1 / half2 m-t | assets |')
    for lo, hi in ((5, 10), (10, 20), (20, 50), (50, 100), (100, 10000)):
        x = [r for r in rows if lo <= sig(r) * 1e4 < hi]
        if len(x) < 10:
            continue
        net = lambda r: r['recv_s'] + r['hedge_s'] + r['basis_s'] - r['rt']
        for r in x:
            # the side chosen by the signal: right when the signal's sign = the payment's
            s_ok = (r['pay'] > 0) == (r['sig_sign'] > 0)
            k = 1 if s_ok else -1
            r['recv_s'], r['hedge_s'], r['basis_s'] = k * r['recv'], k * r['hedge'], k * r['basis']
        nm = [net(r) for r in x]
        print(f"| {lo}-{hi} | {len(x)} | {m([r['recv_s'] for r in x]):+.1f} | {m([r['hedge_s'] for r in x]):+.1f} | {m([r['basis_s'] for r in x]):+.1f} | {m(nm):+.1f} | {m([net(r) - r['rtt'] + r['rt'] for r in x]):+.1f} | {sum(v > 0 for v in nm)/len(nm):.0%} | {m([net(r) for r in x if r['T'] < mid]):+.1f} / {m([net(r) for r in x if r['T'] >= mid]):+.1f} | {len({r['a'] for r in x})} |")
for r in rows: r['sig_sign'] = r['pay']
table(lambda r: abs(r['pay']), 'A. the signal = the payment itself (its estimate is visible before T)')
for r in rows: r['sig_sign'] = r['prev']
table(lambda r: abs(r['prev']), 'B. the signal = the previous settlement (strictly no look-ahead)')
import pickle; pickle.dump(rows, open(f'{DD}/interval_{PRE}_{POST}.pkl', 'wb'))
