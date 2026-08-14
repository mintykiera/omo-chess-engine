use cozy_chess::{Board, Color, Piece, Square};

use super::params::{EvalParams, piece_value, TEMPO_BONUS};
use super::state::EvalState;
use super::pawns;
use super::pieces;
use super::king;

pub(crate) fn calculate_phase(board: &Board) -> i32 {
    let knights = board.pieces(Piece::Knight).len() as i32;
    let bishops = board.pieces(Piece::Bishop).len() as i32;
    let rooks = board.pieces(Piece::Rook).len() as i32;
    let queens = board.pieces(Piece::Queen).len() as i32;

    let phase = knights * 1 + bishops * 1 + rooks * 2 + queens * 4;
    phase.min(24)
}

pub(crate) fn phase_value(piece: Piece) -> i32 {
    match piece {
        Piece::Knight => 1,
        Piece::Bishop => 1,
        Piece::Rook => 2,
        Piece::Queen => 4,
        _ => 0,
    }
}

pub(crate) fn pst_score(piece: Piece, sq: Square, color: Color, params: &EvalParams) -> (i32, i32) {
    let rank = sq.rank() as usize;
    let file = sq.file() as usize;
    let row = match color {
        Color::White => 7 - rank,
        Color::Black => rank,
    };
    let (mg_pst, eg_pst) = match piece {
        Piece::Pawn => (params.pawn_mg[row][file], params.pawn_eg[row][file]),
        Piece::Knight => (params.knight_mg[row][file], params.knight_eg[row][file]),
        Piece::Bishop => (params.bishop_mg[row][file], params.bishop_eg[row][file]),
        Piece::Rook => (params.rook_mg[row][file], params.rook_eg[row][file]),
        Piece::Queen => (params.queen_mg[row][file], params.queen_eg[row][file]),
        Piece::King => (params.king_mg[row][file], params.king_eg[row][file]),
    };
    let val = piece_value(piece, params);
    (val + mg_pst, val + eg_pst)
}

pub fn evaluate_board(board: &Board, params: &EvalParams) -> i32 {
    let phase = calculate_phase(board);
    let mut mg_score = 0;
    let mut eg_score = 0;

    for &piece in &Piece::ALL {
        for &color in &Color::ALL {
            let bitboard = board.pieces(piece) & board.colors(color);

            for square in bitboard {
                let val = piece_value(piece, params);

                let rank = square.rank() as usize;
                let file = square.file() as usize;

                let row = match color {
                    Color::White => 7 - rank,
                    Color::Black => rank,
                };

                let (mg_pst, eg_pst) = match piece {
                    Piece::Pawn => (params.pawn_mg[row][file], params.pawn_eg[row][file]),
                    Piece::Knight => (params.knight_mg[row][file], params.knight_eg[row][file]),
                    Piece::Bishop => (params.bishop_mg[row][file], params.bishop_eg[row][file]),
                    Piece::Rook => (params.rook_mg[row][file], params.rook_eg[row][file]),
                    Piece::Queen => (params.queen_mg[row][file], params.queen_eg[row][file]),
                    Piece::King => (params.king_mg[row][file], params.king_eg[row][file]),
                };

                if color == Color::White {
                    mg_score += val + mg_pst;
                    eg_score += val + eg_pst;
                } else {
                    mg_score -= val + mg_pst;
                    eg_score -= val + eg_pst;
                }
            }
        }
    }

    let w_pawns = pawns::evaluate_pawns(board, Color::White);
    let w_pieces = pieces::evaluate_pieces(board, Color::White);
    let w_king = king::evaluate_king_safety(board, Color::White);

    let b_pawns = pawns::evaluate_pawns(board, Color::Black);
    let b_pieces = pieces::evaluate_pieces(board, Color::Black);
    let b_king = king::evaluate_king_safety(board, Color::Black);

    mg_score += w_pawns + w_pieces + w_king;
    eg_score += w_pawns + w_pieces;

    mg_score -= b_pawns + b_pieces + b_king;
    eg_score -= b_pawns + b_pieces;

    let mut final_score = (mg_score * phase + eg_score * (24 - phase)) / 24;

    if board.side_to_move() == Color::Black {
        final_score = -final_score;
    }

    final_score += TEMPO_BONUS;

    final_score
}

pub fn evaluate_board_incremental(board: &Board, state: &EvalState, _params: &EvalParams) -> i32 {
    let phase = state.phase;
    let mut mg_score = state.mg;
    let mut eg_score = state.eg;

    let w_pawns = pawns::evaluate_pawns(board, Color::White);
    let w_pieces = pieces::evaluate_pieces(board, Color::White);
    let w_king = king::evaluate_king_safety(board, Color::White);

    let b_pawns = pawns::evaluate_pawns(board, Color::Black);
    let b_pieces = pieces::evaluate_pieces(board, Color::Black);
    let b_king = king::evaluate_king_safety(board, Color::Black);

    mg_score += w_pawns + w_pieces + w_king;
    eg_score += w_pawns + w_pieces;

    mg_score -= b_pawns + b_pieces + b_king;
    eg_score -= b_pawns + b_pieces;

    let mut final_score = (mg_score * phase + eg_score * (24 - phase)) / 24;

    if board.side_to_move() == Color::Black {
        final_score = -final_score;
    }

    final_score += TEMPO_BONUS;

    final_score
}
