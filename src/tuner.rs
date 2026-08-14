use crate::eval::{DEFAULT_PARAMS, EvalParams, evaluate_board};
use crate::search::get_best_move;
use crate::transposition::TranspositionTable;
use cozy_chess::{Board, Color, GameStatus};
use rand::Rng;
use rayon::prelude::*;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

pub fn generate_dataset(num_games: usize, output_file: &str) {
    let file = Arc::new(Mutex::new(
        OpenOptions::new()
            .create(true)
            .append(true)
            .open(output_file)
            .expect("Failed to open output file"),
    ));

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

        handles.push(thread::spawn(move || {
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

                    if game_history.iter().filter(|&&h| h == board.hash()).count() >= 3 {
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
                            None,
                            &tt_clone,
                            stop_flag,
                            is_pondering,
                            time_limit_ms,
                            false,
                            0,
                            &params,
                            &mut hist_clone,
                            &None,
                            &None,
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
                    completed_games_clone.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
                if current % 10000 == 0 {
                    println!("Progress: {}/{} games finished.", current, num_games);
                }

                let mut f = file_clone.lock().unwrap();
                for fen in quiet_positions {
                    writeln!(f, "{} | {}", fen, result).unwrap();
                }
            }
        }));
    }

    for handle in handles {
        let _ = handle.join();
    }
}

fn sigmoid(score: f64, k: f64) -> f64 {
    1.0 / (1.0 + (10_f64).powf((-k * score) / 400.0))
}

fn compute_error(dataset: &[(Board, f64)], params: &EvalParams, k: f64) -> f64 {
    let total_error: f64 = dataset
        .par_iter()
        .map(|(board, result)| {
            let score = evaluate_board(board, params) as f64;
            let p = sigmoid(score, k);
            let error = result - p;
            error * error
        })
        .sum();

    total_error / (dataset.len() as f64)
}

fn get_param(params: &mut EvalParams, idx: usize) -> &mut i32 {
    if idx < 5 {
        return &mut params.piece_values[idx];
    }
    let p_idx = idx - 5;
    let table = p_idx / 64;
    let cell = p_idx % 64;
    let row = cell / 8;
    let col = cell % 8;

    match table {
        0 => &mut params.pawn_mg[row][col],
        1 => &mut params.pawn_eg[row][col],
        2 => &mut params.knight_mg[row][col],
        3 => &mut params.knight_eg[row][col],
        4 => &mut params.bishop_mg[row][col],
        5 => &mut params.bishop_eg[row][col],
        6 => &mut params.rook_mg[row][col],
        7 => &mut params.rook_eg[row][col],
        8 => &mut params.queen_mg[row][col],
        9 => &mut params.queen_eg[row][col],
        10 => &mut params.king_mg[row][col],
        11 => &mut params.king_eg[row][col],
        _ => unreachable!(),
    }
}

pub fn tune(dataset_file: &str) {
    println!("Opening dataset: {}...", dataset_file);
    let file = File::open(dataset_file).expect("Failed to open dataset");
    let reader = BufReader::new(file);

    let mut dataset = Vec::new();
    let mut count = 0;

    for line in reader.lines() {
        let line = line.unwrap();
        let parts: Vec<&str> = line.split(" | ").collect();
        if parts.len() == 2 {
            if let Ok(board) = parts[0].parse::<Board>() {
                if let Ok(res) = parts[1].parse::<f64>() {
                    dataset.push((board, res));
                }
            }
        }

        count += 1;
        if count % 500_000 == 0 {
            println!("Parsed {} lines into memory...", count);
        }
    }

    println!("Finished loading {} valid positions.", dataset.len());

    let mut params = DEFAULT_PARAMS.clone();

    let mut best_k = 1.0;
    let mut best_err = compute_error(&dataset, &params, best_k);

    for i in 1..=100 {
        let k = (i as f64) * 0.1;
        let err = compute_error(&dataset, &params, k);
        if err < best_err {
            best_k = k;
            best_err = err;
        }
    }

    println!("Best K: {} (Error: {})", best_k, best_err);

    let mut improved = true;
    let mut iter = 0;
    let num_params = 5 + 12 * 64;

    while improved {
        improved = false;
        iter += 1;
        println!("Iteration {}...", iter);

        for i in 0..num_params {
            let original = *get_param(&mut params, i);

            *get_param(&mut params, i) = original + 1;
            let err_up = compute_error(&dataset, &params, best_k);

            *get_param(&mut params, i) = original - 1;
            let err_down = compute_error(&dataset, &params, best_k);

            if err_up < best_err && err_up < err_down {
                *get_param(&mut params, i) = original + 1;
                best_err = err_up;
                improved = true;
            } else if err_down < best_err {
                *get_param(&mut params, i) = original - 1;
                best_err = err_down;
                improved = true;
            } else {
                *get_param(&mut params, i) = original;
            }
        }

        if improved {
            println!("New Best Error: {}", best_err);
            println!("Piece Values: {:?}", params.piece_values);

            if let Ok(mut out_file) = File::create("/kaggle/working/tuned_parameters.txt") {
                let _ = writeln!(out_file, "{:#?}", params);
            }
        }
    }

    println!("Tuning complete! Saving parameters to file...");

    let mut out_file = File::create("tuned_parameters.txt").expect("Failed to create output file");

    writeln!(out_file, "{:#?}", params).expect("Failed to write params");

    println!("Saved everything to tuned_parameters.txt");
}
