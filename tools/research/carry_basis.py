"""The fstab trades (data/fstab_trades.json) split by the entry basis: dev = log(P_long / P_short) - its median over
the previous 3 days (15-min closes). dev < 0: the long leg cheap vs normal (the basis should help). The trade's full
result = funding - the round trip + the basis change over the hold."""
import sys, os, json, math, statistics, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
tr = json.load(open(f'{DD}/fstab_trades.json'))
DELAY = int(sys.argv[1]) if len(sys.argv) > 1 else 0
cache = {}
def bars(a, v):
    if (a, v) not in cache:
        cache[(a, v)] = load_bars(a, v)
    return cache[(a, v)]
rows = []
for t in tr:
    a = t['asset']
    bl, bs = bars(a, t['long']), bars(a, t['short'])
    if not bl or not bs or not same_token(a, t['long'], t['short']):
        continue
    k0, k1 = t['entry_ms'] // BAR, t['exit_ms'] // BAR
    ks = sorted(set(bl) & set(bs))
    import bisect
    i0 = bisect.bisect_right(ks, k0) - 1; i1 = bisect.bisect_right(ks, k1) - 1
    if i0 < 288 or i1 <= i0:
        continue
    b = lambda i: math.log(bl[ks[i]] / bs[ks[i]])
    normal = statistics.median(b(i) for i in range(i0 - 288, i0))
    dev = b(i0) - normal
    i0 += DELAY
    if i1 <= i0:
        continue
    if abs(dev) > 0.05:
        continue
    basis = b(i1) - b(i0)   # long L short S: + when L gains vs S
    rows.append(dict(dev=dev, basis=basis, fr=t['funding'] - t['rt'], half=t['half'], asset=a))
print(len(rows), 'trades with bars on both legs')
print('| entry dev (long vs short, bp) | trades | funding - costs | basis change | total | win | half1 / half2 total |')
for lo, hi in ((-500, -50), (-50, -20), (-20, -5), (-5, 5), (5, 20), (20, 50), (50, 500)):
    x = [r for r in rows if lo <= r['dev'] * 1e4 < hi]
    if not x:
        continue
    m = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
    tot = [r['fr'] + r['basis'] for r in x]
    print(f"| {lo}..{hi} | {len(x)} | {m([r['fr'] for r in x]):+.1f} | {m([r['basis'] for r in x]):+.1f} | {m(tot):+.1f} | {sum(v > 0 for v in tot)/len(tot):.0%} | {m([r['fr'] + r['basis'] for r in x if r['half'] == 1]):+.1f} / {m([r['fr'] + r['basis'] for r in x if r['half'] == 2]):+.1f} |")
import collections
x = [r for r in rows if r['dev'] * 1e4 < -20]
by = collections.defaultdict(list)
for r in x: by[r['asset']].append(r['fr'] + r['basis'])
top = sorted(by.items(), key=lambda kv: -sum(kv[1]))
print('dev < -20 bp:', len(x), 'trades,', len(by), 'assets; top:', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top[:8]))
rest = [v for a, vs in top[5:] for v in vs]
print(f'  without the top 5 assets: {len(rest)} trades, {sum(rest)/max(1,len(rest))*1e4:+.1f} bp, win {sum(v>0 for v in rest)/max(1,len(rest)):.0%}')
print('  worst:', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top[-5:]))