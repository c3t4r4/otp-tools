use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use image::{ImageFormat, ImageReader, Limits};
use zeroize::Zeroize;

use crate::error::AppError;
use crate::model::{sanitize_terminal_text, QrClassification};
use crate::qrcode::{classify_qr_from_rgba_frame, MAX_IMAGE_DIMENSION, MAX_IMAGE_FILE_BYTES};

const QR_SCAN_INTERVAL: Duration = Duration::from_millis(120);
const PREVIEW_MAX_WIDTH: u32 = 1280;
const DECODE_ALLOC_LIMIT_BYTES: u64 = 128 * 1024 * 1024;

/// Quadro RGBA8 capturado pela câmera, zeroizado automaticamente ao ser descartado.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CameraFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Drop for CameraFrame {
    fn drop(&mut self) {
        self.rgba.zeroize();
    }
}

/// Eventos assíncronos emitidos pela sessão de captura de câmera.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CameraUpdate {
    Preview(CameraFrame),
    Decoded {
        frame: CameraFrame,
        items: Vec<QrClassification>,
    },
    Error(String),
}

struct FrameSink {
    capture_stop: Arc<AtomicBool>,
    latest_frame: Mutex<Option<CameraFrame>>,
    cv: Condvar,
}

impl FrameSink {
    fn new(capture_stop: Arc<AtomicBool>) -> Self {
        Self {
            capture_stop,
            latest_frame: Mutex::new(None),
            cv: Condvar::new(),
        }
    }

    fn push_frame(&self, frame: CameraFrame) {
        if self.capture_stop.load(Ordering::Relaxed) {
            return;
        }
        if let Ok(mut guard) = self.latest_frame.lock() {
            *guard = Some(frame);
            self.cv.notify_one();
        }
    }

    fn take_latest_timeout(&self, timeout: Duration) -> Option<CameraFrame> {
        let Ok(mut guard) = self.latest_frame.lock() else {
            return None;
        };
        if guard.is_none() && !self.capture_stop.load(Ordering::Relaxed) {
            if let Ok((new_guard, _)) = self.cv.wait_timeout(guard, timeout) {
                guard = new_guard;
            } else {
                return None;
            }
        }
        guard.take()
    }
}

/// Sessão assíncrona de captura de câmera com encerramento seguro e parada automática ao ler QR.
pub struct CameraSession {
    user_stopped: Arc<AtomicBool>,
    capture_stop: Arc<AtomicBool>,
    rx: Receiver<CameraUpdate>,
    worker: Option<JoinHandle<()>>,
}

impl Drop for CameraSession {
    fn drop(&mut self) {
        self.stop();
    }
}

impl CameraSession {
    /// Inicia a captura nativa da câmera em thread dedicada.
    pub fn start() -> Result<Self, AppError> {
        let user_stopped = Arc::new(AtomicBool::new(false));
        let capture_stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel::<CameraUpdate>(4);

        let worker_user_stopped = Arc::clone(&user_stopped);
        let worker_capture_stop = Arc::clone(&capture_stop);

        let worker = thread::Builder::new()
            .name("otp-tools-camera".to_string())
            .spawn(move || {
                if let Err(err) = run_native_camera(&worker_user_stopped, &worker_capture_stop, &tx)
                {
                    let _ = send_reliable(
                        &tx,
                        &worker_user_stopped,
                        CameraUpdate::Error(sanitize_terminal_text(&err.to_string())),
                    );
                }
            })
            .map_err(|err| {
                AppError::CameraError(format!("Falha ao iniciar thread da câmera: {err}"))
            })?;

        Ok(Self {
            user_stopped,
            capture_stop,
            rx,
            worker: Some(worker),
        })
    }

    /// Cria uma sessão simulada a partir de frames em memória (útil para testes sem hardware).
    pub fn from_mock_frames(frames: Vec<CameraFrame>) -> Self {
        let user_stopped = Arc::new(AtomicBool::new(false));
        let capture_stop = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel::<CameraUpdate>(8);

        let worker_user_stopped = Arc::clone(&user_stopped);
        let worker_capture_stop = Arc::clone(&capture_stop);

        let worker = thread::Builder::new()
            .name("otp-tools-camera-mock".to_string())
            .spawn(move || {
                let mut queue: VecDeque<CameraFrame> = frames.into_iter().collect();
                while let Some(frame) = queue.pop_front() {
                    if worker_user_stopped.load(Ordering::Acquire)
                        || worker_capture_stop.load(Ordering::Acquire)
                    {
                        break;
                    }

                    let _ = tx.try_send(CameraUpdate::Preview(frame.clone()));
                    if let Ok(items) =
                        classify_qr_from_rgba_frame(frame.width, frame.height, &frame.rgba)
                    {
                        if !items.is_empty() {
                            worker_capture_stop.store(true, Ordering::SeqCst);
                            let _ = send_reliable(
                                &tx,
                                &worker_user_stopped,
                                CameraUpdate::Decoded { frame, items },
                            );
                            return;
                        }
                    }

                    thread::sleep(Duration::from_millis(10));
                }

                if !worker_user_stopped.load(Ordering::Acquire) {
                    let _ = send_reliable(
                        &tx,
                        &worker_user_stopped,
                        CameraUpdate::Error(
                            "Nenhum QR Code detectado nos quadros simulados.".to_string(),
                        ),
                    );
                }
            })
            .ok();

        Self {
            user_stopped,
            capture_stop,
            rx,
            worker,
        }
    }

    /// Coleta todas as atualizações pendentes sem bloquear a interface gráfica.
    pub fn poll(&self) -> Vec<CameraUpdate> {
        let mut updates = Vec::new();
        while let Ok(item) = self.rx.try_recv() {
            updates.push(item);
        }
        updates
    }

    /// Interrompe imediatamente a captura e aguarda o encerramento da thread da câmera.
    pub fn stop(&mut self) {
        self.user_stopped.store(true, Ordering::SeqCst);
        self.capture_stop.store(true, Ordering::SeqCst);
        while self.rx.try_recv().is_ok() {}
        if let Some(handle) = self.worker.take() {
            let _ = handle.join();
        }
    }
}

/// Captura frames da câmera de forma síncrona (para o CLI) até ler um QR Code ou expirar o timeout.
pub fn scan_qr_from_camera_blocking(timeout: Duration) -> Result<Vec<QrClassification>, AppError> {
    if timeout.is_zero() {
        return Err(AppError::CameraError(
            "O tempo limite para captura pela câmera deve ser maior que zero.".to_string(),
        ));
    }
    let deadline = Instant::now().checked_add(timeout).ok_or_else(|| {
        AppError::CameraError(
            "Tempo limite inválido para captura pela câmera (excede o limite do sistema)."
                .to_string(),
        )
    })?;

    let mut session = CameraSession::start()?;

    loop {
        for update in session.poll() {
            match update {
                CameraUpdate::Preview(_) => {}
                CameraUpdate::Decoded { items, .. } => {
                    session.stop();
                    return Ok(items);
                }
                CameraUpdate::Error(msg) => {
                    session.stop();
                    return Err(AppError::CameraError(msg));
                }
            }
        }

        if Instant::now() >= deadline {
            session.stop();
            return Err(AppError::CameraError(format!(
                "Tempo esgotado ({}s) sem detectar nenhum QR Code na câmera.",
                timeout.as_secs()
            )));
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn send_reliable(
    tx: &SyncSender<CameraUpdate>,
    user_stopped: &AtomicBool,
    mut update: CameraUpdate,
) -> bool {
    loop {
        if user_stopped.load(Ordering::Acquire) {
            return false;
        }
        match tx.try_send(update) {
            Ok(()) => return true,
            Err(TrySendError::Full(returned)) => {
                update = returned;
                thread::sleep(Duration::from_millis(15));
            }
            Err(TrySendError::Disconnected(_)) => return false,
        }
    }
}

#[cfg_attr(
    not(any(target_os = "macos", target_os = "linux", target_os = "windows")),
    allow(dead_code)
)]
fn run_qr_scan_loop(
    sink: &FrameSink,
    user_stopped: &AtomicBool,
    capture_stop: &AtomicBool,
    tx: &SyncSender<CameraUpdate>,
) {
    let mut last_scan: Option<Instant> = None;

    while !user_stopped.load(Ordering::Acquire) && !capture_stop.load(Ordering::Acquire) {
        let Some(frame) = sink.take_latest_timeout(Duration::from_millis(80)) else {
            continue;
        };

        if user_stopped.load(Ordering::Acquire) || capture_stop.load(Ordering::Acquire) {
            break;
        }

        let _ = tx.try_send(CameraUpdate::Preview(frame.clone()));

        if last_scan.is_none_or(|t| t.elapsed() >= QR_SCAN_INTERVAL) {
            last_scan = Some(Instant::now());
            if let Ok(items) = classify_qr_from_rgba_frame(frame.width, frame.height, &frame.rgba) {
                if !items.is_empty() {
                    capture_stop.store(true, Ordering::SeqCst);
                    let _ = send_reliable(tx, user_stopped, CameraUpdate::Decoded { frame, items });
                    break;
                }
            }
        }
    }
}

/// Converte um buffer `BGRA` com padding de `stride` para `CameraFrame` RGBA8,
/// aplicando redução inteira caso a largura exceda `max_width`.
pub fn bgra_stride_to_rgba(
    width: u32,
    height: u32,
    stride: usize,
    bgra: &[u8],
    max_width: u32,
) -> Result<CameraFrame, AppError> {
    if width == 0 || height == 0 {
        return Err(AppError::CameraError(
            "Dimensões inválidas do quadro da câmera.".to_string(),
        ));
    }
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        });
    }

    let min_row_bytes = (width as usize)
        .checked_mul(4)
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;
    if stride < min_row_bytes {
        return Err(AppError::CameraError(
            "Stride do quadro BGRA menor que largura * 4.".to_string(),
        ));
    }

    let required_bytes = (height as usize - 1)
        .checked_mul(stride)
        .and_then(|prefix| prefix.checked_add(min_row_bytes))
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;
    if bgra.len() < required_bytes {
        return Err(AppError::CameraError(
            "Buffer BGRA da câmera menor que o esperado.".to_string(),
        ));
    }

    let step: usize = if max_width > 0 && width > max_width {
        ((width as usize) / (max_width as usize)).max(2)
    } else {
        1
    };

    let out_w = (width as usize).div_ceil(step);
    let out_h = (height as usize).div_ceil(step);
    let out_bytes = out_w
        .checked_mul(out_h)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|&bytes| bytes as u64 <= DECODE_ALLOC_LIMIT_BYTES)
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;
    let mut rgba = Vec::with_capacity(out_bytes);

    for y in (0..height as usize).step_by(step) {
        let row_offset = y * stride;
        for x in (0..width as usize).step_by(step) {
            let idx = row_offset + x * 4;
            rgba.push(bgra[idx + 2]);
            rgba.push(bgra[idx + 1]);
            rgba.push(bgra[idx]);
            rgba.push(255);
        }
    }

    Ok(CameraFrame {
        width: out_w as u32,
        height: out_h as u32,
        rgba,
    })
}

/// Converte um quadro `YUYV` (`4:2:2` empacotado, 2 pixels em 4 bytes `[Y0, U, Y1, V]`) para RGBA8.
pub fn yuyv_to_rgba(width: u32, height: u32, yuyv: &[u8]) -> Result<CameraFrame, AppError> {
    if width == 0 || height == 0 || !width.is_multiple_of(2) {
        return Err(AppError::CameraError(
            "Dimensões inválidas para quadro YUYV.".to_string(),
        ));
    }
    let min_stride = (width as usize)
        .checked_mul(2)
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;
    yuyv_stride_to_rgba(width, height, min_stride, yuyv)
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn yuyv_stride_to_rgba(
    width: u32,
    height: u32,
    stride: usize,
    yuyv: &[u8],
) -> Result<CameraFrame, AppError> {
    if width == 0 || height == 0 || !width.is_multiple_of(2) {
        return Err(AppError::CameraError(
            "Dimensões inválidas para quadro YUYV.".to_string(),
        ));
    }
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        });
    }

    let min_row_bytes = (width as usize)
        .checked_mul(2)
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;
    let effective_stride = stride.max(min_row_bytes);
    let expected_len = (height as usize - 1)
        .checked_mul(effective_stride)
        .and_then(|prefix| prefix.checked_add(min_row_bytes))
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;
    if yuyv.len() < expected_len {
        return Err(AppError::CameraError(
            "Buffer YUYV da câmera menor que o esperado.".to_string(),
        ));
    }

    let step: usize = if width > PREVIEW_MAX_WIDTH { 2 } else { 1 };
    if step == 1 {
        let out_bytes = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .filter(|&bytes| bytes as u64 <= DECODE_ALLOC_LIMIT_BYTES)
            .ok_or(AppError::ImageTooLarge {
                max_dimension: MAX_IMAGE_DIMENSION,
            })?;
        let mut rgba = Vec::with_capacity(out_bytes);
        for y in 0..height as usize {
            let start = y * effective_stride;
            let row = &yuyv[start..start + min_row_bytes];
            for chunk in row.chunks_exact(4) {
                let [y0, u, y1, v] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                rgba.extend_from_slice(&yuv_to_rgba_pixel(y0, u, v));
                rgba.extend_from_slice(&yuv_to_rgba_pixel(y1, u, v));
            }
        }
        Ok(CameraFrame {
            width,
            height,
            rgba,
        })
    } else {
        let out_w = (width as usize).div_ceil(2);
        let out_h = (height as usize).div_ceil(2);
        let out_bytes = out_w
            .checked_mul(out_h)
            .and_then(|pixels| pixels.checked_mul(4))
            .filter(|&bytes| bytes as u64 <= DECODE_ALLOC_LIMIT_BYTES)
            .ok_or(AppError::ImageTooLarge {
                max_dimension: MAX_IMAGE_DIMENSION,
            })?;
        let mut rgba = Vec::with_capacity(out_bytes);
        for y in (0..height as usize).step_by(2) {
            let start = y * effective_stride;
            let row = &yuyv[start..start + min_row_bytes];
            for chunk in row.chunks_exact(4) {
                let [y0, u, _, v] = [chunk[0], chunk[1], chunk[2], chunk[3]];
                rgba.extend_from_slice(&yuv_to_rgba_pixel(y0, u, v));
            }
        }
        Ok(CameraFrame {
            width: out_w as u32,
            height: out_h as u32,
            rgba,
        })
    }
}

fn yuv_to_rgba_pixel(y: u8, u: u8, v: u8) -> [u8; 4] {
    let c = i32::from(y) - 16;
    let d = i32::from(u) - 128;
    let e = i32::from(v) - 128;
    let r = (298 * c + 409 * e + 128) >> 8;
    let g = (298 * c - 100 * d - 208 * e + 128) >> 8;
    let b = (298 * c + 516 * d + 128) >> 8;
    [
        r.clamp(0, 255) as u8,
        g.clamp(0, 255) as u8,
        b.clamp(0, 255) as u8,
        255,
    ]
}

/// Converte um DIB Windows `BGR24` bottom-up (com stride alinhado a 4 bytes) para RGBA8 top-down.
pub fn bgr24_bottom_up_to_rgba(
    width: u32,
    height: u32,
    bgr: &[u8],
) -> Result<CameraFrame, AppError> {
    if width == 0 || height == 0 {
        return Err(AppError::CameraError(
            "Dimensões inválidas para quadro BGR24.".to_string(),
        ));
    }
    if width > MAX_IMAGE_DIMENSION || height > MAX_IMAGE_DIMENSION {
        return Err(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        });
    }

    let raw_row_bytes = (width as usize)
        .checked_mul(3)
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;
    let stride = raw_row_bytes.next_multiple_of(4);
    let required_len = stride
        .checked_mul(height as usize)
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;
    if bgr.len() < required_len {
        return Err(AppError::CameraError(
            "Buffer BGR24 da câmera menor que o esperado.".to_string(),
        ));
    }

    let out_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .filter(|&bytes| bytes as u64 <= DECODE_ALLOC_LIMIT_BYTES)
        .ok_or(AppError::ImageTooLarge {
            max_dimension: MAX_IMAGE_DIMENSION,
        })?;
    let mut rgba = Vec::with_capacity(out_bytes);
    for y in 0..height as usize {
        let src_y = height as usize - 1 - y;
        let row_start = src_y * stride;
        for x in 0..width as usize {
            let idx = row_start + x * 3;
            rgba.push(bgr[idx + 2]);
            rgba.push(bgr[idx + 1]);
            rgba.push(bgr[idx]);
            rgba.push(255);
        }
    }

    Ok(CameraFrame {
        width,
        height,
        rgba,
    })
}

/// Decodifica um frame `MJPEG` recebido da câmera aplicando limites rígidos de memória e dimensão.
pub fn mjpeg_to_rgba(jpeg_bytes: &[u8]) -> Result<CameraFrame, AppError> {
    if jpeg_bytes.len() as u64 > MAX_IMAGE_FILE_BYTES {
        return Err(AppError::CameraError(
            "Quadro MJPEG excede o limite de memória permitido.".to_string(),
        ));
    }

    let mut reader = ImageReader::with_format(Cursor::new(jpeg_bytes), ImageFormat::Jpeg);
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(DECODE_ALLOC_LIMIT_BYTES);
    reader.limits(limits);

    let dynamic = reader.decode().map_err(|err| {
        AppError::CameraError(format!("Falha ao decodificar quadro MJPEG: {err}"))
    })?;
    let rgba_img = dynamic.to_rgba8();
    let (width, height) = rgba_img.dimensions();
    Ok(CameraFrame {
        width,
        height,
        rgba: rgba_img.into_raw(),
    })
}

fn run_native_camera(
    user_stopped: &Arc<AtomicBool>,
    capture_stop: &Arc<AtomicBool>,
    tx: &SyncSender<CameraUpdate>,
) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    {
        macos_camera::run(user_stopped, capture_stop, tx)
    }

    #[cfg(target_os = "linux")]
    {
        linux_camera::run(user_stopped, capture_stop, tx)
    }

    #[cfg(target_os = "windows")]
    {
        windows_camera::run(user_stopped, capture_stop, tx)
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let _ = (user_stopped, capture_stop, tx);
        Err(AppError::CameraError(
            "Captura por câmera não suportada neste sistema operacional.".to_string(),
        ))
    }
}

#[cfg(target_os = "macos")]
#[allow(unexpected_cfgs)]
mod macos_camera {
    use std::collections::HashMap;
    use std::ffi::{c_void, CStr, CString};
    use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
    use std::sync::mpsc::SyncSender;
    use std::sync::{Arc, Mutex, Once};
    use std::thread;
    use std::time::Duration;

    use objc::declare::ClassDecl;
    use objc::runtime::{Class, Object, Sel, BOOL, NO, YES};
    use objc::{class, msg_send, sel, sel_impl};

    use super::{
        bgra_stride_to_rgba, run_qr_scan_loop, CameraUpdate, FrameSink, PREVIEW_MAX_WIDTH,
    };
    use crate::error::AppError;

    const K_CV_PIXEL_FORMAT_TYPE_32BGRA: u32 = 0x4247_5241;
    const K_CV_PIXEL_BUFFER_LOCK_READ_ONLY: u64 = 1;

    #[link(name = "AVFoundation", kind = "framework")]
    extern "C" {}
    #[link(name = "CoreMedia", kind = "framework")]
    extern "C" {}
    #[link(name = "CoreVideo", kind = "framework")]
    extern "C" {}
    #[link(name = "Foundation", kind = "framework")]
    extern "C" {}

    extern "C" {
        static AVMediaTypeVideo: *mut Object;
        static AVCaptureSessionPreset1280x720: *mut Object;
        static AVCaptureSessionPreset640x480: *mut Object;
        static kCVPixelBufferPixelFormatTypeKey: *const c_void;
        static _NSConcreteStackBlock: [*const c_void; 32];

        fn _Block_copy(a_block: *const c_void) -> *mut c_void;
        fn _Block_release(a_block: *const c_void);

        fn CMSampleBufferGetImageBuffer(sbuf: *const c_void) -> *mut c_void;
        fn CVPixelBufferLockBaseAddress(pixel_buffer: *mut c_void, lock_flags: u64) -> i32;
        fn CVPixelBufferUnlockBaseAddress(pixel_buffer: *mut c_void, unlock_flags: u64) -> i32;
        fn CVPixelBufferGetPixelFormatType(pixel_buffer: *mut c_void) -> u32;
        fn CVPixelBufferGetWidth(pixel_buffer: *mut c_void) -> usize;
        fn CVPixelBufferGetHeight(pixel_buffer: *mut c_void) -> usize;
        fn CVPixelBufferGetBytesPerRow(pixel_buffer: *mut c_void) -> usize;
        fn CVPixelBufferGetBaseAddress(pixel_buffer: *mut c_void) -> *mut c_void;

        fn dispatch_queue_create(
            label: *const std::ffi::c_char,
            attr: *const c_void,
        ) -> *mut c_void;
        fn dispatch_release(object: *mut c_void);
        fn dispatch_sync_f(
            queue: *mut c_void,
            context: *mut c_void,
            work: extern "C" fn(*mut c_void),
        );
    }

    static REGISTER_DELEGATE: Once = Once::new();
    static NEXT_AUTH_REQ_ID: AtomicU64 = AtomicU64::new(1);
    static AUTH_SENDERS: Mutex<Option<HashMap<u64, std::sync::mpsc::SyncSender<bool>>>> =
        Mutex::new(None);

    #[repr(C)]
    struct BlockDescriptor {
        reserved: libc::c_ulong,
        size: libc::c_ulong,
    }

    #[repr(C)]
    struct AccessBlock {
        isa: *const c_void,
        flags: libc::c_int,
        reserved: libc::c_int,
        invoke: extern "C" fn(*mut AccessBlock, BOOL),
        descriptor: *const BlockDescriptor,
        req_id: u64,
    }

    static ACCESS_BLOCK_DESCRIPTOR: BlockDescriptor = BlockDescriptor {
        reserved: 0,
        size: std::mem::size_of::<AccessBlock>() as libc::c_ulong,
    };

    extern "C" fn access_block_invoke(block: *mut AccessBlock, granted: BOOL) {
        if block.is_null() {
            return;
        }
        // SAFETY: `block` segue o layout `AccessBlock` passado a `requestAccessForMediaType`.
        let req_id = unsafe { (*block).req_id };
        let sender = AUTH_SENDERS
            .lock()
            .ok()
            .and_then(|mut map| map.as_mut().and_then(|m| m.remove(&req_id)));
        if let Some(tx) = sender {
            let _ = tx.send(granted == YES);
        }
    }

    extern "C" fn noop_dispatch(_ctx: *mut c_void) {}

    struct CopiedBlockGuard(*mut c_void);

    impl Drop for CopiedBlockGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    _Block_release(self.0);
                }
            }
        }
    }

    struct AutoreleasePoolGuard(*mut Object);

    impl AutoreleasePoolGuard {
        unsafe fn new() -> Self {
            let pool: *mut Object = msg_send![class!(NSAutoreleasePool), new];
            Self(pool)
        }
    }

    impl Drop for AutoreleasePoolGuard {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe {
                    let _: () = msg_send![self.0, drain];
                }
            }
        }
    }

    struct CaptureCleanupGuard {
        session: *mut Object,
        output: *mut Object,
        delegate: *mut Object,
        queue: *mut c_void,
        sink_ptr: *const FrameSink,
    }

    impl Drop for CaptureCleanupGuard {
        fn drop(&mut self) {
            unsafe {
                if !self.output.is_null() {
                    let nil: *mut Object = std::ptr::null_mut();
                    let nil_q: *mut c_void = std::ptr::null_mut();
                    let _: () = msg_send![self.output, setSampleBufferDelegate: nil queue: nil_q];
                }
                if !self.session.is_null() {
                    let _: () = msg_send![self.session, stopRunning];
                }
                if !self.queue.is_null() {
                    dispatch_sync_f(self.queue, std::ptr::null_mut(), noop_dispatch);
                }
                if !self.delegate.is_null() {
                    let empty: *mut c_void = std::ptr::null_mut();
                    (*self.delegate).set_ivar("_sink", empty);
                    let _: () = msg_send![self.delegate, release];
                }
                if !self.output.is_null() {
                    let _: () = msg_send![self.output, release];
                }
                if !self.session.is_null() {
                    let _: () = msg_send![self.session, release];
                }
                if !self.queue.is_null() {
                    dispatch_release(self.queue);
                }
                if !self.sink_ptr.is_null() {
                    drop(Arc::from_raw(self.sink_ptr));
                }
            }
        }
    }

    fn ensure_camera_authorization(
        user_stopped: &Arc<AtomicBool>,
        capture_stop: &Arc<AtomicBool>,
    ) -> Result<(), AppError> {
        // SAFETY: chamada de método de classe Cocoa em `AVCaptureDevice`.
        let status: isize = unsafe {
            msg_send![
                class!(AVCaptureDevice),
                authorizationStatusForMediaType: AVMediaTypeVideo
            ]
        };

        match status {
            3 => Ok(()), // AVAuthorizationStatusAuthorized
            1 | 2 => Err(AppError::CameraError(
                "Permissão de câmera negada ou restrita no macOS. Habilite o acesso em Ajustes do Sistema > Privacidade e Segurança > Câmera."
                    .to_string(),
            )),
            0 => {
                // AVAuthorizationStatusNotDetermined
                let (auth_tx, auth_rx) = std::sync::mpsc::sync_channel::<bool>(1);
                let req_id = NEXT_AUTH_REQ_ID.fetch_add(1, Ordering::Relaxed);
                if let Ok(mut guard) = AUTH_SENDERS.lock() {
                    let map = guard.get_or_insert_with(HashMap::new);
                    map.insert(req_id, auth_tx);
                } else {
                    return Err(AppError::CameraError(
                        "Falha interna ao coordenar permissão de câmera.".to_string(),
                    ));
                }

                // SAFETY: `AccessBlock` segue exatamente o ABI de Objective-C blocks no macOS
                // (`&_NSConcreteStackBlock` aponta para `__NSStackBlock__`), e `_Block_copy`
                // move o block POD para o heap (`_NSConcreteMallocBlock`) antes da chamada assíncrona.
                let block = AccessBlock {
                    isa: std::ptr::addr_of!(_NSConcreteStackBlock).cast::<c_void>(),
                    flags: 0,
                    reserved: 0,
                    invoke: access_block_invoke,
                    descriptor: &ACCESS_BLOCK_DESCRIPTOR,
                    req_id,
                };
                unsafe {
                    let copied_block = _Block_copy((&raw const block).cast::<c_void>());
                    if copied_block.is_null() {
                        if let Ok(mut guard) = AUTH_SENDERS.lock() {
                            if let Some(map) = guard.as_mut() {
                                map.remove(&req_id);
                            }
                        }
                        return Err(AppError::CameraError(
                            "Falha ao copiar completion handler para permissão de câmera."
                                .to_string(),
                        ));
                    }
                    let _block_guard = CopiedBlockGuard(copied_block);
                    let _: () = msg_send![
                        class!(AVCaptureDevice),
                        requestAccessForMediaType: AVMediaTypeVideo
                        completionHandler: copied_block
                    ];
                }

                for _ in 0..300 {
                    if user_stopped.load(Ordering::Acquire) || capture_stop.load(Ordering::Acquire)
                    {
                        if let Ok(mut guard) = AUTH_SENDERS.lock() {
                            if let Some(map) = guard.as_mut() {
                                map.remove(&req_id);
                            }
                        }
                        return Err(AppError::CameraError(
                            "Captura cancelada durante a solicitação de permissão.".to_string(),
                        ));
                    }
                    match auth_rx.try_recv() {
                        Ok(true) => return Ok(()),
                        Ok(false) => {
                            return Err(AppError::CameraError(
                                "Permissão de câmera recusada pelo usuário.".to_string(),
                            ));
                        }
                        Err(std::sync::mpsc::TryRecvError::Empty) => {
                            thread::sleep(Duration::from_millis(100));
                        }
                        Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
                    }
                }

                if let Ok(mut guard) = AUTH_SENDERS.lock() {
                    if let Some(map) = guard.as_mut() {
                        map.remove(&req_id);
                    }
                }
                Err(AppError::CameraError(
                    "Tempo esgotado aguardando permissão de câmera no macOS.".to_string(),
                ))
            }
            other => Err(AppError::CameraError(format!(
                "Status desconhecido de autorização da câmera: {other}"
            ))),
        }
    }

    extern "C" fn did_output_sample_buffer(
        this: &Object,
        _cmd: Sel,
        _output: *mut Object,
        sample_buffer: *const c_void,
        _connection: *mut Object,
    ) {
        if sample_buffer.is_null() {
            return;
        }
        let sink_ptr: *mut c_void = unsafe { *this.get_ivar("_sink") };
        if sink_ptr.is_null() {
            return;
        }
        // SAFETY: `_sink` permanece válido enquanto o guard estiver vivo (e é zerado antes do release).
        let sink = unsafe { &*(sink_ptr as *const FrameSink) };
        if sink.capture_stop.load(Ordering::Relaxed) {
            return;
        }

        // SAFETY: `sample_buffer` é fornecido por AVFoundation dentro do callback.
        unsafe {
            let pixel_buffer = CMSampleBufferGetImageBuffer(sample_buffer);
            if pixel_buffer.is_null() {
                return;
            }
            if CVPixelBufferLockBaseAddress(pixel_buffer, K_CV_PIXEL_BUFFER_LOCK_READ_ONLY) != 0 {
                return;
            }

            let width = CVPixelBufferGetWidth(pixel_buffer);
            let height = CVPixelBufferGetHeight(pixel_buffer);
            let stride = CVPixelBufferGetBytesPerRow(pixel_buffer);
            let pix_fmt = CVPixelBufferGetPixelFormatType(pixel_buffer);
            let base = CVPixelBufferGetBaseAddress(pixel_buffer);

            if pix_fmt == K_CV_PIXEL_FORMAT_TYPE_32BGRA
                && !base.is_null()
                && width > 0
                && height > 0
            {
                if let (Ok(w_u32), Ok(h_u32), Some(min_row_bytes)) = (
                    u32::try_from(width),
                    u32::try_from(height),
                    width.checked_mul(4),
                ) {
                    if stride >= min_row_bytes {
                        if let Some(total_bytes) = (height - 1)
                            .checked_mul(stride)
                            .and_then(|prefix| prefix.checked_add(min_row_bytes))
                        {
                            let slice = std::slice::from_raw_parts(base as *const u8, total_bytes);
                            if let Ok(frame) =
                                bgra_stride_to_rgba(w_u32, h_u32, stride, slice, PREVIEW_MAX_WIDTH)
                            {
                                sink.push_frame(frame);
                            }
                        }
                    }
                }
            }

            let _ = CVPixelBufferUnlockBaseAddress(pixel_buffer, K_CV_PIXEL_BUFFER_LOCK_READ_ONLY);
        }
    }

    fn delegate_class() -> Result<&'static Class, AppError> {
        REGISTER_DELEGATE.call_once(|| {
            if let Some(mut decl) = ClassDecl::new("OtpToolsAvCaptureDelegate", class!(NSObject)) {
                decl.add_ivar::<*mut c_void>("_sink");
                unsafe {
                    decl.add_method(
                        sel!(captureOutput:didOutputSampleBuffer:fromConnection:),
                        did_output_sample_buffer
                            as extern "C" fn(&Object, Sel, *mut Object, *const c_void, *mut Object),
                    );
                }
                decl.register();
            }
        });

        Class::get("OtpToolsAvCaptureDelegate").ok_or_else(|| {
            AppError::CameraError("Falha ao registrar delegate AVFoundation.".to_string())
        })
    }

    pub fn run(
        user_stopped: &Arc<AtomicBool>,
        capture_stop: &Arc<AtomicBool>,
        tx: &SyncSender<CameraUpdate>,
    ) -> Result<(), AppError> {
        let _pool = unsafe { AutoreleasePoolGuard::new() };
        ensure_camera_authorization(user_stopped, capture_stop)?;

        let delegate_cls = delegate_class()?;
        let sink = Arc::new(FrameSink::new(Arc::clone(capture_stop)));
        let sink_ptr = Arc::into_raw(Arc::clone(&sink));

        let queue_label = CString::new("com.otp-tools.camera-capture").map_err(|_| {
            unsafe { drop(Arc::from_raw(sink_ptr)) };
            AppError::CameraError("Rótulo de fila inválido.".to_string())
        })?;
        let queue = unsafe { dispatch_queue_create(queue_label.as_ptr(), std::ptr::null()) };
        if queue.is_null() {
            unsafe { drop(Arc::from_raw(sink_ptr)) };
            return Err(AppError::CameraError(
                "Falha ao criar fila de vídeo no macOS.".to_string(),
            ));
        }

        let mut guard = CaptureCleanupGuard {
            session: std::ptr::null_mut(),
            output: std::ptr::null_mut(),
            delegate: std::ptr::null_mut(),
            queue,
            sink_ptr,
        };

        unsafe {
            let device: *mut Object = msg_send![
                class!(AVCaptureDevice),
                defaultDeviceWithMediaType: AVMediaTypeVideo
            ];
            if device.is_null() {
                return Err(AppError::CameraError(
                    "Nenhuma câmera/webcam disponível encontrada no macOS.".to_string(),
                ));
            }

            let mut ns_err: *mut Object = std::ptr::null_mut();
            let input: *mut Object = msg_send![
                class!(AVCaptureDeviceInput),
                deviceInputWithDevice: device
                error: &mut ns_err
            ];
            if input.is_null() || !ns_err.is_null() {
                let err_detail = if !ns_err.is_null() {
                    let desc: *mut Object = msg_send![ns_err, localizedDescription];
                    let utf8: *const std::ffi::c_char = msg_send![desc, UTF8String];
                    if utf8.is_null() {
                        "erro desconhecido".to_string()
                    } else {
                        CStr::from_ptr(utf8).to_string_lossy().into_owned()
                    }
                } else {
                    "dispositivo indisponível".to_string()
                };
                return Err(AppError::CameraError(format!(
                    "Não foi possível abrir a câmera: {err_detail}"
                )));
            }

            let session: *mut Object = msg_send![class!(AVCaptureSession), new];
            if session.is_null() {
                return Err(AppError::CameraError(
                    "Falha ao criar AVCaptureSession.".to_string(),
                ));
            }
            guard.session = session;

            let can_hd: BOOL =
                msg_send![session, canSetSessionPreset: AVCaptureSessionPreset1280x720];
            if can_hd == YES {
                let _: () = msg_send![session, setSessionPreset: AVCaptureSessionPreset1280x720];
            } else {
                let can_vga: BOOL =
                    msg_send![session, canSetSessionPreset: AVCaptureSessionPreset640x480];
                if can_vga == YES {
                    let _: () = msg_send![session, setSessionPreset: AVCaptureSessionPreset640x480];
                }
            }

            let can_input: BOOL = msg_send![session, canAddInput: input];
            if can_input != YES {
                return Err(AppError::CameraError(
                    "Não foi possível conectar o dispositivo à sessão de captura.".to_string(),
                ));
            }
            let _: () = msg_send![session, addInput: input];

            let output: *mut Object = msg_send![class!(AVCaptureVideoDataOutput), new];
            if output.is_null() {
                return Err(AppError::CameraError(
                    "Falha ao criar AVCaptureVideoDataOutput.".to_string(),
                ));
            }
            guard.output = output;
            let _: () = msg_send![output, setAlwaysDiscardsLateVideoFrames: YES];

            let format_num: *mut Object =
                msg_send![class!(NSNumber), numberWithUnsignedInt: K_CV_PIXEL_FORMAT_TYPE_32BGRA];
            let settings: *mut Object = msg_send![
                class!(NSDictionary),
                dictionaryWithObject: format_num
                forKey: kCVPixelBufferPixelFormatTypeKey
            ];
            let _: () = msg_send![output, setVideoSettings: settings];

            let can_output: BOOL = msg_send![session, canAddOutput: output];
            if can_output != YES {
                return Err(AppError::CameraError(
                    "Não foi possível configurar saída de vídeo BGRA.".to_string(),
                ));
            }
            let _: () = msg_send![session, addOutput: output];

            let delegate: *mut Object = msg_send![delegate_cls, new];
            if delegate.is_null() {
                return Err(AppError::CameraError(
                    "Falha ao instanciar delegate de vídeo.".to_string(),
                ));
            }
            (*delegate).set_ivar("_sink", sink_ptr as *mut c_void);
            guard.delegate = delegate;

            let _: () = msg_send![output, setSampleBufferDelegate: delegate queue: queue];
            let _: () = msg_send![session, startRunning];

            let running: BOOL = msg_send![session, isRunning];
            if running != YES {
                let _ = NO;
                return Err(AppError::CameraError(
                    "A câmera não conseguiu iniciar a captura.".to_string(),
                ));
            }
        }

        run_qr_scan_loop(&sink, user_stopped, capture_stop, tx);
        drop(guard);
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn stack_block_isa_matches_nsstackblock_class_and_block_copy_works() {
            let stack_isa = std::ptr::addr_of!(_NSConcreteStackBlock).cast::<c_void>();
            let objc_stack_class = Class::get("__NSStackBlock__")
                .expect("classe __NSStackBlock__ deve existir no runtime macOS");
            assert_eq!(
                stack_isa,
                (objc_stack_class as *const Class).cast::<c_void>()
            );

            let block = AccessBlock {
                isa: stack_isa,
                flags: 0,
                reserved: 0,
                invoke: access_block_invoke,
                descriptor: &ACCESS_BLOCK_DESCRIPTOR,
                req_id: 999_999,
            };
            unsafe {
                let copied = _Block_copy((&raw const block).cast::<c_void>());
                assert!(!copied.is_null());
                let _guard = CopiedBlockGuard(copied);
                let copied_cls: *const Class = msg_send![copied as *mut Object, class];
                let expected_malloc =
                    Class::get("__NSMallocBlock__").expect("classe __NSMallocBlock__ deve existir");
                assert_eq!(
                    copied_cls.cast::<c_void>(),
                    (expected_malloc as *const Class).cast::<c_void>()
                );
            }
        }
    }
}

#[cfg(target_os = "linux")]
mod linux_camera {
    use std::ffi::{c_void, CString};
    use std::mem::{size_of, zeroed};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::SyncSender;
    use std::sync::Arc;
    use std::thread;
    use std::time::{Duration, Instant};

    use super::{
        mjpeg_to_rgba, send_reliable, yuyv_stride_to_rgba, CameraUpdate, QR_SCAN_INTERVAL,
    };
    use crate::error::AppError;
    use crate::qrcode::classify_qr_from_rgba_frame;

    const V4L2_BUF_TYPE_VIDEO_CAPTURE: u32 = 1;
    const V4L2_MEMORY_MMAP: u32 = 1;
    const V4L2_FIELD_ANY: u32 = 0;
    const V4L2_CAP_VIDEO_CAPTURE: u32 = 0x0000_0001;
    const V4L2_CAP_STREAMING: u32 = 0x0400_0000;
    const V4L2_CAP_DEVICE_CAPS: u32 = 0x8000_0000;

    const fn v4l2_fourcc(a: u8, b: u8, c: u8, d: u8) -> u32 {
        (a as u32) | ((b as u32) << 8) | ((c as u32) << 16) | ((d as u32) << 24)
    }

    const V4L2_PIX_FMT_YUYV: u32 = v4l2_fourcc(b'Y', b'U', b'Y', b'V');
    const V4L2_PIX_FMT_MJPEG: u32 = v4l2_fourcc(b'M', b'J', b'P', b'G');

    const IOC_WRITE: u32 = 1;
    const IOC_READ: u32 = 2;

    const fn ioc(dir: u32, kind: u8, nr: u8, size: usize) -> u32 {
        (dir << 30) | ((size as u32) << 16) | ((kind as u32) << 8) | (nr as u32)
    }

    #[repr(C)]
    struct V4l2Capability {
        driver: [u8; 16],
        card: [u8; 32],
        bus_info: [u8; 32],
        version: u32,
        capabilities: u32,
        device_caps: u32,
        reserved: [u32; 3],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct V4l2PixFormat {
        width: u32,
        height: u32,
        pixelformat: u32,
        field: u32,
        bytesperline: u32,
        sizeimage: u32,
        colorspace: u32,
        priv_: u32,
        flags: u32,
        ycbcr_enc: u32,
        quantization: u32,
        xfer_func: u32,
    }

    #[repr(C)]
    union V4l2FormatUnion {
        pix: V4l2PixFormat,
        _align_ptr: *const u8,
        raw_data: [u8; 200],
    }

    #[repr(C)]
    struct V4l2Format {
        type_: u32,
        fmt: V4l2FormatUnion,
    }

    #[repr(C)]
    struct V4l2RequestBuffers {
        count: u32,
        type_: u32,
        memory: u32,
        capabilities: u32,
        flags: u8,
        reserved: [u8; 3],
    }

    #[repr(C)]
    struct V4l2Timecode {
        type_: u32,
        flags: u32,
        frames: u8,
        seconds: u8,
        minutes: u8,
        hours: u8,
        userbits: [u8; 4],
    }

    #[repr(C)]
    union V4l2BufferM {
        offset: u32,
        userptr: libc::c_ulong,
        fd: i32,
    }

    #[repr(C)]
    union V4l2BufferTail {
        request_fd: i32,
        reserved: u32,
    }

    #[repr(C)]
    struct V4l2Buffer {
        index: u32,
        type_: u32,
        bytesused: u32,
        flags: u32,
        field: u32,
        timestamp: libc::timeval,
        timecode: V4l2Timecode,
        sequence: u32,
        memory: u32,
        m: V4l2BufferM,
        length: u32,
        reserved2: u32,
        tail: V4l2BufferTail,
    }

    const VIDIOC_QUERYCAP: u32 = ioc(IOC_READ, b'V', 0, size_of::<V4l2Capability>());
    const VIDIOC_S_FMT: u32 = ioc(IOC_READ | IOC_WRITE, b'V', 5, size_of::<V4l2Format>());
    const VIDIOC_REQBUFS: u32 = ioc(
        IOC_READ | IOC_WRITE,
        b'V',
        8,
        size_of::<V4l2RequestBuffers>(),
    );
    const VIDIOC_QUERYBUF: u32 = ioc(IOC_READ | IOC_WRITE, b'V', 9, size_of::<V4l2Buffer>());
    const VIDIOC_QBUF: u32 = ioc(IOC_READ | IOC_WRITE, b'V', 15, size_of::<V4l2Buffer>());
    const VIDIOC_DQBUF: u32 = ioc(IOC_READ | IOC_WRITE, b'V', 17, size_of::<V4l2Buffer>());
    const VIDIOC_STREAMON: u32 = ioc(IOC_WRITE, b'V', 18, size_of::<i32>());
    const VIDIOC_STREAMOFF: u32 = ioc(IOC_WRITE, b'V', 19, size_of::<i32>());

    unsafe fn v4l2_ioctl(fd: libc::c_int, req: u32, arg: *mut c_void) -> libc::c_int {
        loop {
            let ret = libc::ioctl(fd, req as _, arg);
            if ret == -1 {
                let err = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
                if err == libc::EINTR {
                    continue;
                }
            }
            return ret;
        }
    }

    struct MappedBuffer {
        ptr: *mut c_void,
        len: usize,
    }

    struct LinuxCameraGuard {
        fd: libc::c_int,
        streaming: bool,
        buffers: Vec<MappedBuffer>,
    }

    impl Drop for LinuxCameraGuard {
        fn drop(&mut self) {
            unsafe {
                if self.streaming {
                    let mut buf_type: i32 = V4L2_BUF_TYPE_VIDEO_CAPTURE as i32;
                    let _ = v4l2_ioctl(
                        self.fd,
                        VIDIOC_STREAMOFF,
                        (&mut buf_type as *mut i32).cast::<c_void>(),
                    );
                }
                for buf in &self.buffers {
                    if !buf.ptr.is_null() && buf.ptr != libc::MAP_FAILED && buf.len > 0 {
                        libc::munmap(buf.ptr, buf.len);
                    }
                }
                if self.fd >= 0 {
                    libc::close(self.fd);
                }
            }
        }
    }

    fn open_first_capture_device() -> Result<libc::c_int, AppError> {
        let mut saw_permission_denied = false;

        for idx in 0..8 {
            let Ok(path) = CString::new(format!("/dev/video{idx}")) else {
                continue;
            };
            let fd = unsafe { libc::open(path.as_ptr(), libc::O_RDWR | libc::O_NONBLOCK, 0) };
            if fd < 0 {
                if std::io::Error::last_os_error().raw_os_error() == Some(libc::EACCES) {
                    saw_permission_denied = true;
                }
                continue;
            }

            let mut cap: V4l2Capability = unsafe { zeroed() };
            let ret = unsafe {
                v4l2_ioctl(
                    fd,
                    VIDIOC_QUERYCAP,
                    (&mut cap as *mut V4l2Capability).cast::<c_void>(),
                )
            };
            if ret == 0 {
                let effective_caps = if cap.capabilities & V4L2_CAP_DEVICE_CAPS != 0 {
                    cap.device_caps
                } else {
                    cap.capabilities
                };
                if (effective_caps & V4L2_CAP_VIDEO_CAPTURE != 0)
                    && (effective_caps & V4L2_CAP_STREAMING != 0)
                {
                    return Ok(fd);
                }
            }
            unsafe {
                libc::close(fd);
            }
        }

        if saw_permission_denied {
            Err(AppError::CameraError(
                "Sem permissão para acessar /dev/video*. Adicione seu usuário ao grupo 'video'."
                    .to_string(),
            ))
        } else {
            Err(AppError::CameraError(
                "Nenhuma câmera V4L2 com suporte a streaming encontrada em /dev/video0..7."
                    .to_string(),
            ))
        }
    }

    pub fn run(
        user_stopped: &Arc<AtomicBool>,
        capture_stop: &Arc<AtomicBool>,
        tx: &SyncSender<CameraUpdate>,
    ) -> Result<(), AppError> {
        let fd = open_first_capture_device()?;
        let mut guard = LinuxCameraGuard {
            fd,
            streaming: false,
            buffers: Vec::new(),
        };

        let mut chosen_pix: Option<V4l2PixFormat> = None;
        for pixfmt in [V4L2_PIX_FMT_YUYV, V4L2_PIX_FMT_MJPEG] {
            let mut fmt: V4l2Format = unsafe { zeroed() };
            fmt.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            fmt.fmt.pix = V4l2PixFormat {
                width: 1280,
                height: 720,
                pixelformat: pixfmt,
                field: V4L2_FIELD_ANY,
                ..unsafe { zeroed() }
            };
            let ret = unsafe {
                v4l2_ioctl(
                    guard.fd,
                    VIDIOC_S_FMT,
                    (&mut fmt as *mut V4l2Format).cast::<c_void>(),
                )
            };
            if ret == 0 {
                let actual = unsafe { fmt.fmt.pix };
                if actual.pixelformat == pixfmt && actual.width > 0 && actual.height > 0 {
                    chosen_pix = Some(actual);
                    break;
                }
            }
        }

        let pix = chosen_pix.ok_or_else(|| {
            AppError::CameraError(
                "A webcam não suporta os formatos de vídeo YUYV ou MJPEG.".to_string(),
            )
        })?;

        let mut req: V4l2RequestBuffers = unsafe { zeroed() };
        req.count = 4;
        req.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
        req.memory = V4L2_MEMORY_MMAP;
        if unsafe {
            v4l2_ioctl(
                guard.fd,
                VIDIOC_REQBUFS,
                (&mut req as *mut V4l2RequestBuffers).cast::<c_void>(),
            )
        } < 0
            || req.count == 0
        {
            return Err(AppError::CameraError(
                "Falha ao alocar buffers de captura V4L2.".to_string(),
            ));
        }

        for idx in 0..req.count {
            let mut buf: V4l2Buffer = unsafe { zeroed() };
            buf.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            buf.memory = V4L2_MEMORY_MMAP;
            buf.index = idx;
            if unsafe {
                v4l2_ioctl(
                    guard.fd,
                    VIDIOC_QUERYBUF,
                    (&mut buf as *mut V4l2Buffer).cast::<c_void>(),
                )
            } < 0
            {
                return Err(AppError::CameraError(
                    "Falha ao consultar buffer V4L2.".to_string(),
                ));
            }

            let offset = unsafe { buf.m.offset } as libc::off_t;
            let mapped = unsafe {
                libc::mmap(
                    ptr::null_mut(),
                    buf.length as usize,
                    libc::PROT_READ | libc::PROT_WRITE,
                    libc::MAP_SHARED,
                    guard.fd,
                    offset,
                )
            };
            if mapped == libc::MAP_FAILED {
                return Err(AppError::CameraError(
                    "Falha ao mapear memória de vídeo V4L2.".to_string(),
                ));
            }
            guard.buffers.push(MappedBuffer {
                ptr: mapped,
                len: buf.length as usize,
            });

            if unsafe {
                v4l2_ioctl(
                    guard.fd,
                    VIDIOC_QBUF,
                    (&mut buf as *mut V4l2Buffer).cast::<c_void>(),
                )
            } < 0
            {
                return Err(AppError::CameraError(
                    "Falha ao enfileirar buffer V4L2.".to_string(),
                ));
            }
        }

        let mut stream_type: i32 = V4L2_BUF_TYPE_VIDEO_CAPTURE as i32;
        if unsafe {
            v4l2_ioctl(
                guard.fd,
                VIDIOC_STREAMON,
                (&mut stream_type as *mut i32).cast::<c_void>(),
            )
        } < 0
        {
            return Err(AppError::CameraError(
                "Falha ao iniciar stream de vídeo V4L2.".to_string(),
            ));
        }
        guard.streaming = true;

        let mut last_scan: Option<Instant> = None;
        while !user_stopped.load(Ordering::Acquire) && !capture_stop.load(Ordering::Acquire) {
            let mut pfd = libc::pollfd {
                fd: guard.fd,
                events: libc::POLLIN,
                revents: 0,
            };
            let poll_ret = unsafe { libc::poll(&mut pfd, 1, 100) };
            if poll_ret <= 0 {
                continue;
            }

            let mut buf: V4l2Buffer = unsafe { zeroed() };
            buf.type_ = V4L2_BUF_TYPE_VIDEO_CAPTURE;
            buf.memory = V4L2_MEMORY_MMAP;
            if unsafe {
                v4l2_ioctl(
                    guard.fd,
                    VIDIOC_DQBUF,
                    (&mut buf as *mut V4l2Buffer).cast::<c_void>(),
                )
            } < 0
            {
                thread::sleep(Duration::from_millis(10));
                continue;
            }

            let idx = buf.index as usize;
            let frame_res = if idx < guard.buffers.len() {
                let mapped = &guard.buffers[idx];
                let used = (buf.bytesused as usize).min(mapped.len);
                let bytes = unsafe { std::slice::from_raw_parts(mapped.ptr as *const u8, used) };
                if pix.pixelformat == V4L2_PIX_FMT_YUYV {
                    yuyv_stride_to_rgba(pix.width, pix.height, pix.bytesperline as usize, bytes)
                } else {
                    mjpeg_to_rgba(bytes)
                }
            } else {
                Err(AppError::CameraError(
                    "Índice de buffer V4L2 fora dos limites.".to_string(),
                ))
            };

            let _ = unsafe {
                v4l2_ioctl(
                    guard.fd,
                    VIDIOC_QBUF,
                    (&mut buf as *mut V4l2Buffer).cast::<c_void>(),
                )
            };

            if let Ok(frame) = frame_res {
                let _ = tx.try_send(CameraUpdate::Preview(frame.clone()));
                if last_scan.is_none_or(|t| t.elapsed() >= QR_SCAN_INTERVAL) {
                    last_scan = Some(Instant::now());
                    if let Ok(items) =
                        classify_qr_from_rgba_frame(frame.width, frame.height, &frame.rgba)
                    {
                        if !items.is_empty() {
                            capture_stop.store(true, Ordering::SeqCst);
                            let _ = send_reliable(
                                tx,
                                user_stopped,
                                CameraUpdate::Decoded { frame, items },
                            );
                            break;
                        }
                    }
                }
            }
        }

        drop(guard);
        Ok(())
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn v4l2_structs_match_linux_abi_sizes() {
            assert_eq!(size_of::<V4l2Capability>(), 104);
            assert_eq!(size_of::<V4l2PixFormat>(), 48);
            assert_eq!(size_of::<V4l2RequestBuffers>(), 20);
            #[cfg(target_pointer_width = "64")]
            {
                assert_eq!(size_of::<V4l2Format>(), 208);
                assert_eq!(size_of::<V4l2Buffer>(), 88);
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_camera {
    use std::ffi::c_void;
    use std::mem::{size_of, zeroed};
    use std::ptr;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::SyncSender;
    use std::sync::Arc;
    use std::thread;
    use std::time::Duration;

    use super::{
        bgr24_bottom_up_to_rgba, bgra_stride_to_rgba, mjpeg_to_rgba, run_qr_scan_loop,
        yuyv_to_rgba, CameraUpdate, FrameSink, PREVIEW_MAX_WIDTH,
    };
    use crate::error::AppError;

    const WS_POPUP: u32 = 0x8000_0000;
    const PM_REMOVE: u32 = 0x0001;
    const GWLP_USERDATA: i32 = -21;

    const WM_USER: u32 = 0x0400;
    const WM_CAP_SET_CALLBACK_FRAME: u32 = WM_USER + 5;
    const WM_CAP_DRIVER_CONNECT: u32 = WM_USER + 10;
    const WM_CAP_DRIVER_DISCONNECT: u32 = WM_USER + 11;
    const WM_CAP_GET_VIDEOFORMAT: u32 = WM_USER + 44;
    const WM_CAP_SET_VIDEOFORMAT: u32 = WM_USER + 45;
    const WM_CAP_GRAB_FRAME_NOSTOP: u32 = WM_USER + 61;

    const BI_RGB: u32 = 0;

    #[repr(C)]
    struct WinPoint {
        x: i32,
        y: i32,
    }

    #[repr(C)]
    struct WinMsg {
        hwnd: *mut c_void,
        message: u32,
        wparam: usize,
        lparam: isize,
        time: u32,
        pt: WinPoint,
    }

    #[repr(C)]
    struct VideoHdr {
        lp_data: *mut u8,
        dw_buffer_length: u32,
        dw_bytes_used: u32,
        dw_time_captured: u32,
        dw_user: usize,
        dw_flags: u32,
        dw_reserved: [usize; 4],
    }

    #[repr(C)]
    #[derive(Clone, Copy)]
    struct BitmapInfoHeader {
        bi_size: u32,
        bi_width: i32,
        bi_height: i32,
        bi_planes: u16,
        bi_bit_count: u16,
        bi_compression: u32,
        bi_size_image: u32,
        bi_x_pels_per_meter: i32,
        bi_y_pels_per_meter: i32,
        bi_clr_used: u32,
        bi_clr_important: u32,
    }

    struct WinContext {
        sink: Arc<FrameSink>,
        width: i32,
        height: i32,
        bit_count: u16,
        compression: u32,
    }

    #[link(name = "avicap32")]
    extern "system" {
        fn capCreateCaptureWindowW(
            lpsz_window_name: *const u16,
            dw_style: u32,
            x: i32,
            y: i32,
            n_width: i32,
            n_height: i32,
            hwnd_parent: *mut c_void,
            n_id: i32,
        ) -> *mut c_void;
    }

    #[link(name = "user32")]
    extern "system" {
        fn SendMessageW(hwnd: *mut c_void, msg: u32, wparam: usize, lparam: isize) -> isize;
        fn DestroyWindow(hwnd: *mut c_void) -> i32;
        fn SetWindowLongPtrW(hwnd: *mut c_void, n_index: i32, dw_new_long: isize) -> isize;
        fn GetWindowLongPtrW(hwnd: *mut c_void, n_index: i32) -> isize;
        fn PeekMessageW(
            lp_msg: *mut WinMsg,
            hwnd: *mut c_void,
            w_msg_filter_min: u32,
            w_msg_filter_max: u32,
            w_remove_msg: u32,
        ) -> i32;
        fn TranslateMessage(lp_msg: *const WinMsg) -> i32;
        fn DispatchMessageW(lp_msg: *const WinMsg) -> isize;
    }

    extern "system" fn frame_callback(hwnd: *mut c_void, lpvhdr: *mut VideoHdr) -> isize {
        if hwnd.is_null() || lpvhdr.is_null() {
            return 1;
        }
        let ctx_ptr = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) } as *const WinContext;
        if ctx_ptr.is_null() {
            return 1;
        }
        // SAFETY: `ctx_ptr` permanece válido enquanto `WinCaptureGuard` não limpar o callback.
        let ctx = unsafe { &*ctx_ptr };
        if ctx.sink.capture_stop.load(Ordering::Relaxed) {
            return 1;
        }

        let hdr = unsafe { &*lpvhdr };
        if hdr.lp_data.is_null() || hdr.dw_bytes_used == 0 {
            return 1;
        }
        let used = (hdr.dw_bytes_used as usize).min(hdr.dw_buffer_length as usize);
        let data = unsafe { std::slice::from_raw_parts(hdr.lp_data, used) };

        let width_u32 = ctx.width.unsigned_abs();
        let height_u32 = ctx.height.unsigned_abs();
        let yuy2_fourcc = u32::from_le_bytes(*b"YUY2");
        let mjpg_fourcc = u32::from_le_bytes(*b"MJPG");

        let frame_res = if ctx.compression == BI_RGB && ctx.bit_count == 24 {
            bgr24_bottom_up_to_rgba(width_u32, height_u32, data)
        } else if ctx.compression == BI_RGB && ctx.bit_count == 32 {
            bgra_stride_to_rgba(
                width_u32,
                height_u32,
                (width_u32 as usize) * 4,
                data,
                PREVIEW_MAX_WIDTH,
            )
        } else if ctx.compression == yuy2_fourcc {
            yuyv_to_rgba(width_u32, height_u32, data)
        } else if ctx.compression == mjpg_fourcc {
            mjpeg_to_rgba(data)
        } else {
            return 1;
        };

        if let Ok(frame) = frame_res {
            ctx.sink.push_frame(frame);
        }
        1
    }

    struct WinCaptureGuard {
        hwnd: *mut c_void,
        connected: bool,
        ctx_ptr: *mut WinContext,
    }

    impl Drop for WinCaptureGuard {
        fn drop(&mut self) {
            unsafe {
                if !self.hwnd.is_null() {
                    let _ = SendMessageW(self.hwnd, WM_CAP_SET_CALLBACK_FRAME, 0, 0);
                    let _ = SetWindowLongPtrW(self.hwnd, GWLP_USERDATA, 0);
                    if self.connected {
                        let _ = SendMessageW(self.hwnd, WM_CAP_DRIVER_DISCONNECT, 0, 0);
                    }
                    let _ = DestroyWindow(self.hwnd);
                }
                if !self.ctx_ptr.is_null() {
                    drop(Box::from_raw(self.ctx_ptr));
                }
            }
        }
    }

    pub fn run(
        user_stopped: &Arc<AtomicBool>,
        capture_stop: &Arc<AtomicBool>,
        tx: &SyncSender<CameraUpdate>,
    ) -> Result<(), AppError> {
        let title: Vec<u16> = "OTP Tools Camera\0".encode_utf16().collect();
        let hwnd = unsafe {
            capCreateCaptureWindowW(title.as_ptr(), WS_POPUP, 0, 0, 640, 480, ptr::null_mut(), 0)
        };
        if hwnd.is_null() {
            return Err(AppError::CameraError(
                "Não foi possível inicializar janela de captura de vídeo no Windows.".to_string(),
            ));
        }

        let mut guard = WinCaptureGuard {
            hwnd,
            connected: false,
            ctx_ptr: ptr::null_mut(),
        };

        let mut connected_driver = false;
        for driver_idx in 0..10 {
            if unsafe { SendMessageW(hwnd, WM_CAP_DRIVER_CONNECT, driver_idx, 0) } != 0 {
                connected_driver = true;
                break;
            }
        }
        if !connected_driver {
            return Err(AppError::CameraError(
                "Nenhuma câmera/webcam disponível encontrada no Windows.".to_string(),
            ));
        }
        guard.connected = true;

        let mut bmi: BitmapInfoHeader = unsafe { zeroed() };
        bmi.bi_size = size_of::<BitmapInfoHeader>() as u32;
        bmi.bi_width = 640;
        bmi.bi_height = 480;
        bmi.bi_planes = 1;
        bmi.bi_bit_count = 24;
        bmi.bi_compression = BI_RGB;
        let _ = unsafe {
            SendMessageW(
                hwnd,
                WM_CAP_SET_VIDEOFORMAT,
                size_of::<BitmapInfoHeader>(),
                (&bmi as *const BitmapInfoHeader) as isize,
            )
        };

        let mut active_bmi: BitmapInfoHeader = unsafe { zeroed() };
        active_bmi.bi_size = size_of::<BitmapInfoHeader>() as u32;
        if unsafe {
            SendMessageW(
                hwnd,
                WM_CAP_GET_VIDEOFORMAT,
                size_of::<BitmapInfoHeader>(),
                (&mut active_bmi as *mut BitmapInfoHeader) as isize,
            )
        } <= 0
        {
            return Err(AppError::CameraError(
                "Não foi possível obter o formato de vídeo da webcam.".to_string(),
            ));
        }

        let sink = Arc::new(FrameSink::new(Arc::clone(capture_stop)));
        let win_ctx = Box::new(WinContext {
            sink: Arc::clone(&sink),
            width: active_bmi.bi_width,
            height: active_bmi.bi_height,
            bit_count: active_bmi.bi_bit_count,
            compression: active_bmi.bi_compression,
        });
        let ctx_ptr = Box::into_raw(win_ctx);
        guard.ctx_ptr = ctx_ptr;

        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, ctx_ptr as isize);
            SendMessageW(hwnd, WM_CAP_SET_CALLBACK_FRAME, 0, frame_callback as isize);
        }

        let grab_user_stopped = Arc::clone(user_stopped);
        let grab_capture_stop = Arc::clone(capture_stop);
        let grab_sink = Arc::clone(&sink);
        let scan_handle = thread::Builder::new()
            .name("otp-tools-camera-win-scan".to_string())
            .spawn({
                let tx_clone = tx.clone();
                move || {
                    run_qr_scan_loop(
                        &grab_sink,
                        &grab_user_stopped,
                        &grab_capture_stop,
                        &tx_clone,
                    );
                }
            })
            .map_err(|err| {
                AppError::CameraError(format!("Falha ao iniciar scanner de vídeo: {err}"))
            })?;

        while !user_stopped.load(Ordering::Acquire) && !capture_stop.load(Ordering::Acquire) {
            let mut msg: WinMsg = unsafe { zeroed() };
            unsafe {
                while PeekMessageW(&mut msg, hwnd, 0, 0, PM_REMOVE) != 0 {
                    TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
                let _ = SendMessageW(hwnd, WM_CAP_GRAB_FRAME_NOSTOP, 0, 0);
            }
            thread::sleep(Duration::from_millis(33));
        }

        let _ = scan_handle.join();
        drop(guard);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::OtpType;

    fn make_qr_rgba_frame(content: &str) -> CameraFrame {
        let code = ::qrcode::QrCode::new(content.as_bytes()).expect("gerar QR Code de teste");
        let module_size = 6usize;
        let quiet = 4usize;
        let side = (code.width() + 2 * quiet) * module_size;
        let mut rgba = vec![255u8; side * side * 4];
        for y in 0..code.width() {
            for x in 0..code.width() {
                if code[(x, y)] == ::qrcode::Color::Dark {
                    let px = (x + quiet) * module_size;
                    let py = (y + quiet) * module_size;
                    for dy in 0..module_size {
                        for dx in 0..module_size {
                            let idx = ((py + dy) * side + (px + dx)) * 4;
                            rgba[idx] = 0;
                            rgba[idx + 1] = 0;
                            rgba[idx + 2] = 0;
                            rgba[idx + 3] = 255;
                        }
                    }
                }
            }
        }
        CameraFrame {
            width: side as u32,
            height: side as u32,
            rgba,
        }
    }

    #[test]
    fn bgra_stride_to_rgba_converts_padding_and_downscales() {
        // 4x2 BGRA com stride 20 (4 bytes extras de padding por linha)
        let bgra = vec![
            10, 20, 30, 0, 40, 50, 60, 128, 70, 80, 90, 255, 100, 110, 120, 255, 0, 0, 0, 0, 1, 2,
            3, 255, 4, 5, 6, 255, 7, 8, 9, 255, 10, 11, 12, 255, 0, 0, 0, 0,
        ];
        let full = bgra_stride_to_rgba(4, 2, 20, &bgra, 0).expect("converter BGRA com stride");
        assert_eq!(full.width, 4);
        assert_eq!(full.height, 2);
        assert_eq!(&full.rgba[0..4], &[30, 20, 10, 255]);
        assert_eq!(&full.rgba[4..8], &[60, 50, 40, 255]);
        assert_eq!(&full.rgba[16..20], &[3, 2, 1, 255]);

        let downscaled =
            bgra_stride_to_rgba(4, 2, 20, &bgra, 2).expect("reduzir quadro BGRA para <= 2px");
        assert_eq!(downscaled.width, 2);
        assert_eq!(downscaled.height, 1);
        assert_eq!(&downscaled.rgba[0..4], &[30, 20, 10, 255]);
        assert_eq!(&downscaled.rgba[4..8], &[90, 80, 70, 255]);

        assert!(bgra_stride_to_rgba(4, 2, 12, &bgra, 0).is_err());
        assert!(bgra_stride_to_rgba(4, 2, 20, &bgra[..10], 0).is_err());
    }

    #[test]
    fn yuyv_to_rgba_converts_black_and_white_pixels() {
        // 2x1: pixel 0 preto (Y=16, U=128, V=128), pixel 1 branco (Y=235, U=128, V=128)
        let yuyv = [16u8, 128, 235, 128];
        let frame = yuyv_to_rgba(2, 1, &yuyv).expect("converter YUYV 2x1");
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 1);
        assert_eq!(&frame.rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&frame.rgba[4..8], &[255, 255, 255, 255]);

        assert!(yuyv_to_rgba(3, 1, &yuyv).is_err());
        assert!(yuyv_to_rgba(2, 1, &[0, 1]).is_err());
    }

    #[test]
    fn bgr24_bottom_up_to_rgba_flips_rows_and_respects_4_byte_stride() {
        // 2x2 BGR24: raw_row = 6 bytes, stride = 8 bytes (2 bytes de padding por linha).
        // Linha inferior (y=0 no buffer) = verde; linha superior (y=1 no buffer) = vermelho.
        let bgr = [
            0, 255, 0, 0, 255, 0, 0, 0, // linha inferior (salva primeiro no DIB)
            0, 0, 255, 0, 0, 255, 0, 0, // linha superior (vai para o topo RGBA)
        ];
        let frame = bgr24_bottom_up_to_rgba(2, 2, &bgr).expect("converter BGR24 bottom-up");
        assert_eq!(frame.width, 2);
        assert_eq!(frame.height, 2);
        assert_eq!(&frame.rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&frame.rgba[8..12], &[0, 255, 0, 255]);
    }

    #[test]
    fn camera_frame_zeroizes_on_drop() {
        let frame = CameraFrame {
            width: 2,
            height: 2,
            rgba: vec![0xAA; 16],
        };
        drop(frame);
    }

    #[test]
    fn camera_session_mock_frames_emits_preview_decodes_and_stops() {
        let blank = CameraFrame {
            width: 64,
            height: 64,
            rgba: vec![255u8; 64 * 64 * 4],
        };
        let uri = "otpauth://totp/CamTest:user@example.com?secret=JBSWY3DPEHPK3PXP&issuer=CamTest";
        let qr_frame = make_qr_rgba_frame(uri);

        let mut session = CameraSession::from_mock_frames(vec![blank, qr_frame]);
        let mut saw_preview = false;
        let mut decoded_items = None;

        for _ in 0..100 {
            for update in session.poll() {
                match update {
                    CameraUpdate::Preview(_) => saw_preview = true,
                    CameraUpdate::Decoded { items, .. } => decoded_items = Some(items),
                    CameraUpdate::Error(err) => panic!("erro inesperado: {err}"),
                }
            }
            if decoded_items.is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }

        session.stop();
        assert!(saw_preview);
        let items = decoded_items.expect("QR deveria ter sido decodificado");
        assert_eq!(items.len(), 1);
        match &items[0] {
            QrClassification::OtpAuth(account) => {
                assert_eq!(account.otp_type, OtpType::Totp);
                assert_eq!(account.label, "CamTest:user@example.com");
                assert_eq!(account.secret, "JBSWY3DPEHPK3PXP");
            }
            other => panic!("esperava OtpAuth, obtive {other:?}"),
        }
    }

    #[test]
    fn mjpeg_to_rgba_decodes_valid_jpeg_and_rejects_corrupt_or_oversized() {
        let qr_frame =
            make_qr_rgba_frame("otpauth://totp/JpegTest:user@example.com?secret=JBSWY3DPEHPK3PXP");
        let rgb_img = image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(qr_frame.width, qr_frame.height, qr_frame.rgba.clone())
                .expect("montar RgbaImage"),
        )
        .to_rgb8();

        let mut jpeg_bytes = Vec::new();
        rgb_img
            .write_to(&mut Cursor::new(&mut jpeg_bytes), ImageFormat::Jpeg)
            .expect("codificar JPEG");

        let decoded_frame = mjpeg_to_rgba(&jpeg_bytes).expect("decodificar MJPEG");
        assert_eq!(decoded_frame.width, qr_frame.width);
        assert_eq!(decoded_frame.height, qr_frame.height);
        let classified = classify_qr_from_rgba_frame(
            decoded_frame.width,
            decoded_frame.height,
            &decoded_frame.rgba,
        )
        .expect("ler QR de frame MJPEG decodificado");
        assert!(matches!(
            &classified[0],
            QrClassification::OtpAuth(acc) if acc.secret == "JBSWY3DPEHPK3PXP"
        ));

        assert!(matches!(
            mjpeg_to_rgba(b"not-a-jpeg"),
            Err(AppError::CameraError(_))
        ));
    }

    #[test]
    fn scan_qr_from_camera_blocking_rejects_zero_and_overflow_timeouts() {
        assert!(matches!(
            scan_qr_from_camera_blocking(Duration::ZERO),
            Err(AppError::CameraError(_))
        ));
        assert!(matches!(
            scan_qr_from_camera_blocking(Duration::MAX),
            Err(AppError::CameraError(_))
        ));
    }
}
