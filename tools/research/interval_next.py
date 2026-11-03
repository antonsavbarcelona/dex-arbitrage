"""After a big settlement on a 4 h / 8 h venue A (|payment| >= X, known: it was paid), enter at T_prev + 15 min in the
direction that receives the same sign again (long A when A's longs were paid, i.e. the payment < 0), hedge on a 1 h venue,
hold through the next settlement T, out at T + 15 min. Strictly no look-ahead."""
import sys, os, math, collections, bisect
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
ivs = {}
for m in META:
    f = funding(m['base'], m['venue'])
    if f and len(f[0]) > 3:
        g = sorted(b - a for a, b in zip(f[0], f[0][1:]))
        ivs[(m['base'], m['venue'])] = round(g[len(g) // 2] / 3.6e6)
cache = {}
def bars(a, v):
    if (a, v) not in cache: cache[(a, v)] = load_bars(a, v)
    return cache[(a, v)]
by_asset = collections.defaultdict(list)
for (a, v), iv in ivs.items(): by_asset[a].append((v, iv))
out = []
for a, vs in by_asset.items():
    for A, ivA in vs:
        if ivA < 4: continue
        fA = funding(a, A)
        ts, pays = fA
        for B, ivB in vs:
            if ivB != 1 or not same_token(a, A, B): continue
            bA, bB = bars(a, A), bars(a, B)
            if not bA or not bB: continue
            fB = funding(a, B)
            rt = 2 * min(MAKER.get(A, 5e-4) + taker(B, a) + spread(B, a) / 2, MAKER.get(B, 5e-4) + taker(A, a) + spread(A, a) / 2) + 2e-4
            for n in range(1, len(ts) - 1):
                prev = pays[n]
                if abs(prev) < 0.003: continue
                T0, T1 = ts[n], ts[n + 1]
                k0, k1 = T0 // BAR, T1 // BAR   # closes at T0 + 15 min, T1 + 15 min
                if k0 not in bA or k0 not in bB or k1 not in bA or k1 not in bB: continue
                sd = 1 if prev < 0 else -1       # long A when its longs were paid
                o, c = T0 + BAR, T1 + BAR
                recv = -sd * paid(fA, o, c)       # a position p earns -p x paid
                hedge = sd * paid(fB, o, c)
                basis = sd * (math.log(bA[k1] / bA[k0]) - math.log(bB[k1] / bB[k0]))
                out.append(dict(a=a, A=A, B=B, T=T0, prev=abs(prev), recv=recv, hedge=hedge, basis=basis, net=recv + hedge + basis - rt, hold=(T1 - T0) / 3.6e6))
mid = sorted(o['T'] for o in out)[len(out) // 2]
m = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
print(len(out), 'trades')
print('| previous payment bp | n | next payment recv | hedge | basis | net | win | half1/half2 | assets | hold h |')
for lo, hi in ((30, 50), (50, 100), (100, 150), (150, 10000)):
    x = [o for o in out if lo <= o['prev'] * 1e4 < hi]
    if len(x) < 5: continue
    print(f"| {lo}-{hi} | {len(x)} | {m([o['recv'] for o in x]):+.1f} | {m([o['hedge'] for o in x]):+.1f} | {m([o['basis'] for o in x]):+.1f} | {m([o['net'] for o in x]):+.1f} | {sum(o['net']>0 for o in x)/len(x):.0%} | {m([o['net'] for o in x if o['T'] < mid]):+.1f} / {m([o['net'] for o in x if o['T'] >= mid]):+.1f} | {len({o['a'] for o in x})} | {sum(o['hold'] for o in x)/len(x):.1f} |")
x = [o for o in out if o['prev'] >= 0.005]
by = collections.defaultdict(list)
for o in x: by[o['a']].append(o['net'])
top = sorted(by.items(), key=lambda kv: -sum(kv[1]))
print('prev >= 50 bp by asset:', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top))
rest = [v for a, vs in top[3:] for v in vs]
print(f'  without the top 3: {len(rest)} trades {m(rest):+.1f} bp')
