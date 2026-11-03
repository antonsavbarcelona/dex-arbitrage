"""Cross-venue basis reversion on 15-min closes, every pair of venues of each asset (or the stocks: an HL HIP-3 market).
b = log(A / B); its normal level = the median of b over the previous 3 days (no look-ahead); enter when b - normal
passes theta (long the cheap leg, short the rich one) at the NEXT bar's close (delay bars), out when the deviation is
back under theta / 4 or after 3 days; the PnL = the basis change + both legs' funding over the hold - the round trip.
python revert.py [stocks|all] [delay]"""
import sys, os, math, statistics, collections, datetime
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import *

kind = sys.argv[1] if len(sys.argv) > 1 else 'stocks'
delay = int(sys.argv[2]) if len(sys.argv) > 2 else 1
stocks = {m['base'] for m in META if m['venue'] == 'Hyperliquid' and ':' in m['symbol']}
files = glob.glob(f'{DD}/lev_bars/*.csv')
by_asset = collections.defaultdict(list)
for f in files:
    a, v = os.path.basename(f)[:-4].split('__')
    by_asset[a].append(v)
assets = [a for a in by_asset if len(by_asset[a]) >= 2 and ((a in stocks) == (kind == 'stocks') or kind == 'all')]
print(kind, len(assets), 'assets; delay', delay, 'bars')

def seg(k):
    t = datetime.datetime.utcfromtimestamp(k * BAR / 1000)
    if t.weekday() >= 5:
        return 'weekend'
    m = t.hour * 60 + t.minute
    return 'US open' if 13 * 60 + 30 <= m < 20 * 60 else 'US shut'

desc = collections.defaultdict(list)       # (segment) -> |dev| samples
trades = []
for a in assets:
    vs = by_asset[a]
    bars = {v: load_bars(a, v) for v in vs}
    vs = [v for v in vs if bars[v]]
    for i in range(len(vs)):
        for j in range(i + 1, len(vs)):
            A, B = vs[i], vs[j]
            if not same_token(a, A, B):
                continue
            ba, bb = bars[A], bars[B]
            ks = sorted(set(ba) & set(bb))
            if len(ks) < 400:
                continue
            b = [math.log(ba[k] / bb[k]) for k in ks]
            rta = 2 * min(MAKER.get(A, 5e-4) + taker(B, a) + spread(B, a) / 2, MAKER.get(B, 5e-4) + taker(A, a) + spread(A, a) / 2) + 2e-4
            rtt = 2 * (taker(A, a) + taker(B, a)) + spread(A, a) + spread(B, a)
            fa, fb = funding(a, A), funding(a, B)
            win = collections.deque()
            hist = []
            pos = None
            n0 = 288
            for t, k in enumerate(ks):
                if t >= n0:
                    normal = statistics.median(b[t - n0:t]) if t % 4 == 0 or 'normal' not in dir() else normal
                    dev = b[t] - normal
                    desc[seg(k)].append(abs(dev))
                    if pos is None:
                        for th in (0.005, 0.01, 0.02, 0.03):
                            pass
                        if abs(dev) >= 0.005 and t + delay < len(ks):
                            e = t + delay
                            pos = (e, 1 if dev < 0 else -1, dev)  # +1: long A short B (A cheap)
                    else:
                        e, sd, d0 = pos
                        if t > e and (abs(b[t] - normal) < abs(d0) / 4 or (ks[t] - ks[e]) >= 288 or t == len(ks) - 1 or (b[t] - normal) * -sd < abs(d0) / 4):
                            ms0, ms1 = ks[e] * BAR, ks[t] * BAR
                            basis_pnl = sd * (b[t] - b[e])
                            # long A short B: A's funding paid by us, B's received
                            fund = sd * (paid(fb, ms0, ms1) - paid(fa, ms0, ms1))
                            trades.append(dict(asset=a, A=A, B=B, k0=ks[e], k1=ks[t], th=abs(d0), seg=seg(ks[e]), basis=basis_pnl, fund=fund, rta=rta, rtt=rtt, sd=sd))
                            pos = None
for s, x in desc.items():
    x.sort()
    print(f'|dev| from the 3-day normal, {s:8}: median {x[len(x)//2]*1e4:.0f} bp, p90 {x[int(len(x)*.9)]*1e4:.0f}, p99 {x[int(len(x)*.99)]*1e4:.0f} bp ({len(x)} bars)')
if not trades:
    sys.exit()
mid = sorted(t['k0'] for t in trades)[len(trades) // 2]
print(f"\n{len(trades)} trades (entered at |dev| >= 50 bp); by the entry deviation (bp of one leg's notional, means):")
print('| entry |dev| | seg | trades | basis | funding | net maker-taker | net taker | win m-t | half1 / half2 m-t | hold h |')
for lo, hi in ((0.005, 0.01), (0.01, 0.02), (0.02, 0.03), (0.03, 0.05), (0.05, 1)):
    for sg in ('all', 'US open', 'US shut', 'weekend'):
        x = [t for t in trades if lo <= t['th'] < hi and (sg == 'all' or t['seg'] == sg)]
        if len(x) < 5:
            continue
        m = lambda f: sum(f(t) for t in x) / len(x) * 1e4
        nm = [t['basis'] + t['fund'] - t['rta'] for t in x]
        h1 = [t['basis'] + t['fund'] - t['rta'] for t in x if t['k0'] < mid]
        h2 = [t['basis'] + t['fund'] - t['rta'] for t in x if t['k0'] >= mid]
        mm = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
        print(f"| {lo*1e4:.0f}-{hi*1e4:.0f} | {sg} | {len(x)} | {m(lambda t: t['basis']):+.1f} | {m(lambda t: t['fund']):+.1f} | {mm(nm):+.1f} | {m(lambda t: t['basis'] + t['fund'] - t['rtt']):+.1f} | {sum(v > 0 for v in nm) / len(nm):.0%} | {mm(h1):+.1f} / {mm(h2):+.1f} | {sum((t['k1'] - t['k0']) / 4 for t in x) / len(x):.1f} |")
# the venues: which pairs carry it
print('\nby pair (entry |dev| >= 100 bp, maker-taker):')
pp = collections.defaultdict(list)
for t in trades:
    if t['th'] >= 0.01:
        pp[(t['A'], t['B'])].append(t['basis'] + t['fund'] - t['rta'])
for k, v in sorted(pp.items(), key=lambda kv: -len(kv[1]))[:20]:
    print(f'  {k[0]:11} / {k[1]:11} {len(v):4} trades  mean {sum(v)/len(v)*1e4:+7.1f} bp  win {sum(x > 0 for x in v)/len(v):.0%}')
import pickle; pickle.dump(trades, open(f'{DD}/revert_{kind}_{delay}.pkl', 'wb'))
