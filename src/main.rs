mod eval;
mod search;
mod transposition;
mod tuner;

use cozy_chess::{ Board, Color, Move, Piece };
use std::io::{ self, BufRead };
use std::sync::atomic::{ AtomicBool, AtomicU64, Ordering };
use std::sync::{ Arc, Mutex };
use std::thread;
use std::time::{ Duration, Instant };

fn parse_uci_move(board: &Board, m_str: &str) -> Option<Move> {
    let mut m = m_str.parse::<Move>().ok()?;
    if board.piece_on(m.from) == Some(Piece::King) {
        let from_str = m.from.to_string();
        let to_str = m.to.to_string();
        if from_str == "e1" && to_str == "g1" {
            m.to = "h1".parse().unwrap();
        } else if from_str == "e1" && to_str == "c1" {
            m.to = "a1".parse().unwrap();
        } else if from_str == "e8" && to_str == "g8" {
            m.to = "h8".parse().unwrap();
        } else if from_str == "e8" && to_str == "c8" {
            m.to = "a8".parse().unwrap();
        }
    }
    Some(m)
}

pub fn format_uci_move(board: &Board, m: Move) -> String {
    let mut out_move = m;
    if board.piece_on(m.from) == Some(Piece::King) {
        let from_str = m.from.to_string();
        let to_str = m.to.to_string();
        if from_str == "e1" && to_str == "h1" {
            out_move.to = "g1".parse().unwrap();
        } else if from_str == "e1" && to_str == "a1" {
            out_move.to = "c1".parse().unwrap();
        } else if from_str == "e8" && to_str == "h8" {
            out_move.to = "g8".parse().unwrap();
        } else if from_str == "e8" && to_str == "a8" {
            out_move.to = "c8".parse().unwrap();
        }
    }
    out_move.to_string()
}

fn perft(board: &Board, depth: i32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut nodes = 0u64;
    board.generate_moves(|move_list| {
        for m in move_list {
            let mut next = board.clone();
            next.play_unchecked(m);
            nodes += perft(&next, depth - 1);
        }
        false
    });
    nodes
}

fn parse_uci_param(tokens: &[&str], name: &str) -> Option<u64> {
    tokens
        .iter()
        .position(|&r| r == name)
        .and_then(|i| tokens.get(i + 1))
        .and_then(|s| s.parse::<u64>().ok())
}

fn parse_go_time(tokens: &[&str], side: Color) -> Duration {
    if let Some(ms) = parse_uci_param(tokens, "movetime") {
        return Duration::from_millis(ms);
    }

    let time_key = if side == Color::White { "wtime" } else { "btime" };
    let inc_key = if side == Color::White { "winc" } else { "binc" };

    if let Some(our_time) = parse_uci_param(tokens, time_key) {
        let safe_time = our_time.saturating_sub(50);
        if safe_time < 100 {
            return Duration::from_millis(10);
        }

        let our_inc = parse_uci_param(tokens, inc_key).unwrap_or(0);
        let mut mtg = parse_uci_param(tokens, "movestogo").unwrap_or(30);

        if safe_time < 2000 {
            mtg = 40;
        }

        let base = safe_time / mtg.max(1);
        let target = base + (our_inc * 3) / 4;
        let max_time = safe_time / 4;
        let ms = target.min(max_time).max(10);

        return Duration::from_millis(ms);
    }

    if tokens.contains(&"infinite") {
        return Duration::from_secs(3600);
    }

    Duration::from_millis(1000)
}

struct SearchHandle {
    threads: Vec<thread::JoinHandle<()>>,
    stop_flag: Arc<AtomicBool>,
    is_pondering: Arc<AtomicBool>,
    time_limit_ms: Arc<AtomicU64>,
}

impl SearchHandle {
    fn new() -> Self {
        Self {
            threads: Vec::new(),
            stop_flag: Arc::new(AtomicBool::new(false)),
            is_pondering: Arc::new(AtomicBool::new(false)),
            time_limit_ms: Arc::new(AtomicU64::new(0)),
        }
    }

    fn stop_and_join(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        for handle in self.threads.drain(..) {
            let _ = handle.join();
        }
    }

    fn is_searching(&self) -> bool {
        self.threads.iter().any(|h| !h.is_finished())
    }
}

fn main() {
    let board = Arc::new(Mutex::new(Board::default()));
    let tt = Arc::new(transposition::TranspositionTable::new(64));
    let mut handle = SearchHandle::new();
    let mut num_threads = 1;
    let mut game_history: Vec<u64> = Vec::new();

    if std::path::Path::new("omo_memory.bin").exists() {
        if tt.load_from_file("omo_memory.bin").is_ok() {
            println!("info string Transposition table memory restored from disk");
        }
    }

    let stdin = io::stdin();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => {
                break;
            }
        };

        let tokens: Vec<&str> = line.trim().split_whitespace().collect();

        if tokens.is_empty() {
            continue;
        }

        match tokens[0] {
            "setoption" => {
                if let Some(name_idx) = tokens.iter().position(|&r| r == "name") {
                    if let Some(value_idx) = tokens.iter().position(|&r| r == "value") {
                        if
                            name_idx + 1 < tokens.len() &&
                            tokens[name_idx + 1].to_lowercase() == "threads"
                        {
                            if value_idx + 1 < tokens.len() {
                                if let Ok(t) = tokens[value_idx + 1].parse::<usize>() {
                                    num_threads = t.max(1);
                                }
                            }
                        }
                    }
                }
            }
            "uci" => {
                println!("id name Omo");
                println!("id author kieraesque");
                println!("uciok");
            }
            "isready" => {
                println!("readyok");
            }
            "ucinewgame" => {
                handle.stop_and_join();
                *board.lock().unwrap() = Board::default();
                game_history.clear();
                tt.new_search();
            }
            "position" => {
                handle.stop_and_join();
                let mut b = board.lock().unwrap();
                game_history.clear();

                if tokens.contains(&"startpos") {
                    *b = Board::default();
                    game_history.push(b.hash());
                    if let Some(moves_idx) = tokens.iter().position(|&r| r == "moves") {
                        for m_str in &tokens[moves_idx + 1..] {
                            if let Some(m) = parse_uci_move(&b, m_str) {
                                b.play_unchecked(m);
                                game_history.push(b.hash());
                            }
                        }
                    }
                } else if let Some(fen_idx) = tokens.iter().position(|&r| r == "fen") {
                    let fen_start = fen_idx + 1;
                    if let Some(moves_idx) = tokens.iter().position(|&r| r == "moves") {
                        let fen_str = tokens[fen_start..moves_idx].join(" ");
                        if let Ok(parsed) = fen_str.parse::<Board>() {
                            *b = parsed;
                            game_history.push(b.hash());
                            for m_str in &tokens[moves_idx + 1..] {
                                if let Some(m) = parse_uci_move(&b, m_str) {
                                    b.play_unchecked(m);
                                    game_history.push(b.hash());
                                }
                            }
                        }
                    } else {
                        let fen_str = tokens[fen_start..].join(" ");
                        if let Ok(parsed) = fen_str.parse::<Board>() {
                            *b = parsed;
                            game_history.push(b.hash());
                        }
                    }
                }
            }
            "go" => {
                handle.stop_and_join();

                let ponder = tokens.contains(&"ponder");

                let board_snapshot = board.lock().unwrap().clone();
                let mut root_moves = Vec::new();
                board_snapshot.generate_moves(|moves| {
                    root_moves.extend(moves);
                    false
                });

                if root_moves.len() == 1 {
                    println!(
                        "bestmove {}",
                        format_uci_move(&board_snapshot, root_moves[0])
                    );
                    continue;
                }

                let time_limit = parse_go_time(&tokens, board_snapshot.side_to_move());

                let stop_flag = Arc::new(AtomicBool::new(false));
                let is_pondering = Arc::new(AtomicBool::new(ponder));
                let time_limit_ms = Arc::new(AtomicU64::new(time_limit.as_millis() as u64));

                handle.stop_flag = Arc::clone(&stop_flag);
                handle.is_pondering = Arc::clone(&is_pondering);
                handle.time_limit_ms = Arc::clone(&time_limit_ms);

                tt.new_search();

                for i in 0..num_threads {
                    let tt_clone = Arc::clone(&tt);
                    let board_clone = board_snapshot.clone();
                    let sf_clone = Arc::clone(&stop_flag);
                    let ip_clone = Arc::clone(&is_pondering);
                    let tl_clone = Arc::clone(&time_limit_ms);
                    let params = crate::eval::DEFAULT_PARAMS.clone();
                    let mut history_clone = game_history.clone();

                    handle.threads.push(
                        thread::spawn(move || {
                            let (best, ponder_mv) = search::get_best_move(
                                &board_clone,
                                time_limit,
                                &tt_clone,
                                sf_clone.clone(),
                                ip_clone,
                                tl_clone,
                                i == 0,
                                i,
                                &params,
                                &mut history_clone
                            );

                            if i == 0 {
                                sf_clone.store(true, Ordering::Relaxed);

                                let final_move = best.or_else(|| {
                                    let mut fallback = None;
                                    board_clone.generate_moves(|moves| {
                                        if fallback.is_none() {
                                            fallback = moves.into_iter().next();
                                        }
                                        false
                                    });
                                    fallback
                                });

                                let best_str = final_move.map_or("0000".to_string(), |m|
                                    format_uci_move(&board_clone, m)
                                );

                                if let (Some(bm), Some(pm)) = (final_move, ponder_mv) {
                                    let mut after = board_clone.clone();
                                    after.play_unchecked(bm);
                                    println!(
                                        "bestmove {} ponder {}",
                                        best_str,
                                        format_uci_move(&after, pm)
                                    );
                                } else {
                                    println!("bestmove {}", best_str);
                                }
                            }
                        })
                    );
                }
            }
            "ponderhit" => {
                if handle.is_searching() {
                    handle.is_pondering.store(false, Ordering::Relaxed);
                }
            }
            "stop" => {
                handle.stop_and_join();
            }
            "perft" => {
                let depth: i32 = tokens
                    .get(1)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(5);
                let b = board.lock().unwrap().clone();
                let start = Instant::now();
                let nodes = perft(&b, depth);
                let elapsed = start.elapsed();
                let ms = elapsed.as_millis().max(1);
                let nps = ((nodes as u128) * 1000) / ms;
                println!("perft({}) = {} nodes in {}ms ({} nps)", depth, nodes, ms, nps);
            }
            "perftsuite" => {
                let positions = vec![
                    ("rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1", 5, 4865609),
                    (
                        "r3k2r/p1ppqpb1/bn2pnp1/3PN3/1p2P3/2N2Q1p/PPPBBPPP/R3K2R w KQkq - 0 1",
                        4,
                        4085603,
                    ),
                    ("8/2p5/3p4/KP5r/1R3p1k/8/4P1P1/8 w - - 0 1", 5, 674624),
                    ("r3k2r/Pppp1ppp/1b3nbN/nP6/BBP1P3/q4N2/Pp1P2PP/R2Q1RK1 w kq - 0 1", 4, 422333),
                    ("rnbq1k1r/pp1Pbppp/2p5/8/2B5/8/PPP1NnPP/RNBQK2R w KQ - 1 8", 4, 2103487)
                ];
                let mut all_correct = true;
                for (fen, depth, expected) in positions {
                    let b: Board = fen.parse().unwrap();
                    let nodes = perft(&b, depth);
                    if nodes == expected {
                        println!("PASS: {} perft({}) = {}", fen, depth, nodes);
                    } else {
                        println!(
                            "FAIL: {} perft({}) = {} (expected {})",
                            fen,
                            depth,
                            nodes,
                            expected
                        );
                        all_correct = false;
                    }
                }
                if all_correct {
                    println!("perftsuite: ALL PASS");
                } else {
                    println!("perftsuite: FAIL");
                }
            }
            "tactics" => {
                handle.stop_and_join();

                let positions = vec![
                    ("M1 Back Rank", "6k1/5ppp/8/8/8/8/8/4R1K1 w - - 0 1", "e1e8"),
                    (
                        "M1 Scholar",
                        "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4",
                        "h5f7",
                    ),
                    ("M2 Discovered", "r5rk/5p1p/5R2/4B3/8/8/7P/7K w - - 0 1", "f6a6"),
                    ("Hanging Queen", "4k3/8/8/3q4/8/8/3R4/4K3 w - - 0 1", "d2d5"),
                    ("Knight Fork", "8/3k1q2/8/8/8/3N4/8/4K3 w - - 0 1", "d3e5"),
                    ("Rook Pin", "4k3/4q3/8/8/8/8/P7/4R1K1 w - - 0 1", "e1e7"),
                    ("Bishop Skewer", "7q/8/8/4k3/8/8/8/2B3K1 w - - 0 1", "c1b2"),
                    ("Pawn Fork", "4k3/8/8/8/3p4/2N1R3/8/4K3 b - - 0 1", "d4e3")
                ];
                let mut passed = 0;
                for (name, fen, expected) in &positions {
                    let b: Board = fen.parse().unwrap();

                    let stop_flag = Arc::new(AtomicBool::new(false));
                    let is_pondering = Arc::new(AtomicBool::new(false));
                    let time_limit_ms = Arc::new(AtomicU64::new(1000));

                    tt.new_search();
                    println!("Testing {}...", name);
                    let params = crate::eval::DEFAULT_PARAMS.clone();
                    let mut hist = Vec::new();
                    let (best, _) = search::get_best_move(
                        &b,
                        Duration::from_millis(1000),
                        &tt,
                        stop_flag,
                        is_pondering,
                        time_limit_ms,
                        true,
                        0,
                        &params,
                        &mut hist
                    );

                    if let Some(m) = best {
                        let best_str = format_uci_move(&b, m);
                        if best_str == *expected {
                            println!("PASS: {} -> found {} as expected", name, best_str);
                            passed += 1;
                        } else {
                            println!("FAIL: {} -> expected {}, found {}", name, expected, best_str);
                        }
                    } else {
                        println!("FAIL: {} -> found no move", name);
                    }
                    println!("---");
                }
                println!("Tactics score: {}/{}", passed, positions.len());
            }

            "gensamples" => {
                if tokens.len() < 3 {
                    println!("Usage: gensamples <num_games> <output_file>");
                    continue;
                }
                if let Ok(num) = tokens[1].parse::<usize>() {
                    tuner::generate_dataset(num, tokens[2]);
                }
            }
            "tune" => {
                if tokens.len() < 2 {
                    println!("Usage: tune <dataset_file>");
                    continue;
                }
                tuner::tune(tokens[1]);
            }
            "savememory" => {
                if tt.save_to_file("omo_memory.bin").is_ok() {
                    println!("info string Transposition table saved to disk");
                }
            }
            "quit" => {
                handle.stop_and_join();
                let _ = tt.save_to_file("omo_memory.bin");
                break;
            }
            _ => {}
        }
    }
}
