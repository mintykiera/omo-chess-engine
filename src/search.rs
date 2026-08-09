use cozy_chess::{
    Board,
    Move,
    Piece,
    Square,
    Color,
    get_knight_moves,
    get_bishop_moves,
    get_rook_moves,
    BitBoard,
};
use std::sync::Arc;
use std::sync::atomic::{ AtomicBool, AtomicU64, Ordering };
use std::time::{ Duration, Instant };

use crate::eval::{ evaluate_board, piece_value, EvalParams };
use crate::transposition::{ NodeType, TranspositionTable };

const MAX_DEPTH: i32 = 64;
const MATE_SCORE: i32 = 100_000;
const MATE_THRESHOLD: i32 = 90_000;
const MAX_PLY: usize = 128;

#[inline]
fn score_to_tt(score: i32, ply: i32) -> i32 {
    if score > MATE_THRESHOLD {
        score + ply
    } else if score < -MATE_THRESHOLD {
        score - ply
    } else {
        score
    }
}

#[inline]
fn score_from_tt(score: i32, ply: i32) -> i32 {
    if score > MATE_THRESHOLD {
        score - ply
    } else if score < -MATE_THRESHOLD {
        score + ply
    } else {
        score
    }
}

pub fn extract_pv(board: &Board, tt: &TranspositionTable) -> String {
    let mut current_board = board.clone();
    let mut pv_moves = Vec::new();
    let mut visited_hashes = Vec::new();

    for _ in 0..MAX_PLY {
        let hash = current_board.hash();
        if visited_hashes.contains(&hash) {
            break;
        }
        visited_hashes.push(hash);

        if let Some(entry) = tt.get(hash) {
            if let Some(best_move) = entry.best_move {
                let mut is_legal = false;
                current_board.generate_moves(|move_list| {
                    for m in move_list {
                        if m == best_move {
                            is_legal = true;
                        }
                    }
                    false
                });

                if is_legal {
                    pv_moves.push(crate::format_uci_move(&current_board, best_move));
                    current_board.play_unchecked(best_move);
                } else {
                    break;
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }

    pv_moves.join(" ")
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
        time_limit_ms: Arc<AtomicU64>
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
        if (self.nodes & 127) == 0 {
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
                if Instant::now() >= dl {
                    self.aborted = true;
                    return;
                }
            }
        }
    }
}

#[inline]
fn sq_idx(sq: Square) -> usize {
    (sq.rank() as usize) * 8 + (sq.file() as usize)
}

#[inline]
fn lmr_reduction(depth: i32, move_index: i32) -> i32 {
    let d = depth as f64;
    let m = move_index as f64;
    (0.75 + (d.ln() * m.ln()) / 2.25) as i32
}

fn get_least_valuable_attacker(
    board: &Board,
    sq: Square,
    color: Color,
    occupied: BitBoard
) -> Option<(Piece, Square)> {
    let friendly = board.colors(color) & occupied;

    let pawns = board.pieces(Piece::Pawn) & friendly;
    for p in pawns {
        let p_rank = p.rank() as i8;
        let p_file = p.file() as i8;
        let sq_rank = sq.rank() as i8;
        let sq_file = sq.file() as i8;
        if (sq_file - p_file).abs() == 1 {
            if
                (color == Color::White && sq_rank - p_rank == 1) ||
                (color == Color::Black && sq_rank - p_rank == -1)
            {
                return Some((Piece::Pawn, p));
            }
        }
    }

    let knights = board.pieces(Piece::Knight) & friendly;
    for p in knights {
        if get_knight_moves(p).has(sq) {
            return Some((Piece::Knight, p));
        }
    }

    let bishops = board.pieces(Piece::Bishop) & friendly;
    for p in bishops {
        if get_bishop_moves(p, occupied).has(sq) {
            return Some((Piece::Bishop, p));
        }
    }

    let rooks = board.pieces(Piece::Rook) & friendly;
    for p in rooks {
        if get_rook_moves(p, occupied).has(sq) {
            return Some((Piece::Rook, p));
        }
    }

    let queens = board.pieces(Piece::Queen) & friendly;
    for p in queens {
        if get_bishop_moves(p, occupied).has(sq) || get_rook_moves(p, occupied).has(sq) {
            return Some((Piece::Queen, p));
        }
    }

    let kings = board.pieces(Piece::King) & friendly;
    for p in kings {
        let p_rank = p.rank() as i8;
        let p_file = p.file() as i8;
        let sq_rank = sq.rank() as i8;
        let sq_file = sq.file() as i8;
        if (p_rank - sq_rank).abs() <= 1 && (p_file - sq_file).abs() <= 1 {
            return Some((Piece::King, p));
        }
    }
    None
}

fn see(board: &Board, m: Move, params: &EvalParams) -> i32 {
    let mut gains = Vec::with_capacity(32);

    let victim = board
        .piece_on(m.to)
        .map(|p| piece_value(p, params))
        .unwrap_or(0);
    let promo = m.promotion
        .map(|p| piece_value(p, params) - piece_value(Piece::Pawn, params))
        .unwrap_or(0);
    gains.push(victim + promo);

    let mut attacker = board.piece_on(m.from).unwrap();
    if m.promotion.is_some() {
        attacker = m.promotion.unwrap();
    }

    let mut occupied = board.occupied();
    let mut current_color = board.side_to_move();
    let mut attacker_sq = m.from;

    loop {
        occupied &= !attacker_sq.bitboard();
        current_color = !current_color;

        if let Some((p, sq)) = get_least_valuable_attacker(board, m.to, current_color, occupied) {
            gains.push(piece_value(attacker, params));
            attacker = p;
            attacker_sq = sq;
        } else {
            break;
        }
    }

    let mut score = 0;
    for i in (1..gains.len()).rev() {
        score = (gains[i] - score).max(0);
    }
    gains[0] - score
}

fn score_move(
    board: &Board,
    m: &Move,
    tt_move: Option<Move>,
    ply: usize,
    killers: &[[Option<Move>; 2]],
    history: &[[i32; 64]; 64],
    cont_history: &[[i32; 64]; 64],
    prev_move: Option<Move>,
    params: &EvalParams
) -> i32 {
    if Some(*m) == tt_move {
        return 1_000_000;
    }

    if board.color_on(m.to).is_some() {
        let see_score = see(board, *m, params);
        if see_score < 0 {
            return -50_000 + see_score;
        }

        let victim_val = board
            .piece_on(m.to)
            .map(|p| piece_value(p, params))
            .unwrap_or(0);
        let attacker_val = board
            .piece_on(m.from)
            .map(|p| piece_value(p, params))
            .unwrap_or(100);
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

pub fn get_best_move(
    board: &Board,
    time_limit: Duration,
    tt: &TranspositionTable,
    stop_flag: Arc<AtomicBool>,
    is_pondering: Arc<AtomicBool>,
    time_limit_ms: Arc<AtomicU64>,
    is_main_thread: bool,
    thread_id: usize,
    params: &EvalParams,
    history_hashes: &mut Vec<u64>
) -> (Option<Move>, Option<Move>) {
    let mut info = SearchInfo::new(
        time_limit,
        stop_flag,
        is_pondering.clone(),
        time_limit_ms.clone()
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

    let start_depth = if thread_id > 0 { ((thread_id % 2) as i32) + 1 } else { 1 };
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
        let mut alpha = if depth > 1 { best_score - window } else { -MATE_SCORE };
        let mut beta = if depth > 1 { best_score + window } else { MATE_SCORE };

        let (score, current_move) = loop {
            let (score, mv) = negamax(
                board,
                depth,
                alpha,
                beta,
                0,
                &mut info,
                tt,
                params,
                history_hashes,
                None
            );

            if info.aborted {
                break (score, mv);
            }

            if score <= alpha || score >= beta {
                alpha = (score - window).max(-MATE_SCORE);
                beta = (score + window).min(MATE_SCORE);
                window *= 2;
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

        if score.abs() > MATE_SCORE - MAX_DEPTH {
            break;
        }

        if !info.is_pondering.load(Ordering::Relaxed) {
            let current_limit = Duration::from_millis(info.time_limit_ms.load(Ordering::Relaxed));
            let effective_limit = if current_limit.is_zero() { time_limit } else { current_limit };
            if info.start_time.elapsed() >= effective_limit / 2 {
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

fn negamax(
    board: &Board,
    depth: i32,
    mut alpha: i32,
    mut beta: i32,
    ply: i32,
    info: &mut SearchInfo,
    tt: &TranspositionTable,
    params: &EvalParams,
    history_hashes: &mut Vec<u64>,
    prev_move: Option<Move>
) -> (i32, Option<Move>) {
    info.check_time();
    if info.aborted {
        return (0, None);
    }

    info.nodes += 1;

    let hash = board.hash();
    if ply > 0 && history_hashes.contains(&hash) {
        return (0, None);
    }

    let original_alpha = alpha;
    let mut tt_move: Option<Move> = None;

    if let Some(entry) = tt.get(hash) {
        tt_move = entry.best_move;
        if entry.depth >= depth {
            let score = score_from_tt(entry.score, ply);
            match entry.node_type {
                NodeType::Exact => {
                    return (score, entry.best_move);
                }
                NodeType::LowerBound => {
                    alpha = alpha.max(score);
                }
                NodeType::UpperBound => {
                    beta = beta.min(score);
                }
            }
            if alpha >= beta {
                return (score, entry.best_move);
            }
        }
    }

    if depth <= 0 {
        return (quiescence_search(board, alpha, beta, info, params), None);
    }

    let in_check = !board.checkers().is_empty();
    let depth = if in_check { depth + 1 } else { depth };

    if !in_check && ply > 0 && depth <= 3 {
        let static_eval = evaluate_board(board, params);
        if static_eval - 120 * depth >= beta {
            return (beta, None);
        }
    }

    if !in_check && ply > 0 && depth >= 3 {
        let our_pieces =
            (board.pieces(Piece::Knight) |
                board.pieces(Piece::Bishop) |
                board.pieces(Piece::Rook) |
                board.pieces(Piece::Queen)) &
            board.colors(board.side_to_move());
        if our_pieces.len() > 0 {
            if let Some(null_board) = board.null_move() {
                let r = if depth >= 6 { 3 } else { 2 };
                history_hashes.push(hash);
                let (raw, _) = negamax(
                    &null_board,
                    depth - 1 - r,
                    -beta,
                    -beta + 1,
                    ply + 1,
                    info,
                    tt,
                    params,
                    history_hashes,
                    None
                );
                history_hashes.pop();
                if info.aborted {
                    return (0, None);
                }
                let null_score = -raw;
                if null_score >= beta {
                    return (beta, None);
                }
            }
        }
    }

    let mut moves = Vec::new();
    board.generate_moves(|move_list| {
        moves.extend(move_list);
        false
    });

    if moves.is_empty() {
        return if !board.checkers().is_empty() { (-MATE_SCORE + ply, None) } else { (0, None) };
    }

    let ply_idx = ply as usize;
    {
        let killers = &info.killers;
        let history = &info.history;
        let cont_history = &info.cont_history;
        moves.sort_by_key(|m| {
            -score_move(
                board,
                m,
                tt_move,
                ply_idx,
                killers,
                history,
                cont_history,
                prev_move,
                params
            )
        });
    }

    let mut best_move: Option<Move> = None;
    let mut best_score = -MATE_SCORE;

    for (i, m) in moves.iter().enumerate() {
        let mut next_board = board.clone();
        next_board.play_unchecked(*m);

        let mut score;
        let is_capture = board.color_on(m.to).is_some();
        let gives_check = next_board.checkers().len() > 0;

        if i == 0 {
            history_hashes.push(hash);
            let (raw, _) = negamax(
                &next_board,
                depth - 1,
                -beta,
                -alpha,
                ply + 1,
                info,
                tt,
                params,
                history_hashes,
                Some(*m)
            );
            history_hashes.pop();
            score = -raw;
        } else {
            let mut reduced_depth = depth - 1;
            let is_killer =
                ply_idx < MAX_PLY &&
                (info.killers[ply_idx][0] == Some(*m) || info.killers[ply_idx][1] == Some(*m));
            let do_lmr =
                i >= 3 && depth >= 3 && !is_capture && !gives_check && !in_check && !is_killer;
            if do_lmr {
                let r = lmr_reduction(depth, i as i32);
                reduced_depth = (depth - 1 - r).max(1);
            }

            history_hashes.push(hash);
            let (raw, _) = negamax(
                &next_board,
                reduced_depth,
                -alpha - 1,
                -alpha,
                ply + 1,
                info,
                tt,
                params,
                history_hashes,
                Some(*m)
            );
            score = -raw;

            if score > alpha {
                if do_lmr || score < beta {
                    let (raw, _) = negamax(
                        &next_board,
                        depth - 1,
                        -beta,
                        -alpha,
                        ply + 1,
                        info,
                        tt,
                        params,
                        history_hashes,
                        Some(*m)
                    );
                    score = -raw;
                }
            }
            history_hashes.pop();
        }

        if info.aborted {
            return (0, None);
        }

        if score > best_score {
            best_score = score;
            best_move = Some(*m);
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            if board.color_on(m.to).is_none() {
                if ply_idx < MAX_PLY {
                    info.killers[ply_idx][1] = info.killers[ply_idx][0];
                    info.killers[ply_idx][0] = Some(*m);
                }
                let bonus = depth * depth;
                let entry = &mut info.history[sq_idx(m.from)][sq_idx(m.to)];
                *entry = (*entry + bonus).min(10_000);

                if let Some(pm) = prev_move {
                    let centry = &mut info.cont_history[sq_idx(pm.to)][sq_idx(m.to)];
                    *centry = (*centry + bonus).min(10_000);
                }
            }
            break;
        }
    }

    let node_type = if best_score <= original_alpha {
        NodeType::UpperBound
    } else if best_score >= beta {
        NodeType::LowerBound
    } else {
        NodeType::Exact
    };
    tt.insert(hash, depth, score_to_tt(best_score, ply), node_type, best_move);

    (best_score, best_move)
}

fn quiescence_search(
    board: &Board,
    mut alpha: i32,
    beta: i32,
    info: &mut SearchInfo,
    params: &EvalParams
) -> i32 {
    info.check_time();
    if info.aborted {
        return 0;
    }

    info.nodes += 1;

    let in_check = !board.checkers().is_empty();
    let stand_pat = if in_check { -MATE_SCORE } else { evaluate_board(board, params) };

    if !in_check {
        if stand_pat >= beta {
            return beta;
        }
        if stand_pat > alpha {
            alpha = stand_pat;
        }

        const BIG_DELTA: i32 = 1000;
        if stand_pat + BIG_DELTA < alpha {
            return alpha;
        }
    }

    let mut moves = Vec::new();
    board.generate_moves(|move_list| {
        for m in move_list {
            if in_check || board.color_on(m.to).is_some() || m.promotion.is_some() {
                moves.push(m);
            }
        }
        false
    });

    if in_check && moves.is_empty() {
        return -MATE_SCORE;
    }

    moves.sort_by_key(|m| {
        let victim_val = board
            .piece_on(m.to)
            .map(|p| piece_value(p, params))
            .unwrap_or(0);
        let promo_val = m.promotion.map(|p| piece_value(p, params)).unwrap_or(0);
        -(victim_val + promo_val)
    });

    const DELTA_MARGIN: i32 = 200;
    for m in &moves {
        if !in_check {
            let victim_val = board
                .piece_on(m.to)
                .map(|p| piece_value(p, params))
                .unwrap_or(0);
            let promo_val = m.promotion.map(|p| piece_value(p, params)).unwrap_or(0);
            if stand_pat + victim_val + promo_val + DELTA_MARGIN < alpha {
                continue;
            }

            if see(board, *m, params) < 0 {
                continue;
            }
        }

        let mut next_board = board.clone();
        next_board.play_unchecked(*m);

        let score = -quiescence_search(&next_board, -beta, -alpha, info, params);

        if info.aborted {
            return 0;
        }

        if score >= beta {
            return beta;
        }
        if score > alpha {
            alpha = score;
        }
    }

    alpha
}
