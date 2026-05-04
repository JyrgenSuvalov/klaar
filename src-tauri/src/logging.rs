use std::fs;
use std::path::PathBuf;

/// Maximum log file size before rotation (10 MB).
const MAX_LOG_SIZE: u64 = 10 * 1024 * 1024;
/// Number of rotated log files to keep.
const MAX_ROTATED_FILES: u32 = 3;

/// Returns the log directory path: ~/Library/Logs/Klaar/
fn log_dir() -> PathBuf {
    let home = dirs_path();
    home.join("Library").join("Logs").join("Klaar")
}

fn dirs_path() -> PathBuf {
    // Use HOME env var; fallback to /tmp if somehow unset
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/tmp"))
}

/// Returns the main log file path.
fn log_file_path() -> PathBuf {
    log_dir().join("klaar.log")
}

/// Rotate log files if the current log exceeds MAX_LOG_SIZE.
/// klaar.log -> klaar.log.1
/// klaar.log.1 -> klaar.log.2
/// klaar.log.2 -> klaar.log.3
/// klaar.log.3 is deleted
fn rotate_if_needed() {
    let log_path = log_file_path();
    let size = fs::metadata(&log_path).map(|m| m.len()).unwrap_or(0);
    if size < MAX_LOG_SIZE {
        return;
    }

    // Delete the oldest rotated file
    let oldest = log_path.with_extension(format!("log.{}", MAX_ROTATED_FILES));
    let _ = fs::remove_file(&oldest);

    // Shift existing rotated files
    for i in (1..MAX_ROTATED_FILES).rev() {
        let from = log_path.with_extension(format!("log.{}", i));
        let to = log_path.with_extension(format!("log.{}", i + 1));
        let _ = fs::rename(&from, &to);
    }

    // Rotate current log to .1
    let rotated = log_path.with_extension("log.1");
    let _ = fs::rename(&log_path, &rotated);
}

/// Initialise the fern logger with dual output:
/// - stderr (for development / `pnpm tauri dev`)
/// - file at ~/Library/Logs/Klaar/klaar.log
///
/// Log level defaults to `info`. Override with `RUST_LOG` env var.
pub fn init_logging() -> Result<(), Box<dyn std::error::Error>> {
    let dir = log_dir();
    fs::create_dir_all(&dir)?;

    // Rotate before opening the file for append
    rotate_if_needed();

    let log_path = log_file_path();
    let log_file = fern::log_file(&log_path)?;

    // Determine log level from RUST_LOG or default to info
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|s| s.parse::<log::LevelFilter>().ok())
        .unwrap_or(log::LevelFilter::Info);

    fern::Dispatch::new()
        .format(|out, message, record| {
            out.finish(format_args!(
                "[{}][{}][{}] {}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
                record.level(),
                record.target(),
                message
            ))
        })
        .level(level)
        // Suppress noisy third-party crates
        .level_for("tao", log::LevelFilter::Warn)
        .level_for("wry", log::LevelFilter::Warn)
        .chain(std::io::stderr())
        .chain(log_file)
        .apply()?;

    log::info!("Klaar logging initialised (level={})", level);
    Ok(())
}
