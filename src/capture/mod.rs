//! Screenshot capture, strictly separated from the UI: backends deliver an
//! image of the whole desktop; selection and cropping then happen locally.

pub mod permission;
pub mod portal_screenshot;

use std::fmt;
use std::time::Duration;

use gtk::gdk;

use crate::i18n::tr;

/// Image of the whole desktop (all monitors, one image) plus timings.
pub struct DesktopFrame {
    pub texture: gdk::Texture,
    /// Time from the portal call to its response.
    pub portal_time: Duration,
    /// Time spent loading/decoding the image.
    pub load_time: Duration,
}

#[derive(Debug)]
pub enum CaptureError {
    /// The user denied Porthole permission to capture. XDP never asks again by
    /// itself afterwards – only `permission::reset` undoes this.
    PermissionDenied,
    /// No permission yet, and the permission dialog wasn't allowed to appear
    /// because no Porthole window had focus.
    PermissionNeedsFocus,
    Cancelled,
    PortalUnavailable(String),
    Timeout,
    Load(String),
    Failed(String),
}

impl fmt::Display for CaptureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let with_detail = |msgid, detail: &str| tr(msgid).replace("{detail}", detail);
        let message = match self {
            Self::PermissionDenied => tr("Porthole isn't allowed to take screenshots. \
                 Reset the permission with: porthole --reset-permission")
            .to_owned(),
            Self::PermissionNeedsFocus => tr("One-time approval needed: please click the camera \
                 button here and choose “Allow”. After that the shortcut works from anywhere.")
            .to_owned(),
            Self::Cancelled => tr("Capture cancelled.").to_owned(),
            Self::PortalUnavailable(detail) => {
                with_detail("Screenshot portal not available: {detail}", detail)
            }
            Self::Timeout => tr("The screenshot portal didn't respond in time.").to_owned(),
            Self::Load(detail) => with_detail("Screenshot could not be loaded: {detail}", detail),
            Self::Failed(detail) => with_detail("Capture failed: {detail}", detail),
        };
        f.write_str(&message)
    }
}

#[allow(async_fn_in_trait)]
pub trait CaptureBackend {
    async fn capture_desktop(&self) -> Result<DesktopFrame, CaptureError>;
}
