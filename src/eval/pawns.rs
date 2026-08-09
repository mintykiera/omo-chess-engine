use cozy_chess::{ Board, Color, Piece };

pub fn evaluate_pawns(board: &Board, color: Color) -> i32 {
    let mut score = 0;
    let our_pawns = board.pieces(Piece::Pawn) & board.colors(color);
    let their_pawns = board.pieces(Piece::Pawn) & board.colors(!color);

    for sq in our_pawns {
        let file = sq.file() as usize as i8;
        let rank = sq.rank() as usize as i8;

        let mut is_doubled = false;
        let mut is_isolated = true;
        let mut is_passed = true;

        for other in our_pawns {
            if other != sq {
                let other_file = other.file() as usize as i8;
                if other_file == file {
                    is_doubled = true;
                }
                if (other_file - file).abs() == 1 {
                    is_isolated = false;
                }
            }
        }

        for enemy in their_pawns {
            let enemy_file = enemy.file() as usize as i8;
            let enemy_rank = enemy.rank() as usize as i8;

            if (enemy_file - file).abs() <= 1 {
                let in_front = match color {
                    Color::White => enemy_rank > rank,
                    Color::Black => enemy_rank < rank,
                };
                if in_front {
                    is_passed = false;
                }
            }
        }

        if is_doubled {
            score -= 10;
        }
        if is_isolated {
            score -= 20;
        }
        if is_passed {
            let advancement = match color {
                Color::White => rank,
                Color::Black => 7 - rank,
            };
            score += 10 * (advancement as i32);
        }
    }

    score
}
