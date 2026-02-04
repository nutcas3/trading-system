use crate::types::SystemEvent;
use crossbeam::channel::Receiver;
use metrics::counter;
use rocksdb::{DB, IteratorMode, Options};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

pub struct OracleVault {
    db: Arc<DB>,
    sequence: Arc<AtomicU64>,
}

impl OracleVault {
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self, rocksdb::Error> {
        let mut opts = Options::default();
        opts.create_if_missing(true);
        opts.increase_parallelism(4);
        
        let db = DB::open(&opts, path)?;
        
        let sequence = db
            .iterator(IteratorMode::End)
            .next()
            .and_then(|result| result.ok())
            .and_then(|(key, _)| {
                String::from_utf8(key.to_vec())
                    .ok()
                    .and_then(|s| s.parse::<u64>().ok())
            })
            .unwrap_or(0);

        Ok(OracleVault {
            db: Arc::new(db),
            sequence: Arc::new(AtomicU64::new(sequence)),
        })
    }

    pub fn append(&self, event: SystemEvent) -> Result<u64, rocksdb::Error> {
        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let key = format!("{:020}", seq);
        let value = bincode::serialize(&event).unwrap();
        
        self.db.put(key.as_bytes(), &value)?;
        
        counter!("oracle.events_written").increment(1);
        
        Ok(seq)
    }

    pub fn replay_all(&self) -> Vec<SystemEvent> {
        let mut events = Vec::new();
        
        for item in self.db.iterator(IteratorMode::Start) {
            if let Ok((_, value)) = item {
                if let Ok(event) = bincode::deserialize::<SystemEvent>(&value) {
                    events.push(event);
                }
            }
        }
        
        events
    }

    pub fn replay_from(&self, start_seq: u64) -> Vec<SystemEvent> {
        let mut events = Vec::new();
        let start_key = format!("{:020}", start_seq);
        
        for item in self.db.iterator(IteratorMode::From(
            start_key.as_bytes(),
            rocksdb::Direction::Forward,
        )) {
            if let Ok((_, value)) = item {
                if let Ok(event) = bincode::deserialize::<SystemEvent>(&value) {
                    events.push(event);
                }
            }
        }
        
        events
    }

    pub fn event_count(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    pub fn compute_state_hash(&self) -> String {
        let events = self.replay_all();
        let serialized = bincode::serialize(&events).unwrap();
        let mut hasher = Sha256::new();
        hasher.update(&serialized);
        format!("{:x}", hasher.finalize())
    }
}

pub struct OracleEngine {
    vault: Arc<OracleVault>,
    event_rx: Receiver<SystemEvent>,
}

impl OracleEngine {
    pub fn new(
        vault_path: &str,
        event_rx: Receiver<SystemEvent>,
    ) -> Result<Self, rocksdb::Error> {
        let vault = Arc::new(OracleVault::open(vault_path)?);
        
        Ok(OracleEngine { vault, event_rx })
    }

    pub fn run(&self) {
        println!("[Oracle] Event store started");
        
        while let Ok(event) = self.event_rx.recv() {
            match self.vault.append(event) {
                Ok(seq) => {
                    if seq % 1000 == 0 {
                        println!("[Oracle] Checkpoint: {} events persisted", seq);
                    }
                }
                Err(e) => {
                    eprintln!("[Oracle] Failed to persist event: {}", e);
                }
            }
        }
    }

    pub fn get_vault(&self) -> Arc<OracleVault> {
        Arc::clone(&self.vault)
    }
}
