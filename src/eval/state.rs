use cozy_chess::{Board, Color, File, Move, Piece, Square};

use super::params::EvalParams;
use super::evaluate::{calculate_phase, pst_score, phase_value};

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
