//! Einzige Stelle für App-Identität. Die ID muss zum Dateinamen der
//! installierten `.desktop`-Datei passen, sonst lehnt das Portal die
//! Registrierung ab (und damit Screenshot-Permission und globale Shortcuts).

pub const APP_ID: &str = "app.porthole.Porthole";
pub const APP_NAME: &str = "Porthole";
pub const LOG_DOMAIN: &str = "porthole";
