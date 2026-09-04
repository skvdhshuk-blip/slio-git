use crate::error::GitError;
use std::process::Command;

#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn configure_background_command(command: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;

        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

pub fn background_command(program: &str) -> Command {
    let mut command = Command::new(program);
    configure_background_command(&mut command);
    command
}

pub fn require_system_git(operation: &str) -> Result<(), GitError> {
    if crate::capability::system_git() {
        Ok(())
    } else {
        Err(GitError::OperationFailed {
            operation: operation.to_string(),
            details: format!(
                "{operation} needs system git, which is not available in the App Store build"
            ),
        })
    }
}

pub fn git_command() -> Result<Command, GitError> {
    require_system_git("git")?;
    Ok(background_command("git"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_build_rejects_system_git() {
        let result = require_system_git("probe");
        if crate::capability::system_git() {
            assert!(result.is_ok());
            assert!(git_command().is_ok());
        } else {
            assert!(result.is_err());
            assert!(git_command().is_err());
        }
    }
}
