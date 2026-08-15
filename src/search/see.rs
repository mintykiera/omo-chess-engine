use cozy_chess::{
    BitBoard, Board, Color, Move, Piece, Square, get_bishop_moves, get_king_moves,
    get_knight_moves, get_rook_moves,
};

use crate::eval::piece_value;

#[allow(dead_code)]
pub(crate) fn get_least_valuable_attacker(
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

pub(crate) fn all_attackers_to(board: &Board, sq: Square, occupied: BitBoard) -> BitBoard {
    let sq_bb = sq.bitboard();

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

pub(crate) fn see(board: &Board, m: Move) -> i32 {
    let mut gains = [0i32; 32];

    let is_ep = board.piece_on(m.from) == Some(Piece::Pawn)
        && m.from.file() != m.to.file()
        && board.color_on(m.to).is_none();

    let victim = if is_ep {
        piece_value(Piece::Pawn)
    } else {
        board.piece_on(m.to).map(piece_value).unwrap_or(0)
    };

    let promo = m
        .promotion
        .map(|p| piece_value(p) - piece_value(Piece::Pawn))
        .unwrap_or(0);
    gains[0] = victim + promo;

    let mut attacker = board.piece_on(m.from).unwrap();
    if let Some(p) = m.promotion {
        attacker = p;
    }

    let mut occupied = board.occupied();
    occupied &= !m.from.bitboard();
    if is_ep {
        let ep_sq = Square::new(m.to.file(), m.from.rank());
        occupied &= !ep_sq.bitboard();
    }

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
        gains[d] = piece_value(attacker);

        attacker = found_piece;
        occupied &= !found_sq.bitboard();
        attackers &= !found_sq.bitboard();

        let diag = get_bishop_moves(m.to, occupied);
        let orth = get_rook_moves(m.to, occupied);
        let new_sliders = ((diag & (board.pieces(Piece::Bishop) | board.pieces(Piece::Queen)))
            | (orth & (board.pieces(Piece::Rook) | board.pieces(Piece::Queen))))
            & occupied;
        attackers |= new_sliders;

        current_color = !current_color;
    }

    let mut i = d as i32;
    while i >= 1 {
        gains[(i as usize) - 1] -= gains[i as usize].max(0);
        i -= 1;
    }
    gains[0]
}
