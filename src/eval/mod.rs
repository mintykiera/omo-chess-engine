pub mod pst;
pub mod pawns;
pub mod king;
pub mod pieces;

use cozy_chess::{ Board, Color, Piece };
use pst::*;

const TEMPO_BONUS: i32 = 15;

#[derive(Clone, Copy)]
pub struct EvalParams {
    pub pawn_mg: [[i32; 8]; 8],
    pub pawn_eg: [[i32; 8]; 8],
    pub knight_mg: [[i32; 8]; 8],
    pub knight_eg: [[i32; 8]; 8],
    pub bishop_mg: [[i32; 8]; 8],
    pub bishop_eg: [[i32; 8]; 8],
    pub rook_mg: [[i32; 8]; 8],
    pub rook_eg: [[i32; 8]; 8],
    pub queen_mg: [[i32; 8]; 8],
    pub queen_eg: [[i32; 8]; 8],
    pub king_mg: [[i32; 8]; 8],
    pub king_eg: [[i32; 8]; 8],
    pub piece_values: [i32; 6],
}

impl Default for EvalParams {
    fn default() -> Self {
        Self {
            pawn_mg: PAWN_MG_PST,
            pawn_eg: PAWN_EG_PST,
            knight_mg: KNIGHT_MG_PST,
            knight_eg: KNIGHT_EG_PST,
            bishop_mg: BISHOP_MG_PST,
            bishop_eg: BISHOP_EG_PST,
            rook_mg: ROOK_MG_PST,
            rook_eg: ROOK_EG_PST,
            queen_mg: QUEEN_MG_PST,
            queen_eg: QUEEN_EG_PST,
            king_mg: KING_MG_PST,
            king_eg: KING_EG_PST,
            piece_values: [100, 300, 320, 500, 900, 0],
        }
    }
}

pub static DEFAULT_PARAMS: EvalParams = EvalParams {
    pawn_mg: PAWN_MG_PST,
    pawn_eg: PAWN_EG_PST,
    knight_mg: KNIGHT_MG_PST,
    knight_eg: KNIGHT_EG_PST,
    bishop_mg: BISHOP_MG_PST,
    bishop_eg: BISHOP_EG_PST,
    rook_mg: ROOK_MG_PST,
    rook_eg: ROOK_EG_PST,
    queen_mg: QUEEN_MG_PST,
    queen_eg: QUEEN_EG_PST,
    king_mg: KING_MG_PST,
    king_eg: KING_EG_PST,
    piece_values: [100, 300, 320, 500, 900, 0],
};

pub fn piece_value(piece: Piece, params: &EvalParams) -> i32 {
    params.piece_values[piece as usize]
}

fn calculate_phase(board: &Board) -> i32 {
    let knights = board.pieces(Piece::Knight).len() as i32;
    let bishops = board.pieces(Piece::Bishop).len() as i32;
    let rooks = board.pieces(Piece::Rook).len() as i32;
    let queens = board.pieces(Piece::Queen).len() as i32;

    let phase = knights * 1 + bishops * 1 + rooks * 2 + queens * 4;
    phase.min(24)
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