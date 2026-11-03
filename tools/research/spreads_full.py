import gzip, collections, statistics, json, bisect
# every market's median bid-ask spread (bp), time-weighted by sampling the book each 10 s, from the full WS data
last = {}
samples = collections.defaultdict(list)
nxt = {}
for line in gzip.open('D:/dexarb_clean2/ws/20760.csv.gz', 'rt'):
    r = line.split(',')
    if len(r) < 9 or r[0] != 'B':
        continue
    try:
        t = int(r[1]); b = float(r[5]); a = float(r[7])
    except ValueError:
        continue
    if not (0 < b <= a):
        continue
    k = f'{r[3]}|{r[4]}'
    # sample the PREVIOUS state at each 10 s mark passed (time-weighted)
    if k in last:
        while nxt[k] <= t:
            pb, pa = last[k]
            if len(samples[k]) < 3000:
                samples[k].append((pa - pb) / ((pa + pb) / 2) * 1e4)
            nxt[k] += 10_000
    else:
        nxt[k] = (t // 10_000 + 1) * 10_000
    last[k] = (b, a)
out = {k: statistics.median(v) for k, v in samples.items() if len(v) >= 30}
old = json.load(open('D:/dexarb_data/spreads.json'))
json.dump(out, open('D:/dexarb_data/spreads.json', 'w'))
json.dump(old, open('D:/dexarb_data/spreads_old.json', 'w'))
per = collections.defaultdict(list)
for k, v in out.items(): per[k.split('|')[0]].append(v)
print(len(out), 'markets (was', len(old), ')')
for v, x in sorted(per.items()):
    x.sort(); print(f'  {v:12} {len(x):4} markets  median {x[len(x)//2]:5.1f} bp  p75 {x[int(len(x)*.75)]:5.1f}  p90 {x[int(len(x)*.9)]:6.1f}')
