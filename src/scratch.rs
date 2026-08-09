use cozy_chess::{Board, Square, BitBoard, get_knight_moves, get_bishop_attacks, get_rook_attacks};

fn test() {
    let b = Board::default();
    let occupied = b.occupied();
    let sq = Square::A1;
    let n = get_knight_moves(sq);
    let r = get_rook_attacks(sq, occupied);
    let bi = get_bishop_attacks(sq, occupied);
}
