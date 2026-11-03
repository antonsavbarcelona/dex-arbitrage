# Theories tested, 2026-11-03

Data:
- 30 days of funding for 1,595 series.
- 15-min bars (close / high / low) for every shared market on the venues with public bars, in `data/lev_bars`.
- 3.6 h of full WebSocket books and prints: 423 assets, 1,680 markets.

Costs are each market's measured spreads plus the venues' fees. Every rule was checked on both halves of the data and for concentration (the result without the top assets).

| # | Theory | Test | Result |
|---|---|---|---|
| 0 | Funding carry (fstab) | after dropping the ticker collisions (PRL, QNT, XIAOMI, BB, MEME, PURR, CVX; book mids 10%+ apart) | No rule survives the halves: -2.7 / -2.1 bp. The earlier +16 bp was PRL (two tokens). **Closed.** |
| 0b | Leverage on the carry (`dexarb lev`) | 3,076 trades on each leg's bars; cross margin per venue; liquidation at the closes or the wicks | 1–1.5x: no liquidations. 2x with rebalancing: about clean. 3x+: loses. The high-funding coins move 8% median and 25% p95 against a leg over 3 days. |
| 1 | Both legs maker (`ws/mm_entry.py`) | $500 size; a fill needs a print strictly through; a leg unfilled after 2 or 5 min is chased | Both legs fill only 3–5% of the time. Entry cost stays ~17 bp a leg, the same as taker-taker. **Closed.** |
| 2 | Carry entered only when the basis also favours it (`basis2/carry_basis.py`) | entry dev = log(P_long / P_short) minus its 3-day median | Long leg cheap by 20 bp or more gives +23 to +87 bp at the entry bar. One bar (15 min) later it is +0.7, and without the top 5 assets -4.9. The convergence happens inside a 15-min bar. **Closed.** |
| 3 | HIP-3 stocks: HL vs the other venues (`basis2/revert.py stocks`) | reversion of the cross-venue basis to its 3-day median | +17 bp at 1–2% deviations, but without the top 5 assets (ANSEM, BE, PRL, NBIS, DRV) -1 bp. With a 1 h delay +5 bp, and -10 without the top 5. **Closed.** Crypto is the same: BERA carries it. |
| 3b | Executable reversion on the books (`basis2/exec_rev.py`) | taker both legs at the next minute's quotes | -28 to -113 bp, 0–9% win. The gaps close within a minute. **Closed.** |
| 4 | Maker on A off B's book (`dexarb mt`) | prints re-timed to exchange time; then $500 / $2,000 size checks; latency stressed | No size check: +9 bp at the 10 bp target (thin prints on LINEA, PONS, PEARL on ApeX / Aster). $500: -2.9 bp. 600 / 800 ms latency: -14 bp. Only HL limit / GMX hedge stays positive (LIT, XMR, NEAR), n tiny. GMX is filled by a keeper at the next oracle price: with the hedge 2 s late it is still +8 to +16 bp on ~30 trips; 5 s late it is negative. **Closed** except HL→GMX (watch: it needs days of data and the real keeper delay). |
| 5 | The 4 / 8 h venue's settlement vs a 1 h hedge (`basis2/interval*.py`) | open at T-15 min, close at T+15 min | Payment ≥ 100 bp and known before T: +58 to +84 bp per event, 75% win, both halves; ~1 per day, thin coins (BTW, BLUR, KAIA, AXIS, UNITREE, XTEN, EWZ). Signal from the previous settlement: fails. From the price gap: no better than chance. Holding to the next settlement: the hedge pays as much, net negative. Tested on edgeX history (`basis2/edgex_avgprem.py`): its minute rows carry the running avgPremiumIndex, so the rate is rebuilt at T-15 min with no look-ahead. On 712 settlements of the 8 hot assets the estimate gets the sign right 79% of the time (99% on the big ones). Trades **selected by the estimate**: -5 to -36 bp in every bucket. The price moves 30–44 bp against the trade over the 30 min around T: the premium that made the funding collapses after it. The earlier plus came from selecting on the actual payment. **Closed** for edgeX; Aster / GRVT have no history of the forecast, likely the same. |

Settlement mechanics per the venues' docs:
- **GRVT:** "Funding is applied at the settlement timestamp. If you are flat at that instant, you don't pay or receive." It switches to 1 h when the average premium would exceed the cap.
- **edgeX:** "Only traders holding positions at the time of settlement are liable."
- **Aster:** charged at the interval's timestamp, with a ~15 s deviation.
