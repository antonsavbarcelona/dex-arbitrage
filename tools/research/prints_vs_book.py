"""Every venue's trade prints vs its own book at that moment: how far outside its own best bid / ask a print is."""
import sys, os, gzip, collections, bisect
src = 'D:/dexarb_clean/ws/20760_gz.csv.gz'
book = {}      # (venue, asset) -> (bid, ask, ts)
out = collections.defaultdict(lambda: [0, 0, 0, 0, []])   # venue -> n, outside 5bp, outside 20bp, stale-book, sample
first = None
for i, line in enumerate(gzip.open(src, 'rt')):
    r = line.rstrip().split(',')
    if i < 3:
        print(r)
    if len(r) < 8:
        continue
    k = (r[3], r[4])
    try:
        t = int(r[1])
    except ValueError:
        continue
    if r[0] == 'B':
        book[k] = (float(r[5]), float(r[7]), t)
    elif r[0] == 'T' and k in book:
        b, a, bt = book[k]
        px = float(r[5])
        o = out[r[3]]
        o[0] += 1
        if t - bt > 5000:
            o[3] += 1
            continue
        mid = (a + b) / 2
        d = max(b - px, px - a, 0) / mid
        if d > 5e-4: o[1] += 1
        if d > 2e-3:
            o[2] += 1
            if len(o[4]) < 4: o[4].append((r[4], px, b, a, r[6:]))
    if i > 12_000_000:
        break
for v, (n, o5, o20, st, s) in sorted(out.items()):
    print(f'{v:12} prints {n:8}  outside own BBO >5bp {o5/n:6.1%}  >20bp {o20/n:6.1%}  (book older than 5 s: {st/n:.0%})  e.g. {s[:2]}')
