/// Device disconnection monitoring.
///
/// Uses `coreaudio::audio_unit::macos_helpers::AliveListener` to detect when
/// the selected input or output device is unplugged. A background thread polls
/// both listeners at 500 ms intervals. On disconnection it calls the provided
/// callback (which stops the engine and emits a Tauri event to the frontend).
use coreaudio::audio_unit::macos_helpers::AliveListener;
use coreaudio::sys::AudioDeviceID;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// Runs in the background and calls `on_disconnect` when either device dies.
///
/// Dropped when the engine is stopped — the background thread exits at the next
/// poll cycle via the `running` atomic flag.
pub struct DeviceMonitor {
    running: Arc<AtomicBool>,
}

impl DeviceMonitor {
    /// Start monitoring `input_id` and `output_id`. Calls `on_disconnect` (once)
    /// if either device becomes unavailable.
    pub fn start<F>(input_id: AudioDeviceID, output_id: AudioDeviceID, on_disconnect: F) -> Self
    where
        F: Fn() + Send + 'static,
    {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = running.clone();

        thread::spawn(move || {
            // AliveListeners are !Send (they hold raw pointers), so we create them inside
            // the thread that uses them.
            let mut input_listener = AliveListener::new(input_id);
            let mut output_listener = AliveListener::new(output_id);

            if input_listener.register().is_err() || output_listener.register().is_err() {
                // If registration fails, we can't monitor — exit silently.
                return;
            }

            let mut disconnected = false;

            while running_clone.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(500));

                if !disconnected
                    && (!input_listener.is_alive() || !output_listener.is_alive())
                {
                    disconnected = true;
                    on_disconnect();
                }
            }

            // Listeners unregister themselves on drop (AliveListener::Drop calls unregister).
        });

        Self { running }
    }
}

impl Drop for DeviceMonitor {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
    }
}
