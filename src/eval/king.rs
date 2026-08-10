use cozy_chess::{BitBoard, Board, Color, Piece};

pub fn evaluate_king_safety(board: &Board, color: Color) -> i32 {
    let mut score = 0;
    let king_sq = (board.pieces(Piece::King) & board.colors(color))
        .into_iter()
        .next();

    if let Some(sq) = king_sq {
        let file = sq.file() as usize;
        let rank = sq.rank() as usize;

        let file_mask = BitBoard(0x0101010101010101 << file);

        let our_pawns = board.pieces(Piece::Pawn) & board.colors(color);
        let their_pawns = board.pieces(Piece::Pawn) & board.colors(!color);

        let our_pawns_on_file = !(our_pawns & file_mask).is_empty();
        let their_pawns_on_file = !(their_pawns & file_mask).is_empty();

        if !our_pawns_on_file && !their_pawns_on_file {
            score -= 30;
        } else if !our_pawns_on_file {
            score -= 25;
        }

        if file <= 2 || file >= 5 {
            let adj_files = match file {
                0 => BitBoard(0x0303030303030303),
                7 => BitBoard(0xc0c0c0c0c0c0c0c0),
                _ => BitBoard((file_mask.0 >> 1) | file_mask.0 | (file_mask.0 << 1)),
            };

            let shield_mask = match color {
                Color::White => {
                    if rank >= 6 {
                        BitBoard(0)
                    } else {
                        let rank1 = BitBoard(0xff << ((rank + 1) * 8));
                        let rank2 = BitBoard(0xff << ((rank + 2) * 8));
                        (rank1 | rank2) & adj_files
                    }
                }
                Color::Black => {
                    if rank <= 1 {
                        BitBoard(0)
                    } else {
                        let rank1 = BitBoard(0xff << ((rank - 1) * 8));
                        let rank2 = BitBoard(0xff << ((rank - 2) * 8));
                        (rank1 | rank2) & adj_files
                    }
                }
            };
            score += ((our_pawns & shield_mask).len() as i32) * 10;
        }

        let enemy_pawn_attacks = match color {
            Color::White => {
                if rank == 0 {
                    0
                } else {
                    (their_pawns & BitBoard(0xff << ((rank - 1) * 8))).len() as i32
                }
            }
            Color::Black => {
                if rank == 7 {
                    0
                } else {
                    (their_pawns & BitBoard(0xff << ((rank + 1) * 8))).len() as i32
                }
            }
        };
        score -= enemy_pawn_attacks * 15;
    }

    score
}
