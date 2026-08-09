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
            let mut protected = false;
            for p in our_pawns {
                let p_rank = p.rank() as i8;
                let p_file = p.file() as i8;
                let sq_rank = sq.rank() as i8;
                let sq_file = sq.file() as i8;
                
                let rank_diff = sq_rank - p_rank;
                let file_diff = (sq_file - p_file).abs();
                
                if file_diff == 1 {
                    if color == Color::White && rank_diff == 1 {
                        protected = true;
                    } else if color == Color::Black && rank_diff == -1 {
                        protected = true;
                    }
                }
            }
            if protected {
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
