use cozy_chess::{ Board, Color, Piece, BitBoard };

const FILE_MASKS: [BitBoard; 8] = [
    BitBoard(0x0101010101010101),
    BitBoard(0x0202020202020202),
    BitBoard(0x0404040404040404),
    BitBoard(0x0808080808080808),
    BitBoard(0x1010101010101010),
    BitBoard(0x2020202020202020),
    BitBoard(0x4040404040404040),
    BitBoard(0x8080808080808080),
];

const RANK_MASKS: [BitBoard; 8] = [
    BitBoard(0x00000000000000ff),
    BitBoard(0x000000000000ff00),
    BitBoard(0x0000000000ff0000),
    BitBoard(0x00000000ff000000),
    BitBoard(0x000000ff00000000),
    BitBoard(0x0000ff0000000000),
    BitBoard(0x00ff000000000000),
    BitBoard(0xff00000000000000),
];

const fn compute_passed_mask_white() -> [BitBoard; 64] {
    let mut masks = [BitBoard(0); 64];
    let mut sq = 0;
    while sq < 64 {
        let file = sq % 8;
        let rank = sq / 8;
        let mut mask = 0u64;
        let mut r = rank + 1;
        while r < 8 {
            mask |= RANK_MASKS[r].0;
            r += 1;
        }
        let file_span = if file == 0 {
            FILE_MASKS[0].0 | FILE_MASKS[1].0
        } else if file == 7 {
            FILE_MASKS[6].0 | FILE_MASKS[7].0
        } else {
            FILE_MASKS[file - 1].0 | FILE_MASKS[file].0 | FILE_MASKS[file + 1].0
        };
        masks[sq] = BitBoard(mask & file_span);
        sq += 1;
    }
    masks
}

const fn compute_passed_mask_black() -> [BitBoard; 64] {
    let mut masks = [BitBoard(0); 64];
    let mut sq = 0;
    while sq < 64 {
        let file = sq % 8;
        let rank = sq / 8;
        let mut mask = 0u64;
        let mut r = 0;
        while r < rank {
            mask |= RANK_MASKS[r].0;
            r += 1;
        }
        let file_span = if file == 0 {
            FILE_MASKS[0].0 | FILE_MASKS[1].0
        } else if file == 7 {
            FILE_MASKS[6].0 | FILE_MASKS[7].0
        } else {
            FILE_MASKS[file - 1].0 | FILE_MASKS[file].0 | FILE_MASKS[file + 1].0
        };
        masks[sq] = BitBoard(mask & file_span);
        sq += 1;
    }
    masks
}

const PASSED_MASK_WHITE: [BitBoard; 64] = compute_passed_mask_white();
const PASSED_MASK_BLACK: [BitBoard; 64] = compute_passed_mask_black();

pub fn evaluate_pawns(board: &Board, color: Color) -> i32 {
    let mut score = 0;
    let our_pawns = board.pieces(Piece::Pawn) & board.colors(color);
    let their_pawns = board.pieces(Piece::Pawn) & board.colors(!color);

    for file in 0..8 {
        let file_mask = FILE_MASKS[file];
        let our_pawns_on_file = (our_pawns & file_mask).len();
        if our_pawns_on_file > 1 {
            score -= 10;
        }

        let isolated_mask = match file {
            0 => FILE_MASKS[1],
            7 => FILE_MASKS[6],
            _ => FILE_MASKS[file - 1] | FILE_MASKS[file + 1],
        };

        if our_pawns_on_file > 0 && (our_pawns & isolated_mask).is_empty() {
            score -= 20 * (our_pawns_on_file as i32);
        }
    }

    for sq in our_pawns {
        let sq_idx = sq as usize;
        let rank = sq.rank() as usize;

        let passed_mask = if color == Color::White {
            PASSED_MASK_WHITE[sq_idx]
        } else {
            PASSED_MASK_BLACK[sq_idx]
        };

        if (their_pawns & passed_mask).is_empty() {
            let advancement = if color == Color::White { rank } else { 7 - rank };
            score += 10 * (advancement as i32);
        }
    }

    score
}
