use cozy_chess::Piece;

pub(crate) const TEMPO_BONUS: i32 = 15;

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
        super::pst::DEFAULT_PARAMS
    }
}

pub fn piece_value(piece: Piece, params: &EvalParams) -> i32 {
    params.piece_values[piece as usize]
}
