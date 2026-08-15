use cozy_chess::{Board, Move, Piece, Square};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub(crate) const MAX_DEPTH: i32 = 64;
pub(crate) const MATE_SCORE: i32 = 100_000;
pub(crate) const MATE_THRESHOLD: i32 = 90_000;
pub(crate) const MAX_PLY: usize = 128;
pub(crate) const MAX_EXTENSIONS: i32 = 3;

pub struct SharedHistory {
    pub killers: [[AtomicU32; 2]; MAX_PLY],
    pub history: [[AtomicI32; 64]; 64],
    pub cont_history: [[AtomicI32; 64]; 64],
}

impl SharedHistory {
    pub fn new() -> Self {
        let killers = std::array::from_fn(|_| [AtomicU32::new(0), AtomicU32::new(0)]);
        let history = std::array::from_fn(|_| std::array::from_fn(|_| AtomicI32::new(0)));
        let cont_history = std::array::from_fn(|_| std::array::from_fn(|_| AtomicI32::new(0)));

        Self {
            killers,
            history,
            cont_history,
        }
    }

    pub fn clear(&self) {
        for i in 0..MAX_PLY {
            self.killers[i][0].store(0, Ordering::Relaxed);
            self.killers[i][1].store(0, Ordering::Relaxed);
        }
        for i in 0..64 {
            for j in 0..64 {
                self.history[i][j].store(0, Ordering::Relaxed);
                self.cont_history[i][j].store(0, Ordering::Relaxed);
            }
        }
    }

    pub fn decay(&self) {
        for i in 0..64 {
            for j in 0..64 {
                let h = self.history[i][j].load(Ordering::Relaxed);
                self.history[i][j].store(h / 2, Ordering::Relaxed);
                let c = self.cont_history[i][j].load(Ordering::Relaxed);
                self.cont_history[i][j].store(c / 2, Ordering::Relaxed);
            }
        }
    }

    pub fn get_killer(&self, ply: usize, slot: usize) -> Option<Move> {
        let val = self.killers[ply][slot].load(Ordering::Relaxed);
        crate::transposition::unpack_move(val as u16)
    }

    pub fn update_killer(&self, ply: usize, m: Move) {
        let val = crate::transposition::pack_move(Some(m)) as u32;
        let k0 = self.killers[ply][0].load(Ordering::Relaxed);
        if k0 != val {
            self.killers[ply][1].store(k0, Ordering::Relaxed);
            self.killers[ply][0].store(val, Ordering::Relaxed);
        }
    }

    pub fn get_history(&self, from: Square, to: Square) -> i32 {
        self.history[from as usize][to as usize].load(Ordering::Relaxed)
    }

    pub fn add_history(&self, from: Square, to: Square, bonus: i32) {
        let f = from as usize;
        let t = to as usize;
        let val = self.history[f][t].load(Ordering::Relaxed);
        self.history[f][t].store((val + bonus).min(10_000), Ordering::Relaxed);
    }

    pub fn get_cont_history(&self, prev_to: Square, curr_to: Square) -> i32 {
        self.cont_history[prev_to as usize][curr_to as usize].load(Ordering::Relaxed)
    }

    pub fn add_cont_history(&self, prev_to: Square, curr_to: Square, bonus: i32) {
        let p = prev_to as usize;
        let c = curr_to as usize;
        let val = self.cont_history[p][c].load(Ordering::Relaxed);
        self.cont_history[p][c].store((val + bonus).min(10_000), Ordering::Relaxed);
    }
}

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
    pub fn as_slice(&self) -> &[Move] {
        &self.moves[..self.len]
    }

    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    TtMove,
    Captures,
    Killers,
    Quiets,
    Done,
}

pub(crate) struct StagedMovePicker<'a> {
    stack: &'a MoveStack,
    tt_move: Option<Move>,
    killers: [Option<Move>; 2],
    stage: Stage,
    current_index: usize,
    scores: [i32; 256],
    scored_captures: bool,
    scored_quiets: bool,
}

impl<'a> StagedMovePicker<'a> {
    pub fn new(stack: &'a MoveStack, tt_move: Option<Move>, killers: [Option<Move>; 2]) -> Self {
        Self {
            stack,
            tt_move,
            killers,
            stage: Stage::TtMove,
            current_index: 0,
            scores: [i32::MIN; 256],
            scored_captures: false,
            scored_quiets: false,
        }
    }

    #[inline]
    pub fn pick_next(
        &mut self,
        board: &Board,
        shared: &SharedHistory,
        prev_move: Option<Move>,
    ) -> Option<(Move, usize)> {
        loop {
            match self.stage {
                Stage::TtMove => {
                    self.stage = Stage::Captures;
                    if let Some(tm) = self.tt_move {
                        if let Some(idx) = self.stack.as_slice().iter().position(|&m| m == tm) {
                            return Some((tm, idx));
                        }
                    }
                }
                Stage::Captures => {
                    if !self.scored_captures {
                        self.scored_captures = true;
                        for (i, &m) in self.stack.as_slice().iter().enumerate() {
                            if Some(m) == self.tt_move {
                                continue;
                            }
                            let is_cap = board.color_on(m.to).is_some()
                                || (board.piece_on(m.from) == Some(Piece::Pawn)
                                    && m.from.file() != m.to.file()
                                    && board.color_on(m.to).is_none());
                            let is_promo = m.promotion.is_some();
                            if is_cap || is_promo {
                                let victim_val = board
                                    .piece_on(m.to)
                                    .map(crate::eval::piece_value)
                                    .unwrap_or(0);
                                let attacker_val = board
                                    .piece_on(m.from)
                                    .map(crate::eval::piece_value)
                                    .unwrap_or(100);
                                let promo_val =
                                    m.promotion.map(crate::eval::piece_value).unwrap_or(0);
                                self.scores[i] =
                                    100_000 + victim_val * 10 - attacker_val + promo_val * 10;
                            }
                        }
                    }

                    let mut best_idx = None;
                    let mut best_score = i32::MIN;
                    for (i, &score) in self.scores[..self.stack.len].iter().enumerate() {
                        if score > best_score {
                            best_score = score;
                            best_idx = Some(i);
                        }
                    }

                    if let Some(idx) = best_idx {
                        self.scores[idx] = i32::MIN;
                        return Some((self.stack.moves[idx], idx));
                    } else {
                        self.stage = Stage::Killers;
                        self.current_index = 0;
                    }
                }
                Stage::Killers => {
                    if self.current_index < 2 {
                        let k = self.killers[self.current_index];
                        self.current_index += 1;
                        if let Some(km) = k {
                            if Some(km) != self.tt_move {
                                if let Some(idx) =
                                    self.stack.as_slice().iter().position(|&m| m == km)
                                {
                                    let is_cap = board.color_on(km.to).is_some()
                                        || (board.piece_on(km.from) == Some(Piece::Pawn)
                                            && km.from.file() != km.to.file()
                                            && board.color_on(km.to).is_none());
                                    let is_promo = km.promotion.is_some();
                                    if !is_cap && !is_promo {
                                        return Some((km, idx));
                                    }
                                }
                            }
                        }
                    } else {
                        self.stage = Stage::Quiets;
                    }
                }
                Stage::Quiets => {
                    if !self.scored_quiets {
                        self.scored_quiets = true;
                        for (i, &m) in self.stack.as_slice().iter().enumerate() {
                            if Some(m) == self.tt_move || self.killers.contains(&Some(m)) {
                                continue;
                            }
                            let is_cap = board.color_on(m.to).is_some()
                                || (board.piece_on(m.from) == Some(Piece::Pawn)
                                    && m.from.file() != m.to.file()
                                    && board.color_on(m.to).is_none());
                            let is_promo = m.promotion.is_some();
                            if !is_cap && !is_promo {
                                let mut score = shared.get_history(m.from, m.to);
                                if let Some(pm) = prev_move {
                                    score += shared.get_cont_history(pm.to, m.to);
                                }
                                self.scores[i] = score;
                            }
                        }
                    }

                    let mut best_idx = None;
                    let mut best_score = i32::MIN;
                    for (i, &score) in self.scores[..self.stack.len].iter().enumerate() {
                        if score != i32::MIN {
                            if best_idx.is_none() || score > best_score {
                                best_score = score;
                                best_idx = Some(i);
                            }
                        }
                    }

                    if let Some(idx) = best_idx {
                        self.scores[idx] = i32::MIN;
                        return Some((self.stack.moves[idx], idx));
                    } else {
                        self.stage = Stage::Done;
                    }
                }
                Stage::Done => return None,
            }
        }
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
