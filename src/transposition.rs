use cozy_chess::Move;
use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Write, Result as IoResult};
use std::sync::RwLock;
use std::sync::atomic::{AtomicU8, Ordering};

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum NodeType {
    Exact,
    LowerBound,
    UpperBound,
}

#[derive(Clone, Copy)]
pub struct TTEntry {
    pub hash: u64,
    pub depth: i32,
    pub score: i32,
    pub node_type: NodeType,
    pub best_move: Option<Move>,
    pub age: u8,
}

impl Default for TTEntry {
    fn default() -> Self {
        Self {
            hash: 0,
            depth: 0,
            score: 0,
            node_type: NodeType::Exact,
            best_move: None,
            age: 0,
        }
    }
}

pub struct TranspositionTable {
    table: Vec<RwLock<TTEntry>>,
    mask: usize,
    pub age: AtomicU8,
}

impl TranspositionTable {
    pub fn new(size_mb: usize) -> Self {
        let entry_size = std::mem::size_of::<RwLock<TTEntry>>();
        let count = (size_mb * 1024 * 1024) / entry_size;
        let count = if count < 2 {
            1
        } else {
            1usize << (usize::BITS - 1 - (count - 1).leading_zeros())
        };
        
        let mut table = Vec::with_capacity(count);
        for _ in 0..count {
            table.push(RwLock::new(TTEntry::default()));
        }
        
        Self {
            table,
            mask: count - 1,
            age: AtomicU8::new(0),
        }
    }

    #[inline]
    fn index(&self, hash: u64) -> usize {
        (hash as usize) & self.mask
    }

    pub fn get(&self, hash: u64) -> Option<TTEntry> {
        let entry = self.table[self.index(hash)].read().unwrap();
        if entry.hash == hash {
            Some(*entry)
        } else {
            None
        }
    }

    pub fn insert(
        &self,
        hash: u64,
        depth: i32,
        score: i32,
        node_type: NodeType,
        best_move: Option<Move>
    ) {
        let idx = self.index(hash);
        let mut existing = self.table[idx].write().unwrap();
        let current_age = self.age.load(Ordering::Relaxed);

        let dominated =
            existing.hash == 0 ||
            existing.hash == hash ||
            existing.age != current_age ||
            existing.depth <= depth;

        if dominated {
            *existing = TTEntry {
                hash,
                depth,
                score,
                node_type,
                best_move,
                age: current_age,
            };
        }
    }

    pub fn new_search(&self) {
        self.age.fetch_add(1, Ordering::Relaxed);
    }


    pub fn save_to_file(&self, path: &str) -> IoResult<()> {
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);

        for entry_lock in &self.table {
            let entry = entry_lock.read().unwrap();
            
            writer.write_all(&entry.hash.to_le_bytes())?;
            writer.write_all(&entry.depth.to_le_bytes())?;
            writer.write_all(&entry.score.to_le_bytes())?;
            
            let nt_byte = match entry.node_type {
                NodeType::Exact => 0u8,
                NodeType::LowerBound => 1,
                NodeType::UpperBound => 2,
            };
            writer.write_all(&[nt_byte])?;

            if let Some(m) = entry.best_move {
                writer.write_all(&[1u8])?;
                let m_str = m.to_string();
                let mut bytes = [0u8; 5];
                let s_bytes = m_str.as_bytes();
                for i in 0..s_bytes.len().min(5) {
                    bytes[i] = s_bytes[i];
                }
                writer.write_all(&bytes)?;
            } else {
                writer.write_all(&[0u8, 0, 0, 0, 0, 0])?;
            }

            writer.write_all(&[entry.age])?;
        }
        writer.flush()?;
        Ok(())
    }

    pub fn load_from_file(&self, path: &str) -> IoResult<()> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        for entry_lock in &self.table {
            let mut hash_bytes = [0u8; 8];
            if reader.read_exact(&mut hash_bytes).is_err() {
                break;
            }
            let hash = u64::from_le_bytes(hash_bytes);

            let mut depth_bytes = [0u8; 4];
            reader.read_exact(&mut depth_bytes)?;
            let depth = i32::from_le_bytes(depth_bytes);

            let mut score_bytes = [0u8; 4];
            reader.read_exact(&mut score_bytes)?;
            let score = i32::from_le_bytes(score_bytes);

            let mut nt_byte = [0u8; 1];
            reader.read_exact(&mut nt_byte)?;
            let node_type = match nt_byte[0] {
                0 => NodeType::Exact,
                1 => NodeType::LowerBound,
                _ => NodeType::UpperBound,
            };

            let mut mv_flag = [0u8; 1];
            reader.read_exact(&mut mv_flag)?;
            let mut mv_bytes = [0u8; 5];
            reader.read_exact(&mut mv_bytes)?;
            
            let best_move = if mv_flag[0] == 1 {
                let len = mv_bytes.iter().position(|&b| b == 0).unwrap_or(5);
                if let Ok(s) = std::str::from_utf8(&mv_bytes[..len]) {
                    s.parse::<Move>().ok()
                } else {
                    None
                }
            } else {
                None
            };

            let mut age_byte = [0u8; 1];
            reader.read_exact(&mut age_byte)?;
            let _age = age_byte[0];

            let mut entry = entry_lock.write().unwrap();
            entry.hash = hash;
            entry.depth = depth;
            entry.score = score;
            entry.node_type = node_type;
            entry.best_move = best_move;
            entry.age = 0;
        }
        Ok(())
    }
}
