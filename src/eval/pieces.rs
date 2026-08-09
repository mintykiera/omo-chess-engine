use cozy_chess::{ Board, Color, Piece, get_knight_moves, get_bishop_moves, get_rook_moves };

pub fn evaluate_pieces(board: &Board, color: Color) -> i32 {
    let mut score = 0;

    let friendly = board.colors(color);
    let occupied = board.occupied();
    let our_pawns = board.pieces(Piece::Pawn) & friendly;

    let knights = board.pieces(Piece::Knight) & friendly;
    for sq in knights {
        let moves = get_knight_moves(sq) & !friendly;
        score += (moves.len() as i32) * 4;

        let rank = sq.rank() as usize;
        let is_outpost = match color {
            Color::White => rank == 3 || rank == 4 || rank == 5,
            Color::Black => rank == 4 || rank == 3 || rank == 2,
        };

        if is_outpost {
            let sq_bb = sq.bitboard();
            let pawn_protectors = match color {
                Color::White => {
                    let a = (sq_bb.0 >> 9) & 0x7f7f7f7f7f7f7f7f; // not FILE_H (since shift right 9 wraps if it was A) - wait, shift right 9 from B to H, wait...
                    let b = (sq_bb.0 >> 7) & 0xfefefefefefefefe; // not FILE_A
                    cozy_chess::BitBoard(a | b)
                }
                Color::Black => {
                    let a = (sq_bb.0 << 9) & 0xfefefefefefefefe; // not FILE_A
                    let b = (sq_bb.0 << 7) & 0x7f7f7f7f7f7f7f7f; // not FILE_H
                    cozy_chess::BitBoard(a | b)
                }
            };

            if !(pawn_protectors & our_pawns).is_empty() {
                score += 20;
            }
        }
    }

    let bishops = board.pieces(Piece::Bishop) & friendly;
    if bishops.len() >= 2 {
        score += 30;
    }
    for sq in bishops {
        let moves = get_bishop_moves(sq, occupied) & !friendly;
        score += (moves.len() as i32) * 5;
    }

    let rooks = board.pieces(Piece::Rook) & friendly;
    for sq in rooks {
        let moves = get_rook_moves(sq, occupied) & !friendly;
        score += (moves.len() as i32) * 3;
    }

    let queens = board.pieces(Piece::Queen) & friendly;
    for sq in queens {
        let moves = (get_bishop_moves(sq, occupied) | get_rook_moves(sq, occupied)) & !friendly;
        score += (moves.len() as i32) * 2;
    }

    let their_pawns = board.pieces(Piece::Pawn) & board.colors(!color);

    for sq in rooks {
        let file = sq.file() as usize as i8;

        let mut our_pawns_on_file = false;
        for p in our_pawns {
            if (p.file() as usize as i8) == file {
                our_pawns_on_file = true;
                break;
            }
        }

        let mut their_pawns_on_file = false;
        for p in their_pawns {
            if (p.file() as usize as i8) == file {
                their_pawns_on_file = true;
                break;
            }
        }

        if !our_pawns_on_file && !their_pawns_on_file {
            score += 20;
        } else if !our_pawns_on_file {
            score += 10;
        }
    }

    score
}
