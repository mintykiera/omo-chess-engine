# OMO

OMO is a high-performance, multi-threaded classical chess engine written in pure, memory-safe Rust. Built from scratch on top of `cozy-chess`, OMO combines advanced search pruning, contextual move ordering, and persistent session memory with a finely tuned classical evaluation pipeline.

You can play against OMO [here](https://lichess.org/@/omo-engine)

## Architectural Highlights

- **Safe Lazy SMP Concurrency:** Multi-threading is driven by a lockless-style, safely striped Transposition Table (`Vec<RwLock<TTEntry>>`) with zero unsafe blocks. Worker threads use staggered search depths for maximum tree divergence.
- **Organic Persistent Memory (`omo_memory.bin`):** Automatically serializes the Transposition Table to disk on shutdown and restores cached calculations on boot, allowing OMO to retain position evaluations across sessions.
- **Contextual Move Ordering:** Combines standard History and Killer Move heuristics with **Continuation History**, tracking move responses to prioritize quiet moves that refute specific opponent threats.
- **Precision Clock Management:** Driven by absolute deadline timestamps and a built-in safety buffer to eliminate time forfeits under strict time controls.

## Search & Evaluation Stack

### Search Architecture
- **Principal Variation Search (PVS)** with narrowed zero-window alpha-beta bounds.
- **Reverse Futility Pruning (RFP)** and **Null Move Pruning (NMP)**.
- **Late Move Reductions (LMR)** and **Quiescence Search** filtered by Static Exchange Evaluation (**SEE**).
- **In-Search Draw Detection** handling 3-fold repetitions directly within the search stack.

### Classical Evaluation
- **Tapered Phase Evaluation:** Smooth midgame-to-endgame interpolation using dedicated `MG` and `EG` Piece-Square Tables.
- **Advanced Mobility & Outposts:** Dynamic attack counting across bitboards and protected knight outpost scoring.
- **Attacking King Safety:** Pawn storm detection, open-file penalties, and adjacent-rank attack tracking.
- **Positional Factors:** Side-to-move initiative (Tempo Bonus) and advanced passed pawn advancement scoring.


## Author

Made with ❤️ by **mintykiera**