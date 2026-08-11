pub mod king;
pub mod pawns;
pub mod pieces;
pub mod pst;

use cozy_chess::{Board, Color, File, Move, Piece, Square};

const TEMPO_BONUS: i32 = 15;

#[derive(Clone, Copy, Debug)]
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
        pst::TUNED_PARAMS
    }
}

pub use pst::TUNED_PARAMS as DEFAULT_PARAMS;

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

fn phase_value(piece: Piece) -> i32 {
    match piece {
        Piece::Knight => 1,
        Piece::Bishop => 1,
        Piece::Rook => 2,
        Piece::Queen => 4,
        _ => 0,
    }
}

fn pst_score(piece: Piece, sq: Square, color: Color, params: &EvalParams) -> (i32, i32) {
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

#[derive(Clone, Copy)]
pub struct EvalState {
    pub mg: i32,
    pub eg: i32,
    pub phase: i32,
}

impl EvalState {
    pub fn from_board(board: &Board, params: &EvalParams) -> Self {
        let mut mg = 0;
        let mut eg = 0;
        let phase = calculate_phase(board);

        for &piece in &Piece::ALL {
            for &color in &Color::ALL {
                let bitboard = board.pieces(piece) & board.colors(color);
                for square in bitboard {
                    let (mg_val, eg_val) = pst_score(piece, square, color, params);
                    if color == Color::White {
                        mg += mg_val;
                        eg += eg_val;
                    } else {
                        mg -= mg_val;
                        eg -= eg_val;
                    }
                }
            }
        }

        Self { mg, eg, phase }
    }

    #[inline]
    fn add_piece(&mut self, piece: Piece, sq: Square, color: Color, params: &EvalParams) {
        let (mg_val, eg_val) = pst_score(piece, sq, color, params);
        if color == Color::White {
            self.mg += mg_val;
            self.eg += eg_val;
        } else {
            self.mg -= mg_val;
            self.eg -= eg_val;
        }
    }

    #[inline]
    fn remove_piece(&mut self, piece: Piece, sq: Square, color: Color, params: &EvalParams) {
        let (mg_val, eg_val) = pst_score(piece, sq, color, params);
        if color == Color::White {
            self.mg -= mg_val;
            self.eg -= eg_val;
        } else {
            self.mg += mg_val;
            self.eg += eg_val;
        }
    }

    pub fn make_move(&mut self, board: &Board, m: Move, params: &EvalParams) {
        let us = board.side_to_move();
        let moving_piece = board.piece_on(m.from).unwrap();

        if moving_piece == Piece::King {
            if let Some(target_piece) = board.piece_on(m.to) {
                if board.color_on(m.to) == Some(us) && target_piece == Piece::Rook {
                    self.remove_piece(Piece::King, m.from, us, params);
                    self.remove_piece(Piece::Rook, m.to, us, params);

                    let rank = m.from.rank();
                    let (king_dest, rook_dest) =
                        if (m.to.file() as usize) > (m.from.file() as usize) {
                            (Square::new(File::G, rank), Square::new(File::F, rank))
                        } else {
                            (Square::new(File::C, rank), Square::new(File::D, rank))
                        };

                    self.add_piece(Piece::King, king_dest, us, params);
                    self.add_piece(Piece::Rook, rook_dest, us, params);
                    return;
                }
            }
        }

        let is_en_passant = moving_piece == Piece::Pawn
            && m.from.file() != m.to.file()
            && board.piece_on(m.to).is_none();

        self.remove_piece(moving_piece, m.from, us, params);

        if let Some(captured_piece) = board.piece_on(m.to) {
            let them = !us;
            self.remove_piece(captured_piece, m.to, them, params);
            self.phase = (self.phase - phase_value(captured_piece)).max(0);
        } else if is_en_passant {
            let them = !us;
            let captured_sq = Square::new(m.to.file(), m.from.rank());
            self.remove_piece(Piece::Pawn, captured_sq, them, params);
        }

        if let Some(promo_piece) = m.promotion {
            self.add_piece(promo_piece, m.to, us, params);
            self.phase = (self.phase + phase_value(promo_piece)).min(24);
        } else {
            self.add_piece(moving_piece, m.to, us, params);
        }
    }
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
