use cozy_chess::{ Board, Color, GameStatus };
use std::fs::{ File, OpenOptions };
use std::io::{ BufRead, BufReader, Write };
use std::sync::{ Arc, Mutex };
use std::sync::atomic::{ AtomicBool, AtomicU64 };
use std::time::Duration;
use std::thread;
use crate::eval::{ evaluate_board, EvalParams, DEFAULT_PARAMS };
use crate::search::{ get_best_move };
use crate::transposition::TranspositionTable;
use rand::Rng;

pub fn generate_dataset(num_games: usize, output_file: &str) {
    let file = Arc::new(
        Mutex::new(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(output_file)
                .expect("Failed to open output file")
        )
    );

    let tt = Arc::new(TranspositionTable::new(64));
    let num_threads = 224;
    let games_per_thread = num_games / num_threads;
    let remainder = num_games % num_threads;

    let completed_games = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let mut handles = Vec::new();

    for thread_id in 0..num_threads {
        let tt_clone = Arc::clone(&tt);
        let completed_games_clone = Arc::clone(&completed_games);
        let file_clone = Arc::clone(&file);

        let games_to_play = if thread_id == num_threads - 1 {
            games_per_thread + remainder
        } else {
            games_per_thread
        };

        handles.push(
            thread::spawn(move || {
                let mut rng = rand::thread_rng();

                for _game_idx in 0..games_to_play {
                    let mut board = Board::default();
                    let mut game_history = vec![board.hash()];
                    let mut quiet_positions = Vec::new();

                    let mut result = 0.5;
                    let mut ply = 0;
                    let mut eval_history = Vec::new();

                    tt_clone.new_search();

                    loop {
                        match board.status() {
                            GameStatus::Won => {
                                result = if board.side_to_move() == Color::White {
                                    0.0
                                } else {
                                    1.0
                                };
                                break;
                            }
                            GameStatus::Drawn => {
                                result = 0.5;
                                break;
                            }
                            GameStatus::Ongoing => {}
                        }

                        if
                            game_history
                                .iter()
                                .filter(|&&h| h == board.hash())
                                .count() >= 3
                        {
                            result = 0.5;
                            break;
                        }

                        let mut moves = Vec::new();
                        board.generate_moves(|move_list| {
                            moves.extend(move_list);
                            false
                        });

                        if moves.is_empty() {
                            break;
                        }

                        let m = if ply < 6 {
                            moves[rng.gen_range(0..moves.len())]
                        } else {
                            let stop_flag = Arc::new(AtomicBool::new(false));
                            let is_pondering = Arc::new(AtomicBool::new(false));
                            let time_limit_ms = Arc::new(AtomicU64::new(0));

                            let mut hist_clone = game_history.clone();
                            let params = DEFAULT_PARAMS.clone();
                            let (best, _) = get_best_move(
                                &board,
                                Duration::from_millis(15),
                                &tt_clone,
                                stop_flag,
                                is_pondering,
                                time_limit_ms,
                                false,
                                0,
                                &params,
                                &mut hist_clone
                            );

                            if let Some(m) = best {
                                m
                            } else {
                                moves[rng.gen_range(0..moves.len())]
                            }
                        };

                        if ply >= 6 {
                            let eval = evaluate_board(&board, &DEFAULT_PARAMS);
                            let score = if board.side_to_move() == Color::Black {
                                -eval
                            } else {
                                eval
                            };
                            eval_history.push(score);

                            if eval_history.len() >= 5 {
                                let recent = &eval_history[eval_history.len() - 5..];
                                if recent.iter().all(|&s| s > 1000) {
                                    result = 1.0;
                                    break;
                                }
                                if recent.iter().all(|&s| s < -1000) {
                                    result = 0.0;
                                    break;
                                }
                            }

                            if ply >= 40 && eval_history.len() >= 12 {
                                let recent = &eval_history[eval_history.len() - 12..];
                                if recent.iter().all(|&s| s.abs() <= 10) {
                                    result = 0.5;
                                    break;
                                }
                            }
                        }

                        let is_capture = board.color_on(m.to).is_some();
                        let gives_check = {
                            let mut next = board.clone();
                            next.play_unchecked(m);
                            !next.checkers().is_empty()
                        };
                        let in_check = !board.checkers().is_empty();

                        if ply >= 16 && !in_check && !is_capture && !gives_check {
                            let fen = format!("{}", board);
                            quiet_positions.push(fen);
                        }

                        board.play_unchecked(m);
                        game_history.push(board.hash());
                        ply += 1;
                    }

                    let current =
                        completed_games_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed) +
                        1;
                    if current % 10000 == 0 {
                        println!("Progress: {}/{} games finished.", current, num_games);
                    }

                    let mut f = file_clone.lock().unwrap();
                    for fen in quiet_positions {
                        writeln!(f, "{} | {}", fen, result).unwrap();
                    }
                }
            })
        );
    }

    for handle in handles {
        let _ = handle.join();
    }
}

pub fn tune(dataset_file: &str) {
    println!("Starting tuning with dataset: {}", dataset_file);

    let file = File::open(dataset_file).expect("Failed to open dataset file");
    let reader = BufReader::new(file);

    let mut count = 0;
    for line in reader.lines() {
        if let Ok(_l) = line {
            count += 1;
        }
    }
    println!("Read {} positions.", count);

    // Placeholder for tuning logic
    let _params: EvalParams = DEFAULT_PARAMS.clone();
    println!("Tuning logic not yet implemented.");
}
