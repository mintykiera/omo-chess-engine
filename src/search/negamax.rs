use cozy_chess::{Board, Move, Piece};
use nnue_rs::{Accumulator, Network};
use shakmaty::{CastlingMode, Chess};
use shakmaty_syzygy::{Tablebase, Wdl};

use super::ordering::lmr_reduction;
use super::see::see;
use super::types::{
    MATE_SCORE, MAX_EXTENSIONS, MAX_PLY, MoveStack, SearchInfo, SharedHistory, StagedMovePicker,
    score_from_tt, score_to_tt,
};
use crate::eval::piece_value;
use crate::transposition::{NodeType, TranspositionTable};

pub(crate) fn negamax(
    board: &Board,
    depth: i32,
    mut alpha: i32,
    mut beta: i32,
    ply: i32,
    extensions: i32,
    info: &mut SearchInfo,
    tt: &TranspositionTable,
    shared: &SharedHistory,
    history_hashes: &mut Vec<u64>,
    prev_move: Option<Move>,
    network: &Network,
    acc: &Accumulator,
    syzygy: &Option<Tablebase<Chess>>,
    excluded_move: Option<Move>,
) -> (i32, Option<Move>) {
    info.check_time();
    if info.aborted {
        return (0, None);
    }

    info.nodes += 1;

    if ply >= (MAX_PLY as i32) - 1 {
        return (
            network.evaluate_accumulator(acc, crate::eval::OmoBoard(board).side_to_move()),
            None,
        );
    }

    let hash = board.hash();
    if ply > 0 {
        if board.halfmove_clock() >= 100 {
            return (0, None);
        }

        let hc = board.halfmove_clock() as usize;
        let scan_limit = hc.min(history_hashes.len());
        let mut is_repetition = false;
        let mut i = 2;
        while i <= scan_limit {
            let idx = history_hashes.len() - i;
            if history_hashes[idx] == hash {
                is_repetition = true;
                break;
            }
            i += 2;
        }
        if is_repetition {
            return (0, None);
        }
    }

    if board.occupied().len() <= 6 && ply > 0 {
        if let Some(tb) = syzygy {
            let mut s_board = shakmaty::Board::empty();
            for &color in &[cozy_chess::Color::White, cozy_chess::Color::Black] {
                let s_color = if color == cozy_chess::Color::White {
                    shakmaty::Color::White
                } else {
                    shakmaty::Color::Black
                };
                for &piece in &[
                    cozy_chess::Piece::Pawn,
                    cozy_chess::Piece::Knight,
                    cozy_chess::Piece::Bishop,
                    cozy_chess::Piece::Rook,
                    cozy_chess::Piece::Queen,
                    cozy_chess::Piece::King,
                ] {
                    let bb = board.colored_pieces(color, piece);
                    let s_role = match piece {
                        cozy_chess::Piece::Pawn => shakmaty::Role::Pawn,
                        cozy_chess::Piece::Knight => shakmaty::Role::Knight,
                        cozy_chess::Piece::Bishop => shakmaty::Role::Bishop,
                        cozy_chess::Piece::Rook => shakmaty::Role::Rook,
                        cozy_chess::Piece::Queen => shakmaty::Role::Queen,
                        cozy_chess::Piece::King => shakmaty::Role::King,
                    };
                    for sq in bb {
                        s_board.set_piece_at(
                            shakmaty::Square::new(sq as u32),
                            shakmaty::Piece {
                                color: s_color,
                                role: s_role,
                            },
                        );
                    }
                }
            }
            let s_turn = if board.side_to_move() == cozy_chess::Color::White {
                shakmaty::Color::White
            } else {
                shakmaty::Color::Black
            };
            let mut castling = shakmaty::Bitboard::default();
            let w_cr = board.castle_rights(cozy_chess::Color::White);
            if w_cr.short.is_some() {
                castling.add(shakmaty::Square::H1);
            }
            if w_cr.long.is_some() {
                castling.add(shakmaty::Square::A1);
            }
            let b_cr = board.castle_rights(cozy_chess::Color::Black);
            if b_cr.short.is_some() {
                castling.add(shakmaty::Square::H8);
            }
            if b_cr.long.is_some() {
                castling.add(shakmaty::Square::A8);
            }

            let ep_square = board
                .en_passant()
                .map(|sq| shakmaty::Square::new(sq as u32));
            use shakmaty::FromSetup;
            let setup = shakmaty::Setup {
                board: s_board,
                promoted: shakmaty::Bitboard::default(),
                turn: s_turn,
                castling_rights: castling,
                ep_square,
                halfmoves: board.halfmove_clock() as u32,
                fullmoves: std::num::NonZeroU32::new((board.fullmove_number() as u32).max(1))
                    .unwrap(),
                pockets: None,
                remaining_checks: None,
            };
            if let Ok(pos) = shakmaty::Chess::from_setup(setup, CastlingMode::Standard) {
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
            quiescence_search(board, alpha, beta, info, network, acc),
            None,
        );
    }

    let in_check = !board.checkers().is_empty();

    let (depth, extensions) = if in_check && extensions < MAX_EXTENSIONS {
        (depth + 1, extensions + 1)
    } else {
        (depth, extensions)
    };

    let static_eval = if !in_check {
        network.evaluate_accumulator(acc, crate::eval::OmoBoard(board).side_to_move())
    } else {
        -MATE_SCORE
    };

    if !in_check && ply > 0 && depth <= 3 && excluded_move.is_none() {
        if static_eval - 120 * depth >= beta {
            return (beta, None);
        }
    }

    if !in_check
        && ply > 0
        && depth >= 3
        && excluded_move.is_none()
        && prev_move.is_some()
        && static_eval >= beta
    {
        let our_pieces = (board.pieces(Piece::Knight)
            | board.pieces(Piece::Bishop)
            | board.pieces(Piece::Rook)
            | board.pieces(Piece::Queen))
            & board.colors(board.side_to_move());
        if our_pieces.len() > 0 {
            if let Some(null_board) = board.null_move() {
                let r = 3 + depth / 6;
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
                    shared,
                    history_hashes,
                    None,
                    network,
                    acc,
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
                    shared,
                    history_hashes,
                    prev_move,
                    network,
                    acc,
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

    let mut tt_move = tt_entry.and_then(|e| e.best_move);

    let is_pv = beta - alpha > 1;
    if is_pv && depth >= 6 && !in_check && tt_move.is_none() && excluded_move.is_none() {
        let iid_depth = depth - 3;
        let _ = negamax(
            board,
            iid_depth,
            alpha,
            beta,
            ply,
            extensions,
            info,
            tt,
            shared,
            history_hashes,
            prev_move,
            network,
            acc,
            syzygy,
            None,
        );
        if info.aborted {
            return (0, None);
        }
        // Update the TT move with the result of the shallow search
        tt_move = tt.get(hash).and_then(|e| e.best_move);
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

    let mut futility_pruning = false;
    if !in_check && depth <= 4 && ply > 0 {
        if static_eval + (depth * 100) <= alpha {
            futility_pruning = true;
        }
    }

    let ply_idx = ply as usize;

    let killers = if ply_idx < MAX_PLY {
        [shared.get_killer(ply_idx, 0), shared.get_killer(ply_idx, 1)]
    } else {
        [None, None]
    };

    let counter_move = prev_move.and_then(|pm| shared.get_counter_move(board.side_to_move(), pm));

    let mut picker = StagedMovePicker::new(&move_stack, tt_move, killers, counter_move);
    let mut best_move: Option<Move> = None;
    let mut best_score = -MATE_SCORE;
    let mut move_index = 0usize;
    let mut quiet_moves_searched = 0i32;
    let mut searched_quiets = MoveStack::new();

    while let Some((m, _)) = picker.pick_next(board, shared, prev_move) {
        if excluded_move == Some(m) {
            continue;
        }

        let is_ep = board.piece_on(m.from) == Some(Piece::Pawn)
            && m.from.file() != m.to.file()
            && board.color_on(m.to).is_none();
        let is_capture = board.color_on(m.to).is_some() || is_ep;
        let is_quiet = !is_capture && m.promotion.is_none();

        if !in_check && depth <= 4 && is_quiet && quiet_moves_searched > 3 + (2 * depth * depth) {
            continue;
        }

        let mut next_board = board.clone();
        next_board.play_unchecked(m);

        let gives_check = !next_board.checkers().is_empty();

        if futility_pruning && is_quiet && !gives_check {
            continue;
        }

        if is_quiet {
            quiet_moves_searched += 1;
            searched_quiets.push(m);
        }

        let mut child_acc = network.empty_accumulator();
        network.update(
            &crate::eval::OmoBoard(board),
            &crate::eval::OmoBoard(&next_board),
            acc,
            &mut child_acc,
        );

        let singular_ext = if is_singular && Some(m) == singular_move && extensions < MAX_EXTENSIONS
        {
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
                shared,
                history_hashes,
                Some(m),
                network,
                &child_acc,
                syzygy,
                None,
            );
            history_hashes.pop();
            score = -raw;
        } else {
            let mut reduced_depth = depth - 1 + singular_ext;
            let is_killer = ply_idx < MAX_PLY
                && (shared.get_killer(ply_idx, 0) == Some(m)
                    || shared.get_killer(ply_idx, 1) == Some(m));
            let do_lmr = move_index >= 3
                && depth >= 3
                && !is_capture
                && !gives_check
                && !in_check
                && !is_killer;
            if do_lmr {
                let r = lmr_reduction(depth, move_index as i32);
                let history_score = shared.get_history(board.side_to_move(), m.from, m.to);
                let history_adjustment = history_score / 4096;
                let final_r = (r - history_adjustment).max(0);
                reduced_depth = (depth - 1 + singular_ext - final_r).max(1);
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
                shared,
                history_hashes,
                Some(m),
                network,
                &child_acc,
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
                        shared,
                        history_hashes,
                        Some(m),
                        network,
                        &child_acc,
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
            let color = board.side_to_move();
            let bonus = (depth * depth * 16).min(300);

            if is_quiet {
                if ply_idx < MAX_PLY {
                    shared.update_killer(ply_idx, m);
                }

                if let Some(pm) = prev_move {
                    shared.update_counter_move(color, pm, m);
                }

                shared.add_history(color, m.from, m.to, bonus);

                for &qm in searched_quiets.as_slice() {
                    if qm != m {
                        shared.add_history(color, qm.from, qm.to, -bonus);
                    }
                }

                if let Some(pm) = prev_move {
                    shared.add_cont_history(pm.to, m.to, bonus);
                    for &qm in searched_quiets.as_slice() {
                        if qm != m {
                            shared.add_cont_history(pm.to, qm.to, -bonus);
                        }
                    }
                }
            } else {
                shared.add_capture_history(color, m.from, m.to, bonus);
            }
            break;
        }

        move_index += 1;
    }

    if best_score == -MATE_SCORE && !in_check {
        best_score = alpha;
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
    network: &Network,
    acc: &Accumulator,
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
        network.evaluate_accumulator(acc, crate::eval::OmoBoard(board).side_to_move())
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
            let is_ep = board.piece_on(m.from) == Some(Piece::Pawn)
                && m.from.file() != m.to.file()
                && board.color_on(m.to).is_none();
            if in_check || board.color_on(m.to).is_some() || is_ep || m.promotion.is_some() {
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
        let is_ep = board.piece_on(m.from) == Some(Piece::Pawn)
            && m.from.file() != m.to.file()
            && board.color_on(m.to).is_none();
        let victim_val = if is_ep {
            piece_value(Piece::Pawn)
        } else {
            board.piece_on(m.to).map(piece_value).unwrap_or(0)
        };
        let promo_val = m.promotion.map(piece_value).unwrap_or(0);
        -(victim_val + promo_val)
    });

    const DELTA_MARGIN: i32 = 200;
    for m in moves.iter() {
        if !in_check {
            let is_ep = board.piece_on(m.from) == Some(Piece::Pawn)
                && m.from.file() != m.to.file()
                && board.color_on(m.to).is_none();
            let victim_val = if is_ep {
                piece_value(Piece::Pawn)
            } else {
                board.piece_on(m.to).map(piece_value).unwrap_or(0)
            };
            let promo_val = m.promotion.map(piece_value).unwrap_or(0);
            if stand_pat + victim_val + promo_val + DELTA_MARGIN < alpha {
                continue;
            }

            if see(board, *m) < 0 {
                continue;
            }
        }

        let mut next_board = board.clone();
        next_board.play_unchecked(*m);

        let mut child_acc = network.empty_accumulator();
        network.update(
            &crate::eval::OmoBoard(board),
            &crate::eval::OmoBoard(&next_board),
            acc,
            &mut child_acc,
        );

        let score = -quiescence_search(&next_board, -beta, -alpha, info, network, &child_acc);

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
