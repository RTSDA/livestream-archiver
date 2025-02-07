use std::path::PathBuf;
use anyhow::Result;
use notify::{Watcher, RecursiveMode, Event, EventKind};
use tokio::sync::mpsc;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};

mod services;
use services::livestream_archiver::LivestreamArchiver;

#[tokio::main]
async fn main() -> Result<()> {
    let watch_path = PathBuf::from("/home/rockvilleav/Sync/Livestreams");
    let output_path = PathBuf::from("/media/archive/jellyfin/livestreams");

    // Ensure directories exist
    if !watch_path.exists() {
        std::fs::create_dir_all(&watch_path)?;
    }
    if !output_path.exists() {
        std::fs::create_dir_all(&output_path)?;
    }

    println!("Starting livestream archiver service...");
    println!("Watching directory: {}", watch_path.display());
    println!("Output directory: {}", output_path.display());

    let archiver = LivestreamArchiver::new(output_path);
    let processed_files = Arc::new(Mutex::new(HashSet::new()));

    // Process existing files first
    println!("Checking for existing files...");
    if let Ok(entries) = std::fs::read_dir(&watch_path) {
        for entry in entries {
            if let Ok(entry) = entry {
                let path = entry.path();
                println!("Found existing file: {}", path.display());
                if let Err(e) = archiver.process_file(path).await {
                    eprintln!("Error processing existing file: {}", e);
                }
            }
        }
    }

    // Set up file watcher for new files
    let (tx, mut rx) = mpsc::channel(100);
    
    let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
        let tx = tx.clone();
        match res {
            Ok(event) => {
                println!("Received event: {:?}", event);
                if let Err(e) = tx.blocking_send(event) {
                    eprintln!("Error sending event: {}", e);
                }
            }
            Err(e) => eprintln!("Watch error: {}", e),
        }
    })?;

    watcher.watch(&watch_path, RecursiveMode::NonRecursive)?;

    while let Some(event) = rx.recv().await {
        println!("Processing event: {:?}", event);
        
        match event.kind {
            EventKind::Create(_) | EventKind::Modify(_) => {
                for path in event.paths {
                    if let Ok(canonical_path) = std::fs::canonicalize(&path) {
                        let path_str = canonical_path.to_string_lossy().to_string();
                        let mut processed = processed_files.lock().unwrap();
                        
                        if !processed.contains(&path_str) {
                            println!("Processing file: {}", path_str);
                            if let Err(e) = archiver.process_file(path).await {
                                eprintln!("Error processing file: {}", e);
                            } else {
                                processed.insert(path_str);
                                if processed.len() > 1000 {
                                    processed.clear();
                                }
                            }
                        } else {
                            println!("Skipping already processed file: {}", path_str);
                        }
                    }
                }
            },
            _ => println!("Ignoring event: {:?}", event),
        }
    }

    Ok(())
}
