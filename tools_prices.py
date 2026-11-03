import collections, statistics, json, glob, zlib
# every market's median mid (per unit of the underlying) from the WebSocket books: the token check for fstab
def rows(path):
    if path.endswith('.gz'):
        # streamed in 8 MB pieces (members concatenated; the last one may be still open / truncated)
        f = open(path, 'rb'); d = zlib.decompressobj(16 + zlib.MAX_WBITS); rest = b''
        while True:
            piece = d.unused_data + f.read(8 << 20) if d.eof else f.read(8 << 20)
            if d.eof:
                d = zlib.decompressobj(16 + zlib.MAX_WBITS)
            if not piece:
                break
            try:
                o = d.decompress(piece)
            except zlib.error:
                break
            lines = (rest + o).split(b'\n'); rest = lines.pop()
            for line in lines: yield line.decode('utf-8', 'replace').split(',')
    else:
        for line in open(path, encoding='utf-8', errors='replace'): yield line.rstrip().split(',')
mids = collections.defaultdict(list)
for p in sorted(glob.glob('D:/dexarb_data/ws/2*.csv*')):
    for r in rows(p):
        if len(r) >= 9 and r[0] == 'B':
            try: b, a = float(r[5]), float(r[7])
            except ValueError: continue
            if b > 0 and a >= b:
                k = f'{r[3]}|{r[4]}'
                if len(mids[k]) < 400: mids[k].append((a + b) / 2)
out = {k: statistics.median(v) for k, v in mids.items() if len(v) >= 5}
json.dump(out, open('D:/dexarb_data/prices.json', 'w'))
print(len(out), 'markets priced')
by = collections.defaultdict(dict)
for k, p in out.items():
    v, a = k.split('|', 1); by[a][v] = p
for a, d in sorted(by.items()):
    ps = sorted(d.values())
    if ps[-1] / ps[0] > 1.02:
        print('  ', a, {v: f'{p:.5g}' for v, p in d.items()})
