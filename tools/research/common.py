"""Shared loaders for the cross-venue basis studies: 15-min bars (data/lev_bars), funding (data/funding), costs."""
import json, os, glob, bisect, math, statistics
DD = 'D:/dexarb_data'
BAR = 900_000
MAKER = {'Hyperliquid': 0.00015, 'Lighter': 0.0, 'Extended': 0.0, 'Aster': 0.0, 'GRVT': 0.0003, 'ApeX': 0.0002,
         'edgeX': 0.0004, 'Pacifica': 0.00015, 'StandX': 0.0001}

def load_bars(asset, venue):
    p = f"{DD}/lev_bars/{asset.replace(':', '_').replace('/', '_')}__{venue}.csv"
    if not os.path.exists(p):
        return None
    out = {}
    for l in open(p):
        x = l.strip().split(',')
        if len(x) == 4:
            try:
                out[int(x[0]) // BAR] = float(x[1])
            except ValueError:
                pass
    if len(out) < 50:
        return None
    # a bad print: a close 5%+ off the median of its 5 neighbours -> dropped
    ks = sorted(out)
    clean = {}
    for i, k in enumerate(ks):
        w = sorted(out[ks[j]] for j in range(max(0, i - 2), min(len(ks), i + 3)))
        med = w[len(w) // 2]
        if abs(out[k] / med - 1) <= 0.05:
            clean[k] = out[k]
    return clean

META = json.load(open(f'{DD}/funding/meta.json'))
SPREADS = json.load(open(f'{DD}/spreads.json'))
PRICES = json.load(open(f'{DD}/prices.json'))
VENUE_SP = {}
for k, v in SPREADS.items():
    VENUE_SP.setdefault(k.split('|')[0], []).append(v)
VENUE_SP = {k: sorted(v)[len(v) // 2] for k, v in VENUE_SP.items()}

def spread(venue, asset):
    return SPREADS.get(f'{venue}|{asset}', VENUE_SP.get(venue, 10.0)) / 1e4

def taker(venue, asset):
    for m in META:
        if m['venue'] == venue and m['base'] == asset:
            return m.get('taker') or 0.0005
    return {'Hyperliquid': 0.00045}.get(venue, 0.0005)

_fund = {}
def funding(asset, venue):
    """[(settlement ms, rate x interval)] sorted; None if not collected."""
    k = (asset, venue)
    if k in _fund:
        return _fund[k]
    r = None
    for m in META:
        if m['venue'] == venue and m['base'] == asset:
            p = f"{DD}/funding/{m['file']}"
            pts = []
            for l in open(p):
                x = l.strip().split(',')
                try:
                    pts.append((int(x[0]), float(x[1])))
                except (ValueError, IndexError):
                    pass
            pts.sort()
            if len(pts) > 2:
                gaps = sorted(b[0] - a[0] for a, b in zip(pts, pts[1:]))
                iv = max(1.0, round(gaps[len(gaps) // 2] / 3.6e6))
                r = ([p[0] for p in pts], [p[1] * iv for p in pts])
            break
    _fund[k] = r
    return r

def paid(f, a_ms, b_ms):
    """The funding a position held over (a, b] pays (+ = the long pays)."""
    if not f:
        return 0.0
    ts, v = f
    i = bisect.bisect_right(ts, a_ms); j = bisect.bisect_right(ts, b_ms)
    return sum(v[i:j])

def same_token(asset, a, b):
    pa, pb = PRICES.get(f'{a}|{asset}'), PRICES.get(f'{b}|{asset}')
    return not (pa and pb and abs(pa / pb - 1) > 0.10)
