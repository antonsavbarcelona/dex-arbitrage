"""The cost of opening a hedged pair (long A / short B) three ways, on the WebSocket books and prints:
  taker-taker: both at market now;  maker-taker: a bid at A's best, B sold at market when it fills (A unfilled after W:
  A at market too);  maker-maker: a bid at A's best AND an ask at B's best, a leg unfilled after W crossed at market.
A fill: a print strictly through the quote of >= USD notional (re-timed prints). Cost vs the two mids at the start,
fees included (bp of one leg). Every 10 minutes per pair; only pairs of the same token with a book on both sides."""
import gzip, sys, os, collections, bisect, json, random
sys.path.insert(0, 'C:/Users/lugov/AppData/Local/Temp/claude/c6503ce9239121ad/04f4cfe0-85f2-46cf-a2c1-96f7974b23b7/scratchpad/basis2')
from common import taker, MAKER, same_token
USD = float(sys.argv[1]) if len(sys.argv) > 1 else 500
W = int(sys.argv[2]) if len(sys.argv) > 2 else 120_000
LAT = 300
books = collections.defaultdict(lambda: ([], []))   # (venue, asset) -> (times, (bid, ask))
prints = collections.defaultdict(lambda: ([], []))  # (venue, asset) -> (times, (px, usd))
cnt = collections.Counter()
for line in gzip.open('D:/dexarb_clean2/ws/20760.csv.gz', 'rt'):
    r = line.split(',')
    if len(r) < 8:
        continue
    k = (r[3], r[4])
    try:
        t = int(r[1])
        if r[0] == 'B':
            b, a = float(r[5]), float(r[7])
            if 0 < b <= a:
                books[k][0].append(t); books[k][1].append((b, a))
        elif r[0] == 'T':
            px, sz = float(r[5]), float(r[6])
            prints[k][0].append(t); prints[k][1].append((px, px * sz))
    except ValueError:
        pass
for d in (books, prints):
    for k, (ts, v) in d.items():
        o = sorted(range(len(ts)), key=ts.__getitem__)
        d[k] = ([ts[i] for i in o], [v[i] for i in o])
def bk(k, t):
    ts, v = books[k]; i = bisect.bisect_right(ts, t) - 1
    return v[i] if i >= 0 and t - ts[i] < 60_000 else None
def first_through(k, t0, t1, px, buy):
    """the first print in (t0, t1] strictly through a resting buy at px (a print under it) / a sell (over it)"""
    ts, v = prints[k]; i = bisect.bisect_right(ts, t0)
    while i < len(ts) and ts[i] <= t1:
        p, u = v[i]
        if u >= USD and ((buy and p < px) or (not buy and p > px)):
            return ts[i]
        i += 1
    return None
assets = collections.defaultdict(list)
for (v, a) in books:
    if (v, a) in prints and len(prints[(v, a)][0]) > 50:
        assets[a].append(v)
res = collections.defaultdict(list)
t_min = min(ts[0] for ts, _ in books.values()); t_max = max(ts[-1] for ts, _ in books.values())
for a, vs in assets.items():
    for A in vs:
        for B in vs:
            if A == B or not same_token(a, A, B):
                continue
            kA, kB = (A, a), (B, a)
            fA, fB = taker(A, a), taker(B, a)
            mA, mB = MAKER.get(A, 5e-4), MAKER.get(B, 5e-4)
            for t in range(t_min + 1_800_000, t_max - W - 60_000, 600_000):
                qa, qb = bk(kA, t), bk(kB, t)
                if not qa or not qb:
                    continue
                midA, midB = sum(qa) / 2, sum(qb) / 2
                # buy A / sell B: the cost = (paid - midA)/midA + (midB - got)/midB + fees
                tt = (qa[1] - midA) / midA + (midB - qb[0]) / midB + fA + fB
                res['taker-taker'].append(tt)
                # maker-taker: our bid at A's best bid
                fa = first_through(kA, t, t + W, qa[0], True)
                if fa:
                    h = bk(kB, fa + LAT)
                    if not h: continue
                    mt_ = (qa[0] - midA) / midA + (midB - h[0]) / midB + mA + fB
                else:
                    x, h = bk(kA, t + W), bk(kB, t + W)
                    if not x or not h: continue
                    mt_ = (x[1] - midA) / midA + (midB - h[0]) / midB + fA + fB
                res['maker-taker'].append(mt_)
                # maker-maker: also an ask at B's best ask
                fb = first_through(kB, t, t + W, qb[1], False)
                if fa and fb:
                    mm = (qa[0] - midA) / midA + (midB - qb[1]) / midB + mA + mB
                    res['mm: both filled'].append(mm)
                elif fa:
                    h = bk(kB, t + W)
                    if not h: continue
                    mm = (qa[0] - midA) / midA + (midB - h[0]) / midB + mA + fB
                    res['mm: B chased'].append(mm)
                elif fb:
                    x = bk(kA, t + W)
                    if not x: continue
                    mm = (x[1] - midA) / midA + (midB - qb[1]) / midB + fA + mB
                    res['mm: A chased'].append(mm)
                else:
                    x, h = bk(kA, t + W), bk(kB, t + W)
                    if not x or not h: continue
                    mm = (x[1] - midA) / midA + (midB - h[0]) / midB + fA + fB
                    res['mm: both chased'].append(mm)
                res['maker-maker'].append(mm)
print(f'size ${USD:.0f}, wait {W/1000:.0f} s; pair-starts {len(res["taker-taker"])}')
for k, v in res.items():
    v2 = sorted(v)
    print(f'  {k:18} n {len(v):6}  mean cost {sum(v)/len(v)*1e4:+6.1f} bp  median {v2[len(v2)//2]*1e4:+6.1f}  p90 {v2[int(len(v2)*.9)]*1e4:+6.1f}')
