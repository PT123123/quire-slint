//! Startup registration for the immutable, versioned Workshop builds.
//!
//! Installer and portable builds keep their own launch behavior. The Workshop
//! copy is special: each release lives in a new `C:\workshop\quire-slint-*`
//! directory, so that executable repairs its own per-user Run entry whenever
//! it starts.

use std::path::Path;

/// Make this Workshop executable run when the current Windows user signs in.
///
/// Returns `Ok(false)` for builds outside `C:\workshop\quire-slint-*`.
/// Registration is per-user, does not require elevation, and uses the exact
/// executable path (quoted for Windows paths with spaces).
pub fn ensure_workshop_autostart() -> Result<bool, String> {
    let exe = std::env::current_exe().map_err(|e| format!("current executable: {e}"))?;
    if !is_workshop_executable(&exe) {
        return Ok(false);
    }

    #[cfg(target_os = "windows")]
    {
        ensure_windows_run_value(&exe)?;
        Ok(true)
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = exe;
        Ok(false)
    }
}

fn is_workshop_executable(exe: &Path) -> bool {
    let normalized = exe.to_string_lossy().replace('/', "\\");
    let normalized = normalized.strip_prefix(r"\\?\").unwrap_or(&normalized);
    let mut parts = normalized.rsplitn(3, '\\');
    let Some(exe_name) = parts.next() else {
        return false;
    };
    let Some(dir_name) = parts.next() else {
        return false;
    };
    let Some(root) = parts.next() else {
        return false;
    };
    root.eq_ignore_ascii_case(r"C:\workshop")
        && dir_name
            .get(.."quire-slint-".len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case("quire-slint-"))
        && exe_name.eq_ignore_ascii_case(&format!("{dir_name}.exe"))
}

#[cfg(target_os = "windows")]
fn ensure_windows_run_value(exe: &Path) -> Result<(), String> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "Quire";

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (run, _) = hkcu
        .create_subkey(RUN_KEY)
        .map_err(|e| format!("open current-user startup registry key: {e}"))?;
    let expected = format!("\"{}\"", exe.display());
    let current: Result<String, _> = run.get_value(VALUE_NAME);
    if current
        .as_deref()
        .is_ok_and(|value| value.eq_ignore_ascii_case(&expected))
    {
        return Ok(());
    }
    run.set_value(VALUE_NAME, &expected)
        .map_err(|e| format!("register Quire for sign-in: {e}"))
}

#[cfg(test)]
mod tests {
    use super::is_workshop_executable;
    use std::path::Path;

    #[test]
    fn recognizes_versioned_workshop_executable_only() {
        assert!(is_workshop_executable(Path::new(
            r"C:\workshop\quire-slint-0.1.47\quire-slint-0.1.47.exe"
        )));
        assert!(is_workshop_executable(Path::new(
            r"c:\WORKSHOP\QUIRE-SLINT-0.2.0\quire-slint-0.2.0.exe"
        )));
        assert!(!is_workshop_executable(Path::new(
            r"C:\workshop\quire-slint-0.1.47\quire-slint.exe"
        )));
        assert!(!is_workshop_executable(Path::new(
            r"C:\src\quire\target\debug\quire-slint.exe"
        )));
    }
}
