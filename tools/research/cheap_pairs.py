"""fstab's trades by the round trip's cost (maker-taker, the cheaper venue for the limit): which venue pairs are cheap,
and what the carry nets on them (the funding - the round trip + the pair's price change over the hold, from the bars)."""
import sys, os, json, math, collections, bisect
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
tr = json.load(open(f'{DD}/fstab_trades.json'))
cache = {}
def bars(a, v):
    if (a, v) not in cache: cache[(a, v)] = load_bars(a, v)
    return cache[(a, v)]
rows = []
for t in tr:
    a, L, S = t['asset'], t['long'], t['short']
    if not same_token(a, L, S): continue
    price = None
    bl, bs = bars(a, L), bars(a, S)
    if bl and bs:
        k0, k1 = t['entry_ms'] // BAR, t['exit_ms'] // BAR
        if k0 in bl and k0 in bs and k1 in bl and k1 in bs:
            price = math.log(bl[k1] / bs[k1]) - math.log(bl[k0] / bs[k0])
    rows.append(dict(a=a, pair=tuple(sorted((L, S))), rt=t['rt'], fr=t['funding'] - t['rt'], fund=t['funding'], price=price, half=t['half']))
m = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
print('| round trip bp | trades | funding | rt | funding - rt | + price (where bars) | half1 / half2 (f - rt) | assets |')
for lo, hi in ((0, 4), (4, 8), (8, 15), (15, 30), (30, 1000)):
    x = [r for r in rows if lo <= r['rt'] * 1e4 < hi]
    if not x: continue
    px = [r['fr'] + r['price'] for r in x if r['price'] is not None]
    print(f"| {lo}-{hi} | {len(x)} | {m([r['fund'] for r in x]):+.1f} | {m([r['rt'] for r in x]):.1f} | {m([r['fr'] for r in x]):+.1f} | {m(px):+.1f} ({len(px)}) | {m([r['fr'] for r in x if r['half']==1]):+.1f} / {m([r['fr'] for r in x if r['half']==2]):+.1f} | {len({r['a'] for r in x})} |")
print('\nthe cheapest venue pairs (mean rt):')
pp = collections.defaultdict(list)
for r in rows: pp[r['pair']].append(r)
for k, v in sorted(pp.items(), key=lambda kv: sum(r['rt'] for r in kv[1]) / len(kv[1]))[:14]:
    print(f"  {k[0]:11} + {k[1]:11} {len(v):4} trades  rt {m([r['rt'] for r in v]):5.1f}  funding {m([r['fund'] for r in v]):+6.1f}  net {m([r['fr'] for r in v]):+6.1f}  halves {m([r['fr'] for r in v if r['half']==1]):+.1f} / {m([r['fr'] for r in v if r['half']==2]):+.1f}")
