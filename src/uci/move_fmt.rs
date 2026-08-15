use cozy_chess::{Board, Move, Piece};

pub fn parse_uci_move(board: &Board, m_str: &str) -> Option<Move> {
    let mut m = m_str.parse::<Move>().ok()?;
    if board.piece_on(m.from) == Some(Piece::King) {
        let from_str = m.from.to_string();
        let to_str = m.to.to_string();
        if from_str == "e1" && to_str == "g1" {
            m.to = "h1".parse().unwrap();
        } else if from_str == "e1" && to_str == "c1" {
            m.to = "a1".parse().unwrap();
        } else if from_str == "e8" && to_str == "g8" {
            m.to = "h8".parse().unwrap();
        } else if from_str == "e8" && to_str == "c8" {
            m.to = "a8".parse().unwrap();
        }
    }
    Some(m)
}

pub fn format_uci_move(board: &Board, m: Move) -> String {
    let mut out_move = m;
    let is_castling = board.piece_on(m.from) == Some(Piece::King)
        && !(board.colors(board.side_to_move()) & m.to.bitboard()).is_empty();
    if is_castling {
        let from_str = m.from.to_string();
        let to_str = m.to.to_string();
        if from_str == "e1" && to_str == "h1" {
            out_move.to = "g1".parse().unwrap();
        } else if from_str == "e1" && to_str == "a1" {
            out_move.to = "c1".parse().unwrap();
        } else if from_str == "e8" && to_str == "h8" {
            out_move.to = "g8".parse().unwrap();
        } else if from_str == "e8" && to_str == "a8" {
            out_move.to = "c8".parse().unwrap();
        }
    }
    out_move.to_string()
}
