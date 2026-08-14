use cozy_chess::Board;

pub(crate) fn perft(board: &Board, depth: i32) -> u64 {
    if depth == 0 {
        return 1;
    }
    let mut nodes = 0u64;
    board.generate_moves(|move_list| {
        for m in move_list {
            let mut next = board.clone();
            next.play_unchecked(m);
            nodes += perft(&next, depth - 1);
        }
        false
    });
    nodes
}
