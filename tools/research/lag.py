import gzip, collections
lag = collections.defaultdict(lambda: {'B': [], 'T': []})
for i, line in enumerate(gzip.open('D:/dexarb_clean/ws/20760_gz.csv.gz', 'rt')):
    r = line.split(',')
    if len(r) < 8 or r[0] not in 'BT':
        continue
    try:
        rc, ex = int(r[1]), int(r[2])
    except ValueError:
        continue
    l = lag[r[3]][r[0]]
    if len(l) < 200000:
        l.append(rc - ex)
    if i > 15_000_000:
        break
q = lambda x, p: x[int((len(x) - 1) * p)]
for v, d in sorted(lag.items()):
    for k in 'TB':
        x = sorted(d[k])
        if x:
            print(f'{v:12} {k}: n {len(x):7}  recv - exchange ms: p10 {q(x,.1):7}  median {q(x,.5):7}  p90 {q(x,.9):7}  p99 {q(x,.99):8}')
