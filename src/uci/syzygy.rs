use shakmaty::Chess;
use shakmaty_syzygy::Tablebase;

pub fn load_syzygy(path: &str) -> Option<Tablebase<Chess>> {
    let mut tb = Tablebase::new();
    match tb.add_directory(path) {
        Ok(count) => {
            if count > 0 {
                println!("info string Loaded {} Syzygy tables from {}", count, path);
                Some(tb)
            } else {
                println!("info string No Syzygy tables found in {}", path);
                None
            }
        }
        Err(e) => {
            println!(
                "info string Failed to load Syzygy tables from {}: {}",
                path, e
            );
            None
        }
    }
}
