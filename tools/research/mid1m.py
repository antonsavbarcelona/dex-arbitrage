import glob, zlib, csv, sys
# every market's last bid / ask each minute (by the receive time) from the WebSocket books -> D:/dexarb_data/mid1m.csv
import os
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gzread import rows
last = {}  # (minute, venue, asset) -> (bid, ask)
n = 0
for p in sorted(glob.glob('D:/dexarb_data/ws/2*.csv*')):
    for r in rows(p):
        if len(r) >= 9 and r[0] == 'B':
            try:
                t = int(r[1]); b = float(r[5]); a = float(r[7])
            except ValueError:
                continue
            if b > 0 and a >= b:
                last[(t // 60000, r[3], r[4])] = (b, a)
                n += 1
    print(p, n, len(last), file=sys.stderr, flush=True)
with open('D:/dexarb_data/mid1m.csv', 'w', newline='') as f:
    w = csv.writer(f)
    w.writerow(['minute', 'venue', 'asset', 'bid', 'ask'])
    for (m, v, a), (b, x) in sorted(last.items()):
        w.writerow([m, v, a, b, x])
print('rows', len(last))
