use anyhow::Result;
use objc2_foundation::NSBundle;

/// Apple Developer Team ID used for code signing and validation.
// moca: equipo de Apple de Moca (Jose David Moreira).
pub const APPLE_TEAM_ID: &str = "DSXB7C9XQA";

/// Get the path to the macOS `.app` bundle.
pub fn get_bundle_path() -> Result<String> {
    let bundle = NSBundle::mainBundle();
    let path = bundle.bundlePath();
    Ok(path.to_string())
}
