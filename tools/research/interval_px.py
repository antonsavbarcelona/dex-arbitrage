"""The interval trade with a signal known before T: the price gap g = log(P_A / P_B) at T - 15 min (A the 4/8 h venue,
B the 1 h hedge; 15-min closes). A cheap (g < 0) -> A's funding should be negative (shorts pay longs): long A / short B
over T. PnL = A's payment received + the hedge's funding + the basis change - the round trip (maker-taker)."""
import pickle, collections, sys
rows = pickle.load(open('D:/dexarb_data/interval_1_1.pkl', 'rb'))
import os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import load_bars, BAR
import math
cache = {}
def bars(a, v):
    if (a, v) not in cache: cache[(a, v)] = load_bars(a, v)
    return cache[(a, v)]
out = []
for r in rows:
    bA, bB = bars(r['a'], r['A']), bars(r['a'], r['B'])
    k = r['T'] // BAR - 2      # the close at T - 15 min
    if k not in bA or k not in bB or (k - 1) not in bA:
        continue
    g = math.log(bA[k] / bB[k])
    # the trade by the signal: g < 0 -> long A (sd = +1); g > 0 -> short A
    sd = 1 if g < 0 else -1
    # rows hold the trade in the PAYMENT's receiving direction (r['sd']); flip when the signal says the other way
    k_ = 1 if sd == r['sd'] else -1
    net = k_ * (r['recv'] + r['hedge'] + r['basis']) - r['rt']
    out.append(dict(g=g, net=net, pay=r['pay'], recv=k_ * r['recv'], hedge=k_ * r['hedge'], basis=k_ * r['basis'], T=r['T'], a=r['a'], A=r['A']))
mid = sorted(o['T'] for o in out)[len(out) // 2]
m = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
print(len(out), 'settlement x hedge')
print('| |gap| at T-15 | n | payment recv | hedge | basis | net m-t | win | half1/half2 | assets | the payment had the signal sign |')
for lo, hi in ((0, 0.002), (0.002, 0.005), (0.005, 0.01), (0.01, 0.02), (0.02, 0.05), (0.05, 1)):
    x = [o for o in out if lo <= abs(o['g']) < hi]
    if len(x) < 10: continue
    print(f"| {lo*1e4:.0f}-{hi*1e4:.0f} | {len(x)} | {m([o['recv'] for o in x]):+.1f} | {m([o['hedge'] for o in x]):+.1f} | {m([o['basis'] for o in x]):+.1f} | {m([o['net'] for o in x]):+.1f} | {sum(o['net'] > 0 for o in x)/len(x):.0%} | {m([o['net'] for o in x if o['T'] < mid]):+.1f} / {m([o['net'] for o in x if o['T'] >= mid]):+.1f} | {len({o['a'] for o in x})} | {sum(o['recv'] > 0 for o in x)/len(x):.0%} |")
x = [o for o in out if abs(o['g']) >= 0.01]
by = collections.defaultdict(list)
for o in x: by[o['a']].append(o['net'])
top = sorted(by.items(), key=lambda kv: -sum(kv[1]))
print('gap >= 1%: top', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top[:8]))
rest = [v for a, vs in top[5:] for v in vs]
print(f'  without the top 5: {len(rest)} {m(rest):+.1f} bp, win {sum(v > 0 for v in rest)/max(1,len(rest)):.0%}')
