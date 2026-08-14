use cozy_chess::{Board, Move, Square};

use crate::eval::piece_value;

#[inline]
pub(crate) fn sq_idx(sq: Square) -> usize {
    (sq.rank() as usize) * 8 + (sq.file() as usize)
}

#[inline]
pub(crate) fn lmr_reduction(depth: i32, move_index: i32) -> i32 {
    let d = depth as f64;
    let m = move_index as f64;
    (0.75 + (d.ln() * m.ln()) / 2.25) as i32
}

pub(crate) fn score_move(
    board: &Board,
    m: &Move,
    tt_move: Option<Move>,
    ply: usize,
    killers: &[[Option<Move>; 2]],
    history: &[[i32; 64]; 64],
    cont_history: &[[i32; 64]; 64],
    prev_move: Option<Move>,
) -> i32 {
    if Some(*m) == tt_move {
        return 1_000_000;
    }

    if board.color_on(m.to).is_some() {
        let victim_val = board.piece_on(m.to).map(piece_value).unwrap_or(0);
        let attacker_val = board.piece_on(m.from).map(piece_value).unwrap_or(100);
        return 100_000 + victim_val * 10 - attacker_val;
    }

    if ply < killers.len() {
        if killers[ply][0] == Some(*m) {
            return 90_000;
        }
        if killers[ply][1] == Some(*m) {
            return 80_000;
        }
    }

    let mut score = history[sq_idx(m.from)][sq_idx(m.to)];
    if board.color_on(m.to).is_none() {
        if let Some(pm) = prev_move {
            score += cont_history[sq_idx(pm.to)][sq_idx(m.to)];
        }
    }
    score
}
