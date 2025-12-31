use std::error::Error;
use std::sync::Arc;

use enc_video::devices::ActivatedDevice;
use enc_video::devices::{VideoDevices, activated_device::Output};
use enc_video::i_capture::ICapture;
use enc_video::monitor::Monitor;
use minifb::{Window, WindowOptions};
use tokio::sync::mpsc;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx};

/// Determines if the camera or monior will run
pub enum CaptureType {
    /// Monitor (monitor index) -> 0 index based
    Monitor(u32),
    Camera,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    //this could easily be changed to camera.
    let capture_type = CaptureType::Monitor(0);

    let capture = Arc::new(get_capture(capture_type));
    let dimensions = capture.get_dimensions()?;
    let (width, height) = (dimensions.width as usize, dimensions.height as usize);

    //create channel to send converted frame data to the main thread.
    let (tx, mut rx) = mpsc::channel::<Vec<u32>>(2);

    // task to capture and convert raw data from the camera or window
    let recv = capture.clone_receiver().clone();
    tokio::spawn(async move {
        loop {
            //get the data before the conversion and drop right after
            let data = {
                let mut guard = recv.lock().await;
                guard.recv().await
            };

            if let Some(raw_data) = data {
                if raw_data.is_empty() {
                    continue;
                }

                //convert to u32
                let mut frame_u32 = vec![0u32; width * height];
                for i in 0..width * height {
                    let b = raw_data[i * 4] as u32;
                    let g = raw_data[i * 4 + 1] as u32;
                    let r = raw_data[i * 4 + 2] as u32;
                    frame_u32[i] = (r << 16) | (g << 8) | b;
                }

                //try sending, we do not need to send the data, as we can afford to lose frames
                let _ = tx.try_send(frame_u32);
            } else {
                break;
            }
        }
    });

    // start capturing data on a different future
    let capture_clone = capture.clone();
    tokio::spawn(async move {
        //deref get capture ref to Arc<dynIcapture> and clone it
        let capture_clone = (*capture_clone).as_ref().clone();

        capture_clone
            .start_capturing()
            .await
            .expect("Could not capture, failed.");
    });

    let mut current_frame = vec![0u32; width * height];

    let mut window = create_window(width, height);

    while window.is_open() && !window.is_key_pressed(minifb::Key::Escape, minifb::KeyRepeat::Yes) {
        // Non-blocking check for new frames
        // try_recv ensures we don't block the UI thread if no frame is ready
        while let Ok(new_frame) = rx.try_recv() {
            current_frame = new_frame;
        }

        window.update_with_buffer(&current_frame, width, height)?;
    }

    //stop capturing the screen
    (*capture).as_ref().clone().stop_capturing().await?;

    Ok(())
}

/// This function is not really used within this, but shows how you can return an ICapture which is capable of being interchangeable with the Monitor and or VideoDevice.
/// This allows you to use the same code in the main whether you use a Monitor or Camera.
fn get_capture(cap_type: CaptureType) -> Box<Arc<dyn ICapture<CaptureOutput = Vec<u8>>>> {
    match cap_type {
        CaptureType::Monitor(id) => {
            let monitor: Arc<Monitor>;

            unsafe {
                monitor = Monitor::from_monitor(id).expect("Could not get monitor {id}");
            }

            Box::new(monitor)
        }
        CaptureType::Camera => {
            let device: Arc<ActivatedDevice>;

            unsafe {
                let hr = CoInitializeEx(None, COINIT_MULTITHREADED);

                if hr != windows::Win32::Foundation::S_OK {
                    panic!("Could not initialize CoInit with error {hr}");
                }

                let video_devices = VideoDevices::new().expect("Could not aggregate video devices");

                device = video_devices
                    .activate_device(video_devices.devices[0], Some(Output::RGB32))
                    .expect("Could not activate device.");
            }

            Box::new(device)
        }
    }
}

fn create_window(width: usize, height: usize) -> Window {
    let mut opts = WindowOptions::default();
    opts.resize = true;
    opts.scale_mode = minifb::ScaleMode::UpperLeft;
    opts.scale = minifb::Scale::X1;
    let mut window = Window::new("Video Capture", width, height, opts).expect("Could not start application because Window refused to open!");
    window.set_target_fps(60);

    return window;
}
