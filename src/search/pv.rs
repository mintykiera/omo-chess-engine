use cozy_chess::Board;

use super::types::MAX_PLY;
use crate::transposition::TranspositionTable;
use crate::uci::format_uci_move;

pub fn extract_pv(board: &Board, tt: &TranspositionTable) -> String {
    let mut current_board = board.clone();
    let mut pv_moves = Vec::new();
    let mut visited_hashes = Vec::new();

    for _ in 0..MAX_PLY {
        let hash = current_board.hash();
        if visited_hashes.contains(&hash) {
            break;
        }
        visited_hashes.push(hash);

        if let Some(entry) = tt.get(hash) {
            if let Some(best_move) = entry.best_move {
                let mut is_legal = false;
                current_board.generate_moves(|move_list| {
                    for m in move_list {
                        if m == best_move {
                            is_legal = true;
                        }
                    }
                    false
                });

                if is_legal {
                    pv_moves.push(format_uci_move(&current_board, best_move));
                    current_board.play_unchecked(best_move);
                } else {
                    break;
                }
            } else {
                break;
            }
        } else {
            break;
        }
    }

    pv_moves.join(" ")
}
