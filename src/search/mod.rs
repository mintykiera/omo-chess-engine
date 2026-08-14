mod types;
mod ordering;
mod see;
mod pv;
mod negamax;

pub use types::SearchInfo;

use cozy_chess::{Board, Move};
use polyglot_book_rs::PolyglotBook;
use shakmaty::Chess;
use shakmaty_syzygy::Tablebase;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::eval::{EvalParams, EvalState};
use crate::transposition::TranspositionTable;
use crate::uci::{parse_uci_move, format_uci_move};
use types::{MAX_DEPTH, MATE_SCORE};
use pv::extract_pv;
use negamax::negamax;

pub fn get_best_move(
    board: &Board,
    time_limit: Duration,
    total_clock: Option<Duration>,
    tt: &TranspositionTable,
    stop_flag: Arc<AtomicBool>,
    is_pondering: Arc<AtomicBool>,
    time_limit_ms: Arc<AtomicU64>,
    is_main_thread: bool,
    thread_id: usize,
    params: &EvalParams,
    history_hashes: &mut Vec<u64>,
    opening_book: &Option<PolyglotBook>,
    syzygy: &Option<Tablebase<Chess>>,
) -> (Option<Move>, Option<Move>) {
    if let Some(book) = opening_book {
        let fen = board.to_string();
        if let Some(entry) = book.get_best_move_from_fen(&fen) {
            if let Some(book_move) = parse_uci_move(board, &entry.move_string) {
                let mut is_legal = false;
                board.generate_moves(|move_list| {
                    for m in move_list {
                        if m == book_move {
                            is_legal = true;
                        }
                    }
                    false
                });
                if is_legal {
                    if is_main_thread {
                        println!(
                            "info string Book move: {}",
                            format_uci_move(board, book_move)
                        );
                    }
                    return (Some(book_move), None);
                }
            }
        }
    }

    let mut info = SearchInfo::new(
        time_limit,
        stop_flag,
        is_pondering.clone(),
        time_limit_ms.clone(),
    );

    for i in 0..64 {
        for j in 0..64 {
            info.history[i][j] /= 2;
            info.cont_history[i][j] /= 2;
        }
    }

    let mut best_move: Option<Move> = None;
    let mut best_score = 0i32;
    let mut total_nodes: u64 = 0;
    let eval_state = EvalState::from_board(board, params);

    let mut stable_count: i32 = 0;
    let mut prev_best_move: Option<Move> = None;
    let mut prev_score: i32 = 0;

    let start_depth = if thread_id > 0 {
        ((thread_id % 2) as i32) + 1
    } else {
        1
    };
    for depth in start_depth..=MAX_DEPTH {
        info.nodes = 0;

        if !is_pondering.load(Ordering::Relaxed) {
            let current_ms = time_limit_ms.load(Ordering::Relaxed);
            if current_ms > 0 {
                info.time_limit = Duration::from_millis(current_ms);
                let sl = if info.time_limit > Duration::from_millis(25) {
                    info.time_limit - Duration::from_millis(15)
                } else {
                    (info.time_limit * 8) / 10
                };
                info.deadline = Some(info.start_time + sl);
            }
            if depth > 1 {
                let limit = info.time_limit;
                if Instant::now() >= info.start_time + (limit * 4) / 10 {
                    break;
                }
            }
        }

        let mut window = 25;
        let mut alpha = if depth > 1 {
            best_score - window
        } else {
            -MATE_SCORE
        };
        let mut beta = if depth > 1 {
            best_score + window
        } else {
            MATE_SCORE
        };

        let (score, current_move) = loop {
            let (score, mv) = negamax(
                board,
                depth,
                alpha,
                beta,
                0,
                0,
                &mut info,
                tt,
                params,
                history_hashes,
                None,
                &eval_state,
                syzygy,
                None,
            );

            if info.aborted {
                break (score, mv);
            }

            if score <= alpha && alpha > -MATE_SCORE {
                alpha = (score - window).max(-MATE_SCORE);
                window = window.saturating_mul(2);
            } else if score >= beta && beta < MATE_SCORE {
                beta = (score + window).min(MATE_SCORE);
                window = window.saturating_mul(2);
            } else {
                break (score, mv);
            }
        };
        total_nodes += info.nodes;

        if info.aborted {
            break;
        }

        if let Some(m) = current_move {
            best_move = Some(m);
            best_score = score;
        }

        let elapsed_ms = info.start_time.elapsed().as_millis().max(1);
        let nps = ((total_nodes as u128) * 1000) / elapsed_ms;

        if is_main_thread {
            println!(
                "info depth {} time {} nodes {} nps {} score cp {} pv {}",
                depth,
                elapsed_ms,
                total_nodes,
                nps,
                best_score,
                extract_pv(board, tt)
            );
        }

        // Emergency Extra Time on sudden score drop
        if depth > 1 && score < prev_score - 150 {
            let scaled_ms = ((info.time_limit.as_millis() as f64) * 2.5) as u64;
            let mut new_limit = Duration::from_millis(scaled_ms);
            if let Some(clock) = total_clock {
                let max_clock = Duration::from_millis(((clock.as_millis() as f64) * 0.20) as u64);
                if new_limit > max_clock {
                    new_limit = max_clock;
                }
            }
            if new_limit > info.time_limit {
                info.time_limit = new_limit;
                let sl = if info.time_limit > Duration::from_millis(25) {
                    info.time_limit - Duration::from_millis(15)
                } else {
                    (info.time_limit * 8) / 10
                };
                info.deadline = Some(info.start_time + sl);
            }
        }
        prev_score = score;

        // Instant-Stop on stable best move
        if depth > 1 {
            if best_move == prev_best_move && best_move.is_some() {
                stable_count += 1;
            } else {
                stable_count = 1;
            }
        } else {
            stable_count = 1;
        }
        prev_best_move = best_move;

        if depth >= 12 && stable_count >= 4 && best_score > 100 {
            break;
        }

        if score.abs() > MATE_SCORE - MAX_DEPTH {
            break;
        }

        if !info.is_pondering.load(Ordering::Relaxed) {
            let current_limit = Duration::from_millis(info.time_limit_ms.load(Ordering::Relaxed));
            let effective_limit = if current_limit.is_zero() {
                info.time_limit
            } else {
                current_limit
            };
            let soft_limit = (effective_limit * 6) / 10;
            if info.start_time.elapsed() >= soft_limit {
                break;
            }
        }
    }

    let ponder_move = best_move.and_then(|bm| {
        let mut after = board.clone();
        after.play_unchecked(bm);
        let hash = after.hash();
        tt.get(hash).and_then(|entry| entry.best_move)
    });

    (best_move, ponder_move)
}
