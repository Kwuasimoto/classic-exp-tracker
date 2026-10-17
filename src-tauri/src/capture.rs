//! Windows Graphics Capture of the PRIMARY MONITOR (never a window, never the game
//! process). Each accepted frame is cropped to the calibrated regions on the GPU,
//! copied to memory, and sent to the pipeline. Nothing is written to disk.

use std::sync::mpsc::{SyncSender, TrySendError};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings,
    SecondaryWindowSettings, Settings,
};

use crate::geom::{Calibration, Crop, Rect};
use crate::now_unix;

pub const SAMPLE_INTERVAL: Duration = Duration::from_secs(1);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Regions {
    pub width: u32,
    pub height: u32,
    pub calibration: Calibration,
}

/// State shared between the capture thread, the pipeline and commands.
#[derive(Default)]
pub struct Shared {
    /// Calibration for the current monitor resolution; `None` until known.
    pub regions: RwLock<Option<Regions>>,
    /// One-shot request for a full frame (calibration screen).
    pub full_frame_request: Mutex<Option<SyncSender<Crop>>>,
}

#[derive(Debug)]
pub enum CaptureMsg {
    Regions { t: i64, exp: Crop, map: Crop, level: Option<Crop> },
    Uncalibrated { width: u32, height: u32 },
    Closed,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error("monitor: {0}")]
    Monitor(#[from] windows_capture::monitor::Error),
    #[error("frame: {0}")]
    Frame(#[from] windows_capture::frame::Error),
    #[error("frame buffer has unexpected size")]
    BufferSize,
    #[error("shared capture state lock poisoned")]
    Poisoned,
    #[error("could not start capture: {0}")]
    Start(String),
}

pub struct Flags {
    pub tx: SyncSender<CaptureMsg>,
    pub shared: Arc<Shared>,
}

pub struct Handler {
    tx: SyncSender<CaptureMsg>,
    shared: Arc<Shared>,
    last_sent: Option<Instant>,
}

pub type Control = CaptureControl<Handler, CaptureError>;

fn copy_region(frame: &mut Frame, r: Rect) -> Result<Crop, CaptureError> {
    let buffer = frame.buffer_crop(r.x, r.y, r.x + r.w, r.y + r.h)?;
    let mut scratch = Vec::new();
    let pixels = buffer.as_nopadding_buffer(&mut scratch).to_vec();
    Crop::new(r.w, r.h, pixels).ok_or(CaptureError::BufferSize)
}

impl GraphicsCaptureApiHandler for Handler {
    type Flags = Flags;
    type Error = CaptureError;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { tx: ctx.flags.tx, shared: ctx.flags.shared, last_sent: None })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, control: InternalCaptureControl) -> Result<(), Self::Error> {
        let (width, height) = (frame.width(), frame.height());

        let requester = self.shared.full_frame_request.lock().map_err(|_| CaptureError::Poisoned)?.take();
        if let Some(requester) = requester {
            let full = copy_region(frame, Rect { x: 0, y: 0, w: width, h: height })?;
            if requester.try_send(full).is_err() {
                // The requester timed out and dropped its receiver; nobody to deliver to.
            }
        }

        if self.last_sent.is_some_and(|t| t.elapsed() < SAMPLE_INTERVAL) {
            return Ok(());
        }
        self.last_sent = Some(Instant::now());

        let regions = *self.shared.regions.read().map_err(|_| CaptureError::Poisoned)?;
        let msg = match regions {
            Some(r)
                if r.width == width
                    && r.height == height
                    && r.calibration.exp.fits_in(width, height)
                    && r.calibration.map.fits_in(width, height)
                    && r.calibration.level.is_none_or(|l| l.fits_in(width, height)) =>
            {
                CaptureMsg::Regions {
                    t: now_unix(),
                    exp: copy_region(frame, r.calibration.exp)?,
                    map: copy_region(frame, r.calibration.map)?,
                    level: r.calibration.level.map(|l| copy_region(frame, l)).transpose()?,
                }
            }
            _ => CaptureMsg::Uncalibrated { width, height },
        };
        match self.tx.try_send(msg) {
            // Full: the pipeline is behind; dropping one sample is correct (the next arrives in 1 s).
            Ok(()) | Err(TrySendError::Full(_)) => Ok(()),
            Err(TrySendError::Disconnected(_)) => {
                control.stop();
                Ok(())
            }
        }
    }

    fn on_closed(&mut self) -> Result<(), Self::Error> {
        // Best effort: the pipeline also notices via `Control::is_finished`.
        if self.tx.try_send(CaptureMsg::Closed).is_err() {
            // Channel full or pipeline gone; is_finished() covers both.
        }
        Ok(())
    }
}

/// Start capturing the primary monitor on a dedicated thread.
pub fn start(tx: SyncSender<CaptureMsg>, shared: Arc<Shared>) -> Result<Control, CaptureError> {
    let settings = Settings::new(
        Monitor::primary()?,
        CursorCaptureSettings::WithoutCursor,
        DrawBorderSettings::WithoutBorder,
        SecondaryWindowSettings::Default,
        MinimumUpdateIntervalSettings::Default,
        DirtyRegionSettings::Default,
        ColorFormat::Rgba8,
        Flags { tx, shared },
    );
    Handler::start_free_threaded(settings).map_err(|e| CaptureError::Start(format!("{e:?}")))
}
