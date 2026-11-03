"""edgeX: the minute previews of the funding rate before each settlement vs the settled rate (is the payment known
15 min ahead?). For the big settlements (|rate| x interval >= 50 bp) of the last 30 days."""
import json, urllib.request, pickle, datetime, time
API = 'https://edgex-prod-v2.edgex.exchange/api/v2/public'
def get(u):
    for _ in range(4):
        try:
            return json.load(urllib.request.urlopen(urllib.request.Request(u, headers={'User-Agent': 'dexarb-research'}), timeout=20))
        except Exception as e:
            time.sleep(2)
    return {}
meta = get(f'{API}/meta/getMetaData')['data']['contractList']
ids = {c['contractName']: c['contractId'] for c in meta}
rows = pickle.load(open('D:/dexarb_data/interval_1_1.pkl', 'rb'))
ev = sorted({(r['a'], r['T'], r['pay']) for r in rows if r['A'] == 'edgeX' and abs(r['pay']) >= 0.005})
print(len(ev), 'edgeX settlements paying >= 50 bp')
out = []
for a, T, pay in ev:
    cid = ids.get(f'{a}USD') or ids.get(f'{a}USDC') or ids.get(f'{a}2USD')
    if not cid:
        print('no id', a); continue
    r = get(f'{API}/funding/getFundingRatePage?contractId={cid}&size=100&filterBeginTimeInclusive={T - 40*60000}&filterEndTimeExclusive={T + 60000}')
    pts = r.get('data', {}).get('dataList', [])
    pv = sorted((int(x['fundingTimestamp']) if x.get('fundingTimestamp') else int(x['fundingTime']), float(x['fundingRate']), float(x.get('forecastFundingRate') or 'nan'), x.get('isSettlement')) for x in pts)
    def at(mins):
        c = [p for p in pv if p[0] <= T - mins * 60000]
        return c[-1] if c else None
    settled = [p for p in pv if p[3]]
    s = settled[-1][1] if settled else None
    p15, p5, p30 = at(15), at(5), at(30)
    out.append((a, T, pay, s, p30, p15, p5))
    f = lambda p: f"{p[1]*1e4:+7.1f}/{p[2]*1e4:+7.1f}" if p else '   -   '
    print(f"{datetime.datetime.utcfromtimestamp(T/1000):%m-%d %H:%M} {a:8} paid {pay*1e4:+7.1f} bp (settled field {s*1e4 if s is not None else float('nan'):+7.1f})  preview rate/forecast  T-30 {f(p30)}  T-15 {f(p15)}  T-5 {f(p5)}  ({len(pv)} points)")
    time.sleep(0.3)
pickle.dump(out, open('D:/dexarb_data/edgex_pred.pkl', 'wb'))
