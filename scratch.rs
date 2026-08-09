use nnue_rs::Network;

fn main() {
    let net = Network::from_file("omo.nnue").unwrap();
    let start_fen = "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1";
    let scholar_fen = "r1bqkb1r/pppp1ppp/2n2n2/4p2Q/2B1P3/8/PPPP1PPP/RNB1K1NR w KQkq - 4 4";
    
    match net.evaluate_fen(start_fen) {
        Ok(score) => println!("Start pos score: {}", score),
        Err(e) => println!("Start pos error: {:?}", e),
    }

    match net.evaluate_fen(scholar_fen) {
        Ok(score) => println!("Scholar score: {}", score),
        Err(e) => println!("Scholar error: {:?}", e),
    }
}
