//! Single source of the app's identity. The ID must match the file name of the
//! installed `.desktop` file, otherwise the portal rejects the registration
//! (and with it the screenshot permission and global shortcuts).

pub const APP_ID: &str = "app.porthole.Porthole";
pub const APP_NAME: &str = "Porthole";
pub const LOG_DOMAIN: &str = "porthole";
