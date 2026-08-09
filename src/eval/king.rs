use cozy_chess::{ Board, Color, Piece };

pub fn evaluate_king_safety(board: &Board, color: Color) -> i32 {
    let mut score = 0;
    let king_sq = (board.pieces(Piece::King) & board.colors(color)).into_iter().next();

    if let Some(sq) = king_sq {
        let file = sq.file() as usize as i8;
        let rank = sq.rank() as usize as i8;

        let our_pawns = board.pieces(Piece::Pawn) & board.colors(color);
        let their_pawns = board.pieces(Piece::Pawn) & board.colors(!color);

        let mut our_pawns_on_file = false;
        let mut their_pawns_on_file = false;

        for p in our_pawns {
            if (p.file() as usize as i8) == file {
                our_pawns_on_file = true;
                break;
            }
        }
        for p in their_pawns {
            if (p.file() as usize as i8) == file {
                their_pawns_on_file = true;
                break;
            }
        }

        if !our_pawns_on_file && !their_pawns_on_file {
            score -= 30;
        } else if !our_pawns_on_file {
            score -= 25;
        }

        if file <= 2 || file >= 5 {
            let mut shield_pawns = 0;
            for p in our_pawns {
                let p_file = p.file() as usize as i8;
                let p_rank = p.rank() as usize as i8;

                if (p_file - file).abs() <= 1 {
                    let in_front = match color {
                        Color::White => p_rank == rank + 1 || p_rank == rank + 2,
                        Color::Black => p_rank == rank - 1 || p_rank == rank - 2,
                    };
                    if in_front {
                        shield_pawns += 1;
                    }
                }
            }
            score += shield_pawns * 10;
        }

        let mut enemy_pawn_attacks = 0;
        for p in their_pawns {
            let p_rank = p.rank() as usize as i8;
            let attacking_rank = match !color {
                Color::White => p_rank + 1,
                Color::Black => p_rank - 1,
            };
            if attacking_rank == rank {
                enemy_pawn_attacks += 1;
            }
        }
        score -= enemy_pawn_attacks * 15;
    }

    score
}
