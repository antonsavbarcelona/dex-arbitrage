import json, collections, sys, os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import SPREADS, VENUE_SP
for f in sys.argv[1:]:
    tr = json.load(open(f))
    net = lambda t: t['funding'] - t['rt']
    m = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
    print(f, len(tr), 'trades, mean', f"{m([net(t) for t in tr]):+.1f} bp")
    by = collections.defaultdict(list)
    for t in tr: by[t['asset']].append(net(t))
    top = sorted(by.items(), key=lambda kv: -sum(kv[1]))
    print('  assets', len(by), '; top:', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top[:10]))
    for k in (3, 5, 10):
        rest = [v for a, vs in top[k:] for v in vs]
        print(f'  without the top {k}: {len(rest)} trades {m(rest):+.1f} bp, win {sum(x > 0 for x in rest)/max(1,len(rest)):.0%}')
    print('  worst:', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top[-6:]))
    pv = collections.Counter(tuple(sorted((t['long'], t['short']))) for t in tr)
    print('  pairs:', pv.most_common(8))
    # the spread of the Lighter leg: measured or the venue median
    meas = sum(1 for t in tr for v in (t['long'], t['short']) if v == 'Lighter' and f"Lighter|{t['asset']}" in SPREADS)
    tot = sum(1 for t in tr for v in (t['long'], t['short']) if v == 'Lighter')
    sp = [SPREADS.get(f"Lighter|{t['asset']}") for t in tr if 'Lighter' in (t['long'], t['short'])]
    sp = sorted(x for x in sp if x is not None)
    print(f'  Lighter legs with a measured spread {meas}/{tot}; the venue median {VENUE_SP.get("Lighter"):.1f} bp; measured median {sp[len(sp)//2] if sp else float("nan"):.1f}, p90 {sp[int(len(sp)*.9)] if sp else float("nan"):.1f}')
