# Funding stability (416 underlyings on 2+ venues, 30 days, 625220 pair-hours with a known spread > 10% APR)

The known spread at hour H = the short venue's last settled rate - the long venue's (what one sees then); the paid = the settlements after H (each at its interval), per hour of the hold. Retention = paid / known.

## 1. How much of a known spread gets paid, by its level

| known spread (APR) | pair-hours | paid next 1 h | 4 h | 8 h | 24 h | 72 h | next 8 h pay < 0 | retention 24 h |
|---|---|---|---|---|---|---|---|---|
| 10-25% | 302419 | +12% | +11% | +11% | +9% | +7% | 16% | 57% |
| 25-50% | 152779 | +24% | +21% | +19% | +15% | +11% | 18% | 43% |
| 50-100% | 118642 | +36% | +32% | +29% | +22% | +13% | 21% | 30% |
| 100-200% | 36064 | +47% | +40% | +33% | +21% | +12% | 27% | 16% |
| 200-500% | 11772 | +78% | +62% | +43% | +29% | +18% | 37% | 10% |
| 500-inf | 3544 | +145% | +182% | +113% | +90% | +62% | 37% | 9% |

## 2. Persistence: a spread >= 50% APR now that has held m hours in a row

| held (h) | pair-hours | known now (APR) | paid next 8 h | 24 h | 72 h | next 24 h pay < 0 | retention 24 h |
|---|---|---|---|---|---|---|---|
| 1 | 170022 | +117% | +33% | +23% | +14% | 28% | 20% |
| 2 | 120593 | +119% | +38% | +27% | +17% | 26% | 23% |
| 4 | 70884 | +116% | +42% | +32% | +19% | 24% | 27% |
| 8 | 29346 | +109% | +50% | +38% | +22% | 21% | 35% |
| 12 | 14117 | +107% | +56% | +43% | +25% | 20% | 40% |
| 24 | 2109 | +119% | +65% | +49% | +31% | 20% | 41% |

## 3. The spike: the spread now over its own 72 h mean (spreads >= 50% APR)

| now / 72 h mean | pair-hours | known now (APR) | 72 h mean (APR) | paid next 24 h | retention 24 h | next 24 h pay < 0 |
|---|---|---|---|---|---|---|
| < 1.5 (or the mean <= 0) | 67980 | +108% | +7% | +19% | 18% | 33% |
| 1.5-3 | 34934 | +97% | +47% | +37% | 38% | 16% |
| 3-10 | 39376 | +121% | +24% | +26% | 22% | 23% |
| >= 10 | 27732 | +162% | +7% | +13% | 8% | 34% |

## 4. By kind and by venue (spreads >= 50% APR held 4 h)

| group | pair-hours | known (APR) | paid next 24 h | retention | next 24 h < 0 |
|---|---|---|---|---|---|
| crypto | 19833 | +128% | +35% | 27% | 18% |
| stocks / HIP-3 style | 51051 | +111% | +30% | 27% | 26% |
| the SHORT leg on Hyperliquid | 9368 | +149% | +42% | 28% | 23% |
| the LONG leg on Hyperliquid | 9309 | +124% | +32% | 25% | 23% |
| the SHORT leg on Lighter | 6584 | +158% | +41% | 26% | 26% |
| the LONG leg on Lighter | 8296 | +111% | +28% | 25% | 24% |
| the SHORT leg on Extended | 4476 | +120% | +25% | 21% | 26% |
| the LONG leg on Extended | 6692 | +153% | +49% | 32% | 18% |
| the SHORT leg on Aster | 9958 | +126% | +27% | 22% | 27% |
| the LONG leg on Aster | 13685 | +126% | +42% | 33% | 18% |
| the SHORT leg on GRVT | 7433 | +104% | +14% | 14% | 32% |
| the LONG leg on GRVT | 8466 | +95% | +23% | 24% | 25% |
| the SHORT leg on ApeX | 2024 | +187% | +71% | 38% | 9% |
| the LONG leg on ApeX | 1530 | +140% | +38% | 27% | 12% |
| the SHORT leg on edgeX | 29184 | +90% | +31% | 34% | 21% |
| the LONG leg on edgeX | 19657 | +103% | +23% | 22% | 32% |
| the SHORT leg on Pacifica | 1592 | +124% | +25% | 20% | 24% |
| the LONG leg on Pacifica | 2603 | +112% | +34% | 30% | 18% |
| the SHORT leg on StandX | 265 | +79% | +18% | 22% | 34% |
| the LONG leg on StandX | 646 | +81% | +33% | 41% | 15% |

## 5. Trading it (one position a pair; out when the known spread < theta / 3 or after 7 days)

The cost of a round trip, % of one leg, with each market's measured median spread: taker = 2 x (both takers) + both full spreads; maker-taker = 2 x (the limit leg's maker + the hedge's taker + half the hedge's spread) + 2 bp, the limit on whichever venue makes it cheaper. bp per trade of one leg's notional; at 3x the capital is ~ one leg's notional.

| theta (APR) | held m h | half 1: trades / mean bp (taker) / (maker-taker) | half 2: trades / mean bp (taker) / (maker-taker) / win (m-t) / mean hold h |
|---|---|---|---|
| 25% | 1 | 32051 / -28.5 / -12.3 | 38968 / -28.6 / -12.2 / 5% / 6 |
| 25% | 4 | 10533 / -27.1 / -11.5 | 12262 / -28.6 / -11.9 / 10% / 9 |
| 25% | 8 | 4719 / -26.9 / -10.5 | 5379 / -28.2 / -11.0 / 13% / 12 |
| 25% | 24 | 609 / -22.3 / -6.0 | 721 / -24.5 / -7.1 / 25% / 19 |
| 50% | 1 | 18541 / -29.6 / -12.6 | 22402 / -30.2 / -12.7 / 7% / 5 |
| 50% | 4 | 6533 / -28.5 / -12.6 | 7589 / -30.8 / -12.9 / 11% / 8 |
| 50% | 8 | 2703 / -28.7 / -11.8 | 2982 / -30.4 / -12.2 / 13% / 10 |
| 50% | 24 | 161 / -26.1 / -8.8 | 180 / -26.9 / -9.7 / 19% / 13 |
| 100% | 1 | 8817 / -31.5 / -13.0 | 10006 / -31.7 / -12.6 / 9% / 4 |
| 100% | 4 | 2204 / -28.9 / -12.5 | 2463 / -33.4 / -11.8 / 14% / 6 |
| 100% | 8 | 483 / -30.5 / -10.1 | 498 / -38.7 / -10.9 / 18% / 6 |
| 100% | 24 | 6 / -2.2 / +6.8 | 13 / -24.9 / +7.0 / 46% / 10 |
| 200% | 1 | 3410 / -30.9 / -12.0 | 3635 / -35.3 / -12.0 / 14% / 3 |
| 200% | 4 | 453 / -24.7 / -6.6 | 495 / -37.4 / -6.1 / 25% / 5 |
| 200% | 8 | 118 / -28.3 / -5.1 | 101 / -53.8 / -5.1 / 24% / 6 |
| 200% | 24 | 0 / +0.0 / +0.0 | 2 / -102.1 / +24.2 / 50% / 4 |

**Chosen on half 1 (maker-taker):** theta 200% APR, held 8 h (half 1 -5.1 bp a trade). **Half 2:** 101 trades, -5.1 bp, win 24%, mean hold 6 h.


## 5b. The exit: out when the known spread < theta x f (f = 1/3, or 0: only when it turns negative), after a minimum hold (maker-taker costs)

| theta | held m h | exit f | min hold h | half 1: trades / mean bp | half 2: trades / mean bp / win / mean hold h |
|---|---|---|---|---|---|
| 50% | 4 | 1/3 | 0 | 6533 / -12.6 | 7589 / -12.9 / 11% / 8 |
| 50% | 4 | 0 | 0 | 5344 / -9.5 | 6341 / -10.4 / 16% / 15 |
| 50% | 4 | 0 | 24 | 4050 / -6.5 | 4592 / -6.9 / 27% / 34 |
| 50% | 4 | 0 | 72 | 2949 / -3.4 | 3249 / -4.0 / 37% / 73 |
| 50% | 8 | 1/3 | 0 | 2703 / -11.8 | 2982 / -12.2 / 13% / 10 |
| 50% | 8 | 0 | 0 | 2239 / -6.8 | 2516 / -8.0 / 22% / 19 |
| 50% | 8 | 0 | 24 | 1947 / -3.9 | 2162 / -4.5 / 31% / 35 |
| 50% | 8 | 0 | 72 | 1600 / -0.7 | 1722 / -0.6 / 40% / 74 |
| 100% | 4 | 1/3 | 0 | 2204 / -12.5 | 2463 / -11.8 / 14% / 6 |
| 100% | 4 | 0 | 0 | 1939 / -7.5 | 2155 / -6.8 / 24% / 16 |
| 100% | 4 | 0 | 24 | 1695 / -5.8 | 1781 / -3.5 / 33% / 33 |
| 100% | 4 | 0 | 72 | 1418 / -3.7 | 1431 / -1.5 / 42% / 73 |
| 100% | 8 | 1/3 | 0 | 483 / -10.1 | 498 / -10.9 / 18% / 6 |
| 100% | 8 | 0 | 0 | 465 / -6.1 | 483 / -7.1 / 25% / 15 |
| 100% | 8 | 0 | 24 | 424 / -1.6 | 444 / -1.3 / 38% / 32 |
| 100% | 8 | 0 | 72 | 393 / +3.9 | 405 / +3.8 / 42% / 71 |
| 200% | 4 | 1/3 | 0 | 453 / -6.6 | 495 / -6.1 / 25% / 5 |
| 200% | 4 | 0 | 0 | 445 / -5.3 | 482 / -3.7 / 31% / 9 |
| 200% | 4 | 0 | 24 | 383 / -2.0 | 372 / +7.1 / 50% / 29 |
| 200% | 4 | 0 | 72 | 328 / -1.6 | 291 / +15.0 / 54% / 68 |
| 200% | 8 | 1/3 | 0 | 118 / -5.1 | 101 / -5.1 / 24% / 6 |
| 200% | 8 | 0 | 0 | 115 / -4.8 | 100 / -1.8 / 36% / 14 |
| 200% | 8 | 0 | 24 | 107 / +6.5 | 90 / +10.0 / 52% / 30 |
| 200% | 8 | 0 | 72 | 100 / +15.2 | 88 / +16.2 / 50% / 67 |

**Chosen on half 1:** theta 200%, held 8 h, exit f 0, min hold 72 h (half 1 +15.2 bp). **Half 2:** 88 trades, +16.2 bp, win 50%, mean hold 67 h.

