import pickle, sys, collections
DD = 'D:/dexarb_data'
for name in sys.argv[1:]:
    tr = pickle.load(open(f'{DD}/{name}.pkl', 'rb'))
    x = [t for t in tr if 0.01 <= t['th'] < 0.05]
    net = lambda t: t['basis'] + t['fund'] - t['rta']
    if not x:
        print(name, 'none'); continue
    mid = sorted(t['k0'] for t in x)[len(x) // 2]
    h = lambda f: [net(t) for t in x if f(t)]
    m = lambda v: sum(v) / len(v) * 1e4 if v else float('nan')
    print(f"{name}: |dev| 1-5%: {len(x)} trades, net m-t {m([net(t) for t in x]):+.1f} bp, halves {m(h(lambda t: t['k0'] < mid)):+.1f} / {m(h(lambda t: t['k0'] >= mid)):+.1f}, basis {m([t['basis'] for t in x]):+.1f}")
    by = collections.defaultdict(list)
    for t in x:
        by[t['asset']].append(net(t))
    top = sorted(by.items(), key=lambda kv: -sum(kv[1]))
    tot = sum(net(t) for t in x)
    print('   total', round(tot * 1e4), 'bp; top assets:', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top[:8]))
    rest = [v for a, vs in top[5:] for v in vs]
    print(f'   without the top 5 assets: {len(rest)} trades, {m(rest):+.1f} bp')
    print('   worst assets:', ', '.join(f"{a} {sum(v)*1e4:+.0f} ({len(v)})" for a, v in top[-5:]))
