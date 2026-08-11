use cozy_chess::{
    BitBoard, Board, Color, Move, Piece, Square, get_bishop_moves, get_king_moves,
    get_knight_moves, get_rook_moves,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use crate::eval::{EvalParams, EvalState, evaluate_board_incremental, piece_value};
use crate::transposition::{NodeType, TranspositionTable};

const MAX_DEPTH: i32 = 64;
const MATE_SCORE: i32 = 100_000;
const MATE_THRESHOLD: i32 = 90_000;
const MAX_PLY: usize = 128;
const MAX_EXTENSIONS: i32 = 3;

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

struct MoveStack {
    moves: [Move; 256],
    len: usize,
}

impl MoveStack {
    fn new() -> Self {
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
    fn push(&mut self, m: Move) {
        if self.len < 256 {
            self.moves[self.len] = m;
            self.len += 1;
        }
    }

    #[inline(always)]
    fn as_mut_slice(&mut self) -> &mut [Move] {
        &mut self.moves[..self.len]
    }

    #[inline(always)]
    fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// Zero-allocation staged move picker. Scores all moves upfront, then
// yields them best-first via partial selection sort — only the next-best
// move is found on each call, so moves after a beta cutoff are never sorted.
struct ScoredMoveList {
    moves: [Move; 256],
    scores: [i32; 256],
    len: usize,
    current: usize,
}

impl ScoredMoveList {
    fn from_stack(
        stack: &MoveStack,
        board: &Board,
        tt_move: Option<Move>,
        ply: usize,
        killers: &[[Option<Move>; 2]],
        history: &[[i32; 64]; 64],
        cont_history: &[[i32; 64]; 64],
        prev_move: Option<Move>,
        params: &EvalParams,
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
                params,
            );
        }
        result
    }

    /// Pick the next best move via partial selection sort.
    #[inline]
    fn pick_next(&mut self) -> Option<(Move, usize)> {
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
        if (self.nodes & 4095) == 0 {
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

#[allow(dead_code)]
fn get_least_valuable_attacker(
    board: &Board,
    sq: Square,
    color: Color,
    occupied: BitBoard,
) -> Option<(Piece, Square)> {
    let friendly = board.colors(color) & occupied;
    let sq_bb = sq.bitboard();

    let pawn_attackers = match color {
        Color::White => {
            let a = (sq_bb.0 >> 9) & 0x7f7f7f7f7f7f7f7f;
            let b = (sq_bb.0 >> 7) & 0xfefefefefefefefe;
            cozy_chess::BitBoard(a | b)
        }
        Color::Black => {
            let a = (sq_bb.0 << 9) & 0xfefefefefefefefe;
            let b = (sq_bb.0 << 7) & 0x7f7f7f7f7f7f7f7f;
            cozy_chess::BitBoard(a | b)
        }
    };

    let pawns = pawn_attackers & board.pieces(Piece::Pawn) & friendly;
    if let Some(p) = pawns.into_iter().next() {
        return Some((Piece::Pawn, p));
    }

    let knights = get_knight_moves(sq) & board.pieces(Piece::Knight) & friendly;
    if let Some(p) = knights.into_iter().next() {
        return Some((Piece::Knight, p));
    }

    let bishops = get_bishop_moves(sq, occupied) & board.pieces(Piece::Bishop) & friendly;
    if let Some(p) = bishops.into_iter().next() {
        return Some((Piece::Bishop, p));
    }

    let rooks = get_rook_moves(sq, occupied) & board.pieces(Piece::Rook) & friendly;
    if let Some(p) = rooks.into_iter().next() {
        return Some((Piece::Rook, p));
    }

    let queens = (get_bishop_moves(sq, occupied) | get_rook_moves(sq, occupied))
        & board.pieces(Piece::Queen)
        & friendly;
    if let Some(p) = queens.into_iter().next() {
        return Some((Piece::Queen, p));
    }

    let kings = get_king_moves(sq) & board.pieces(Piece::King) & friendly;
    if let Some(p) = kings.into_iter().next() {
        return Some((Piece::King, p));
    }

    None
}

/// Compute all pieces of BOTH colors attacking a given square,
/// using the provided occupied bitboard for sliding piece ray casting.
fn all_attackers_to(board: &Board, sq: Square, occupied: BitBoard) -> BitBoard {
    let sq_bb = sq.bitboard();

    // Squares where White/Black pawns would be to attack sq
    let w_pawn_from =
        BitBoard(((sq_bb.0 >> 9) & 0x7f7f7f7f7f7f7f7f) | ((sq_bb.0 >> 7) & 0xfefefefefefefefe));
    let b_pawn_from =
        BitBoard(((sq_bb.0 << 9) & 0xfefefefefefefefe) | ((sq_bb.0 << 7) & 0x7f7f7f7f7f7f7f7f));

    let pawns = ((w_pawn_from & board.colors(Color::White))
        | (b_pawn_from & board.colors(Color::Black)))
        & board.pieces(Piece::Pawn);
    let knights = get_knight_moves(sq) & board.pieces(Piece::Knight);
    let kings = get_king_moves(sq) & board.pieces(Piece::King);

    let diag = get_bishop_moves(sq, occupied);
    let orth = get_rook_moves(sq, occupied);
    let bishops_queens = diag & (board.pieces(Piece::Bishop) | board.pieces(Piece::Queen));
    let rooks_queens = orth & (board.pieces(Piece::Rook) | board.pieces(Piece::Queen));

    (pawns | knights | kings | bishops_queens | rooks_queens) & occupied
}

/// Static Exchange Evaluation with proper X-ray discovery.
/// Uses a stack-allocated gains array (zero heap allocation) and
/// recalculates sliding attackers after each piece removal to unmask
/// any pieces that were hiding behind the captured/moved piece.
fn see(board: &Board, m: Move, params: &EvalParams) -> i32 {
    let mut gains = [0i32; 32];

    let victim = board
        .piece_on(m.to)
        .map(|p| piece_value(p, params))
        .unwrap_or(0);
    let promo = m
        .promotion
        .map(|p| piece_value(p, params) - piece_value(Piece::Pawn, params))
        .unwrap_or(0);
    gains[0] = victim + promo;

    let mut attacker = board.piece_on(m.from).unwrap();
    if let Some(p) = m.promotion {
        attacker = p;
    }

    let mut occupied = board.occupied();
    occupied &= !m.from.bitboard();

    // Compute initial attacker set with the moving piece already removed
    let mut attackers = all_attackers_to(board, m.to, occupied);
    let mut current_color = !board.side_to_move();
    let mut d: usize = 0;

    let piece_order = [
        Piece::Pawn,
        Piece::Knight,
        Piece::Bishop,
        Piece::Rook,
        Piece::Queen,
        Piece::King,
    ];

    loop {
        let color_attackers = attackers & board.colors(current_color) & occupied;
        if color_attackers.is_empty() {
            break;
        }

        // Find least valuable attacker of current_color
        let mut found_piece = Piece::King;
        let mut found_sq = Square::A1;
        let mut found = false;
        for &p in &piece_order {
            let candidates = color_attackers & board.pieces(p);
            if let Some(sq) = candidates.into_iter().next() {
                found_piece = p;
                found_sq = sq;
                found = true;
                break;
            }
        }
        if !found {
            break;
        }

        d += 1;
        gains[d] = piece_value(attacker, params);

        attacker = found_piece;
        occupied &= !found_sq.bitboard();
        attackers &= !found_sq.bitboard();

        // X-ray discovery: recalculate sliding attackers with updated occupied.
        // Any Bishop/Rook/Queen that was hidden behind the removed piece is now unmasked.
        let diag = get_bishop_moves(m.to, occupied);
        let orth = get_rook_moves(m.to, occupied);
        let new_sliders = ((diag & (board.pieces(Piece::Bishop) | board.pieces(Piece::Queen)))
            | (orth & (board.pieces(Piece::Rook) | board.pieces(Piece::Queen))))
            & occupied;
        attackers |= new_sliders;

        current_color = !current_color;
    }

    // Resolve gains with negamax (equivalent to the standard SEE resolution)
    let mut i = d as i32;
    while i >= 1 {
        gains[(i as usize) - 1] -= gains[i as usize].max(0);
        i -= 1;
    }
    gains[0]
}

/// Score a move for ordering. Uses pure MVV-LVA for captures (no SEE),
/// hash move priority, killer heuristic, and history/countermove tables.
fn score_move(
    board: &Board,
    m: &Move,
    tt_move: Option<Move>,
    ply: usize,
    killers: &[[Option<Move>; 2]],
    history: &[[i32; 64]; 64],
    cont_history: &[[i32; 64]; 64],
    prev_move: Option<Move>,
    params: &EvalParams,
) -> i32 {
    if Some(*m) == tt_move {
        return 1_000_000;
    }

    // Pure MVV-LVA for captures — SEE is too expensive for move ordering
    if board.color_on(m.to).is_some() {
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
    history_hashes: &mut Vec<u64>,
) -> (Option<Move>, Option<Move>) {
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

        if score.abs() > MATE_SCORE - MAX_DEPTH {
            break;
        }

        if !info.is_pondering.load(Ordering::Relaxed) {
            let current_limit = Duration::from_millis(info.time_limit_ms.load(Ordering::Relaxed));
            let effective_limit = if current_limit.is_zero() {
                time_limit
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

fn negamax(
    board: &Board,
    depth: i32,
    mut alpha: i32,
    mut beta: i32,
    ply: i32,
    extensions: i32,
    info: &mut SearchInfo,
    tt: &TranspositionTable,
    params: &EvalParams,
    history_hashes: &mut Vec<u64>,
    prev_move: Option<Move>,
    eval_state: &EvalState,
) -> (i32, Option<Move>) {
    info.check_time();
    if info.aborted {
        return (0, None);
    }

    info.nodes += 1;

    let hash = board.hash();
    if ply > 0 {
        let hc = board.halfmove_clock() as usize;
        let scan_limit = hc.min(history_hashes.len());
        let start_idx = history_hashes.len() - scan_limit;

        let mut is_repetition = false;
        for i in (start_idx..history_hashes.len()).rev().step_by(2) {
            if history_hashes[i] == hash {
                is_repetition = true;
                break;
            }
        }

        if is_repetition {
            return (0, None);
        }
    }

    let original_alpha = alpha;

    if let Some(entry) = tt.get(hash) {
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
        return (
            quiescence_search(board, alpha, beta, info, params, eval_state),
            None,
        );
    }

    let in_check = !board.checkers().is_empty();

    // Capped check extension: only extend if we haven't exceeded MAX_EXTENSIONS on this path
    let (depth, extensions) = if in_check && extensions < MAX_EXTENSIONS {
        (depth + 1, extensions + 1)
    } else {
        (depth, extensions)
    };

    // Reverse futility pruning
    if !in_check && ply > 0 && depth <= 3 {
        let static_eval = evaluate_board_incremental(board, eval_state, params);
        if static_eval - 120 * depth >= beta {
            return (beta, None);
        }
    }

    // Null move pruning
    if !in_check && ply > 0 && depth >= 3 {
        let our_pieces = (board.pieces(Piece::Knight)
            | board.pieces(Piece::Bishop)
            | board.pieces(Piece::Rook)
            | board.pieces(Piece::Queen))
            & board.colors(board.side_to_move());
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
                    extensions,
                    info,
                    tt,
                    params,
                    history_hashes,
                    None,
                    eval_state,
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

    let mut move_stack = MoveStack::new();
    board.generate_moves(|move_list| {
        for m in move_list {
            move_stack.push(m);
        }
        false
    });

    if move_stack.is_empty() {
        if in_check {
            return (-MATE_SCORE + (ply as i32), None);
        } else {
            return (0, None);
        }
    }

    let tt_move = tt.get(hash).and_then(|e| e.best_move);
    let ply_idx = ply as usize;

    // Zero-allocation staged move picker: scores all moves, then yields
    // best-first via selection sort (O(1) per pick, no heap allocation).
    let mut picker = ScoredMoveList::from_stack(
        &move_stack,
        board,
        tt_move,
        ply_idx,
        &info.killers,
        &info.history,
        &info.cont_history,
        prev_move,
        params,
    );

    let mut best_move: Option<Move> = None;
    let mut best_score = -MATE_SCORE;
    let mut move_index = 0usize;

    while let Some((m, _)) = picker.pick_next() {
        // Incremental eval: copy-make pattern (EvalState is 12 bytes, trivially Copy)
        let mut child_eval = *eval_state;
        child_eval.make_move(board, m, params);

        let mut next_board = board.clone();
        next_board.play_unchecked(m);

        let mut score;
        let is_capture = board.color_on(m.to).is_some();
        let gives_check = next_board.checkers().len() > 0;

        if move_index == 0 {
            // Full window search for PV move
            history_hashes.push(hash);
            let (raw, _) = negamax(
                &next_board,
                depth - 1,
                -beta,
                -alpha,
                ply + 1,
                extensions,
                info,
                tt,
                params,
                history_hashes,
                Some(m),
                &child_eval,
            );
            history_hashes.pop();
            score = -raw;
        } else {
            // Late Move Reductions + PVS
            let mut reduced_depth = depth - 1;
            let is_killer = ply_idx < MAX_PLY
                && (info.killers[ply_idx][0] == Some(m) || info.killers[ply_idx][1] == Some(m));
            let do_lmr = move_index >= 3
                && depth >= 3
                && !is_capture
                && !gives_check
                && !in_check
                && !is_killer;
            if do_lmr {
                let r = lmr_reduction(depth, move_index as i32);
                reduced_depth = (depth - 1 - r).max(1);
            }

            history_hashes.push(hash);
            let (raw, _) = negamax(
                &next_board,
                reduced_depth,
                -alpha - 1,
                -alpha,
                ply + 1,
                extensions,
                info,
                tt,
                params,
                history_hashes,
                Some(m),
                &child_eval,
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
                        extensions,
                        info,
                        tt,
                        params,
                        history_hashes,
                        Some(m),
                        &child_eval,
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
            best_move = Some(m);
        }
        if score > alpha {
            alpha = score;
        }
        if alpha >= beta {
            if board.color_on(m.to).is_none() {
                if ply_idx < MAX_PLY {
                    info.killers[ply_idx][1] = info.killers[ply_idx][0];
                    info.killers[ply_idx][0] = Some(m);
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

        move_index += 1;
    }

    let node_type = if best_score <= original_alpha {
        NodeType::UpperBound
    } else if best_score >= beta {
        NodeType::LowerBound
    } else {
        NodeType::Exact
    };
    tt.insert(
        hash,
        depth,
        score_to_tt(best_score, ply),
        node_type,
        best_move,
    );

    (best_score, best_move)
}

fn quiescence_search(
    board: &Board,
    mut alpha: i32,
    beta: i32,
    info: &mut SearchInfo,
    params: &EvalParams,
    eval_state: &EvalState,
) -> i32 {
    info.check_time();
    if info.aborted {
        return 0;
    }

    info.nodes += 1;

    let in_check = !board.checkers().is_empty();
    let stand_pat = if in_check {
        -MATE_SCORE
    } else {
        evaluate_board_incremental(board, eval_state, params)
    };

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

    let mut move_stack = MoveStack::new();
    board.generate_moves(|move_list| {
        for m in move_list {
            if in_check || board.color_on(m.to).is_some() || m.promotion.is_some() {
                move_stack.push(m);
            }
        }
        false
    });

    if in_check && move_stack.is_empty() {
        return -MATE_SCORE;
    }

    let moves = move_stack.as_mut_slice();
    moves.sort_unstable_by_key(|m| {
        let victim_val = board
            .piece_on(m.to)
            .map(|p| piece_value(p, params))
            .unwrap_or(0);
        let promo_val = m.promotion.map(|p| piece_value(p, params)).unwrap_or(0);
        -(victim_val + promo_val)
    });

    const DELTA_MARGIN: i32 = 200;
    for m in moves.iter() {
        if !in_check {
            let victim_val = board
                .piece_on(m.to)
                .map(|p| piece_value(p, params))
                .unwrap_or(0);
            let promo_val = m.promotion.map(|p| piece_value(p, params)).unwrap_or(0);
            if stand_pat + victim_val + promo_val + DELTA_MARGIN < alpha {
                continue;
            }

            // SEE pruning in qsearch — this is where expensive SEE belongs
            if see(board, *m, params) < 0 {
                continue;
            }
        }

        let mut child_eval = *eval_state;
        child_eval.make_move(board, *m, params);

        let mut next_board = board.clone();
        next_board.play_unchecked(*m);

        let score = -quiescence_search(&next_board, -beta, -alpha, info, params, &child_eval);

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
