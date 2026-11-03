# Funding stability (416 underlyings on 2+ venues, 30 days, 616557 pair-hours with a known spread > 10% APR)

The pairs of two tokens under one ticker (the mids 10%+ apart on the books, data/prices.json) left out.

The known spread at hour H = the short venue's last settled rate - the long venue's (what one sees then); the paid = the settlements after H (each at its interval), per hour of the hold. Retention = paid / known.

## 1. How much of a known spread gets paid, by its level

| known spread (APR) | pair-hours | paid next 1 h | 4 h | 8 h | 24 h | 72 h | next 8 h pay < 0 | retention 24 h |
|---|---|---|---|---|---|---|---|---|
| 10-25% | 300547 | +12% | +11% | +11% | +9% | +7% | 16% | 57% |
| 25-50% | 151063 | +24% | +21% | +19% | +15% | +11% | 18% | 43% |
| 50-100% | 116724 | +35% | +32% | +29% | +22% | +13% | 21% | 30% |
| 100-200% | 34733 | +44% | +39% | +31% | +20% | +11% | 28% | 16% |
| 200-500% | 10709 | +63% | +51% | +32% | +19% | +9% | 39% | 6% |
| 500-inf | 2781 | +40% | +135% | +72% | +54% | +31% | 41% | 6% |

## 2. Persistence: a spread >= 50% APR now that has held m hours in a row

| held (h) | pair-hours | known now (APR) | paid next 8 h | 24 h | 72 h | next 24 h pay < 0 | retention 24 h |
|---|---|---|---|---|---|---|---|
| 1 | 164947 | +113% | +30% | +22% | +13% | 28% | 19% |
| 2 | 117241 | +114% | +35% | +25% | +15% | 26% | 22% |
| 4 | 69075 | +111% | +39% | +29% | +16% | 24% | 26% |
| 8 | 28579 | +104% | +46% | +34% | +19% | 22% | 33% |
| 12 | 13679 | +101% | +51% | +37% | +21% | 21% | 37% |
| 24 | 2009 | +113% | +59% | +42% | +25% | 21% | 37% |

## 3. The spike: the spread now over its own 72 h mean (spreads >= 50% APR)

| now / 72 h mean | pair-hours | known now (APR) | 72 h mean (APR) | paid next 24 h | retention 24 h | next 24 h pay < 0 |
|---|---|---|---|---|---|---|
| < 1.5 (or the mean <= 0) | 65309 | +104% | +5% | +18% | 17% | 33% |
| 1.5-3 | 34015 | +91% | +44% | +34% | 37% | 17% |
| 3-10 | 38466 | +113% | +22% | +24% | 21% | 24% |
| >= 10 | 27157 | +161% | +7% | +13% | 8% | 34% |

## 4. By kind and by venue (spreads >= 50% APR held 4 h)

| group | pair-hours | known (APR) | paid next 24 h | retention | next 24 h < 0 |
|---|---|---|---|---|---|
| crypto | 19774 | +128% | +35% | 27% | 17% |
| stocks / HIP-3 style | 49301 | +104% | +26% | 25% | 27% |
| the SHORT leg on Hyperliquid | 8614 | +141% | +31% | 22% | 24% |
| the LONG leg on Hyperliquid | 9166 | +124% | +32% | 26% | 23% |
| the SHORT leg on Lighter | 6324 | +139% | +31% | 23% | 27% |
| the LONG leg on Lighter | 8125 | +110% | +28% | 26% | 24% |
| the SHORT leg on Extended | 4078 | +119% | +25% | 21% | 27% |
| the LONG leg on Extended | 6063 | +130% | +34% | 26% | 18% |
| the SHORT leg on Aster | 9863 | +125% | +28% | 22% | 27% |
| the LONG leg on Aster | 13082 | +114% | +34% | 30% | 18% |
| the SHORT leg on GRVT | 7433 | +104% | +14% | 14% | 32% |
| the LONG leg on GRVT | 8466 | +95% | +23% | 24% | 25% |
| the SHORT leg on ApeX | 1855 | +133% | +48% | 36% | 8% |
| the LONG leg on ApeX | 1494 | +131% | +40% | 30% | 11% |
| the SHORT leg on edgeX | 29051 | +90% | +31% | 34% | 21% |
| the LONG leg on edgeX | 19430 | +103% | +23% | 22% | 32% |
| the SHORT leg on Pacifica | 1592 | +124% | +25% | 20% | 24% |
| the LONG leg on Pacifica | 2603 | +112% | +34% | 30% | 18% |
| the SHORT leg on StandX | 265 | +79% | +18% | 22% | 34% |
| the LONG leg on StandX | 646 | +81% | +33% | 41% | 15% |

## 5. Trading it (one position a pair; out when the known spread < theta / 3 or after 7 days)

The cost of a round trip, % of one leg, with each market's measured median spread: taker = 2 x (both takers) + both full spreads; maker-taker = 2 x (the limit leg's maker + the hedge's taker + half the hedge's spread) + 2 bp, the limit on whichever venue makes it cheaper. bp per trade of one leg's notional; at 3x the capital is ~ one leg's notional.

| theta (APR) | held m h | half 1: trades / mean bp (taker) / (maker-taker) | half 2: trades / mean bp (taker) / (maker-taker) / win (m-t) / mean hold h |
|---|---|---|---|
| 25% | 1 | 31272 / -28.7 / -12.4 | 37848 / -28.8 / -12.4 / 5% / 6 |
| 25% | 4 | 10346 / -27.4 / -11.7 | 11954 / -28.9 / -12.1 / 10% / 9 |
| 25% | 8 | 4647 / -27.2 / -10.8 | 5290 / -28.4 / -11.2 / 13% / 12 |
| 25% | 24 | 600 / -22.6 / -6.3 | 706 / -24.9 / -7.4 / 25% / 19 |
| 50% | 1 | 17948 / -29.9 / -12.9 | 21469 / -30.7 / -12.9 / 6% / 6 |
| 50% | 4 | 6400 / -29.0 / -13.0 | 7349 / -31.2 / -13.2 / 10% / 8 |
| 50% | 8 | 2660 / -29.3 / -12.3 | 2921 / -30.9 / -12.5 / 13% / 10 |
| 50% | 24 | 158 / -27.2 / -9.7 | 173 / -27.9 / -10.6 / 17% / 12 |
| 100% | 1 | 8407 / -32.3 / -13.5 | 9300 / -32.7 / -13.2 / 8% / 4 |
| 100% | 4 | 2138 / -30.0 / -13.5 | 2309 / -34.8 / -12.6 / 12% / 6 |
| 100% | 8 | 460 / -32.8 / -12.0 | 463 / -41.0 / -12.1 / 16% / 6 |
| 100% | 24 | 6 / -2.2 / +6.8 | 11 / -34.9 / +0.1 / 36% / 7 |
| 200% | 1 | 3209 / -32.5 / -13.1 | 3190 / -38.4 / -13.7 / 11% / 3 |
| 200% | 4 | 418 / -29.2 / -10.7 | 409 / -42.6 / -7.6 / 24% / 5 |
| 200% | 8 | 102 / -36.6 / -12.1 | 95 / -59.6 / -8.6 / 19% / 5 |
| 200% | 24 | 0 / +0.0 / +0.0 | 2 / -102.1 / +24.2 / 50% / 4 |

**Chosen on half 1 (maker-taker):** theta 25% APR, held 24 h (half 1 -6.3 bp a trade). **Half 2:** 706 trades, -7.4 bp, win 25%, mean hold 19 h.


## 5b. The exit: out when the known spread < theta x f (f = 1/3, or 0: only when it turns negative), after a minimum hold (maker-taker costs)

| theta | held m h | exit f | min hold h | half 1: trades / mean bp | half 2: trades / mean bp / win / mean hold h |
|---|---|---|---|---|---|
| 50% | 4 | 1/3 | 0 | 6400 / -13.0 | 7349 / -13.2 / 10% / 8 |
| 50% | 4 | 0 | 0 | 5220 / -10.0 | 6115 / -10.7 / 16% / 15 |
| 50% | 4 | 0 | 24 | 3955 / -7.3 | 4451 / -7.5 / 26% / 34 |
| 50% | 4 | 0 | 72 | 2876 / -4.6 | 3163 / -4.6 / 37% / 73 |
| 50% | 8 | 1/3 | 0 | 2660 / -12.3 | 2921 / -12.5 / 13% / 10 |
| 50% | 8 | 0 | 0 | 2199 / -7.4 | 2461 / -8.3 / 21% / 19 |
| 50% | 8 | 0 | 24 | 1915 / -5.0 | 2119 / -5.2 / 31% / 35 |
| 50% | 8 | 0 | 72 | 1572 / -2.7 | 1694 / -2.1 / 39% / 74 |
| 100% | 4 | 1/3 | 0 | 2138 / -13.5 | 2309 / -12.6 / 12% / 6 |
| 100% | 4 | 0 | 0 | 1881 / -8.6 | 2007 / -7.5 / 23% / 16 |
| 100% | 4 | 0 | 24 | 1649 / -7.6 | 1690 / -4.6 / 32% / 34 |
| 100% | 4 | 0 | 72 | 1380 / -6.4 | 1373 / -2.8 / 42% / 74 |
| 100% | 8 | 1/3 | 0 | 460 / -12.0 | 463 / -12.1 / 16% / 6 |
| 100% | 8 | 0 | 0 | 446 / -8.5 | 448 / -8.1 / 23% / 15 |
| 100% | 8 | 0 | 24 | 409 / -6.3 | 417 / -4.0 / 35% / 32 |
| 100% | 8 | 0 | 72 | 380 / -3.9 | 387 / -1.4 / 40% / 71 |
| 200% | 4 | 1/3 | 0 | 418 / -10.7 | 409 / -7.6 / 24% / 5 |
| 200% | 4 | 0 | 0 | 414 / -9.7 | 398 / -5.1 / 29% / 10 |
| 200% | 4 | 0 | 24 | 360 / -9.1 | 320 / +3.2 / 46% / 29 |
| 200% | 4 | 0 | 72 | 309 / -10.2 | 257 / +9.1 / 51% / 67 |
| 200% | 8 | 1/3 | 0 | 102 / -12.1 | 95 / -8.6 / 19% / 5 |
| 200% | 8 | 0 | 0 | 101 / -12.7 | 94 / -5.1 / 32% / 14 |
| 200% | 8 | 0 | 24 | 97 / -8.6 | 84 / +4.0 / 49% / 30 |
| 200% | 8 | 0 | 72 | 92 / -6.0 | 82 / +4.9 / 46% / 67 |

**Chosen on half 1:** theta 50%, held 8 h, exit f 0, min hold 72 h (half 1 -2.7 bp). **Half 2:** 1694 trades, -2.1 bp, win 39%, mean hold 74 h.

