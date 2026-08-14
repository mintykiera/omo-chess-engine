use cozy_chess::{Board, Move, Square};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use super::ordering::score_move;

pub(crate) const MAX_DEPTH: i32 = 64;
pub(crate) const MATE_SCORE: i32 = 100_000;
pub(crate) const MATE_THRESHOLD: i32 = 90_000;
pub(crate) const MAX_PLY: usize = 128;
pub(crate) const MAX_EXTENSIONS: i32 = 3;

#[inline]
pub(crate) fn score_to_tt(score: i32, ply: i32) -> i32 {
    if score > MATE_THRESHOLD {
        score + ply
    } else if score < -MATE_THRESHOLD {
        score - ply
    } else {
        score
    }
}

pub(crate) struct MoveStack {
    moves: [Move; 256],
    len: usize,
}

impl MoveStack {
    pub fn new() -> Self {
        Self {
            moves: [Move {
                from: Square::A1,
                to: Square::A1,
                promotion: None,
            }; 256],
            len: 0,
        }
    }

    #[inline(always)]
    pub fn push(&mut self, m: Move) {
        if self.len < 256 {
            self.moves[self.len] = m;
            self.len += 1;
        }
    }

    #[inline(always)]
    pub fn as_mut_slice(&mut self) -> &mut [Move] {
        &mut self.moves[..self.len]
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

pub(crate) struct ScoredMoveList {
    moves: [Move; 256],
    scores: [i32; 256],
    len: usize,
    current: usize,
}

impl ScoredMoveList {
    pub fn from_stack(
        stack: &MoveStack,
        board: &Board,
        tt_move: Option<Move>,
        ply: usize,
        killers: &[[Option<Move>; 2]],
        history: &[[i32; 64]; 64],
        cont_history: &[[i32; 64]; 64],
        prev_move: Option<Move>,
    ) -> Self {
        let mut result = Self {
            moves: [Move {
                from: Square::A1,
                to: Square::A1,
                promotion: None,
            }; 256],
            scores: [0; 256],
            len: stack.len,
            current: 0,
        };
        for i in 0..stack.len {
            result.moves[i] = stack.moves[i];
            result.scores[i] = score_move(
                board,
                &stack.moves[i],
                tt_move,
                ply,
                killers,
                history,
                cont_history,
                prev_move,
            );
        }
        result
    }

    #[inline]
    pub fn pick_next(&mut self) -> Option<(Move, usize)> {
        if self.current >= self.len {
            return None;
        }
        let mut best_idx = self.current;
        for i in self.current + 1..self.len {
            if self.scores[i] > self.scores[best_idx] {
                best_idx = i;
            }
        }
        self.moves.swap(self.current, best_idx);
        self.scores.swap(self.current, best_idx);
        let idx = self.current;
        self.current += 1;
        Some((self.moves[idx], idx))
    }
}

#[inline]
pub(crate) fn score_from_tt(score: i32, ply: i32) -> i32 {
    if score > MATE_THRESHOLD {
        score - ply
    } else if score < -MATE_THRESHOLD {
        score + ply
    } else {
        score
    }
}

pub struct SearchInfo {
    pub start_time: Instant,
    pub time_limit: Duration,
    pub deadline: Option<Instant>,
    pub nodes: u64,
    pub aborted: bool,
    pub killers: [[Option<Move>; 2]; MAX_PLY],
    pub history: [[i32; 64]; 64],
    pub cont_history: [[i32; 64]; 64],
    pub stop_flag: Arc<AtomicBool>,
    pub is_pondering: Arc<AtomicBool>,
    pub time_limit_ms: Arc<AtomicU64>,
    pub was_pondering: bool,
}

impl SearchInfo {
    pub fn new(
        time_limit: Duration,
        stop_flag: Arc<AtomicBool>,
        is_pondering: Arc<AtomicBool>,
        time_limit_ms: Arc<AtomicU64>,
    ) -> Self {
        let safe_limit = if time_limit > Duration::from_millis(25) {
            time_limit - Duration::from_millis(15)
        } else {
            (time_limit * 8) / 10
        };
        let deadline = Some(Instant::now() + safe_limit);
        Self {
            start_time: Instant::now(),
            time_limit,
            deadline,
            nodes: 0,
            aborted: false,
            killers: [[None; 2]; MAX_PLY],
            history: [[0; 64]; 64],
            cont_history: [[0; 64]; 64],
            stop_flag,
            is_pondering,
            time_limit_ms,
            was_pondering: false,
        }
    }

    #[inline]
    pub fn check_time(&mut self) {
        if (self.nodes & 1023) == 0 {
            if self.stop_flag.load(Ordering::Relaxed) {
                self.aborted = true;
                return;
            }

            let currently_pondering = self.is_pondering.load(Ordering::Relaxed);

            if !currently_pondering && self.was_pondering {
                self.start_time = Instant::now();
                let sl = if self.time_limit > Duration::from_millis(25) {
                    self.time_limit - Duration::from_millis(15)
                } else {
                    (self.time_limit * 8) / 10
                };
                self.deadline = Some(self.start_time + sl);
                self.was_pondering = false;
            }

            if currently_pondering {
                return;
            }

            if let Some(dl) = self.deadline {
                let now = Instant::now();
                if now >= dl {
                    self.aborted = true;
                    return;
                }
                if dl.duration_since(now) < Duration::from_millis(50) {
                    self.aborted = true;
                    return;
                }
            }
        }
    }
}
