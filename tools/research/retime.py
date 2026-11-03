"""The prints re-timed to their exchange time on our clock (exchange ts + the venue's minimum lag, p10: the clock skew
plus the network), so the simulator cannot see B's book later than the print happened. Books kept at receive time."""
import gzip, os
SKEW = {'ApeX': 371, 'Aster': 407, 'Extended': 391, 'GRVT': 372, 'Hyperliquid': 559, 'Lighter': 366, 'Pacifica': 362, 'edgeX': 379}
os.makedirs('D:/dexarb_clean2/ws', exist_ok=True)
n = m = 0
with gzip.open('D:/dexarb_clean2/ws/20760.csv.gz', 'wt', compresslevel=1) as w:
    for src in ('D:/dexarb_clean/ws/20760_raw.csv.gz', 'D:/dexarb_clean/ws/20760_gz.csv.gz'):
        for line in gzip.open(src, 'rt'):
            r = line.rstrip('\n').split(',')
            if r[0] == 'T' and len(r) >= 8 and r[3] in SKEW:
                try:
                    rc, ex = int(r[1]), int(r[2])
                    if 0 < rc - ex < 60_000:
                        r[1] = str(min(rc, ex + SKEW[r[3]])); m += 1
                except ValueError:
                    pass
            w.write(','.join(r) + '\n'); n += 1
print(n, 'lines;', m, 'prints re-timed')
