import pickle, collections, datetime
rows = pickle.load(open('D:/dexarb_data/interval_1_1.pkl', 'rb'))
# the trade at the payment (signal A), the best hedge only (the lowest hedge cost) per settlement
best = {}
for r in rows:
    if abs(r['pay']) < 0.01:
        continue
    k = (r['a'], r['A'], r['T'])
    net = r['recv'] + r['hedge'] + r['basis'] - r['rt']
    if k not in best or net > best[k][0]:
        best[k] = (net, r)
print(len(best), 'settlements paying >= 100 bp (each with its best hedge, chosen in hindsight: an upper bound)')
by = collections.Counter((r['A'], r['a']) for _, r in best.values())
print(by.most_common(20))
allh = collections.defaultdict(list)
for r in rows:
    if abs(r['pay']) >= 0.01:
        allh[(r['a'], r['A'], r['T'])].append(r['recv'] + r['hedge'] + r['basis'] - r['rt'])
v = [sum(x) / len(x) for x in allh.values()]
print('the mean over all hedges per settlement: %+.1f bp, win %.0f%%' % (sum(v) / len(v) * 1e4, sum(a > 0 for a in v) / len(v) * 100))
for k in sorted(allh, key=lambda k: k[2])[:40]:
    a, A, T = k
    print(f"{datetime.datetime.utcfromtimestamp(T/1000):%m-%d %H:%M} {a:10} {A:6} pay {[r['pay'] for r in rows if (r['a'], r['A'], r['T']) == k][0]*1e4:+7.1f} bp  hedges {len(allh[k])}  mean net {sum(allh[k])/len(allh[k])*1e4:+7.1f}")
