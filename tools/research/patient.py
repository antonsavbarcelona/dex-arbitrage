"""The carry (fstab's trades) with a patient entry and exit: the funding signal says go at H; we wait up to W hours for
the price corridor: dev = log(P_long / P_short) - its 3-day median <= -x (the long leg cheap); the fill at the NEXT
bar's close after the trigger (no same-bar bounce); no trigger within W -> enter at W anyway. The exit the mirror:
from the fstab exit time, wait up to W for dev >= +x. The result: the funding over the actual hold + the price change of
the pair - both legs' fees and full spreads (taker; the timing is ours, no fill risk) - nothing else.
python patient.py"""
import sys, os, json, math, statistics, collections, bisect
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *
tr = json.load(open(f'{DD}/fstab_trades.json'))
cache = {}
def bars(a, v):
    if (a, v) not in cache: cache[(a, v)] = load_bars(a, v)
    return cache[(a, v)]
prep = []
for t in tr:
    a, L, S = t['asset'], t['long'], t['short']
    bl, bs = bars(a, L), bars(a, S)
    if not bl or not bs or not same_token(a, L, S): continue
    ks = sorted(set(bl) & set(bs))
    if len(ks) < 600: continue
    b = [math.log(bl[k] / bs[k]) for k in ks]
    i0 = bisect.bisect_left(ks, t['entry_ms'] // BAR); i1 = bisect.bisect_left(ks, t['exit_ms'] // BAR)
    if i0 < 288 or i1 >= len(ks) - 60 or i1 <= i0: continue
    cost1 = taker(L, a) + taker(S, a) + spread(L, a) / 2 + spread(S, a) / 2   # one side (entry or exit), both legs
    prep.append(dict(a=a, L=L, S=S, ks=ks, b=b, i0=i0, i1=i1, cost1=cost1, half=t['half'], fL=funding(a, L), fS=funding(a, S)))
print(len(prep), 'trades with bars on both legs')
# a running 3-day median, once per bar index (cached per trade on demand)
def normal(p, i):
    c = p.setdefault('_n', {})
    if i not in c: c[i] = statistics.median(p['b'][i - 288:i])
    return c[i]
def run(x, W):
    out = []
    for p in prep:
        ks, b = p['ks'], p['b']
        wb = int(W * 4)
        # entry: the first bar in [i0, i0 + wb) with dev <= -x, filled at the next bar; else at i0 + wb
        e = None
        for i in range(p['i0'], min(p['i0'] + wb, p['i1'])):
            if b[i] - normal(p, i) <= -x:
                e = i + 1; break
        if e is None: e = min(p['i0'] + wb, p['i1'] - 1) if x > -1 else p['i0']
        if x <= -1: e = p['i0'] + 1     # the baseline: at the signal (+1 bar)
        # exit: from i1, wait up to wb for dev >= +x
        q = None
        if x > -1:
            for i in range(p['i1'], min(p['i1'] + wb, len(ks) - 1)):
                if b[i] - normal(p, i) >= x:
                    q = i + 1; break
            if q is None: q = min(p['i1'] + wb, len(ks) - 1)
        else:
            q = p['i1'] + 1
        if q <= e: continue
        ms0, ms1 = ks[e] * BAR + BAR, ks[q] * BAR + BAR
        fund = paid(p['fS'], ms0, ms1) - paid(p['fL'], ms0, ms1)   # long L short S: receive S's, pay L's
        price = b[q] - b[e]
        net = fund + price - 2 * p['cost1']
        out.append(dict(a=p['a'], net=net, fund=fund, price=price, cost=2 * p['cost1'], half=p['half'], hold=(ks[q] - ks[e]) / 4))
    return out
m = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
print('| x bp | wait W h | trades | funding | price | costs | net | win | half1 / half2 | without top 5 assets | hold h |')
for x, W in [(-9, 0)] + [(x, W) for x in (0.0, 0.001, 0.002, 0.004) for W in (2, 6, 12, 24)]:
    o = run(x, W)
    by = collections.defaultdict(float)
    for r in o: by[r['a']] += r['net']
    top = {a for a, _ in sorted(by.items(), key=lambda kv: -kv[1])[:5]}
    rest = [r['net'] for r in o if r['a'] not in top]
    print(f"| {'signal' if x < -1 else f'{x*1e4:.0f}'} | {W} | {len(o)} | {m([r['fund'] for r in o]):+.1f} | {m([r['price'] for r in o]):+.1f} | {m([r['cost'] for r in o]):.1f} | {m([r['net'] for r in o]):+.1f} | {sum(r['net']>0 for r in o)/max(1,len(o)):.0%} | {m([r['net'] for r in o if r['half']==1]):+.1f} / {m([r['net'] for r in o if r['half']==2]):+.1f} | {m(rest):+.1f} | {sum(r['hold'] for r in o)/max(1,len(o)):.0f} |", flush=True)
