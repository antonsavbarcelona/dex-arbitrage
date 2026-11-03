import sys, os, gzip, glob
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from gzread import rows
out_dir = 'D:/dexarb_clean/ws'
os.makedirs(out_dir, exist_ok=True)
for p in sorted(glob.glob('D:/dexarb_data/ws/2*.csv*')):
    name = os.path.basename(p).replace('.csv.gz', '.csv')
    dst = os.path.join(out_dir, name.replace('.csv', '_' + ('gz' if p.endswith('.gz') else 'raw') + '.csv.gz'))
    n = 0
    with gzip.open(dst, 'wt', compresslevel=1, newline='') as w:
        for r in rows(p):
            if len(r) >= 5:
                w.write(','.join(r) + '\n'); n += 1
    print(dst, n, 'lines', flush=True)
