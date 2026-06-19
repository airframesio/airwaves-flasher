//! Build the OS-native privilege-escalation command that launches the elevated
//! helper. The helper streams progress to a file (passed in `args`) so we don't
//! depend on capturing the elevated process's stdout.

use std::process::Command;

/// Returns a `Command` that, when spawned and waited on, runs `helper args…`
/// with elevated privileges (Touch ID / polkit / UAC prompt).
pub fn elevated_command(helper: &str, args: &[String]) -> Command {
    #[cfg(target_os = "macos")]
    {
        // osascript: do shell script "<helper> <args>" with administrator privileges
        let mut script = shell_quote(helper);
        for a in args {
            script.push(' ');
            script.push_str(&shell_quote(a));
        }
        let mut cmd = Command::new("osascript");
        cmd.arg("-e").arg(format!(
            "do shell script {} with administrator privileges",
            applescript_quote(&script)
        ));
        cmd
    }
    #[cfg(target_os = "linux")]
    {
        let launcher = if which("pkexec") {
            "pkexec"
        } else if which("sudo") {
            "sudo"
        } else {
            "doas"
        };
        let mut cmd = Command::new(launcher);
        cmd.arg(helper);
        cmd.args(args);
        cmd
    }
    #[cfg(target_os = "windows")]
    {
        // PowerShell Start-Process -Verb RunAs triggers UAC and waits.
        let arglist = args
            .iter()
            .map(|a| format!("'{}'", a.replace('\'', "''")))
            .collect::<Vec<_>>()
            .join(",");
        let mut cmd = Command::new("powershell");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            &format!(
                "$p = Start-Process -FilePath '{}' -ArgumentList {} -Verb RunAs -Wait -PassThru; exit $p.ExitCode",
                helper.replace('\'', "''"),
                if arglist.is_empty() { "@()".into() } else { arglist }
            ),
        ]);
        cmd
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    {
        let mut cmd = Command::new(helper);
        cmd.args(args);
        cmd
    }
}

#[cfg(target_os = "linux")]
fn which(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|p| p.join(bin).exists())
        })
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn shell_quote(s: &str) -> String {
    // POSIX single-quote escaping for the inner shell command
    format!("'{}'", s.replace('\'', r"'\''"))
}

#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    // AppleScript string literal: wrap in double quotes, escape \ and "
    let escaped = s.replace('\\', r"\\").replace('"', r#"\""#);
    format!("\"{escaped}\"")
}
