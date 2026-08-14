<h1 align="center">OMO</h1>

<p align="center">
  <a href="LICENSE.md"><img src="https://img.shields.io/badge/License-All%20Rights%20Reserved-red.svg?style=flat-square" alt="License: All Rights Reserved"></a>
  <a href="https://lichess.org/@/omo-engine"><img src="https://img.shields.io/badge/Lichess-Play%20OMO-orange?style=flat-square&logo=lichess" alt="Lichess"></a>
  <img src="https://img.shields.io/badge/Rust-2024%20Edition-black?style=flat-square&logo=rust" alt="Rust">
</p>

OMO is a high-performance, multi-threaded UCI chess engine written in pure, memory-safe Rust on top of `cozy-chess`. It pairs a custom **NNUE** evaluation network with an optimized alpha-beta search pipeline featuring modern pruning, reductions, and lockless concurrency.

Play against OMO on Lichess: [lichess.org/@/omo-engine](https://lichess.org/@/omo-engine)

---

## Performance & Benchmarks

OMO has been benchmarked across **300-game tournament matches** against calibrated Stockfish skill level baselines (900 games total):

| Opponent Baseline                               |  Record (300 Games)   |       Score Rate        |       Elo Diff      | Estimated Performance | Draw Rate |
| :---------------------------------------------- | :-------------------: | :---------------------: | :-----------------: | :-------------------: | :-------: |
| **Stockfish (Skill Level 15 &bull; ~3070 Elo)** | **200W – 67L – 33D**  | **72.2%** (216.5 / 300) |        ~+164        |     **~3234 Elo**     |   11.0%   |
| **Stockfish (Skill Level 16 &bull; ~3111 Elo)** | **154W – 75L – 71D**  | **63.2%** (189.5 / 300) |        ~+95         |     **~3206 Elo**     |   23.7%   |
| **Stockfish (Skill Level 17 &bull; ~3141 Elo)** | **105W – 84L – 111D** | **53.5%** (160.5 / 300) | +24.4 &plusmn; 31.3 |     **~3165 Elo**     |   37.0%   |

### Detailed Match Breakdowns

#### Stockfish Skill Level 15 (~3070 Elo Baseline)

| Metric                    | Result                                      |
| :------------------------ | :------------------------------------------ |
| **Opponent**              | Stockfish (Skill Level 15 &bull; ~3070 Elo) |
| **Record**                | **200W / 67L / 33D** (300 games)            |
| **Score Rate**            | **72.2%** (216.5 / 300)                     |
| **Elo Difference**        | **~+164**                                   |
| **Estimated Performance** | **~3234 Elo**                               |
| **Draw Ratio**            | **11.0%**                                   |
| **Color Splits**          | White: 85.0% • Black: 59.3%                 |

#### Stockfish Skill Level 16 (~3111 Elo Baseline)

| Metric                    | Result                                      |
| :------------------------ | :------------------------------------------ |
| **Opponent**              | Stockfish (Skill Level 16 &bull; ~3111 Elo) |
| **Record**                | **154W / 75L / 71D** (300 games)            |
| **Score Rate**            | **63.2%** (189.5 / 300)                     |
| **Elo Difference**        | **~+95**                                    |
| **Estimated Performance** | **~3206 Elo**                               |
| **Draw Ratio**            | **23.7%**                                   |
| **Color Splits**          | White: 64.7% • Black: 61.7%                 |

#### Stockfish Skill Level 17 (~3141 Elo Baseline)

| Metric                              | Result                                                               |
| :---------------------------------- | :------------------------------------------------------------------- |
| **Opponent**                        | Stockfish (Skill Level 17 &bull; ~3141 Elo)                          |
| **Record**                          | **105W / 84L / 111D** (300 games)                                    |
| **Score Rate**                      | **53.5%** (160.5 / 300)                                              |
| **Elo Difference**                  | **+24.4 &plusmn; 31.3**                                              |
| **Estimated Performance**           | **~3165 Elo**                                                        |
| **Likelihood of Superiority (LOS)** | **93.7%**                                                            |
| **Draw Ratio**                      | **37.0%**                                                            |
| **Color Splits**                    | White: 58.3% (63W - 38L - 49D) &bull; Black: 48.7% (42W - 46L - 62D) |

---

## Architecture & Features

### Evaluation

- **Custom NNUE (`omo.nnue`):** Efficiently Updatable Neural Network trained on **76 million positions**, evaluated with incremental accumulator updates on move make/unmake via `nnue-rs`.
- **Classical Eval & Tuner:** Fully parameterized fallback evaluator with tapered phase scoring (MG/EG), piece-square tables, pawn structure evaluation, piece mobility/outposts, and king safety.

### Search Pipeline

- **Principal Variation Search (PVS):** Negamax framework with dynamic aspiration windows.
- **Pruning & Reductions:**
  - **Null Move Pruning (NMP):** Adaptive depth reduction for fast cutoffs.
  - **Reverse Futility Pruning (RFP):** Static evaluation margins at shallow depths.
  - **Futility Pruning (FP):** Prunes unpromising quiet moves near leaf nodes.
  - **Late Move Pruning (LMP):** Skips quiet moves beyond depth-scaled move thresholds (3 + 2 * depth^2).
  - **Late Move Reductions (LMR):** Logarithmic reductions for late quiet moves.
  - **Static Exchange Evaluation (SEE):** Full ray-caster capture verification to filter bad captures in Quiescence Search and tactical moves.
- **Extensions:** Check extensions and **Singular Extensions** to verify critical TT moves.
- **Quiescence Search:** Tactical capture/promotion resolution with delta pruning and SEE filtering.

### Move Ordering

1. **Transposition Table Move** (Hash Move)
2. **MVV-LVA** (Most Valuable Victim – Least Valuable Attacker)
3. **Killer Move Heuristic** (2 killers per ply)
4. **History & Continuation History:** Tracks move success and refutations against opponent's prior moves.

### Concurrency & System

- **Lockless Lazy SMP:** Multi-threaded parallel search using an atomic 64-bit packed Transposition Table (`AtomicU64`) and staggered thread depths with zero mutex overhead during search.
- **Endgame & Opening Support:** Syzygy endgame tablebase probing (WDL up to 6-piece) and Polyglot opening book (`book.bin`) integration.
- **Persistent Memory (`omo_memory.bin`):** Automatically saves and loads cached Transposition Table entries across sessions.
- **Smart Time Management:** Dynamic soft/hard allocation with panic buffers, sudden score-drop extensions, and instant-stop stability detection.

---

## UCI Commands & Utilities

- `perft` / `perftsuite`: Built-in move generation correctness and speed validation.
- `tactics`: Built-in tactical test suite runner.
- `savememory`: Manual transposition table serialization to disk.

---

## Author

Made with ♟️ by **mintykiera**

---

## License

Copyright &copy; 2026 mintykiera. All rights reserved. See [`LICENSE.md`](LICENSE.md) for terms.