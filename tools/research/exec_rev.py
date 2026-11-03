"""Executable cross-venue reversion on the WebSocket books (data/mid1m.csv: each market's last bid / ask a minute).
For a pair (A, B) of one asset: the mid basis m = log(midA / midB), its normal = the median of the previous N minutes.
Signal at minute t: the executable edge to buy A / sell B = log(bidB / askA) - (-normal) ... i.e. the rich-cheap gap net
of the spreads, over the takers. Entry at minute t + delay at THAT minute's quotes (taker both legs), out when the
mid basis is back within `back` of the normal (taker both legs at that minute's quotes) or after H minutes."""
import sys, os, csv, math, collections, statistics
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import taker, same_token, PRICES
N, DELAY, H = 60, int(sys.argv[1]) if len(sys.argv) > 1 else 1, 240
q = collections.defaultdict(dict)   # (asset) -> venue -> {minute: (bid, ask)}
for r in csv.DictReader(open('D:/dexarb_data/mid1m.csv')):
    q[r['asset']].setdefault(r['venue'], {})[int(r['minute'])] = (float(r['bid']), float(r['ask']))
allm = sorted({m for a in q.values() for v in a.values() for m in v})
print('minutes', len(allm), 'from', allm[0], 'to', allm[-1], '; assets', len(q))
trades = []
for a, vs in q.items():
    names = list(vs)
    for i in range(len(names)):
        for j in range(len(names)):
            if i == j:
                continue
            A, B = names[i], names[j]
            if not same_token(a, A, B):
                continue
            qa, qb = vs[A], vs[B]
            ks = sorted(set(qa) & set(qb))
            if len(ks) < N + 30:
                continue
            fee = taker(A, a) + taker(B, a)
            mb = [math.log((qa[k][0] + qa[k][1]) / (qb[k][0] + qb[k][1])) for k in ks]
            t = N
            while t < len(ks) - DELAY:
                normal = statistics.median(mb[t - N:t])
                bA, aA = qa[ks[t]]; bB, aB = qb[ks[t]]
                # buy A at its ask, sell B at its bid: the gap vs the normal, net of the spreads
                edge = math.log(bB / aA) + normal
                if edge > 2 * fee + 0.0005:
                    e = t + DELAY
                    if ks[e] - ks[t] > DELAY + 2:
                        t += 1; continue
                    bA, aA = qa[ks[e]]; bB, aB = qb[ks[e]]
                    ent = math.log(bB / aA)          # short B long A: what we lock in
                    x = e + 1
                    while x < len(ks) - 1 and ks[x] - ks[e] < H and mb[x] < normal - 0.0002:
                        x += 1
                    bA2, aA2 = qa[ks[x]]; bB2, aB2 = qb[ks[x]]
                    ext = math.log(aB2 / bA2)        # buy B back at its ask, sell A at its bid
                    pnl = ent - ext - 2 * fee
                    trades.append(dict(asset=a, A=A, B=B, edge=edge, pnl=pnl, hold=ks[x] - ks[e], k=ks[t]))
                    t = x + 1
                else:
                    t += 1
print(len(trades), 'trades (delay', DELAY, 'min)')
if trades:
    m = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
    for lo, hi in ((0, 10), (10, 30), (30, 100), (100, 10000)):
        x = [t for t in trades if lo <= (t['edge'] - 2 * (taker(t['A'], t['asset']) + taker(t['B'], t['asset']))) * 1e4 < hi]
        if x:
            print(f"  signal edge over costs {lo}-{hi} bp: {len(x)} trades, pnl {m([t['pnl'] for t in x]):+.1f} bp, win {sum(t['pnl']>0 for t in x)/len(x):.0%}, hold {sum(t['hold'] for t in x)/len(x):.0f} min, assets {len({t['asset'] for t in x})}")
    by = collections.defaultdict(list)
    for t in trades: by[t['asset']].append(t['pnl'])
    top = sorted(by.items(), key=lambda kv: -sum(kv[1]))
    print('  top:', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top[:8]))
    print('  worst:', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top[-6:]))
    rest = [v for a, vs in top[5:] for v in vs]
    print(f'  without the top 5: {len(rest)} trades {m(rest):+.1f} bp')
    pv = collections.defaultdict(list)
    for t in trades: pv[(t['A'], t['B'])].append(t['pnl'])
    for k, v in sorted(pv.items(), key=lambda kv: -len(kv[1]))[:12]:
        print(f'   buy {k[0]:11} sell {k[1]:11} {len(v):4} trades {m(v):+7.1f} bp win {sum(x>0 for x in v)/len(v):.0%}')
