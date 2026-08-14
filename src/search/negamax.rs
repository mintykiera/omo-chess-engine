use cozy_chess::{Board, Move, Piece};
use shakmaty::{CastlingMode, Chess, fen::Fen};
use shakmaty_syzygy::{Tablebase, Wdl};

use crate::eval::{EvalParams, EvalState, evaluate_board_incremental, piece_value};
use crate::transposition::{NodeType, TranspositionTable};
use super::ordering::{sq_idx, lmr_reduction};
use super::see::see;
use super::types::{
    MoveStack, ScoredMoveList, SearchInfo,
    MATE_SCORE, MAX_EXTENSIONS, MAX_PLY,
    score_from_tt, score_to_tt,
};

pub(crate) fn negamax(
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
    syzygy: &Option<Tablebase<Chess>>,
    excluded_move: Option<Move>,
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

    // Syzygy Endgame Tablebase probe
    if board.occupied().len() <= 6 && ply > 0 {
        if let Some(tb) = syzygy {
            let fen_str = board.to_string();
            if let Ok(fen) = fen_str.parse::<Fen>() {
                if let Ok(pos) = fen.into_position::<Chess>(CastlingMode::Standard) {
                    if let Ok(wdl) = tb.probe_wdl_after_zeroing(&pos) {
                        let egtb_score = match wdl {
                            Wdl::Win => 20_000 - ply,
                            Wdl::Loss => -20_000 + ply,
                            _ => 0,
                        };
                        return (egtb_score, None);
                    }
                }
            }
        }
    }

    let original_alpha = alpha;
    let tt_entry = tt.get(hash);

    if excluded_move.is_none() {
        if let Some(entry) = tt_entry {
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
    }

    if depth <= 0 {
        return (
            quiescence_search(board, alpha, beta, info, params, eval_state),
            None,
        );
    }

    let in_check = !board.checkers().is_empty();

    let (depth, extensions) = if in_check && extensions < MAX_EXTENSIONS {
        (depth + 1, extensions + 1)
    } else {
        (depth, extensions)
    };

    if !in_check && ply > 0 && depth <= 3 && excluded_move.is_none() {
        let static_eval = evaluate_board_incremental(board, eval_state, params);
        if static_eval - 120 * depth >= beta {
            return (beta, None);
        }
    }

    if !in_check && ply > 0 && depth >= 3 && excluded_move.is_none() {
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
                    syzygy,
                    None,
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

    // Singular Extensions verification search
    let mut is_singular = false;
    let mut singular_move = None;

    if excluded_move.is_none() && depth >= 8 {
        if let Some(entry) = tt_entry {
            if (entry.node_type == NodeType::Exact || entry.node_type == NodeType::LowerBound)
                && entry.best_move.is_some()
            {
                let tt_m = entry.best_move.unwrap();
                let tt_score = score_from_tt(entry.score, ply);
                let margin = depth * 2;
                let singular_beta = tt_score - margin;
                let singular_alpha = singular_beta - 1;
                let singular_depth = (depth - 1) / 2;

                let (sing_score, _) = negamax(
                    board,
                    singular_depth,
                    singular_alpha,
                    singular_beta,
                    ply,
                    extensions,
                    info,
                    tt,
                    params,
                    history_hashes,
                    prev_move,
                    eval_state,
                    syzygy,
                    Some(tt_m),
                );

                if info.aborted {
                    return (0, None);
                }

                if sing_score <= singular_alpha {
                    is_singular = true;
                    singular_move = Some(tt_m);
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

    // Futility Pruning flag before move loop
    let mut futility_pruning = false;
    if !in_check && depth <= 4 && ply > 0 {
        let static_eval = evaluate_board_incremental(board, eval_state, params);
        if static_eval + (depth * 100) <= alpha {
            futility_pruning = true;
        }
    }

    let tt_move = tt.get(hash).and_then(|e| e.best_move);
    let ply_idx = ply as usize;

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
    let mut quiet_moves_searched = 0i32;

    while let Some((m, _)) = picker.pick_next() {
        if excluded_move == Some(m) {
            continue;
        }

        let is_capture = board.color_on(m.to).is_some();
        let is_quiet = !is_capture && m.promotion.is_none();

        // Late Move Pruning (LMP)
        if !in_check && depth <= 4 && is_quiet && quiet_moves_searched > 3 + (2 * depth * depth) {
            continue;
        }

        let mut child_eval = *eval_state;
        child_eval.make_move(board, m, params);

        let mut next_board = board.clone();
        next_board.play_unchecked(m);

        let gives_check = !next_board.checkers().is_empty();

        // Futility Pruning
        if futility_pruning && is_quiet && !gives_check {
            continue;
        }

        if is_quiet {
            quiet_moves_searched += 1;
        }

        let singular_ext = if is_singular && Some(m) == singular_move && extensions < MAX_EXTENSIONS {
            1
        } else {
            0
        };

        let mut score;

        if move_index == 0 {
            history_hashes.push(hash);
            let (raw, _) = negamax(
                &next_board,
                depth - 1 + singular_ext,
                -beta,
                -alpha,
                ply + 1,
                extensions + singular_ext,
                info,
                tt,
                params,
                history_hashes,
                Some(m),
                &child_eval,
                syzygy,
                None,
            );
            history_hashes.pop();
            score = -raw;
        } else {
            let mut reduced_depth = depth - 1 + singular_ext;
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
                extensions + singular_ext,
                info,
                tt,
                params,
                history_hashes,
                Some(m),
                &child_eval,
                syzygy,
                None,
            );
            score = -raw;

            if score > alpha {
                if do_lmr || score < beta {
                    let (raw, _) = negamax(
                        &next_board,
                        depth - 1 + singular_ext,
                        -beta,
                        -alpha,
                        ply + 1,
                        extensions + singular_ext,
                        info,
                        tt,
                        params,
                        history_hashes,
                        Some(m),
                        &child_eval,
                        syzygy,
                        None,
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

    if excluded_move.is_none() {
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
    }

    (best_score, best_move)
}

pub(crate) fn quiescence_search(
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
